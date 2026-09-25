use std::path::{Component, Path, PathBuf};
use std::process::Command;

use super::config::GameEntry;
use super::detect;

/// An executable is stored as a filename, never as an arbitrary path.
pub fn executable_path(entry: &GameEntry) -> Result<PathBuf, &'static str> {
    let name = Path::new(&entry.executable);
    if name.components().count() != 1 || !matches!(name.components().next(), Some(Component::Normal(_)))
        || !entry.executable.to_ascii_lowercase().ends_with(".exe")
        || entry.executable.contains('/') || entry.executable.contains('\\')
    {
        return Err("invalid_executable");
    }
    Ok(Path::new(&entry.path).join(name))
}

/// Reports every bad field so the edit dialog can describe what to correct.
pub fn validate(entry: &GameEntry) -> Vec<&'static str> {
    let mut errors = Vec::new();
    if entry.name.trim().is_empty() {
        errors.push("invalid_name");
    }
    if !Path::new(&entry.path).is_dir() {
        errors.push("invalid_game_path");
    }
    if entry.profile.trim().is_empty() {
        errors.push("invalid_profile");
    }
    if !executable_path(entry).is_ok_and(|path| path.is_file()) {
        errors.push("invalid_executable");
    }
    errors
}

pub fn detect_for_entry(entry: &GameEntry, display: Option<&str>, known_exes: &[String]) -> Option<String> {
    detect::launch_exe(Path::new(&entry.path), &entry.profile, display, known_exes)
}

/// Starts the game detached from the GUI with the game folder as working directory.
pub fn launch(entry: &GameEntry) -> Result<(), String> {
    if !Path::new(&entry.path).is_dir() {
        return Err("invalid_game_path".into());
    }
    let exe = executable_path(entry).map_err(str::to_string)?;
    if !exe.is_file() {
        return Err("invalid_executable".into());
    }
    Command::new(&exe)
        .current_dir(&entry.path)
        .spawn()
        .map(|_| ())
        .map_err(|e| crate::i18n::err_key("launch_failed", &e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(dir: &Path) -> GameEntry {
        GameEntry { path: dir.to_string_lossy().into_owned(), name: "My game".into(), executable: "Game.exe".into(), profile: "generic".into(), profile_mode: "generic".into() }
    }

    #[test]
    fn validates_all_fields_and_blocks_escaping_the_game_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = entry(dir.path());
        std::fs::write(dir.path().join("Game.exe"), b"test").unwrap();
        assert!(validate(&e).is_empty());
        for bad in ["../Game.exe", "sub\\Game.exe", "C:\\Game.exe", "", "not.exe.txt"] {
            e.executable = bad.into();
            assert!(validate(&e).contains(&"invalid_executable"), "{bad}");
        }
        e.executable = "Game.exe".into();
        e.name = " ".into();
        e.profile.clear();
        e.path = dir.path().join("missing").to_string_lossy().into_owned();
        assert_eq!(validate(&e), vec!["invalid_name", "invalid_game_path", "invalid_profile", "invalid_executable"]);
    }

    #[test]
    fn missing_executable_does_not_launch() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(launch(&entry(dir.path())).unwrap_err(), "invalid_executable");
    }

    #[test]
    fn failed_process_start_is_reported_to_the_ui() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Game.exe"), b"not a Windows executable").unwrap();
        let error = launch(&entry(dir.path())).unwrap_err();
        assert!(error.starts_with("launch_failed\u{1}"), "{error}");
    }

    #[test]
    fn entry_detects_a_root_executable_for_legacy_records() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("setup.exe"), b"x").unwrap();
        std::fs::write(dir.path().join("Royal.exe"), b"x").unwrap();
        let mut e = entry(dir.path());
        e.profile = "royal".into();
        e.executable.clear();
        assert_eq!(detect_for_entry(&e, None, &[]).as_deref(), Some("Royal.exe"));
    }
}
