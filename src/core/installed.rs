use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::paths::installed_file;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Installed {
    pub mod_version: String,
    pub profile: String,
    pub profile_mode: String,
    #[serde(default)]
    pub voice_arch: String,
    #[serde(default)]
    pub installed_at: String,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}

pub fn read(game_dir: &Path) -> Option<Installed> {
    let path = installed_file(game_dir);
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn is_installed(game_dir: &Path) -> bool {
    installed_file(game_dir).exists()
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModVersion {
    pub version: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub min_launcher: String,
}

pub fn parse_version_json(text: &str) -> Option<ModVersion> {
    serde_json::from_str(text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_installed() {
        let json = r#"{
            "mod_version": "0.8.1",
            "profile": "reminiscencia",
            "profile_mode": "specific",
            "voice_arch": "x64",
            "installed_at": "2026-01-01T00:00:00",
            "files": { "lang/en.txt": "abc123", "core/nav/locator.rb": "def456" }
        }"#;
        let inst: Installed = serde_json::from_str(json).unwrap();
        assert_eq!(inst.mod_version, "0.8.1");
        assert_eq!(inst.profile, "reminiscencia");
        assert_eq!(inst.profile_mode, "specific");
        assert_eq!(inst.files.len(), 2);
        assert_eq!(inst.files.get("lang/en.txt").unwrap(), "abc123");
    }

    #[test]
    fn parse_version() {
        let mv = parse_version_json(r#"{"version":"0.8.1","min_launcher":"0.1.0"}"#).unwrap();
        assert_eq!(mv.version, "0.8.1");
        assert_eq!(mv.min_launcher, "0.1.0");
    }
}
