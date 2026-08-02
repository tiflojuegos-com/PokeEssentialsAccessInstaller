use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::paths::{launcher_config_dir, raw_url};

#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    pub key: String,
    pub display: String,
    #[serde(default)]
    pub titles: Vec<String>,
    #[serde(default)]
    pub detect: Option<String>,
    #[serde(default)]
    pub exes: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub engine: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Catalog {
    pub profiles: Vec<Profile>,
}

impl Catalog {
    pub fn from_json(text: &str) -> Result<Catalog, String> {
        serde_json::from_str(text).map_err(|e| format!("catalog.json invalido: {}", e))
    }

    /// The catalog for this run: the repo's copy when GitHub answers, otherwise
    /// the last one that parsed. Without the cache a launcher started with no
    /// network knows no profiles at all, so every game it adds gets the generic
    /// one and loses its own screens, and the dialog says nothing about it.
    pub fn fetch() -> Result<Catalog, String> {
        let cat = match fetch_remote() {
            Ok((cat, text)) => {
                write_cache(&cache_path(), &text);
                cat
            }
            Err(e) => match read_cache(&cache_path()) {
                Some(cached) => {
                    super::logging::append(&format!("catalog.json: sin conexion ({}), uso la copia guardada", e));
                    cached
                }
                None => return Err(e),
            },
        };
        for key in cat.invalid_detects() {
            super::logging::append(&format!(
                "catalog.json: el perfil {} trae un detect invalido, ese juego no se detectara solo",
                key
            ));
        }
        Ok(cat)
    }

    /// Profile keys whose `detect` regex does not compile. Such a profile can
    /// never win the folder layer, so `fetch` writes them to the launcher log
    /// instead of letting a typo in catalog.json make a game quietly
    /// undetectable.
    pub fn invalid_detects(&self) -> Vec<&str> {
        self.profiles
            .iter()
            .filter(|p| p.detect.as_deref().is_some_and(|pat| compile_detect(pat).is_none()))
            .map(|p| p.key.as_str())
            .collect()
    }

    pub fn display_of(&self, key: &str) -> String {
        self.profiles
            .iter()
            .find(|p| p.key == key)
            .map(|p| p.display.clone())
            .unwrap_or_else(|| key.to_string())
    }

    /// Picks the profile for a game by trying three layers in order: the
    /// declared titles, the folder and exe regex, and finally the exe name.
    /// Within the first two layers the longest match wins, so a spin-off never
    /// loses to the base game. None means nothing matched and the caller should
    /// offer the generic profile.
    ///
    /// `titles` is every name the folder declares, most trustworthy first: a
    /// title that matches no declared one is not an answer, so the next
    /// candidate gets its turn instead of the whole layer giving up. That is
    /// what keeps mkxp-z's `"Custom Title"` placeholder from burying the real
    /// name that Game.ini carries.
    pub fn detect(&self, titles: &[String], folder_and_exe: &str, exe_name: Option<&str>) -> Option<&Profile> {
        for t in titles {
            if let Some(p) = self.by_declared_title(t) {
                return Some(p);
            }
        }
        let hay = folder_and_exe.to_lowercase();
        let mut best: Option<(&Profile, usize)> = None;
        for p in &self.profiles {
            if let Some(re) = p.detect.as_deref().and_then(compile_detect) {
                if let Some(m) = re.find(&hay) {
                    let len = m.end() - m.start();
                    if best.map(|(_, l)| len > l).unwrap_or(true) {
                        best = Some((p, len));
                    }
                }
            }
        }
        if let Some((p, _)) = best {
            return Some(p);
        }
        if let Some(e) = exe_name {
            let el = e.to_lowercase();
            for p in &self.profiles {
                if p.exes.iter().any(|x| x.to_lowercase() == el) {
                    return Some(p);
                }
            }
        }
        None
    }

    /// The profile whose longest declared title this name contains.
    fn by_declared_title(&self, title: &str) -> Option<&Profile> {
        let tl = title.to_lowercase();
        let mut best: Option<(&Profile, usize)> = None;
        for p in &self.profiles {
            for cand in &p.titles {
                let c = cand.to_lowercase();
                if !c.is_empty() && tl.contains(&c) && best.map(|(_, l)| c.len() > l).unwrap_or(true) {
                    best = Some((p, c.len()));
                }
            }
        }
        best.map(|(p, _)| p)
    }
}

fn fetch_remote() -> Result<(Catalog, String), String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("PokeEssentialsAccessLauncher")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("cliente HTTP: {}", e))?;
    let resp = client
        .get(raw_url("games/catalog.json"))
        .send()
        .map_err(|e| format!("conexion GitHub: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub devolvio estado {}", resp.status()));
    }
    let text = resp.text().map_err(|e| format!("lectura respuesta: {}", e))?;
    let cat = Catalog::from_json(&text)?;
    Ok((cat, text))
}

fn cache_path() -> PathBuf {
    launcher_config_dir().join("catalog.json")
}

/// Keeps the catalog next to the launcher's own config. Best effort: a cache
/// that cannot be written is a launcher that will need the network next time,
/// never a run that fails.
fn write_cache(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, text);
}

/// Only ever returns a catalog that still parses: a half written or hand edited
/// cache must send the launcher offline, not feed it broken profiles.
fn read_cache(path: &Path) -> Option<Catalog> {
    Catalog::from_json(&std::fs::read_to_string(path).ok()?).ok()
}

/// Compiles a profile's `detect` pattern the one way the launcher understands
/// it: case insensitive, and None when catalog.json carries a broken regex.
fn compile_detect(pattern: &str) -> Option<regex::Regex> {
    regex::Regex::new(&format!("(?i){}", pattern)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Catalog {
        Catalog::from_json(
            r#"{"profiles":[
                {"key":"pokemon_z","display":"Pokemon Z","titles":["pokemon z"],"detect":"pokemon ?z\\b","exes":[],"engine":"gen6"},
                {"key":"opalo","display":"Pokemon Opalo","titles":["pokemon opalo"],"detect":"opalo|ópalo","exes":[],"engine":"gen6"},
                {"key":"reminiscencia","display":"Pokemon Reminiscencia","titles":["reminiscencia"],"detect":"reminisc","exes":["Reminiscencia.exe"],"engine":"gen6"},
                {"key":"infinitefusion_hoenn","display":"Pokemon Infinite Fusion 2 Hoenn","titles":["infinitefusion-hoenn"],"detect":"infinite ?fusion ?2|fusion.*hoenn","exes":["InfiniteFusion2.exe"],"engine":"gamedata"},
                {"key":"infinitefusion","display":"Pokemon Infinite Fusion","titles":["infinitefusion"],"detect":"infinite ?fusion","exes":[],"engine":"gamedata"},
                {"key":"generic","display":"Perfil generico","titles":[],"detect":null,"exes":[],"engine":"any"}
            ]}"#,
        )
        .unwrap()
    }

    fn titles(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn detect_by_folder() {
        let c = sample();
        assert_eq!(c.detect(&[], "D:/Games/ReminiscenciaV2", None).unwrap().key, "reminiscencia");
        assert_eq!(c.detect(&[], "F:/POKEMON Z V2.18/game.exe", None).unwrap().key, "pokemon_z");
        assert_eq!(c.detect(&[], "F:/juegos/Pokemon Z/Game.exe", None).unwrap().key, "pokemon_z");
        assert_eq!(c.detect(&[], "C:/games/pokemonz/game.exe", None).unwrap().key, "pokemon_z");
    }

    #[test]
    fn pokemon_z_detect_does_not_shadow_others() {
        let c = sample();
        assert_eq!(c.detect(&[], "Z:/juegos/Opalo/Game.exe", None).unwrap().key, "opalo");
        assert_eq!(c.detect(&[], "D:/games/ReminiscenciaV2 mkxp-z.exe", None).unwrap().key, "reminiscencia");
        assert_eq!(c.detect(&[], "Z:/games/ReminiscenciaV2/mkxp-z.exe", None).unwrap().key, "reminiscencia");
    }

    #[test]
    fn detect_unknown_folder_is_none() {
        let c = sample();
        assert!(c.detect(&[], "D:/Games/anlskjfuqwer", None).is_none());
    }

    #[test]
    fn title_layer_wins_over_folder_regex() {
        let c = sample();
        let p = c.detect(&titles(&["Pokemon Opalo"]), "D:/Games/Pokemon Z/Game.exe", None).unwrap();
        assert_eq!(p.key, "opalo");
    }

    #[test]
    fn title_contains_picks_the_longest_declared_match() {
        let c = sample();
        assert_eq!(c.detect(&titles(&["infinitefusion-hoenn"]), "D:/x", None).unwrap().key, "infinitefusion_hoenn");
        assert_eq!(c.detect(&titles(&["infinitefusion"]), "D:/x", None).unwrap().key, "infinitefusion");
    }

    #[test]
    fn folder_regex_picks_the_longest_match_regardless_of_order() {
        let c = sample();
        assert_eq!(c.detect(&[], "D:/Games/Pokemon Infinite Fusion 2 Hoenn", None).unwrap().key, "infinitefusion_hoenn");
        assert_eq!(c.detect(&[], "D:/Games/Pokemon Infinite Fusion", None).unwrap().key, "infinitefusion");
    }

    #[test]
    fn exe_layer_is_the_last_resort() {
        let c = sample();
        assert_eq!(c.detect(&[], "D:/Games/MiCarpeta", Some("InfiniteFusion2.exe")).unwrap().key, "infinitefusion_hoenn");
        assert!(c.detect(&[], "D:/Games/MiCarpeta", Some("Game.exe")).is_none());
    }

    #[test]
    fn unknown_title_falls_through_to_folder() {
        let c = sample();
        assert_eq!(c.detect(&titles(&["Juego Raro"]), "D:/Games/Opalo", None).unwrap().key, "opalo");
    }

    #[test]
    fn a_placeholder_title_lets_the_next_candidate_answer() {
        let c = sample();
        let both = titles(&["Custom Title", "Pokemon Opalo"]);
        assert_eq!(c.detect(&both, "D:/Games/carpeta renombrada", None).unwrap().key, "opalo");
    }

    #[test]
    fn the_first_title_that_matches_wins_over_later_ones() {
        let c = sample();
        let both = titles(&["Pokemon Z", "Pokemon Opalo"]);
        assert_eq!(c.detect(&both, "D:/x", None).unwrap().key, "pokemon_z");
    }

    #[test]
    fn generic_never_autodetects() {
        let c = sample();
        assert!(c.profiles.iter().find(|p| p.key == "generic").unwrap().detect.is_none());
    }

    #[test]
    fn a_broken_detect_regex_is_flagged_not_swallowed() {
        let c = Catalog::from_json(
            r#"{"profiles":[
                {"key":"roto","display":"Roto","titles":[],"detect":"pokemon ?[z","exes":[],"engine":"gen6"},
                {"key":"sano","display":"Sano","titles":[],"detect":"sano","exes":[],"engine":"gen6"}
            ]}"#,
        )
        .unwrap();
        assert_eq!(c.invalid_detects(), vec!["roto"]);
        assert!(c.detect(&[], "d:/juegos/pokemon z", None).is_none());
        assert_eq!(c.detect(&[], "d:/juegos/sano", None).unwrap().key, "sano");
    }

    #[test]
    fn the_cached_catalog_still_detects_when_github_is_unreachable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("catalog.json");
        write_cache(&path, r#"{"profiles":[{"key":"opalo","display":"Pokemon Opalo","titles":["pokemon opalo"],"detect":"opalo","exes":[],"engine":"gen6"}]}"#);
        let cached = read_cache(&path).expect("la copia guardada debe volver a parsear");
        assert_eq!(cached.detect(&titles(&["Pokemon Opalo"]), "D:/x", None).unwrap().key, "opalo");
        assert_eq!(cached.display_of("opalo"), "Pokemon Opalo");
    }

    #[test]
    fn a_corrupt_cache_is_ignored_instead_of_serving_broken_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("catalog.json");
        write_cache(&path, "{\"profiles\": [ esto no es json");
        assert!(read_cache(&path).is_none());
        assert!(read_cache(&dir.path().join("no_existe.json")).is_none());
    }

    #[test]
    fn a_healthy_catalog_flags_nothing() {
        assert!(sample().invalid_detects().is_empty());
    }

    #[test]
    fn display_fallback_to_key() {
        let c = sample();
        assert_eq!(c.display_of("pokemon_z"), "Pokemon Z");
        assert_eq!(c.display_of("desconocido"), "desconocido");
    }
}
