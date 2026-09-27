use std::collections::BTreeMap;
use std::path::Path;

use crate::i18n::{err_key, I18n};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameStatus {
    NotInstalled,
    UpToDate,
    UpdateAvailable,
    Outdated,
    Unknown,
}

pub fn entry_key(entry: &super::config::GameEntry, available: &str) -> &'static str {
    let dir = std::path::Path::new(&entry.path);
    if !dir.is_dir() {
        return "status_missing";
    }
    let installed = super::installed::read(dir);
    status_key(compute(installed.as_ref().map(|i| i.mod_version.as_str()), available))
}

pub fn compute(installed_version: Option<&str>, available_version: &str) -> GameStatus {
    match installed_version {
        None => GameStatus::NotInstalled,
        Some(_) if available_version.trim().is_empty() => GameStatus::Unknown,
        Some(v) => {
            if semver_parse(v) == semver_parse(available_version) {
                GameStatus::UpToDate
            } else if newer(available_version, v) {
                GameStatus::UpdateAvailable
            } else {
                GameStatus::Outdated
            }
        }
    }
}

fn newer(a: &str, b: &str) -> bool {
    match (semver_parse(a), semver_parse(b)) {
        (Some(va), Some(vb)) => va > vb,
        _ => a != b,
    }
}

fn semver_parse(s: &str) -> Option<Vec<u64>> {
    let s = s.trim_start_matches('v');
    let parts: Vec<u64> = s.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts)
    }
}

pub fn status_key(s: GameStatus) -> &'static str {
    match s {
        GameStatus::NotInstalled => "status_notinstalled",
        GameStatus::UpToDate => "status_uptodate",
        GameStatus::UpdateAvailable => "status_update",
        GameStatus::Outdated => "status_outdated",
        GameStatus::Unknown => "status_unknown",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    NotLoaded,
    ExeMissing,
    FilesMissing(usize),
}

impl Fault {
    pub fn message(&self) -> String {
        match self {
            Fault::NotLoaded => "health_not_loaded".to_string(),
            Fault::ExeMissing => "health_exe_missing".to_string(),
            Fault::FilesMissing(n) => err_key("health_files_missing", &n.to_string()),
        }
    }
}

pub fn faults(game_dir: &Path) -> Vec<Fault> {
    let Some(sealed) = super::installed::read(game_dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let json = std::fs::read(super::mkxp::mkxp_json(game_dir)).unwrap_or_default();
    if !super::mkxp::is_registered(&super::detect::decode_text(&json)) {
        found.push(Fault::NotLoaded);
    }
    if super::convert::missing_accessible_exe(game_dir).is_some() {
        found.push(Fault::ExeMissing);
    }
    let files: BTreeMap<String, String> =
        sealed.files.into_iter().filter(|(rel, _)| !super::install::is_user_data(rel)).collect();
    let missing = files.len() - super::apply::intact_files(game_dir, &files).len();
    if missing > 0 {
        found.push(Fault::FilesMissing(missing));
    }
    found
}

pub fn repair_note(i18n: &I18n, key: &str, faults: &[Fault]) -> Option<String> {
    let said: Vec<String> = faults.iter().map(|fault| i18n.t_err(&fault.message())).collect();
    (!said.is_empty()).then(|| i18n.tf(key, &said.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listed_game_reads_as_gone_without_the_mod_or_behind_the_available_version() {
        let root = tempfile::tempdir().unwrap();
        let entry = |dir: &std::path::Path| super::super::config::GameEntry {
            path: dir.to_string_lossy().into_owned(),
            name: String::new(),
            executable: String::new(),
            profile: "generic".into(),
            profile_mode: "generic".into(),
        };
        assert_eq!(entry_key(&entry(&root.path().join("movida")), "0.6.0"), "status_missing");
        assert_eq!(entry_key(&entry(root.path()), "0.6.0"), "status_notinstalled");
        let files = std::collections::BTreeMap::new();
        super::super::apply::seal_installed(root.path(), "0.5.0", "generic", "generic", "x86", "now", files).unwrap();
        assert_eq!(entry_key(&entry(root.path()), "0.6.0"), "status_update");
        assert_eq!(entry_key(&entry(root.path()), ""), "status_unknown");
    }

    #[test]
    fn not_installed() {
        assert_eq!(compute(None, "0.1.0"), GameStatus::NotInstalled);
    }

    #[test]
    fn up_to_date() {
        assert_eq!(compute(Some("0.1.0"), "0.1.0"), GameStatus::UpToDate);
    }

    #[test]
    fn update_available() {
        assert_eq!(compute(Some("0.1.0"), "0.2.0"), GameStatus::UpdateAvailable);
        assert_eq!(compute(Some("0.1.0"), "0.1.1"), GameStatus::UpdateAvailable);
        assert_eq!(compute(Some("0.9.0"), "0.10.0"), GameStatus::UpdateAvailable);
    }

    #[test]
    fn outdated_when_installed_is_newer() {
        assert_eq!(compute(Some("0.3.0"), "0.2.0"), GameStatus::Outdated);
    }

    #[test]
    fn tolerates_v_prefix() {
        assert_eq!(compute(Some("v0.1.0"), "0.1.0"), GameStatus::UpToDate);
    }

    #[test]
    fn empty_available_is_unknown_not_outdated() {
        assert_eq!(compute(Some("0.1.0"), ""), GameStatus::Unknown);
        assert_eq!(compute(Some("v0.3.2"), "  "), GameStatus::Unknown);
    }

    #[test]
    fn empty_available_still_not_installed_without_mod() {
        assert_eq!(compute(None, ""), GameStatus::NotInstalled);
    }

    fn sound_install(root: &Path) -> std::path::PathBuf {
        let dir = root.join("Juego");
        std::fs::create_dir_all(&dir).unwrap();
        let mut files = BTreeMap::new();
        for (rel, body) in [("core/a.rb", "a"), ("boot.rb", "b"), ("lib/voz.dll", "dll")] {
            super::super::apply::write_dest_file(&dir, rel, body.as_bytes()).unwrap();
            files.insert(rel.to_string(), super::super::install::git_blob_sha1(body.as_bytes()));
        }
        files.insert("data/settings.ini".to_string(), "sellado por un instalador antiguo".to_string());
        super::super::apply::seal_installed(&dir, "0.6.0", "generic", "generic", "x86", "now", files).unwrap();
        super::super::mkxp::register(&dir, None).unwrap();
        dir
    }

    #[test]
    fn a_sound_install_has_nothing_to_repair_and_a_game_without_the_mod_is_not_checked() {
        let root = tempfile::tempdir().unwrap();
        assert!(faults(&sound_install(root.path())).is_empty());
        let bare = root.path().join("Sin mod");
        std::fs::create_dir_all(bare.join("accessibility").join("data")).unwrap();
        assert!(faults(&bare).is_empty());
        assert!(faults(&root.path().join("movida")).is_empty());
    }

    #[test]
    fn a_game_that_no_longer_loads_the_mod_is_told() {
        let root = tempfile::tempdir().unwrap();
        let dir = sound_install(root.path());
        std::fs::write(dir.join("mkxp.json"), "{\n  // \"preloadScript\": [\"accessibility/preload_access.rb\"]\n}").unwrap();
        assert_eq!(faults(&dir), vec![Fault::NotLoaded]);
        std::fs::remove_file(dir.join("mkxp.json")).unwrap();
        assert_eq!(faults(&dir), vec![Fault::NotLoaded]);
    }

    #[test]
    fn a_converted_game_without_its_accessible_exe_is_told() {
        let root = tempfile::tempdir().unwrap();
        let dir = sound_install(root.path());
        let record = r#"{"engine":"e1","exe":"Game (PokeAccess).exe","installed":"x"}"#;
        std::fs::write(dir.join("accessibility").join("data").join("engine.json"), record).unwrap();
        assert_eq!(faults(&dir), vec![Fault::ExeMissing]);
        std::fs::write(dir.join("Game (PokeAccess).exe"), b"MZ").unwrap();
        assert!(faults(&dir).is_empty());
    }

    #[test]
    fn mod_files_gone_or_changed_since_the_seal_are_counted_and_the_players_data_is_not() {
        let root = tempfile::tempdir().unwrap();
        let dir = sound_install(root.path());
        let acc = dir.join("accessibility");
        std::fs::remove_file(acc.join("lib").join("voz.dll")).unwrap();
        std::fs::write(acc.join("boot.rb"), "cambiado").unwrap();
        assert_eq!(faults(&dir), vec![Fault::FilesMissing(2)]);
        std::fs::remove_file(dir.join("mkxp.json")).unwrap();
        let all = faults(&dir);
        assert_eq!(all, vec![Fault::NotLoaded, Fault::FilesMissing(2)]);
        let es = I18n::new("es");
        let note = repair_note(&es, "health_repair", &all).unwrap();
        assert!(note.contains(&es.t("health_not_loaded")) && note.contains(": 2") && note.contains("Instalar o actualizar"), "{note}");
        let console = repair_note(&es, "cli_health_repair", &all).unwrap();
        assert!(console.contains("install") && !console.contains("health_"), "{console}");
        assert_eq!(repair_note(&es, "health_repair", &[]), None);
    }
}
