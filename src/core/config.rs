use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::paths::launcher_config_file;
use crate::i18n::err_key;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameEntry {
    pub path: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub executable: String,
    #[serde(default)]
    pub profile: String,
    #[serde(default)]
    pub profile_mode: String,
}

impl GameEntry {
    pub fn profile_changed(&self, other: &GameEntry) -> bool {
        self.profile != other.profile || self.profile_mode != other.profile_mode
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub games: Vec<GameEntry>,
    #[serde(default)]
    pub last_mod_version_seen: String,
    #[serde(default)]
    pub last_notified_tag: String,
    #[serde(skip)]
    base: Option<Box<Config>>,
}

pub fn map_locale(loc: &str) -> String {
    let l = loc.to_lowercase();
    for code in crate::i18n::LANGS {
        if l.starts_with(code) {
            return code.to_string();
        }
    }
    "en".to_string()
}

pub fn detect_system_language() -> String {
    map_locale(&sys_locale::get_locale().unwrap_or_default())
}

impl Config {
    pub fn resolve_language(&self) -> String {
        self.language.clone().unwrap_or_else(detect_system_language)
    }

    pub fn set_language(&mut self, lang: &str) {
        self.language = Some(lang.to_string());
    }

    pub fn load() -> Config {
        Config::load_from(&launcher_config_file())
    }

    fn load_from(path: &Path) -> Config {
        let mut cfg = match fs::read_to_string(path) {
            Err(_) => Config::default(),
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| {
                let bak = path.with_extension("json.bak");
                if bak.exists() {
                    let _ = fs::remove_file(path);
                } else {
                    let _ = fs::rename(path, &bak);
                }
                Config::default()
            }),
        };
        cfg.base = cfg.snapshot();
        cfg
    }

    pub fn save(&mut self) -> Result<(), String> {
        self.save_to(&launcher_config_file())
    }

    pub fn commit<T>(&mut self, change: impl FnOnce(&mut Config) -> Result<T, String>) -> Result<T, String> {
        self.commit_to(&launcher_config_file(), change)
    }

    fn commit_to<T>(&mut self, path: &Path, change: impl FnOnce(&mut Config) -> Result<T, String>) -> Result<T, String> {
        let previous = self.clone();
        let done = change(self).and_then(|value| self.save_to(path).map(|_| value));
        if done.is_err() {
            *self = previous;
        }
        done
    }

    fn save_to(&mut self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| io_failure("err_io_mkdir", path, e))?;
        }
        let _lock = lock(&path.with_extension("lock"))?;
        let base = self.base.as_deref().cloned().unwrap_or_default();
        let stored = stored(path)?.unwrap_or_else(|| base.clone());
        write_replacing(path, &self.merged(&base, stored))?;
        self.base = self.snapshot();
        Ok(())
    }

    fn snapshot(&self) -> Option<Box<Config>> {
        Some(Box::new(Config { base: None, ..self.clone() }))
    }

    fn merged(&self, base: &Config, stored: Config) -> Config {
        let mut games = Vec::new();
        for ours in &self.games {
            match (entry_for(&base.games, &ours.path), entry_for(&stored.games, &ours.path)) {
                (Some(old), Some(theirs)) if old == ours => games.push(theirs.clone()),
                (Some(old), None) if old == ours => {}
                _ => games.push(ours.clone()),
            }
        }
        let added_elsewhere = stored.games.iter()
            .filter(|g| entry_for(&self.games, &g.path).is_none() && entry_for(&base.games, &g.path).is_none());
        games.extend(added_elsewhere.cloned());
        Config {
            language: changed_or(&self.language, &base.language, stored.language),
            games,
            last_mod_version_seen: changed_or(&self.last_mod_version_seen, &base.last_mod_version_seen, stored.last_mod_version_seen),
            last_notified_tag: changed_or(&self.last_notified_tag, &base.last_notified_tag, stored.last_notified_tag),
            base: None,
        }
    }

    pub fn upsert_game(&mut self, entry: GameEntry) {
        if let Some(g) = self.games.iter_mut().find(|g| same_path(&g.path, &entry.path)) {
            *g = entry;
        } else {
            self.games.push(entry);
        }
    }

    pub fn remove_game(&mut self, path: &str) {
        self.games.retain(|g| !same_path(&g.path, path));
    }

    pub fn edit_game(&mut self, index: usize, entry: GameEntry) -> Result<bool, &'static str> {
        if index >= self.games.len() {
            return Err("no_selection");
        }
        if self.games.iter().enumerate().any(|(i, g)| i != index && same_path(&g.path, &entry.path)) {
            return Err("duplicate_game");
        }
        let changed = self.games[index].profile_changed(&entry);
        self.games[index] = entry;
        Ok(changed)
    }

    pub fn set_executable(&mut self, path: &str, exe: &str) -> bool {
        match self.games.iter_mut().find(|g| same_path(&g.path, path)) {
            Some(g) => {
                g.executable = exe.to_string();
                true
            }
            None => false,
        }
    }

    pub fn set_profile(&mut self, path: &str, profile: &str) -> bool {
        match self.games.iter_mut().find(|g| same_path(&g.path, path)) {
            Some(g) => {
                g.profile = profile.to_string();
                g.profile_mode = "specific".to_string();
                true
            }
            None => false,
        }
    }
}

pub fn same_path(a: &str, b: &str) -> bool {
    normalize(a) == normalize(b)
}

fn normalize(p: &str) -> String {
    p.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

fn entry_for<'a>(games: &'a [GameEntry], path: &str) -> Option<&'a GameEntry> {
    games.iter().find(|g| same_path(&g.path, path))
}

fn changed_or<T: Clone + PartialEq>(ours: &T, base: &T, stored: T) -> T {
    if ours != base { ours.clone() } else { stored }
}

fn lock(path: &Path) -> Result<File, String> {
    let file = OpenOptions::new().create(true).truncate(false).write(true).open(path)
        .map_err(|e| io_failure("err_io_create", path, e))?;
    file.lock().map_err(|e| io_failure("err_io_write", path, e))?;
    Ok(file)
}

fn stored(path: &Path) -> Result<Option<Config>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).ok()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_failure("err_io_read", path, e)),
    }
}

fn write_replacing(path: &Path, cfg: &Config) -> Result<(), String> {
    let json = serde_json::to_string_pretty(cfg).map_err(|e| io_failure("err_io_write", path, e))?;
    let temp = path.with_extension("json.tmp");
    File::create(&temp)
        .and_then(|mut file| {
            file.write_all(json.as_bytes())?;
            file.sync_all()
        })
        .map_err(|e| io_failure("err_io_write", &temp, e))?;
    fs::rename(&temp, path).map_err(|e| io_failure("err_io_write", path, e))
}

fn io_failure(key: &str, path: &Path, reason: impl std::fmt::Display) -> String {
    err_key(key, &format!("{} ({})", path.display(), reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_that_fails_or_does_not_save_leaves_the_config_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut c = Config::default();
        let entry = GameEntry { path: "D:/Games/Z".into(), name: String::new(), executable: String::new(), profile: "generic".into(), profile_mode: "generic".into() };
        c.commit_to(&path, |c| {
            c.upsert_game(entry.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(Config::load_from(&path).games, vec![entry.clone()]);
        let refused = c.commit_to(&path, |c| {
            c.games.clear();
            c.edit_game(5, entry.clone()).map_err(str::to_string)
        });
        assert_eq!(refused, Err("no_selection".to_string()));
        assert_eq!(c.games, vec![entry.clone()]);
        let blocked = dir.path().join("file");
        fs::write(&blocked, "x").unwrap();
        assert!(c.commit_to(&blocked.join("config.json"), |c| {
            c.set_language("de");
            Ok(())
        })
        .is_err());
        assert_eq!((c.language.clone(), c.games.clone()), (None, vec![entry]));
    }

    #[test]
    fn upsert_replaces_same_path_ci_and_slashes() {
        let mut c = Config::default();
        c.upsert_game(GameEntry { path: "D:/Games/Z".into(), name: String::new(), executable: String::new(), profile: "pokemon_z".into(), profile_mode: "specific".into() });
        c.upsert_game(GameEntry { path: "d:\\games\\z".into(), name: String::new(), executable: String::new(), profile: "generic".into(), profile_mode: "generic".into() });
        assert_eq!(c.games.len(), 1);
        assert_eq!(c.games[0].profile, "generic");
    }

    #[test]
    fn remove_game_works() {
        let mut c = Config::default();
        c.upsert_game(GameEntry { path: "D:/Games/Z".into(), name: String::new(), executable: String::new(), profile: "pokemon_z".into(), profile_mode: "specific".into() });
        c.remove_game("D:/Games/Z");
        assert!(c.games.is_empty());
    }

    #[test]
    fn a_folder_record_takes_a_new_executable_whatever_it_had() {
        let mut c = Config::default();
        c.upsert_game(GameEntry { path: "D:\\Games\\Uranium".into(), name: String::new(), executable: "Uranium.exe".into(), profile: "uranium".into(), profile_mode: "specific".into() });
        assert!(c.set_executable("d:/games/uranium/", "Uranium (PokeAccess).exe"));
        assert_eq!(c.games[0].executable, "Uranium (PokeAccess).exe");
        assert!(!c.set_executable("D:/Games/Other", "Game.exe"));
    }

    #[test]
    fn a_folder_record_takes_the_profile_its_conversion_kept() {
        let mut c = Config::default();
        c.upsert_game(GameEntry { path: "D:\\Games\\Uranium".into(), name: String::new(), executable: String::new(), profile: "generic".into(), profile_mode: "generic".into() });
        assert!(c.set_profile("d:/games/uranium/", "uranium"));
        assert_eq!((c.games[0].profile.as_str(), c.games[0].profile_mode.as_str()), ("uranium", "specific"));
        assert!(!c.set_profile("D:/Games/Other", "uranium"));
    }

    #[test]
    fn default_language_is_unset_until_resolved() {
        assert_eq!(Config::default().language, None);
    }

    #[test]
    fn old_config_entries_default_new_fields() {
        let e: GameEntry = serde_json::from_str(r#"{"path":"C:/Game","profile":"generic","profile_mode":"generic"}"#).unwrap();
        assert_eq!(e.name, "");
        assert_eq!(e.executable, "");
        let saved = serde_json::to_string(&e).unwrap();
        assert!(saved.contains("\"executable\""));
    }

    #[test]
    fn editing_moves_record_and_repatches_only_on_profile_change() {
        let mut c = Config::default();
        let original = GameEntry { path: "C:/Old".into(), name: "Old".into(), executable: "Game.exe".into(), profile: "generic".into(), profile_mode: "generic".into() };
        assert_eq!(c.edit_game(0, original.clone()), Err("no_selection"));
        c.upsert_game(original.clone());
        let mut updated = original.clone();
        updated.path = "C:/New".into();
        updated.name = "New".into();
        assert!(!c.edit_game(0, updated.clone()).unwrap());
        assert_eq!(c.games.len(), 1);
        assert_eq!(c.games[0].path, "C:/New");
        updated.profile = "royal".into();
        updated.profile_mode = "specific".into();
        assert!(c.edit_game(0, updated).unwrap());
        c.upsert_game(original);
        let duplicate = c.games[1].clone();
        assert_eq!(c.edit_game(0, duplicate), Err("duplicate_game"));
        assert_eq!(c.games[0].path, "C:/New");
    }

    #[test]
    fn resolve_uses_saved_language_over_system() {
        let mut c = Config::default();
        c.set_language("en");
        assert_eq!(c.resolve_language(), "en");
    }

    #[test]
    fn detect_maps_es_variants_and_fallback() {
        assert_eq!(map_locale("es-ES"), "es");
        assert_eq!(map_locale("es_MX"), "es");
        assert_eq!(map_locale("ES"), "es");
        assert_eq!(map_locale("en-US"), "en");
        assert_eq!(map_locale("fr-FR"), "fr");
        assert_eq!(map_locale("pt-BR"), "pt");
        assert_eq!(map_locale("de-DE"), "de");
        assert_eq!(map_locale("pl-PL"), "pl");
        assert_eq!(map_locale("it-IT"), "en");
        assert_eq!(map_locale(""), "en");
    }

    fn game(path: &str) -> GameEntry {
        GameEntry { path: path.into(), name: String::new(), executable: String::new(), profile: "generic".into(), profile_mode: "generic".into() }
    }

    fn paths(cfg: &Config) -> Vec<&str> {
        cfg.games.iter().map(|g| g.path.as_str()).collect()
    }

    fn saved_with(path: &Path, games: &[&str]) {
        let mut cfg = Config::default();
        for g in games {
            cfg.upsert_game(game(g));
        }
        cfg.save_to(path).unwrap();
    }

    #[test]
    fn a_save_writes_the_whole_file_and_leaves_no_temporary_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("launcher").join("config.json");
        saved_with(&path, &["C:/A", "C:/B"]);
        assert_eq!(paths(&Config::load_from(&path)), ["C:/A", "C:/B"]);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn a_failed_save_keeps_the_previous_file_and_says_why_in_the_players_language() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        saved_with(&path, &["C:/A"]);
        fs::create_dir(path.with_extension("json.tmp")).unwrap();
        let mut cfg = Config::load_from(&path);
        cfg.upsert_game(game("C:/B"));
        let shown = crate::i18n::I18n::new("en").t_err(&cfg.save_to(&path).unwrap_err());
        assert!(shown.starts_with("Could not write ") && shown.contains("config.json.tmp"), "{}", shown);
        assert_eq!(paths(&Config::load_from(&path)), ["C:/A"]);
    }

    #[test]
    fn two_processes_saving_one_after_the_other_keep_both_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        saved_with(&path, &["C:/A"]);
        let mut window = Config::load_from(&path);
        let mut console = Config::load_from(&path);
        window.upsert_game(game("C:/B"));
        window.save_to(&path).unwrap();
        console.upsert_game(game("C:/C"));
        console.save_to(&path).unwrap();
        assert_eq!(paths(&Config::load_from(&path)), ["C:/A", "C:/C", "C:/B"]);
        window.set_language("en");
        window.save_to(&path).unwrap();
        let on_disk = Config::load_from(&path);
        assert_eq!(paths(&on_disk), ["C:/A", "C:/B", "C:/C"]);
        assert_eq!(on_disk.language.as_deref(), Some("en"));
        assert_eq!(paths(&window), ["C:/A", "C:/B"]);
    }

    #[test]
    fn what_one_process_removes_or_edits_the_other_does_not_undo() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        saved_with(&path, &["C:/A", "C:/B"]);
        let mut window = Config::load_from(&path);
        let mut console = Config::load_from(&path);
        window.remove_game("C:/A");
        window.save_to(&path).unwrap();
        console.set_profile("C:/B", "royal");
        console.last_notified_tag = "v1.2.0".into();
        console.save_to(&path).unwrap();
        window.set_language("de");
        window.save_to(&path).unwrap();
        let on_disk = Config::load_from(&path);
        assert_eq!(paths(&on_disk), ["C:/B"]);
        assert_eq!((on_disk.games[0].profile.as_str(), on_disk.last_notified_tag.as_str()), ("royal", "v1.2.0"));
        assert_eq!(on_disk.language.as_deref(), Some("de"));
    }

    #[test]
    fn saves_racing_from_many_processes_lose_none_of_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        saved_with(&path, &["C:/A"]);
        let start = std::sync::Barrier::new(8);
        std::thread::scope(|s| {
            for i in 0..8 {
                let (path, start) = (&path, &start);
                s.spawn(move || {
                    let mut cfg = Config::load_from(path);
                    start.wait();
                    cfg.upsert_game(game(&format!("C:/G{}", i)));
                    cfg.save_to(path).unwrap();
                });
            }
        });
        let on_disk = Config::load_from(&path);
        assert_eq!(on_disk.games.len(), 9);
        for i in 0..8 {
            assert!(entry_for(&on_disk.games, &format!("C:/G{}", i)).is_some(), "G{} perdido", i);
        }
    }

    #[test]
    fn a_broken_file_gives_way_to_the_config_in_memory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        saved_with(&path, &["C:/A"]);
        let mut cfg = Config::load_from(&path);
        fs::write(&path, "{ roto").unwrap();
        cfg.upsert_game(game("C:/B"));
        cfg.save_to(&path).unwrap();
        assert_eq!(paths(&Config::load_from(&path)), ["C:/A", "C:/B"]);
    }
}
