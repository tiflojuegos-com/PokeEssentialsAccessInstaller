use std::fs;

use serde::{Deserialize, Serialize};

use super::paths::{launcher_config_dir, launcher_config_file};

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub games: Vec<GameEntry>,
    #[serde(default)]
    pub last_mod_version_seen: String,
    #[serde(default)]
    pub last_notified_tag: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            language: None,
            games: Vec::new(),
            last_mod_version_seen: String::new(),
            last_notified_tag: String::new(),
        }
    }
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
        let path = launcher_config_file();
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => return Config::default(),
        };
        match serde_json::from_str(&text) {
            Ok(cfg) => cfg,
            Err(_) => {
                let bak = path.with_extension("json.bak");
                if bak.exists() {
                    let _ = fs::remove_file(&path);
                } else {
                    let _ = fs::rename(&path, &bak);
                }
                Config::default()
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        fs::create_dir_all(launcher_config_dir())
            .map_err(|e| format!("crear carpeta config: {}", e))?;
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("serializar config: {}", e))?;
        fs::write(launcher_config_file(), json).map_err(|e| format!("escribir config: {}", e))
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

    /// Edits the selected record instead of leaving an old record after a move.
    pub fn edit_game(&mut self, index: usize, entry: GameEntry) -> Result<bool, &'static str> {
        if index >= self.games.len() {
            return Err("no_selection");
        }
        if self.games.iter().enumerate().any(|(i, g)| i != index && same_path(&g.path, &entry.path)) {
            return Err("duplicate_game");
        }
        let changed = self.games[index].profile != entry.profile
            || self.games[index].profile_mode != entry.profile_mode;
        self.games[index] = entry;
        Ok(changed)
    }
}

fn same_path(a: &str, b: &str) -> bool {
    normalize(a) == normalize(b)
}

fn normalize(p: &str) -> String {
    p.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
