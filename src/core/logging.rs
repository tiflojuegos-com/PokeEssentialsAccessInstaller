use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::paths::launcher_config_dir;

pub fn log_file() -> PathBuf {
    launcher_config_dir().join("launcher.log")
}

pub fn append(line: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = std::fs::create_dir_all(launcher_config_dir());
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log_file()) {
        let _ = writeln!(f, "[{}] {}", ts, line);
    }
}
