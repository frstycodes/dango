// Self-update from GitHub Releases, out of the user's way: shortly after launch and about hourly after,
// a newer release's asset downloads into the data dir's `updates/` and is kept once its SHA-256 matches.
// The next launch, or Restart in the user panel, swaps it in without touching the network.
//
// `updates/` holds `<version>.zip` (verified) and `<version>.zip.part` (still downloading); the name is
// the record of which version it is, and a `.part` only becomes a `.zip` after its checksum matched.
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime},
};

use futures::{
    StreamExt,
    channel::mpsc::{UnboundedReceiver, UnboundedSender},
};
use rand::Rng;
use reqwest::{StatusCode, header::RANGE};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

const LATEST: &str = "https://api.github.com/repos/frstycodes/dango/releases/latest";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const RELEASES: &str = "https://github.com/frstycodes/dango/releases/tag";
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
// A download that sends nothing for this long is given up (and resumed on the next check); a slow but
// moving one is not.
const STALL_TIMEOUT: Duration = Duration::from_secs(30);
const FIRST_CHECK: Duration = Duration::from_secs(30);
const INTERVAL: Duration = Duration::from_secs(60 * 60);
// Points the updater at another release JSON (same shape as GitHub's), e.g. a local server for testing.
// It also stages under the temp dir, so a test never leaves a build for a real install to pick up.
const FEED_VAR: &str = "DANGO_UPDATE_FEED";

pub fn enabled() -> bool {
    std::env::var_os(FEED_VAR).is_some() || (!cfg!(debug_assertions) && std::env::var_os("CHORD_DEMO").is_none())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Staged {
    pub version: String,
    path: PathBuf,
}

#[cfg(debug_assertions)]
impl Staged {
    // For `CHORD_DEMO=update-ready`: shows the row; Restart fails to install and says so.
    pub fn demo(version: &str) -> Self {
        Self { version: version.into(), path: PathBuf::new() }
    }
}

struct Update {
    version: String,
    url: String,
    checksum_url: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn feed() -> String {
    std::env::var(FEED_VAR).unwrap_or_else(|_| LATEST.into())
}

fn dir() -> Option<PathBuf> {
    if std::env::var_os(FEED_VAR).is_some() {
        return Some(std::env::temp_dir().join("dango-updates-test"));
    }
    Some(crate::data_dir()?.join("updates"))
}

fn http() -> reqwest::Client {
    // GitHub's API rejects requests without a User-Agent.
    reqwest::Client::builder().user_agent(concat!("Dango/", env!("CARGO_PKG_VERSION"))).connect_timeout(CHECK_TIMEOUT).read_timeout(STALL_TIMEOUT).build().unwrap_or_default()
}

// A verified update for a newer version, if one is staged. Clears everything else out of the staging dir.
pub fn pending() -> Option<Staged> {
    tidy(&dir()?, CURRENT)
}

// Where the updater is, for the user card's row and the Updates page.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Status {
    // No check has finished since launch.
    #[default]
    Idle,
    Checking,
    UpToDate,
    // Found, waiting for the user to ask: automatic downloads are off.
    Available(String),
    Downloading { version: String, done: u64, total: Option<u64> },
    Ready(Staged),
    CheckFailed,
    DownloadFailed(String),
}

impl Status {
    // The version a check found, for its release notes.
    pub fn version(&self) -> Option<&str> {
        match self {
            Status::Available(v) | Status::Downloading { version: v, .. } | Status::DownloadFailed(v) => Some(v),
            Status::Ready(staged) => Some(&staged.version),
            _ => None,
        }
    }
}

// The view's side of the updater: its status, when a check last finished, and the loop to ask.
#[derive(Default)]
pub struct Updates {
    pub status: Status,
    pub checked_at: Option<SystemTime>,
    // None where updates are off (debug and demo builds without `DANGO_UPDATE_FEED`).
    pub updater: Option<Updater>,
}

impl Updates {
    pub fn staged(&self) -> Option<&Staged> {
        match &self.status {
            Status::Ready(staged) => Some(staged),
            _ => None,
        }
    }

    // A check has finished once the status leaves Checking; download progress isn't one.
    pub fn apply(&mut self, status: Status, now: SystemTime) {
        if self.status == Status::Checking && status != Status::Checking {
            self.checked_at = Some(now);
        }
        self.status = status;
    }
}

pub fn release_notes(version: &str) -> String {
    format!("{RELEASES}/v{version}")
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Check,
    Download,
}

// Asks the background loop for a check or a download, and carries the automatic-download preference.
pub struct Updater {
    commands: UnboundedSender<Command>,
    auto: Arc<AtomicBool>,
}

impl Updater {
    pub fn send(&self, command: Command) {
        self.commands.unbounded_send(command).ok();
    }

    pub fn set_auto(&self, on: bool) {
        self.auto.store(on, Ordering::Relaxed);
    }
}

// What a wake-up does: a timer tick or Check looks for a release, and downloads it only when automatic
// downloads are on; Download fetches what the last check found without asking GitHub again.
#[derive(Debug, PartialEq)]
struct Plan {
    check: bool,
    download: bool,
}

fn plan(asked: Option<Command>, auto: bool, known: bool) -> Plan {
    let wants = asked == Some(Command::Download);
    Plan { check: !(wants && known), download: wants || auto }
}

// Checks soon after launch, about hourly after, and whenever asked; each status change is sent on.
// One loop, so a check never starts while a download is in flight (a command waits for it).
pub fn watch(auto: bool) -> (Updater, UnboundedReceiver<Status>) {
    let (tx, rx) = futures::channel::mpsc::unbounded();
    let (commands, mut asked) = futures::channel::mpsc::unbounded();
    let auto = Arc::new(AtomicBool::new(auto));
    let updater = Updater { commands, auto: auto.clone() };
    crate::discord::runtime().spawn(async move {
        let Some(dir) = dir() else { return };
        let client = http();
        let send = |status| tx.unbounded_send(status).is_ok();
        let mut wait = FIRST_CHECK;
        let mut known: Option<Update> = None;
        loop {
            let command = tokio::select! {
                () = tokio::time::sleep(jittered(wait)) => None,
                command = asked.next() => match command {
                    Some(command) => Some(command),
                    None => return,
                },
            };
            wait = INTERVAL;
            let plan = plan(command, auto.load(Ordering::Relaxed), known.is_some());
            let update = match known.take() {
                Some(update) if !plan.check => update,
                _ => {
                    if !send(Status::Checking) {
                        return;
                    }
                    match check(&client).await {
                        Ok(Some(update)) => update,
                        Ok(None) => {
                            send(Status::UpToDate);
                            continue;
                        }
                        Err(e) => {
                            eprintln!("[updater] check failed: {e}");
                            send(Status::CheckFailed);
                            continue;
                        }
                    }
                }
            };
            if let Some(staged) = tidy(&dir, CURRENT).filter(|s| s.version == update.version) {
                send(Status::Ready(staged));
                continue;
            }
            if !plan.download {
                send(Status::Available(update.version.clone()));
                known = Some(update);
                continue;
            }
            let version = update.version.clone();
            let progress = |done, total| {
                send(Status::Downloading { version: version.clone(), done, total });
            };
            match stage(&dir, &update, &client, CURRENT, progress).await {
                Ok(staged) => {
                    send(Status::Ready(staged));
                }
                Err(e) => {
                    eprintln!("[updater] staging {} failed: {e}", update.version);
                    send(Status::DownloadFailed(update.version.clone()));
                    known = Some(update);
                }
            }
        }
    });
    (updater, rx)
}

// ±1/6 of the wait, so clients that launched together don't all ask GitHub at once.
fn jittered(wait: Duration) -> Duration {
    let spread = wait / 6;
    rand::thread_rng().gen_range(wait - spread..=wait + spread)
}

// A newer release for this platform, None when there's none, Err when GitHub couldn't be asked.
async fn check(client: &reqwest::Client) -> Result<Option<Update>, String> {
    let request = client.get(feed()).header("Accept", "application/vnd.github+json").timeout(CHECK_TIMEOUT);
    let release: Release = request.send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;
    if !newer(&release.tag_name, CURRENT) {
        return Ok(None);
    }
    Ok(pick(&release, std::env::consts::OS, std::env::consts::ARCH))
}

// Downloads `update` into `dir`, carrying on from a `.part` an earlier attempt left, and keeps it only
// once its checksum matches. A mismatch drops the `.part`, so the next check starts over.
// `progress` hears (bytes so far, whole size when the server says), once per percent.
async fn stage(dir: &Path, update: &Update, client: &reqwest::Client, current: &str, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Staged, String> {
    tokio::fs::create_dir_all(dir).await.map_err(|e| e.to_string())?;
    if let Some(staged) = tidy(dir, current).filter(|s| s.version == update.version) {
        return Ok(staged);
    }
    let path = dir.join(staged_name(&update.version));
    let part = dir.join(format!("{}.part", staged_name(&update.version)));
    let checksum = client.get(&update.checksum_url).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?.text().await.map_err(|e| e.to_string())?;
    let have = tokio::fs::metadata(&part).await.map_or(0, |m| m.len());
    let mut request = client.get(&update.url);
    if have > 0 {
        request = request.header(RANGE, format!("bytes={have}-"));
    }
    let mut response = request.send().await.map_err(|e| e.to_string())?;
    // 416: the `.part` is already whole (an earlier run stopped before renaming it), so only verify it.
    let whole = have > 0 && response.status() == StatusCode::RANGE_NOT_SATISFIABLE;
    if !whole {
        response = response.error_for_status().map_err(|e| e.to_string())?;
        // A server that ignores the range sends everything again (200), so start the file over.
        let resume = response.status() == StatusCode::PARTIAL_CONTENT;
        let mut done = if resume { have } else { 0 };
        let total = response.content_length().map(|len| done + len);
        let mut file = tokio::fs::OpenOptions::new().create(true).write(true).append(resume).truncate(!resume).open(&part).await.map_err(|e| e.to_string())?;
        let mut shown = None;
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            file.write_all(&chunk).await.map_err(|e| e.to_string())?;
            done += chunk.len() as u64;
            let step = progress_step(done, total);
            if shown != Some(step) {
                shown = Some(step);
                progress(done, total);
            }
        }
        file.flush().await.map_err(|e| e.to_string())?;
    }
    let hashed = part.clone();
    let digest = tokio::task::spawn_blocking(move || sha256_file(&hashed)).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    if let Err(e) = verify(&digest, &checksum) {
        tokio::fs::remove_file(&part).await.ok();
        return Err(e);
    }
    tokio::fs::rename(&part, &path).await.map_err(|e| e.to_string())?;
    tidy(dir, current).ok_or_else(|| "staged update vanished".into())
}

// Progress is reported per whole percent, or per MiB when the size is unknown.
fn progress_step(done: u64, total: Option<u64>) -> u64 {
    match total {
        Some(total) if total > 0 => done * 100 / total,
        _ => done >> 20,
    }
}

fn staged_name(version: &str) -> String {
    format!("{version}.zip")
}

fn tidy(dir: &Path, current: &str) -> Option<Staged> {
    let names: Vec<String> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    let (best, delete) = survey(&names, current);
    for name in delete {
        let path = dir.join(name);
        std::fs::remove_dir_all(&path).or_else(|_| std::fs::remove_file(&path)).ok();
    }
    best.map(|version| Staged { path: dir.join(staged_name(&version)), version })
}

// The newest verified download newer than `current`, and what to delete: every other verified one,
// every `.part` that isn't newer than what's staged (or running), and anything that isn't ours.
fn survey(names: &[String], current: &str) -> (Option<String>, Vec<String>) {
    let Some(current) = version(current) else { return (None, names.to_vec()) };
    let parsed: Vec<_> = names.iter().map(|name| (name, parse_name(name))).collect();
    let best = parsed.iter().filter_map(|(_, p)| *p).filter(|&(v, partial)| !partial && v > current).map(|(v, _)| v).max();
    let floor = best.unwrap_or(current);
    let delete = parsed
        .iter()
        .filter(|(_, p)| match *p {
            Some((v, false)) => Some(v) != best,
            Some((v, true)) => v <= floor,
            None => true,
        })
        .map(|(name, _)| (*name).clone())
        .collect();
    (best.map(|(a, b, c)| format!("{a}.{b}.{c}")), delete)
}

// `0.4.0.zip` → (0.4.0, verified); `0.4.0.zip.part` → (0.4.0, partial).
fn parse_name(name: &str) -> Option<((u64, u64, u64), bool)> {
    let (stem, partial) = name.strip_suffix(".part").map_or((name, false), |s| (s, true));
    let core = stem.strip_suffix(".zip")?;
    // Only plain `x.y.z`, as `staged_name` writes it.
    core.split('.').all(|p| p.chars().all(|c| c.is_ascii_digit())).then_some(())?;
    Some((version(core)?, partial))
}

fn sha256_file(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut hasher = Sha256::new();
    std::io::copy(&mut std::fs::File::open(path)?, &mut hasher)?;
    Ok(hasher.finalize().to_vec())
}

// Swaps the staged build in place of the running one; `cx.restart()` then opens it. The staged
// file goes either way: one that failed to install would only fail again, and the next check refetches.
pub fn install(staged: &Staged) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let result = platform::put_in_place(&exe, &staged.path);
    std::fs::remove_file(&staged.path).ok();
    result
}

// The build a previous update set aside; Windows can't delete it while it's still running.
pub fn clean_up() {
    let Ok(exe) = std::env::current_exe() else { return };
    let target = platform::target(&exe);
    let old = aside(&target);
    std::fs::remove_dir_all(&old).or_else(|_| std::fs::remove_file(&old)).ok();
}

fn newer(latest: &str, current: &str) -> bool {
    matches!((version(latest), version(current)), (Some(latest), Some(current)) if latest > current)
}

// `v1.2.3` or `1.2.3`; pre-release and build suffixes are ignored.
fn version(tag: &str) -> Option<(u64, u64, u64)> {
    let core = tag.trim_start_matches('v').split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

// Must match the names the release workflow uploads. v0.4.0 and older fetch `dango-windows-x86_64.exe`,
// which the workflow still publishes for them.
fn asset_name(os: &str, arch: &str) -> Option<String> {
    matches!(os, "macos" | "windows").then(|| format!("dango-{os}-{arch}.zip"))
}

fn pick(release: &Release, os: &str, arch: &str) -> Option<Update> {
    let name = asset_name(os, arch)?;
    let url = |name: &str| release.assets.iter().find(|a| a.name == name).map(|a| a.browser_download_url.clone());
    let (a, b, c) = version(&release.tag_name)?;
    Some(Update { version: format!("{a}.{b}.{c}"), url: url(&name)?, checksum_url: url(&format!("{name}.sha256"))? })
}

// `checksum` is a `shasum -a 256` line: the hex digest, then the file name.
fn verify(digest: &[u8], checksum: &str) -> Result<(), String> {
    let expected = checksum.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    let actual: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    if actual != expected {
        return Err(format!("checksum mismatch: expected {expected}, got {actual}"));
    }
    Ok(())
}

fn aside(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    target.with_file_name(name)
}

// Moves the running build aside and `staged` into its place, moving it back if that fails.
// A running executable (or the bundle around it) can be renamed on both macOS and Windows.
fn swap(target: &Path, staged: &Path) -> Result<(), String> {
    let old = aside(target);
    std::fs::remove_dir_all(&old).or_else(|_| std::fs::remove_file(&old)).ok();
    std::fs::rename(target, &old).map_err(|e| format!("set aside {}: {e}", target.display()))?;
    if let Err(e) = std::fs::rename(staged, target) {
        std::fs::rename(&old, target).ok();
        return Err(format!("put in place {}: {e}", target.display()));
    }
    Ok(())
}

// Unpacks `zip` into a fresh `.dango-update` beside `target`, hands its contents to `place`, then
// removes the folder whatever happened.
fn unpacked(target: &Path, zip: &Path, unpack: impl FnOnce(&Path, &Path) -> Result<(), String>, place: impl FnOnce(&Path) -> Result<(), String>) -> Result<(), String> {
    let staging = target.with_file_name(".dango-update");
    std::fs::remove_dir_all(&staging).ok();
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let result = unpack(zip, &staging).and_then(|()| place(&staging));
    std::fs::remove_dir_all(&staging).ok();
    result
}

fn run(command: &mut std::process::Command) -> Result<(), String> {
    let status = command.status().map_err(|e| e.to_string())?;
    status.success().then_some(()).ok_or_else(|| format!("{command:?} exited with {status}"))
}

#[cfg(target_os = "windows")]
mod platform {
    use std::{
        path::{Path, PathBuf},
        process::Command,
    };

    pub fn target(exe: &Path) -> PathBuf {
        exe.to_path_buf()
    }

    // The asset is a zip holding `Dango.exe`. Windows' own bsdtar (10 1803+) reads zips; it's called by
    // full path because a GNU tar earlier on PATH (Git for Windows) can't.
    pub fn put_in_place(exe: &Path, zip: &Path) -> Result<(), String> {
        let tar = PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into())).join("System32\\tar.exe");
        super::unpacked(exe, zip, |zip, staging| super::run(Command::new(&tar).arg("-xf").arg(zip).arg("-C").arg(staging)), |staging| super::swap(exe, &staging.join("Dango.exe")))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::{
        path::{Path, PathBuf},
        process::Command,
    };

    // The whole `.app` when running from one, else the bare binary (`cargo build --release`).
    pub fn target(exe: &Path) -> PathBuf {
        bundle(exe).unwrap_or_else(|| exe.to_path_buf())
    }

    fn bundle(exe: &Path) -> Option<PathBuf> {
        let app = exe.parent()?.parent()?.parent()?;
        (app.extension()? == "app").then(|| app.to_path_buf())
    }

    // The asset is a zipped `Dango.app`; `ditto` unpacks it keeping permissions and symlinks.
    pub fn put_in_place(exe: &Path, zip: &Path) -> Result<(), String> {
        let target = target(exe);
        super::unpacked(&target, zip, |zip, staging| super::run(Command::new("ditto").arg("-x").arg("-k").arg(zip).arg(staging)), |staging| {
            let app = staging.join("Dango.app");
            let staged = if bundle(exe).is_some() { app } else { app.join("Contents/MacOS").join(exe.file_name().unwrap_or_default()) };
            // We downloaded it ourselves, so it shouldn't carry quarantine; clear it anyway so Gatekeeper
            // never blocks the relaunch. Failure here is harmless.
            Command::new("xattr").arg("-dr").arg("com.apple.quarantine").arg(&staged).status().ok();
            super::swap(&target, &staged)
        })?;
        std::fs::remove_dir_all(super::aside(&target)).or_else(|_| std::fs::remove_file(super::aside(&target))).ok();
        Ok(())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod platform {
    use std::path::{Path, PathBuf};

    pub fn target(exe: &Path) -> PathBuf {
        exe.to_path_buf()
    }

    pub fn put_in_place(_: &Path, _: &Path) -> Result<(), String> {
        Err("self-update is macOS and Windows only".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, names: &[&str]) -> Release {
        Release { tag_name: tag.into(), assets: names.iter().map(|n| Asset { name: (*n).into(), browser_download_url: format!("https://dl/{n}") }).collect() }
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dango-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn auto_download_off_checks_but_waits_to_be_asked() {
        assert_eq!(plan(None, true, false), Plan { check: true, download: true }, "the hourly check downloads by default");
        assert_eq!(plan(None, false, false), Plan { check: true, download: false });
        assert_eq!(plan(Some(Command::Check), false, true), Plan { check: true, download: false }, "Check never downloads with auto off");
        assert_eq!(plan(Some(Command::Check), true, false), Plan { check: true, download: true });
        assert_eq!(plan(Some(Command::Download), false, true), Plan { check: false, download: true }, "Download fetches what the last check found");
        assert_eq!(plan(Some(Command::Download), false, false), Plan { check: true, download: true }, "nothing found yet: check first");
    }

    #[test]
    fn last_checked_moves_only_when_a_check_finishes() {
        let (t1, t2) = (SystemTime::UNIX_EPOCH + Duration::from_secs(1), SystemTime::UNIX_EPOCH + Duration::from_secs(2));
        let mut updates = Updates::default();
        updates.apply(Status::Checking, t1);
        assert_eq!(updates.checked_at, None, "starting a check isn't finishing one");
        updates.apply(Status::Downloading { version: "0.8.0".into(), done: 1, total: Some(2) }, t1);
        assert_eq!(updates.checked_at, Some(t1));
        updates.apply(Status::Downloading { version: "0.8.0".into(), done: 2, total: Some(2) }, t2);
        assert_eq!(updates.checked_at, Some(t1), "progress isn't a check");
        updates.apply(Status::Ready(Staged::demo("0.8.0")), t2);
        assert_eq!((updates.checked_at, updates.staged().map(|s| s.version.as_str())), (Some(t1), Some("0.8.0")));
        updates.apply(Status::Checking, t2);
        updates.apply(Status::CheckFailed, t2);
        assert_eq!(updates.checked_at, Some(t2), "a failed check still counts as checked");
        assert!(updates.staged().is_none());
    }

    #[test]
    fn auto_download_defaults_on_for_new_and_old_prefs_files() {
        assert!(crate::settings::Prefs::default().auto_download_updates);
        let old: crate::settings::Prefs = serde_json::from_str("{}").unwrap();
        assert!(old.auto_download_updates, "a file from before the setting downloads as before");
    }

    #[test]
    fn progress_reports_per_percent_or_per_mib() {
        assert_eq!(progress_step(0, Some(200)), 0);
        assert_eq!(progress_step(3, Some(200)), 1);
        assert_eq!(progress_step(200, Some(200)), 100);
        assert_eq!(progress_step(5, Some(0)), 0);
        assert_eq!(progress_step(3 << 20, None), 3);
    }

    #[test]
    fn release_notes_link_the_tag() {
        assert_eq!(release_notes("0.8.0"), "https://github.com/frstycodes/dango/releases/tag/v0.8.0");
        assert_eq!(Status::Downloading { version: "0.8.0".into(), done: 0, total: None }.version(), Some("0.8.0"));
        assert_eq!(Status::UpToDate.version(), None);
    }

    #[test]
    fn compares_versions_numerically() {
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(newer("1.0.0", "0.99.99"));
        assert!(!newer("v0.1.0", "0.1.0"));
        assert!(!newer("v0.1.0", "0.2.0"));
        assert!(!newer("v0.2.0-beta.1", "0.2.0"));
        assert!(!newer("nightly", "0.1.0"));
        assert!(!newer("v1.2", "0.1.0"));
        assert!(!newer("v1.2.3.4", "0.1.0"));
    }

    #[test]
    fn picks_this_platforms_zip_and_its_checksum() {
        let r = release("v0.4.0", &["dango-macos-aarch64.zip", "dango-macos-aarch64.zip.sha256", "dango-windows-x86_64.exe", "dango-windows-x86_64.exe.sha256", "dango-windows-x86_64.zip", "dango-windows-x86_64.zip.sha256"]);
        let mac = pick(&r, "macos", "aarch64").unwrap();
        assert_eq!((mac.version.as_str(), mac.url.as_str(), mac.checksum_url.as_str()), ("0.4.0", "https://dl/dango-macos-aarch64.zip", "https://dl/dango-macos-aarch64.zip.sha256"));
        assert_eq!(pick(&r, "windows", "x86_64").unwrap().url, "https://dl/dango-windows-x86_64.zip", "the compressed build, not the bare .exe");
        assert!(pick(&r, "macos", "x86_64").is_none());
        assert!(pick(&r, "linux", "x86_64").is_none());
        assert!(pick(&release("v0.4.0", &["dango-windows-x86_64.zip"]), "windows", "x86_64").is_none(), "no checksum, no update");
    }

    #[test]
    fn rejects_a_checksum_mismatch() {
        let sum = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let digest = Sha256::digest(b"hello");
        assert!(verify(&digest, &format!("{sum}  dango-windows-x86_64.zip\n")).is_ok());
        assert!(verify(&digest, &sum.to_uppercase()).is_ok());
        assert!(verify(&Sha256::digest(b"hellO"), sum).is_err());
        assert!(verify(&digest, "").is_err());
    }

    #[test]
    fn stages_only_the_newest_verified_newer_version() {
        let (best, delete) = survey(&names(&["0.4.0.zip", "0.5.0.zip", "0.3.0.zip", "0.2.0.zip"]), "0.3.0");
        assert_eq!(best.as_deref(), Some("0.5.0"));
        assert_eq!(delete, names(&["0.4.0.zip", "0.3.0.zip", "0.2.0.zip"]), "older and current builds go");
        assert_eq!(survey(&names(&["0.3.0.zip"]), "0.3.0"), (None, names(&["0.3.0.zip"])), "the running version is never reinstalled");
        assert_eq!(survey(&[], "0.3.0"), (None, vec![]));
    }

    #[test]
    fn keeps_an_interrupted_download_newer_than_what_is_staged() {
        let (best, delete) = survey(&names(&["0.4.0.zip", "0.4.0.zip.part", "0.5.0.zip.part", "0.2.0.zip.part"]), "0.3.0");
        assert_eq!(best.as_deref(), Some("0.4.0"), "a .part is never staged");
        assert_eq!(delete, names(&["0.4.0.zip.part", "0.2.0.zip.part"]), "0.5.0's download resumes next check");
        assert_eq!(survey(&names(&["0.5.0.zip.part"]), "0.3.0"), (None, vec![]));
    }

    #[test]
    fn clears_out_anything_that_isnt_a_staged_update() {
        let (_, delete) = survey(&names(&["notes.txt", "v0.5.0.zip", "0.5.0-beta.zip", "0.5.zip", "0.5.0.exe", ".DS_Store"]), "0.3.0");
        assert_eq!(delete.len(), 6);
    }

    #[test]
    fn tidy_finds_the_staged_update_and_deletes_the_rest() {
        let dir = temp_dir("tidy");
        for name in ["0.2.0.zip", "0.4.0.zip", "0.5.0.zip.part", "junk"] {
            std::fs::write(dir.join(name), name).unwrap();
        }
        let staged = tidy(&dir, "0.3.0").unwrap();
        assert_eq!((staged.version.as_str(), staged.path.clone()), ("0.4.0", dir.join("0.4.0.zip")));
        let mut left: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        left.sort();
        assert_eq!(left, ["0.4.0.zip", "0.5.0.zip.part"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn swap_replaces_the_target_and_sets_the_old_one_aside() {
        let dir = temp_dir("swap");
        let (target, staged) = (dir.join("dango"), dir.join("dango.new"));
        std::fs::write(&target, "old").unwrap();
        std::fs::write(&staged, "new").unwrap();
        swap(&target, &staged).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(aside(&target)).unwrap(), "old");
        assert!(swap(&target, &dir.join("missing")).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new", "a failed swap puts the running build back");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // A tiny HTTP server for `stage`: serves `body` (honouring `Range`) and its checksum, and cuts the
    // first download off after `cut` bytes, as a dropped connection would.
    async fn serve(body: Vec<u8>, checksum: String, cut: usize) -> String {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let mut first = true;
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = vec![0; 4096];
                let n = socket.read(&mut request).await.unwrap();
                let request = String::from_utf8_lossy(&request[..n]).to_lowercase();
                let reply = if request.starts_with("get /sum") {
                    format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{checksum}", checksum.len()).into_bytes()
                } else if let Some(from) = request.split("range: bytes=").nth(1).and_then(|r| r.split('-').next()).and_then(|f| f.parse::<usize>().ok()) {
                    let rest = &body[from.min(body.len())..];
                    [format!("HTTP/1.1 206 Partial Content\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", rest.len()).into_bytes(), rest.to_vec()].concat()
                } else if std::mem::take(&mut first) {
                    // Promises the whole body, sends part of it, hangs up.
                    [format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len()).into_bytes(), body[..cut].to_vec()].concat()
                } else {
                    [format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len()).into_bytes(), body.clone()].concat()
                };
                socket.write_all(&reply).await.ok();
                socket.shutdown().await.ok();
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn an_interrupted_download_resumes_and_stages_once_verified() {
        let dir = temp_dir("stage");
        let body: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let sum: String = Sha256::digest(&body).iter().map(|b| format!("{b:02x}")).collect();
        let base = serve(body.clone(), format!("{sum}  dango-macos-aarch64.zip\n"), 70_000).await;
        let update = Update { version: "0.4.0".into(), url: format!("{base}/zip"), checksum_url: format!("{base}/sum") };
        let client = reqwest::Client::new();

        assert!(stage(&dir, &update, &client, "0.3.0", |_, _| {}).await.is_err(), "the cut-off download fails");
        // Not quite all 70,000: hyper drops what it buffered when the connection ends early.
        let kept = std::fs::metadata(dir.join("0.4.0.zip.part")).unwrap().len();
        assert!((1..=70_000).contains(&kept), "what arrived is kept: {kept}");
        assert!(tidy(&dir, "0.3.0").is_none(), "a .part is not an update");

        let mut seen = Vec::new();
        let staged = stage(&dir, &update, &client, "0.3.0", |done, total| seen.push((done, total))).await.unwrap();
        assert_eq!(staged.version, "0.4.0");
        let whole = body.len() as u64;
        assert_eq!(seen.last(), Some(&(whole, Some(whole))), "a resumed download counts from the .part");
        assert!(seen.first().is_some_and(|&(done, _)| done > kept), "starts past what was kept: {seen:?}");
        assert_eq!(std::fs::read(&staged.path).unwrap(), body, "resumed where the .part ended");
        assert!(!dir.join("0.4.0.zip.part").exists());
        assert_eq!(stage(&dir, &update, &client, "0.3.0", |_, _| {}).await.unwrap(), staged, "already staged: nothing to fetch");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn a_corrupt_download_is_dropped_so_the_next_check_starts_over() {
        let dir = temp_dir("corrupt");
        let base = serve(b"not the build".to_vec(), format!("{}  x\n", "0".repeat(64)), 13).await;
        let update = Update { version: "0.4.0".into(), url: format!("{base}/zip"), checksum_url: format!("{base}/sum") };
        let error = stage(&dir, &update, &reqwest::Client::new(), "0.3.0", |_, _| {}).await.unwrap_err();
        assert!(error.contains("checksum mismatch"), "{error}");
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none(), "nothing left behind");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
