// Launch-time self-update from GitHub Releases: the splash checks for a newer version, downloads this
// platform's asset, verifies its SHA-256, swaps it in and relaunches. Any failure carries on into the app.
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use futures::channel::mpsc::UnboundedSender;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const LATEST: &str = "https://api.github.com/repos/frstycodes/dango/releases/latest";
const CHECK_TIMEOUT: Duration = Duration::from_secs(4);
// A download that sends nothing for this long is given up; a slow but moving one is not.
const STALL_TIMEOUT: Duration = Duration::from_secs(15);

pub fn enabled() -> bool {
    !cfg!(debug_assertions) && std::env::var_os("CHORD_DEMO").is_none()
}

pub struct Update {
    pub version: String,
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

fn http() -> reqwest::Client {
    // GitHub's API rejects requests without a User-Agent.
    reqwest::Client::builder().user_agent(concat!("Dango/", env!("CARGO_PKG_VERSION"))).connect_timeout(CHECK_TIMEOUT).read_timeout(STALL_TIMEOUT).build().unwrap_or_default()
}

pub async fn check() -> Option<Update> {
    let request = http().get(LATEST).header("Accept", "application/vnd.github+json").timeout(CHECK_TIMEOUT);
    let release: Release = request.send().await.ok()?.error_for_status().ok()?.json().await.ok()?;
    if !newer(&release.tag_name, env!("CARGO_PKG_VERSION")) {
        return None;
    }
    pick(&release, std::env::consts::OS, std::env::consts::ARCH)
}

// Downloads, verifies and installs `update`, reporting progress (0..=1) as it goes. Returns the
// executable to relaunch.
pub async fn install(update: Update, progress: UnboundedSender<f32>) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let client = http();
    let checksum = client.get(&update.checksum_url).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?.text().await.map_err(|e| e.to_string())?;
    let mut response = client.get(&update.url).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    let total = response.content_length().unwrap_or(0);
    let mut bytes = Vec::with_capacity(total as usize);
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        bytes.extend_from_slice(&chunk);
        if total > 0 {
            progress.unbounded_send(bytes.len() as f32 / total as f32).ok();
        }
    }
    verify(&bytes, &checksum)?;
    tokio::task::spawn_blocking(move || platform::put_in_place(&exe, &bytes).map(|()| exe)).await.map_err(|e| e.to_string())?
}

// Starts the new build with this run's arguments and ends this one.
pub fn relaunch(exe: &Path) -> ! {
    if let Err(e) = std::process::Command::new(exe).args(std::env::args_os().skip(1)).spawn() {
        eprintln!("[updater] relaunch failed: {e}");
    }
    std::process::exit(0)
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

// Must match the names the release workflow uploads.
fn asset_name(os: &str, arch: &str) -> Option<String> {
    let ext = match os {
        "macos" => "zip",
        "windows" => "exe",
        _ => return None,
    };
    Some(format!("dango-{os}-{arch}.{ext}"))
}

fn pick(release: &Release, os: &str, arch: &str) -> Option<Update> {
    let name = asset_name(os, arch)?;
    let url = |name: &str| release.assets.iter().find(|a| a.name == name).map(|a| a.browser_download_url.clone());
    Some(Update { version: release.tag_name.trim_start_matches('v').to_string(), url: url(&name)?, checksum_url: url(&format!("{name}.sha256"))? })
}

// `checksum` is a `shasum -a 256` line: the hex digest, then the file name.
fn verify(bytes: &[u8], checksum: &str) -> Result<(), String> {
    let expected = checksum.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    let actual: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
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

#[cfg(target_os = "windows")]
mod platform {
    use std::path::{Path, PathBuf};

    pub fn target(exe: &Path) -> PathBuf {
        exe.to_path_buf()
    }

    pub fn put_in_place(exe: &Path, bytes: &[u8]) -> Result<(), String> {
        let staged = exe.with_extension("exe.new");
        std::fs::write(&staged, bytes).map_err(|e| e.to_string())?;
        super::swap(exe, &staged).inspect_err(|_| {
            std::fs::remove_file(&staged).ok();
        })
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
    pub fn put_in_place(exe: &Path, bytes: &[u8]) -> Result<(), String> {
        let target = target(exe);
        let staging = target.with_file_name(".dango-update");
        std::fs::remove_dir_all(&staging).ok();
        std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
        let result = unpack_and_swap(exe, &target, &staging, bytes);
        std::fs::remove_dir_all(&staging).ok();
        result?;
        std::fs::remove_dir_all(super::aside(&target)).or_else(|_| std::fs::remove_file(super::aside(&target))).ok();
        Ok(())
    }

    fn unpack_and_swap(exe: &Path, target: &Path, staging: &Path, bytes: &[u8]) -> Result<(), String> {
        let zip = staging.join("update.zip");
        std::fs::write(&zip, bytes).map_err(|e| e.to_string())?;
        run(Command::new("ditto").arg("-x").arg("-k").arg(&zip).arg(staging))?;
        let app = staging.join("Dango.app");
        let staged = if bundle(exe).is_some() { app } else { app.join("Contents/MacOS").join(exe.file_name().unwrap_or_default()) };
        // We downloaded it ourselves, so it shouldn't carry quarantine; clear it anyway so Gatekeeper
        // never blocks the relaunch. Failure here is harmless.
        Command::new("xattr").arg("-dr").arg("com.apple.quarantine").arg(&staged).status().ok();
        super::swap(target, &staged)
    }

    fn run(command: &mut Command) -> Result<(), String> {
        let status = command.status().map_err(|e| e.to_string())?;
        status.success().then_some(()).ok_or_else(|| format!("{command:?} exited with {status}"))
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod platform {
    use std::path::{Path, PathBuf};

    pub fn target(exe: &Path) -> PathBuf {
        exe.to_path_buf()
    }

    pub fn put_in_place(_: &Path, _: &[u8]) -> Result<(), String> {
        Err("self-update is macOS and Windows only".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, names: &[&str]) -> Release {
        Release { tag_name: tag.into(), assets: names.iter().map(|n| Asset { name: (*n).into(), browser_download_url: format!("https://dl/{n}") }).collect() }
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
    fn picks_this_platforms_asset_and_its_checksum() {
        let r = release("v0.2.0", &["dango-macos-aarch64.zip", "dango-macos-aarch64.zip.sha256", "dango-windows-x86_64.exe", "dango-windows-x86_64.exe.sha256"]);
        let mac = pick(&r, "macos", "aarch64").unwrap();
        assert_eq!((mac.version.as_str(), mac.url.as_str(), mac.checksum_url.as_str()), ("0.2.0", "https://dl/dango-macos-aarch64.zip", "https://dl/dango-macos-aarch64.zip.sha256"));
        assert_eq!(pick(&r, "windows", "x86_64").unwrap().url, "https://dl/dango-windows-x86_64.exe");
        assert!(pick(&r, "macos", "x86_64").is_none());
        assert!(pick(&r, "linux", "x86_64").is_none());
        assert!(pick(&release("v0.2.0", &["dango-windows-x86_64.exe"]), "windows", "x86_64").is_none(), "no checksum, no update");
    }

    #[test]
    fn rejects_a_checksum_mismatch() {
        let sum = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert!(verify(b"hello", &format!("{sum}  dango-windows-x86_64.exe\n")).is_ok());
        assert!(verify(b"hello", &sum.to_uppercase()).is_ok());
        assert!(verify(b"hellO", sum).is_err());
        assert!(verify(b"hello", "").is_err());
    }

    #[test]
    fn swap_replaces_the_target_and_sets_the_old_one_aside() {
        let dir = std::env::temp_dir().join(format!("dango-swap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
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
}
