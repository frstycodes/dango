// Crash logs: a panic on any thread writes `crash-<unix seconds>.log` (message, where, thread, version,
// OS, backtrace) under the data dir's `crashes`, newest kept, oldest dropped past `KEEP`. Settings ›
// Advanced lists them while Developer mode is on.
// GAP: only Rust panics; a native crash (a signal in a driver or AppKit) leaves only macOS's own report.
use std::{
    io::Write as _,
    path::{Path, PathBuf},
};

const KEEP: usize = 20;

pub fn install() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(dir) = dir() {
            let _ = write(&dir, info);
        }
        previous(info);
    }));
}

pub fn dir() -> Option<PathBuf> {
    Some(crate::data_dir()?.join("crashes"))
}

/// Newest first.
pub fn logs() -> Vec<PathBuf> {
    dir().map(|dir| logs_in(&dir)).unwrap_or_default()
}

fn logs_in(dir: &Path) -> Vec<PathBuf> {
    let mut logs: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| is_log(p)).collect();
    // The names carry the time, so name order is age order.
    logs.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    logs
}

// A saved crash as Settings lists it: when, and the panic's first line.
pub struct Log {
    pub path: PathBuf,
    pub unix: i64,
    pub summary: String,
}

/// Newest first, read back for display.
pub fn saved() -> Vec<Log> {
    logs().into_iter().map(|path| read(&path)).collect()
}

fn read(path: &Path) -> Log {
    let unix = path.file_stem().and_then(|n| n.to_str()).and_then(|n| n.strip_prefix("crash-")).and_then(|n| n.parse().ok()).unwrap_or(0);
    let text = std::fs::read_to_string(path).unwrap_or_default();
    // The message is the line after "… panicked at …".
    let summary = text.lines().skip_while(|l| !l.contains(" panicked at ")).nth(1).unwrap_or("(no message)").trim().to_owned();
    Log { path: path.to_owned(), unix, summary }
}

fn is_log(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("crash-") && n.ends_with(".log"))
}

fn write(dir: &Path, info: &std::panic::PanicHookInfo) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let mut file = std::fs::File::create(dir.join(format!("crash-{:010}.log", now.as_secs())))?;
    writeln!(file, "{}", report(info, now.as_secs()))?;
    for old in logs_in(dir).into_iter().skip(KEEP) {
        let _ = std::fs::remove_file(old);
    }
    Ok(())
}

fn report(info: &std::panic::PanicHookInfo, unix: u64) -> String {
    let message = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_else(|| "(no message)".into());
    let at = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
    let thread = std::thread::current().name().unwrap_or("unnamed").to_owned();
    let backtrace = std::backtrace::Backtrace::force_capture();
    format!(
        "Waffle {} ({} {})\nunix time {unix}\nthread '{thread}' panicked at {at}:\n{message}\n\n{backtrace}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_writes_a_log_and_old_ones_are_listed_newest_first() {
        let dir = std::env::temp_dir().join(format!("waffle-crash-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("crash-0000000001.log"), "old").unwrap();
        std::fs::write(dir.join("notes.txt"), "not a log").unwrap();
        let hook = std::panic::take_hook();
        let (target, me) = (dir.clone(), std::thread::current().id());
        // The hook is process-wide: other tests' panics (caught on their threads) must not land here.
        std::panic::set_hook(Box::new(move |info| {
            if std::thread::current().id() == me {
                _ = write(&target, info);
            }
        }));
        let _ = std::panic::catch_unwind(|| panic!("boom"));
        std::panic::set_hook(hook);
        let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| is_log(Path::new(n))).collect();
        names.sort();
        assert_eq!(names.len(), 2, "{names:?}");
        let newest = std::fs::read_to_string(dir.join(names.last().unwrap())).unwrap();
        assert!(newest.contains("panicked at") && newest.contains("boom"), "{newest}");
        assert_eq!(read(&dir.join(names.last().unwrap())).summary, "boom");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
