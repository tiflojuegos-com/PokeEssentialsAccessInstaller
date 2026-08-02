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
    let text = text.trim_start_matches('\u{feff}');
    serde_json::from_str(text).ok()
}

pub fn is_installed(game_dir: &Path) -> bool {
    installed_file(game_dir).exists()
}

/// The game-specific profile the folder already carries, if any. Both
/// installers seal it and until now nobody read it back: a game installed by
/// `install.ps1` and then added to the launcher was offered the generic profile
/// as the default answer, so accepting the dialog swapped that game's own
/// screens for the generic layer without a word. `generic` is not an answer
/// here, it is what the dialog already defaults to.
pub fn specific_profile(game_dir: &Path) -> Option<String> {
    read(game_dir)
        .map(|i| i.profile.trim().to_string())
        .filter(|p| !p.is_empty() && p != "generic")
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModVersion {
    pub version: String,
    #[serde(default)]
    pub min_launcher: String,
    #[serde(default)]
    pub launcher: String,
}

/// Reads the mod's remote version.json, tolerating a UTF-8 BOM: losing it here
/// would silently skip the launcher gate and seal the game as version 0.0.0.
pub fn parse_version_json(text: &str) -> Option<ModVersion> {
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
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

    #[test]
    fn parse_version_tolerates_utf8_bom() {
        let mv = parse_version_json("\u{feff}{\"version\":\"0.8.1\",\"min_launcher\":\"0.1.0\"}")
            .expect("el BOM no debe impedir el parseo");
        assert_eq!(mv.version, "0.8.1");
        assert_eq!(mv.min_launcher, "0.1.0");
    }

    fn seal(dir: &Path, json: &str) {
        let path = installed_file(dir);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, json).unwrap();
    }

    #[test]
    fn the_sealed_profile_is_readable_for_the_next_install() {
        let dir = tempfile::tempdir().unwrap();
        seal(dir.path(), r#"{"mod_version":"0.1.3","profile":"royal","profile_mode":"specific"}"#);
        assert_eq!(specific_profile(dir.path()).unwrap(), "royal");
    }

    #[test]
    fn a_generic_or_missing_seal_preselects_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(specific_profile(dir.path()).is_none());
        seal(dir.path(), r#"{"mod_version":"0.1.3","profile":"generic","profile_mode":"generic"}"#);
        assert!(specific_profile(dir.path()).is_none());
        seal(dir.path(), r#"{"mod_version":"0.1.3","profile":"  ","profile_mode":""}"#);
        assert!(specific_profile(dir.path()).is_none());
    }

    #[test]
    fn read_tolerates_utf8_bom() {
        let dir = tempfile::tempdir().unwrap();
        let path = installed_file(dir.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(
            br#"{"mod_version":"0.8.1","profile":"reminiscencia","profile_mode":"specific"}"#,
        );
        fs::write(&path, bytes).unwrap();
        let inst = read(dir.path()).expect("el BOM no debe impedir el parseo");
        assert_eq!(inst.mod_version, "0.8.1");
        assert_eq!(inst.profile, "reminiscencia");
    }
}
