use serde::Deserialize;

use super::paths::raw_url;

#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    pub key: String,
    pub display: String,
    #[serde(default)]
    pub detect: Option<String>,
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

    pub fn fetch() -> Result<Catalog, String> {
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
        Catalog::from_json(&text)
    }

    pub fn display_of(&self, key: &str) -> String {
        self.profiles
            .iter()
            .find(|p| p.key == key)
            .map(|p| p.display.clone())
            .unwrap_or_else(|| key.to_string())
    }

    pub fn detect(&self, folder_and_exe: &str) -> Option<&Profile> {
        let hay = folder_and_exe.to_lowercase();
        for p in &self.profiles {
            if let Some(pat) = &p.detect {
                if let Ok(re) = regex::Regex::new(&format!("(?i){}", pat)) {
                    if re.is_match(&hay) {
                        return Some(p);
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Catalog {
        Catalog::from_json(
            r#"{"profiles":[
                {"key":"pokemon_z","display":"Pokemon Z","detect":"pokemon ?z\\b","engine":"gen6"},
                {"key":"opalo","display":"Pokemon Opalo","detect":"opalo|ópalo","engine":"gen6"},
                {"key":"reminiscencia","display":"Pokemon Reminiscencia","detect":"reminisc","engine":"gen6"},
                {"key":"generic","display":"Perfil generico","detect":null,"engine":"any"}
            ]}"#,
        )
        .unwrap()
    }

    #[test]
    fn detect_by_folder() {
        let c = sample();
        assert_eq!(c.detect("D:/Games/ReminiscenciaV2").unwrap().key, "reminiscencia");
        assert_eq!(c.detect("F:/POKEMON Z V2.18/game.exe").unwrap().key, "pokemon_z");
        assert_eq!(c.detect("F:/juegos/Pokemon Z/Game.exe").unwrap().key, "pokemon_z");
        assert_eq!(c.detect("C:/games/pokemonz/game.exe").unwrap().key, "pokemon_z");
    }

    #[test]
    fn pokemon_z_detect_does_not_shadow_others() {
        let c = sample();
        assert_eq!(c.detect("Z:/juegos/Opalo/Game.exe").unwrap().key, "opalo");
        assert_eq!(c.detect("D:/games/ReminiscenciaV2 mkxp-z.exe").unwrap().key, "reminiscencia");
        assert_eq!(c.detect("Z:/games/ReminiscenciaV2/mkxp-z.exe").unwrap().key, "reminiscencia");
    }

    #[test]
    fn detect_unknown_folder_is_none() {
        let c = sample();
        assert!(c.detect("D:/Games/anlskjfuqwer").is_none());
    }

    #[test]
    fn generic_never_autodetects() {
        let c = sample();
        assert!(c.profiles.iter().find(|p| p.key == "generic").unwrap().detect.is_none());
    }

    #[test]
    fn display_fallback_to_key() {
        let c = sample();
        assert_eq!(c.display_of("pokemon_z"), "Pokemon Z");
        assert_eq!(c.display_of("desconocido"), "desconocido");
    }
}
