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
    #[serde(default)]
    prerelease: bool,
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
    format!("https://api.github.com/repos/{}/{}/releases?per_page=30", GITHUB_OWNER, GITHUB_REPO)
}

fn parse_releases(text: &str) -> Vec<Release> {
    serde_json::from_str(text).unwrap_or_default()
}

fn latest_asset(releases: &[Release]) -> Option<(String, String, String)> {
    for rel in releases.iter().filter(|r| !r.draft && !r.prerelease) {
        if let Some(a) = rel.assets.iter().find(|a| a.name.eq_ignore_ascii_case(LAUNCHER_ASSET)) {
            return Some((a.browser_download_url.clone(), rel.body.clone(), rel.tag_name.clone()));
        }
    }
    None
}

fn normalize_tag(tag: &str) -> String {
    tag.trim().trim_start_matches(['v', 'V']).to_string()
}

/// Parses a tag tolerating the `v` prefix and missing minor/patch components.
fn parse_version(tag: &str) -> Option<semver::Version> {
    let normalized = normalize_tag(tag);
    if normalized.is_empty() {
        return None;
    }
    let mut parts: Vec<&str> = normalized.split('.').collect();
    while parts.len() < 3 {
        parts.push("0");
    }
    semver::Version::parse(&parts.join(".")).ok()
}

pub fn is_newer(tag: &str, current: &str) -> bool {
    match (parse_version(tag), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => normalize_tag(tag) != normalize_tag(current),
    }
}

/// Strict version gate: true only when both sides parse and `required` is newer.
/// Unparseable or missing requirements never lock the user out.
pub fn requires_newer_than(required: &str, current: &str) -> bool {
    match (parse_version(required), parse_version(current)) {
        (Some(req), Some(cur)) => req > cur,
        _ => false,
    }
}

/// Decides whether to offer an update, given the mod's version.json and its release list.
///
/// The launcher's version is declared by version.json's `launcher` field, NOT by the release tag:
/// the launcher and the mod share one release stream, and those tags carry the MOD's version
/// (release.yml enforces tag == version.json). Comparing a mod tag against this build's version
/// offers a phantom update on every boot as soon as the mod's number passes the launcher's, and
/// re-downloads the same exe forever. A version.json without the field announces nothing: silence
/// is the safe failure, a loop is not.
fn pick_update(version_json: &str, releases_json: &str, current: &str) -> Option<LauncherUpdate> {
    let declared = super::installed::parse_version_json(version_json)?.launcher;
    if declared.trim().is_empty() || !is_newer(&declared, current) {
        return None;
    }
    let (url, notes, _tag) = latest_asset(&parse_releases(releases_json))?;
    Some(LauncherUpdate { url, notes, tag: normalize_tag(&declared) })
}

pub fn check() -> Option<LauncherUpdate> {
    let version_bytes =
        super::github::download_bytes(&super::paths::raw_url("version.json")).ok()?;
    let list_bytes = super::github::download_bytes(&releases_url()).ok()?;
    pick_update(
        &String::from_utf8_lossy(&version_bytes),
        &String::from_utf8_lossy(&list_bytes),
        env!("CARGO_PKG_VERSION"),
    )
}

pub fn apply(update: &LauncherUpdate) -> Result<(), String> {
    let bytes = super::github::download_bytes(&update.url)
        .map_err(|e| crate::i18n::err_key("err_selfupdate_download", &crate::i18n::I18n::new("es").t_err(&e)))?;
    if bytes.len() < 65536 || &bytes[0..2] != b"MZ" {
        return Err("err_selfupdate_invalid".to_string());
    }
    let tmp = std::env::temp_dir().join("pokeessentialsaccess-launcher-new.exe");
    std::fs::write(&tmp, &bytes).map_err(|e| crate::i18n::err_key("err_selfupdate_replace", &e.to_string()))?;
    self_replace::self_replace(&tmp).map_err(|e| crate::i18n::err_key("err_selfupdate_replace", &e.to_string()))?;
    let _ = std::fs::remove_file(&tmp);
    Ok(())
}

pub fn restart() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe).spawn().map_err(|e| e.to_string())?;
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

    #[test]
    fn prerelease_is_not_offered() {
        let json = r#"[
            {"tag_name":"v0.3.0","draft":false,"prerelease":true,"assets":[{"name":"pokeessentialsaccess-launcher.exe","browser_download_url":"https://x/pre.exe"}]},
            {"tag_name":"v0.2.0","body":"estable","draft":false,"prerelease":false,"assets":[{"name":"pokeessentialsaccess-launcher.exe","browser_download_url":"https://x/stable.exe"}]}
        ]"#;
        let (url, _, tag) = latest_asset(&parse_releases(json)).unwrap();
        assert_eq!(url, "https://x/stable.exe");
        assert_eq!(tag, "v0.2.0");
    }

    #[test]
    fn only_prerelease_means_no_update() {
        let json = r#"[{"tag_name":"v0.3.0","draft":false,"prerelease":true,"assets":[{"name":"pokeessentialsaccess-launcher.exe","browser_download_url":"https://x/pre.exe"}]}]"#;
        assert!(latest_asset(&parse_releases(json)).is_none());
    }

    #[test]
    fn missing_prerelease_field_defaults_to_stable() {
        let json = r#"[{"tag_name":"v0.2.0","draft":false,"assets":[{"name":"pokeessentialsaccess-launcher.exe","browser_download_url":"https://x/stable.exe"}]}]"#;
        assert!(latest_asset(&parse_releases(json)).is_some());
    }

    #[test]
    fn requires_newer_than_only_blocks_on_a_real_newer_version() {
        assert!(requires_newer_than("0.9.0", "0.1.1"));
        assert!(requires_newer_than("v0.2", "0.1.1"));
        assert!(!requires_newer_than("0.1.1", "0.1.1"));
        assert!(!requires_newer_than("0.1.0", "0.1.1"));
        assert!(!requires_newer_than("", "0.1.1"));
        assert!(!requires_newer_than("   ", "0.1.1"));
        assert!(!requires_newer_than("proximamente", "0.1.1"));
    }

    const RELEASE_WITH_ASSET: &str = r#"[{"tag_name":"v9.9.9","body":"- notas","draft":false,
        "assets":[{"name":"pokeessentialsaccess-launcher.exe","browser_download_url":"https://x/l.exe"}]}]"#;

    #[test]
    fn offers_update_when_version_json_declares_a_newer_launcher() {
        let u = pick_update(
            r#"{"version":"0.1.3","min_launcher":"0.1.0","launcher":"0.1.2"}"#,
            RELEASE_WITH_ASSET,
            "0.1.1",
        )
        .expect("un launcher declarado mas nuevo debe ofrecerse");
        assert_eq!(u.url, "https://x/l.exe");
        assert_eq!(u.notes, "- notas");
    }

    #[test]
    fn the_announced_version_is_the_launchers_not_the_release_tag() {
        let u = pick_update(
            r#"{"version":"9.9.9","launcher":"v0.1.2"}"#,
            RELEASE_WITH_ASSET,
            "0.1.1",
        )
        .unwrap();
        assert_eq!(u.tag, "0.1.2");
    }

    #[test]
    fn a_newer_mod_alone_never_offers_a_launcher_update() {
        assert!(pick_update(
            r#"{"version":"9.9.9","min_launcher":"0.1.0"}"#,
            RELEASE_WITH_ASSET,
            "0.1.1"
        )
        .is_none());
    }

    #[test]
    fn silent_when_the_declared_launcher_is_not_newer() {
        assert!(pick_update(r#"{"version":"0.1.3","launcher":"0.1.1"}"#, RELEASE_WITH_ASSET, "0.1.1")
            .is_none());
        assert!(pick_update(r#"{"version":"0.1.3","launcher":"0.1.0"}"#, RELEASE_WITH_ASSET, "0.1.1")
            .is_none());
        assert!(pick_update(r#"{"version":"0.1.3","launcher":"   "}"#, RELEASE_WITH_ASSET, "0.1.1")
            .is_none());
    }

    #[test]
    fn silent_when_declared_newer_but_no_asset_is_attached() {
        let no_asset = r#"[{"tag_name":"v9.9.9","draft":false,
            "assets":[{"name":"PokeEssentialsAccess_v9.9.9.zip","browser_download_url":"https://x/m.zip"}]}]"#;
        assert!(pick_update(r#"{"version":"9.9.9","launcher":"0.2.0"}"#, no_asset, "0.1.1").is_none());
    }

    #[test]
    fn unreadable_version_json_announces_nothing() {
        assert!(pick_update("no soy json", RELEASE_WITH_ASSET, "0.1.1").is_none());
    }
}
