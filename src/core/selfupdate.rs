use serde::Deserialize;

use super::paths::{GITHUB_OWNER, GITHUB_REPO};

pub const LAUNCHER_ASSET: &str = "pokeessentialsaccess-launcher.exe";

#[derive(Debug, Clone, Deserialize)]
struct Release {
    #[serde(default, deserialize_with = "null_to_default")]
    tag_name: String,
    #[serde(default, deserialize_with = "null_to_default")]
    body: String,
    #[serde(default)]
    draft: bool,
    #[serde(default, deserialize_with = "null_to_default")]
    assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn null_to_default<'de, D, T>(de: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + serde::Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(de)?.unwrap_or_default())
}

#[derive(Debug, Clone)]
pub struct LauncherUpdate {
    pub url: String,
    pub notes: String,
    pub tag: String,
}

fn releases_url() -> String {
    format!("https://api.github.com/repos/{}/{}/releases?per_page=10", GITHUB_OWNER, GITHUB_REPO)
}

fn parse_releases(text: &str) -> Vec<Release> {
    serde_json::from_str(text).unwrap_or_default()
}

fn latest_asset(releases: &[Release]) -> Option<(String, String, String)> {
    for rel in releases.iter().filter(|r| !r.draft) {
        if let Some(a) = rel.assets.iter().find(|a| a.name.eq_ignore_ascii_case(LAUNCHER_ASSET)) {
            return Some((a.browser_download_url.clone(), rel.body.clone(), rel.tag_name.clone()));
        }
    }
    None
}

fn normalize_tag(tag: &str) -> String {
    tag.trim().trim_start_matches(['v', 'V']).to_string()
}

pub fn is_newer(tag: &str, current: &str) -> bool {
    match (semver::Version::parse(&normalize_tag(tag)), semver::Version::parse(&normalize_tag(current))) {
        (Ok(a), Ok(b)) => a > b,
        _ => normalize_tag(tag) != normalize_tag(current),
    }
}

pub fn check() -> Option<LauncherUpdate> {
    let list_bytes = super::github::download_bytes(&releases_url()).ok()?;
    let releases = parse_releases(&String::from_utf8_lossy(&list_bytes));
    let (url, notes, tag) = latest_asset(&releases)?;

    if is_newer(&tag, env!("CARGO_PKG_VERSION")) {
        Some(LauncherUpdate { url, notes, tag })
    } else {
        None
    }
}

pub fn apply(update: &LauncherUpdate) -> Result<(), String> {
    let bytes = super::github::download_bytes(&update.url)
        .map_err(|e| format!("descargar instalador nuevo: {}", e))?;
    if bytes.len() < 65536 || &bytes[0..2] != b"MZ" {
        return Err("la descarga del instalador no es un ejecutable valido".to_string());
    }
    let tmp = std::env::temp_dir().join("pokeessentialsaccess-launcher-new.exe");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("guardar temporal: {}", e))?;
    self_replace::self_replace(&tmp).map_err(|e| format!("reemplazar ejecutable: {}", e))?;
    let _ = std::fs::remove_file(&tmp);
    Ok(())
}

pub fn restart() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("ruta ejecutable: {}", e))?;
    std::process::Command::new(exe).spawn().map_err(|e| format!("reiniciar: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_asset_from_first_nondraft_with_notes() {
        let json = r#"[
            {"tag_name":"v0.2","draft":true,"assets":[{"name":"pokeessentialsaccess-launcher.exe","browser_download_url":"https://x/draft.exe"}]},
            {"tag_name":"v0.1","body":"- cambio uno","draft":false,"assets":[{"name":"pokeessentialsaccess-launcher.exe","browser_download_url":"https://x/good.exe"}]}
        ]"#;
        let (url, notes, tag) = latest_asset(&parse_releases(json)).unwrap();
        assert_eq!(url, "https://x/good.exe");
        assert_eq!(notes, "- cambio uno");
        assert_eq!(tag, "v0.1");
    }

    #[test]
    fn none_when_no_asset() {
        let json = r#"[{"tag_name":"v0.1","draft":false,"assets":[{"name":"mod.zip","browser_download_url":"https://x/mod.zip"}]}]"#;
        assert!(latest_asset(&parse_releases(json)).is_none());
    }

    #[test]
    fn parses_release_with_null_body() {
        let json = r#"[
            {"tag_name":"v0.1.1","body":"- añadido un instalador","draft":false,"assets":[{"name":"pokeessentialsaccess-launcher.exe","browser_download_url":"https://x/new.exe"}]},
            {"tag_name":"v0.1","body":null,"draft":false,"assets":[{"name":"mod.zip","browser_download_url":"https://x/mod.zip"}]}
        ]"#;
        let rels = parse_releases(json);
        assert_eq!(rels.len(), 2, "el body null NO debe vaciar el array");
        let (_, notes, tag) = latest_asset(&rels).unwrap();
        assert_eq!(tag, "v0.1.1");
        assert_eq!(notes, "- añadido un instalador");
    }

    #[test]
    fn empty_releases() {
        assert!(latest_asset(&parse_releases("[]")).is_none());
    }

    #[test]
    fn is_newer_compares_semver_with_v_prefix() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.2.0", "v0.1.0"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1.0", "0.2.0"));
    }

    #[test]
    fn is_newer_falls_back_to_string_diff_when_unparseable() {
        assert!(is_newer("beta", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
    }
}
