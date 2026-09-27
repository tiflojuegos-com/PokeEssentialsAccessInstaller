use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::apply::io_error;
use super::catalog::Catalog;
use super::source::ContentEntry;
use super::install::git_blob_sha1;
use super::paths::accessibility_dir;
use crate::i18n::err_key;

pub const MAX_GAME_PATH: usize = 126;

pub const LONG_GAME_PATH: usize = 200;

pub const BACKUP_SUFFIX: &str = ".access.bak";

pub const ACCESS_TAG: &str = " (PokeAccess)";

pub const MANIFEST: &str = "engine.json";

pub const ENGINE_ROOT: &str = "assets/engine";
pub const ENGINE_FILE: &str = "mkxp-z.exe";
pub const EXTRAS_DIR: &str = "extras";

#[derive(Debug, Clone, PartialEq)]
pub struct RgssPlayer {
    pub exe: String,
    pub stem: String,
    pub library: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Offer {
    pub engine: String,
    pub markers: Vec<String>,
    pub profile: String,
    pub display: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub engine: String,
    #[serde(default)]
    pub profile: String,
    pub exe: String,
    pub installed: String,
    #[serde(default)]
    pub added: BTreeMap<String, String>,
    #[serde(default)]
    pub mkxp_json: String,
    #[serde(default)]
    pub converted_at: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
}

pub struct Bundle {
    pub engine: ContentEntry,
    pub extras: Vec<(String, ContentEntry)>,
}

pub fn rgss_player(dir: &Path) -> Option<RgssPlayer> {
    for exe in exe_names(dir) {
        let stem = match Path::new(&exe).file_stem() {
            Some(s) => s.to_string_lossy().into_owned(),
            None => continue,
        };
        let bytes = match fs::read(dir.join(format!("{}.ini", stem))) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if let Some(library) = rgss1_library(&super::detect::decode_text(&bytes)) {
            return Some(RgssPlayer { exe, stem, library });
        }
    }
    None
}

fn rgss1_library(ini: &str) -> Option<String> {
    let re = regex::Regex::new(r"(?i)^library\s*=\s*(rgss10\d[ej]\.dll)$")
        .expect("the RGSS1 library pattern is a literal and always compiles");
    ini.lines().find_map(|l| {
        re.captures(l.trim_start_matches('\u{feff}').trim())
            .map(|c| c[1].to_string())
    })
}

fn exe_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = match fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .map(|x| x.eq_ignore_ascii_case("exe"))
                        .unwrap_or(false)
            })
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort_by_key(|n| n.to_lowercase());
    names
}

pub fn offer_for(cat: &Catalog, dir: &Path) -> Option<Offer> {
    if super::mkxp::has_mkxp_json(dir) {
        return None;
    }
    let player = rgss_player(dir)?;
    if super::detect::scan_exes(dir).supports_preload {
        return None;
    }
    let exe = dir.join(&player.exe);
    let hay = super::detect::folder_and_exe_string(dir, Some(&exe));
    let p = cat.detect(&super::detect::game_titles(dir), &hay, Some(&player.exe))?;
    let engine = p.convert.clone().filter(|e| !e.trim().is_empty())?;
    Some(Offer {
        engine,
        markers: p.markers.clone(),
        profile: p.key.clone(),
        display: p.display.clone(),
    })
}

pub fn incompatible(dir: &Path) -> String {
    if rgss_player(dir).is_some() {
        "rgss_player_unsupported"
    } else {
        "not_compatible"
    }
    .to_string()
}

pub fn path_len(dir: &Path) -> usize {
    dir.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .encode_utf16()
        .count()
}

pub fn long_path_notice(dir: &Path) -> Option<String> {
    path_notice(path_len(dir), read_manifest(dir).is_some())
}

pub(super) fn path_notice(len: usize, converted: bool) -> Option<String> {
    match converted {
        true if len > LONG_GAME_PATH => Some(err_key("long_path_warning", &len.to_string())),
        false if len > MAX_GAME_PATH => Some(err_key("long_path_own_engine", &len.to_string())),
        _ => None,
    }
}

pub fn access_exe_name(player: &RgssPlayer) -> String {
    format!("{}{}.exe", player.stem, ACCESS_TAG)
}

pub fn converted_exe(dir: &Path) -> Option<String> {
    read_manifest(dir)
        .filter(|m| !m.pending)
        .map(|m| m.exe)
        .filter(|exe| dir.join(exe).is_file())
}

pub fn adoptable_exe(dir: &Path, current: &str) -> Option<String> {
    let access = converted_exe(dir)?;
    let player = format!("{}.exe", access.strip_suffix(".exe")?.strip_suffix(ACCESS_TAG)?);
    player.eq_ignore_ascii_case(current).then_some(access)
}

pub fn accessible_sha(dir: &Path) -> Option<String> {
    let m = read_manifest(dir)?;
    fs::read(dir.join(&m.exe)).ok().map(|b| git_blob_sha1(&b))
}

pub fn check(dir: &Path, offer: &Offer) -> Result<RgssPlayer, String> {
    let player = rgss_player(dir).ok_or_else(|| "not_compatible".to_string())?;
    let missing: Vec<&str> = offer
        .markers
        .iter()
        .map(|m| m.as_str())
        .filter(|m| !dir.join(m).exists())
        .collect();
    if !missing.is_empty() {
        return Err(err_key("convert_markers_missing", &missing.join(", ")));
    }
    let resumed = read_manifest(dir).is_some_and(|m| m.pending);
    if super::mkxp::has_mkxp_json(dir) && !resumed {
        return Err("convert_mkxp_json_present".to_string());
    }
    let access = access_exe_name(&player);
    if dir.join(&access).exists() && !resumed {
        return Err(err_key("convert_exe_exists", &access));
    }
    Ok(player)
}

pub fn bundle_dir(engine: &str) -> String {
    format!("{}/{}", ENGINE_ROOT, engine)
}

pub fn bundle_in(remote: &[ContentEntry], engine: &str) -> Option<Bundle> {
    let dir = bundle_dir(engine);
    let exe_path = format!("{}/{}", dir, ENGINE_FILE);
    let extras_prefix = format!("{}/{}/", dir, EXTRAS_DIR);
    let engine_entry = remote.iter().find(|e| e.path == exe_path)?.clone();
    let extras = remote
        .iter()
        .filter_map(|e| {
            e.path
                .strip_prefix(&extras_prefix)
                .filter(|n| !n.contains('/'))
                .map(|n| (n.to_string(), e.clone()))
        })
        .collect();
    Some(Bundle {
        engine: engine_entry,
        extras,
    })
}

fn manifest_path(dir: &Path) -> PathBuf {
    accessibility_dir(dir).join("data").join(MANIFEST)
}

pub fn read_manifest(dir: &Path) -> Option<Manifest> {
    let text = fs::read_to_string(manifest_path(dir)).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

fn write_manifest(dir: &Path, m: &Manifest) -> Result<(), String> {
    let path = manifest_path(dir);
    let rel = format!("data/{}", MANIFEST);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| io_error(&rel, "mkdir", &e))?;
    }
    let json = serde_json::to_string_pretty(m)
        .map_err(|e| err_key("err_io_write", &format!("{} ({})", rel, e)))?;
    let temp = path.with_extension("json.tmp");
    fs::File::create(&temp)
        .and_then(|mut file| {
            file.write_all(json.as_bytes())?;
            file.sync_all()
        })
        .and_then(|_| fs::rename(&temp, &path))
        .map_err(|e| io_error(&rel, "write", &e))
}

fn sha_of(path: &Path, rel: &str) -> Result<String, String> {
    fs::read(path)
        .map(|b| git_blob_sha1(&b))
        .map_err(|e| io_error(rel, "read", &e))
}

fn engine_json(stem: &str, soundfont: Option<&str>) -> String {
    let mut keys = vec![
        "    \"rgssVersion\": 1".to_string(),
        format!("    \"execName\": \"{}\"", stem),
    ];
    if let Some(sf) = soundfont {
        keys.push(format!("    \"midiSoundFont\": \"{}\"", sf));
    }
    format!("{{\n{}\n}}\n", keys.join(",\n"))
}

#[derive(Default)]
struct Done {
    exe: Option<PathBuf>,
    added: Vec<PathBuf>,
    json: bool,
    manifest: bool,
}

impl Done {
    fn undo(&self, dir: &Path) {
        if self.manifest {
            let _ = fs::remove_file(manifest_path(dir));
        }
        if self.json {
            let json = super::mkxp::mkxp_json(dir);
            let _ = fs::remove_file(json.with_extension(format!("json{}", BACKUP_SUFFIX)));
            let _ = fs::remove_file(json);
        }
        for f in &self.added {
            let _ = fs::remove_file(f);
        }
        if let Some(exe) = &self.exe {
            let _ = fs::remove_file(exe);
        }
    }
}

pub fn convert(
    dir: &Path,
    player: &RgssPlayer,
    engine_id: &str,
    profile: &str,
    engine: &[u8],
    extras: &[(String, Vec<u8>)],
    now: &str,
) -> Result<Manifest, String> {
    let mut done = Done::default();
    let result = convert_steps(
        dir, player, engine_id, profile, engine, extras, now, &mut done,
    );
    if result.is_err() {
        done.undo(dir);
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn convert_steps(
    dir: &Path,
    player: &RgssPlayer,
    engine_id: &str,
    profile: &str,
    engine: &[u8],
    extras: &[(String, Vec<u8>)],
    now: &str,
    done: &mut Done,
) -> Result<Manifest, String> {
    let added = extras
        .iter()
        .filter(|(name, _)| !dir.join(name).exists())
        .map(|(name, bytes)| (name.clone(), git_blob_sha1(bytes)))
        .collect();
    let mut m = Manifest {
        engine: engine_id.to_string(),
        profile: profile.to_string(),
        exe: access_exe_name(player),
        installed: git_blob_sha1(engine),
        added,
        mkxp_json: "created".to_string(),
        converted_at: now.to_string(),
        pending: true,
    };
    done.manifest = true;
    write_manifest(dir, &m)?;
    let exe = dir.join(&m.exe);
    done.exe = Some(exe.clone());
    fs::write(&exe, engine).map_err(|e| io_error(&m.exe, "write", &e))?;
    for (name, bytes) in extras.iter().filter(|(name, _)| m.added.contains_key(name)) {
        let target = dir.join(name);
        done.added.push(target.clone());
        fs::write(&target, bytes).map_err(|e| io_error(name, "write", &e))?;
    }
    let soundfont = soundfont_in(extras.iter().map(|(n, _)| n.as_str()));
    done.json = true;
    fs::write(super::mkxp::mkxp_json(dir), engine_json(&player.stem, soundfont))
        .map_err(|e| io_error("mkxp.json", "create", &e))?;
    m.pending = false;
    write_manifest(dir, &m)?;
    Ok(m)
}

pub fn refresh(dir: &Path, engine: Option<&[u8]>, extras: &[(String, Vec<u8>)]) -> Result<usize, String> {
    let Some(mut m) = read_manifest(dir) else {
        return Ok(0);
    };
    let before = m.clone();
    let written = put_engine_files(dir, &mut m, engine, extras);
    if m != before {
        write_manifest(dir, &m)?;
    }
    written
}

fn put_engine_files(
    dir: &Path,
    m: &mut Manifest,
    engine: Option<&[u8]>,
    extras: &[(String, Vec<u8>)],
) -> Result<usize, String> {
    if let Some(bytes) = engine {
        fs::write(dir.join(&m.exe), bytes).map_err(|e| io_error(&m.exe, "write", &e))?;
        m.installed = git_blob_sha1(bytes);
    }
    for (name, bytes) in extras {
        fs::write(dir.join(name), bytes).map_err(|e| io_error(name, "write", &e))?;
        m.added.insert(name.clone(), git_blob_sha1(bytes));
    }
    Ok(usize::from(engine.is_some()) + extras.len())
}

pub fn soundfont_in<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    names.into_iter().find(|n| {
        let n = n.to_ascii_lowercase();
        n.ends_with(".sf2") || n.ends_with(".sf3")
    })
}

pub fn restore_json(dir: &Path, soundfont: Option<&str>) -> Result<bool, String> {
    let m = match read_manifest(dir) {
        Some(m) => m,
        None => return Ok(false),
    };
    let json = super::mkxp::mkxp_json(dir);
    let stem = match m.exe.strip_suffix(".exe").and_then(|s| s.strip_suffix(ACCESS_TAG)) {
        Some(s) if m.mkxp_json == "created" && !json.exists() => s,
        _ => return Ok(false),
    };
    let font = soundfont.or_else(|| soundfont_in(m.added.keys().map(String::as_str)));
    fs::write(&json, engine_json(stem, font)).map_err(|e| io_error("mkxp.json", "create", &e))?;
    Ok(true)
}

pub fn missing_accessible_exe(dir: &Path) -> Option<String> {
    read_manifest(dir).filter(|m| m.pending || !dir.join(&m.exe).is_file()).map(|m| m.exe)
}

pub fn restore(dir: &Path) -> Result<Vec<String>, String> {
    let m = match read_manifest(dir) {
        Some(m) => m,
        None => return Ok(Vec::new()),
    };
    let mut notes = Vec::new();
    let exe = dir.join(&m.exe);
    if exe.is_file() {
        if m.pending || sha_of(&exe, &m.exe)? == m.installed {
            fs::remove_file(&exe).map_err(|e| io_error(&m.exe, "delete", &e))?;
            notes.push(err_key("convert_removed", &m.exe));
        } else {
            notes.push(err_key("convert_exe_changed", &m.exe));
        }
    }
    for (name, sha) in &m.added {
        let f = dir.join(name);
        if !f.is_file() {
            continue;
        }
        if m.pending || sha_of(&f, name)? == *sha {
            fs::remove_file(&f).map_err(|e| io_error(name, "delete", &e))?;
        } else {
            notes.push(err_key("convert_extra_kept", name));
        }
    }
    if m.mkxp_json == "created" {
        let json = super::mkxp::mkxp_json(dir);
        let _ = fs::remove_file(json.with_extension(format!("json{}", BACKUP_SUFFIX)));
        if json.exists() {
            fs::remove_file(&json).map_err(|e| io_error("mkxp.json", "delete", &e))?;
        }
    }
    fs::remove_file(manifest_path(dir))
        .map_err(|e| io_error(&format!("data/{}", MANIFEST), "delete", &e))?;
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER: &[u8] = b"MZ fake RGSS player";
    const ENGINE: &[u8] = b"MZ fake engine preloadScript";

    fn rgss_game(dir: &Path) {
        fs::write(dir.join("Game.exe"), PLAYER).unwrap();
        fs::write(
            dir.join("Game.ini"),
            "[Game]\r\nLibrary=RGSS102E.dll\r\nTitle=Pokemon Insurgence\r\n",
        )
        .unwrap();
        fs::write(dir.join("Game.rgssad"), b"RGSSAD\0\x01").unwrap();
        fs::write(dir.join("MGC_Hmode7.dll"), b"dll").unwrap();
    }

    fn offer() -> Offer {
        Offer {
            engine: "mkxp-z-1.3.0-x86".into(),
            markers: vec!["Game.rgssad".into(), "MGC_Hmode7.dll".into()],
            profile: "insurgence".into(),
            display: "Pokemon Insurgence".into(),
        }
    }

    fn catalog(convert: &str) -> Catalog {
        Catalog::from_json(&format!(
            r#"{{"profiles":[{{"key":"insurgence","display":"Pokemon Insurgence","titles":["pokemon insurgence"],
                "detect":"insurgence","exes":[],"engine":"gen6"{},"markers":["Game.rgssad"]}}]}}"#,
            convert
        ))
        .unwrap()
    }

    fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push((
                        p.strip_prefix(dir).unwrap().to_string_lossy().into_owned(),
                        fs::read(&p).unwrap(),
                    ));
                }
            }
        }
        out.sort();
        out
    }

    #[test]
    fn the_rgss1_player_is_recognised_by_its_own_ini_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        rgss_game(dir.path());
        let p = rgss_player(dir.path()).unwrap();
        assert_eq!(
            (p.exe.as_str(), p.stem.as_str(), p.library.as_str()),
            ("Game.exe", "Game", "RGSS102E.dll")
        );

        let named = tempfile::tempdir().unwrap();
        fs::write(named.path().join("Uranium.exe"), PLAYER).unwrap();
        fs::write(
            named.path().join("Uranium.ini"),
            "[Game]\nlibrary = rgss104j.dll\n",
        )
        .unwrap();
        let named_player = rgss_player(named.path()).unwrap();
        assert_eq!(
            (named_player.exe.as_str(), named_player.stem.as_str(), named_player.library.as_str()),
            ("Uranium.exe", "Uranium", "rgss104j.dll")
        );

        for library in ["RGSS202E.dll", "System\\RGSS301.dll"] {
            let other = tempfile::tempdir().unwrap();
            fs::write(other.path().join("Game.exe"), PLAYER).unwrap();
            fs::write(
                other.path().join("Game.ini"),
                format!("[Game]\nLibrary={}\n", library),
            )
            .unwrap();
            assert!(rgss_player(other.path()).is_none(), "{}", library);
        }
        let bare = tempfile::tempdir().unwrap();
        fs::write(bare.path().join("Game.exe"), PLAYER).unwrap();
        assert!(rgss_player(bare.path()).is_none());
    }

    #[test]
    fn a_conversion_is_offered_only_for_a_catalog_entry_that_names_an_engine() {
        let dir = tempfile::tempdir().unwrap();
        rgss_game(dir.path());
        let o = offer_for(&catalog(r#","convert":"mkxp-z-1.3.0-x86""#), dir.path()).unwrap();
        assert_eq!(
            (o.engine.as_str(), o.profile.as_str()),
            ("mkxp-z-1.3.0-x86", "insurgence")
        );
        assert_eq!(o.markers, vec!["Game.rgssad".to_string()]);
        assert!(offer_for(&catalog(""), dir.path()).is_none());
        assert_eq!(incompatible(dir.path()), "rgss_player_unsupported");

        fs::write(dir.path().join("mkxp.json"), "{}").unwrap();
        assert!(offer_for(&catalog(r#","convert":"mkxp-z-1.3.0-x86""#), dir.path()).is_none());
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(incompatible(empty.path()), "not_compatible");
    }

    #[test]
    fn the_check_refuses_missing_markers_an_mkxp_json_or_a_taken_name_but_not_a_long_path() {
        let dir = tempfile::tempdir().unwrap();
        rgss_game(dir.path());
        assert_eq!(check(dir.path(), &offer()).unwrap().exe, "Game.exe");

        let mut deep = dir.path().to_path_buf();
        while path_len(&deep) <= MAX_GAME_PATH {
            deep = deep.join("carpeta_de_juegos");
        }
        fs::create_dir_all(&deep).unwrap();
        rgss_game(&deep);
        assert_eq!(check(&deep, &offer()).unwrap().exe, "Game.exe");

        let mut o = offer();
        o.markers.push("Missing.dll".into());
        assert_eq!(
            check(dir.path(), &o).unwrap_err(),
            err_key("convert_markers_missing", "Missing.dll")
        );

        fs::write(dir.path().join("Game (PokeAccess).exe"), b"old").unwrap();
        assert_eq!(
            check(dir.path(), &offer()).unwrap_err(),
            err_key("convert_exe_exists", "Game (PokeAccess).exe")
        );
        fs::remove_file(dir.path().join("Game (PokeAccess).exe")).unwrap();
        fs::write(dir.path().join("mkxp.json"), "{}").unwrap();
        assert_eq!(
            check(dir.path(), &offer()).unwrap_err(),
            "convert_mkxp_json_present"
        );
    }

    #[test]
    fn a_path_longer_than_its_engine_reads_reliably_gets_its_own_warning() {
        let short = PathBuf::from("C:\\Juegos\\Pokemon Uranium\\");
        assert_eq!(path_len(&short), "C:\\Juegos\\Pokemon Uranium".len());
        assert!(long_path_notice(&short).is_none());
        assert_eq!(path_notice(126, false), None);
        assert_eq!(path_notice(127, false), Some(err_key("long_path_own_engine", "127")));
        assert_eq!(path_notice(200, true), None);
        assert_eq!(path_notice(201, true), Some(err_key("long_path_warning", "201")));
        assert_eq!(path_notice(250, false), Some(err_key("long_path_own_engine", "250")));

        let base = tempfile::tempdir().unwrap();
        let mut deep = base.path().to_path_buf();
        while path_len(&deep) <= MAX_GAME_PATH {
            deep = deep.join("carpeta_de_juegos");
        }
        fs::create_dir_all(&deep).unwrap();
        let len = path_len(&deep).to_string();
        assert_eq!(long_path_notice(&deep), Some(err_key("long_path_own_engine", &len)));
        rgss_game(&deep);
        convert(&deep, &rgss_player(&deep).unwrap(), "e", "insurgence", ENGINE, &[], "now").unwrap();
        assert!(long_path_notice(&deep).is_none(), "convertido, {len} caracteres no pasan de {LONG_GAME_PATH}");

        for (key, words) in [("long_path_own_engine", "sin voz"), ("long_path_warning", "Si algo no carga")] {
            let shown = crate::i18n::I18n::new("es").t_err(&err_key(key, "215"));
            assert!(shown.contains("(215 caracteres)") && shown.contains(words) && shown.contains("C:\\Juegos"), "{shown}");
        }
    }

    #[test]
    fn convert_then_restore_leaves_the_folder_byte_for_byte() {
        let dir = tempfile::tempdir().unwrap();
        rgss_game(dir.path());
        let before = snapshot(dir.path());
        let player = check(dir.path(), &offer()).unwrap();
        let extras = vec![
            ("fluidsynth.dll".to_string(), b"fs".to_vec()),
            ("soundfont.sf2".to_string(), b"sf".to_vec()),
        ];
        let m = convert(
            dir.path(),
            &player,
            "mkxp-z-1.3.0-x86",
            "insurgence",
            ENGINE,
            &extras,
            "now",
        )
        .unwrap();

        assert_eq!(fs::read(dir.path().join("Game.exe")).unwrap(), PLAYER);
        assert_eq!(
            fs::read(dir.path().join("Game (PokeAccess).exe")).unwrap(),
            ENGINE
        );
        assert!(!dir.path().join("Game.exe.access.bak").exists());
        let json = fs::read_to_string(dir.path().join("mkxp.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["rgssVersion"], 1);
        assert_eq!(parsed["midiSoundFont"], "soundfont.sf2");
        assert_eq!(parsed["execName"], "Game");
        assert_eq!(read_manifest(dir.path()).unwrap(), m);
        assert_eq!(m.exe, "Game (PokeAccess).exe");
        assert_eq!(m.installed, git_blob_sha1(ENGINE));
        assert_eq!(converted_exe(dir.path()).as_deref(), Some("Game (PokeAccess).exe"));

        super::super::mkxp::register(dir.path(), None).unwrap();
        let notes = restore(dir.path()).unwrap();
        assert_eq!(notes, vec![err_key("convert_removed", "Game (PokeAccess).exe")]);
        let _ = fs::remove_dir_all(accessibility_dir(dir.path()));
        assert_eq!(snapshot(dir.path()), before);
    }

    #[test]
    fn a_failed_step_takes_the_conversion_back() {
        let dir = tempfile::tempdir().unwrap();
        rgss_game(dir.path());
        let before = snapshot(dir.path());
        let player = check(dir.path(), &offer()).unwrap();
        fs::create_dir_all(dir.path().join("accessibility")).unwrap();
        fs::write(
            dir.path().join("accessibility").join("data"),
            b"a file where the record's folder goes",
        )
        .unwrap();
        let extras = vec![("fluidsynth.dll".to_string(), b"fs".to_vec())];
        assert!(convert(
            dir.path(),
            &player,
            "e",
            "insurgence",
            ENGINE,
            &extras,
            "now"
        )
        .is_err());
        fs::remove_dir_all(dir.path().join("accessibility")).unwrap();
        assert_eq!(snapshot(dir.path()), before);
    }

    #[test]
    fn an_accessible_exe_changed_after_the_conversion_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        rgss_game(dir.path());
        let player = check(dir.path(), &offer()).unwrap();
        convert(dir.path(), &player, "e", "insurgence", ENGINE, &[], "now").unwrap();
        let access = dir.path().join("Game (PokeAccess).exe");
        fs::write(&access, b"MZ changed by hand").unwrap();
        assert_eq!(
            restore(dir.path()).unwrap(),
            vec![err_key("convert_exe_changed", "Game (PokeAccess).exe")]
        );
        assert_eq!(fs::read(&access).unwrap(), b"MZ changed by hand");
        assert_eq!(fs::read(dir.path().join("Game.exe")).unwrap(), PLAYER);
        assert!(!dir.path().join("mkxp.json").exists());
        assert!(read_manifest(dir.path()).is_none());
    }

    #[test]
    fn a_newer_or_missing_engine_is_put_in_place_of_the_accessible_exe() {
        let dir = tempfile::tempdir().unwrap();
        rgss_game(dir.path());
        let player = check(dir.path(), &offer()).unwrap();
        convert(dir.path(), &player, "e", "insurgence", ENGINE, &[], "now").unwrap();
        let access = dir.path().join("Game (PokeAccess).exe");
        let newer: &[u8] = b"MZ newer engine preloadScript";
        assert_eq!(accessible_sha(dir.path()), Some(git_blob_sha1(ENGINE)));
        assert_eq!(refresh(dir.path(), Some(newer), &[]).unwrap(), 1);
        assert_eq!(fs::read(&access).unwrap(), newer);
        assert_eq!(
            read_manifest(dir.path()).unwrap().installed,
            git_blob_sha1(newer)
        );
        assert_eq!(refresh(dir.path(), None, &[]).unwrap(), 0);
        fs::remove_file(&access).unwrap();
        assert!(accessible_sha(dir.path()).is_none());
        assert_eq!(refresh(dir.path(), Some(newer), &[]).unwrap(), 1);
        assert_eq!(fs::read(&access).unwrap(), newer);
        assert_eq!(fs::read(dir.path().join("Game.exe")).unwrap(), PLAYER);
        assert_eq!(
            restore(dir.path()).unwrap(),
            vec![err_key("convert_removed", "Game (PokeAccess).exe")]
        );
        assert!(!access.exists());
        assert_eq!(fs::read(dir.path().join("Game.exe")).unwrap(), PLAYER);
    }

    #[test]
    fn without_a_record_nothing_is_taken_out() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Game.exe"), PLAYER).unwrap();
        fs::write(dir.path().join("Game (PokeAccess).exe"), ENGINE).unwrap();
        assert!(restore(dir.path()).unwrap().is_empty());
        assert!(converted_exe(dir.path()).is_none());
        assert_eq!(fs::read(dir.path().join("Game (PokeAccess).exe")).unwrap(), ENGINE);
        assert_eq!(fs::read(dir.path().join("Game.exe")).unwrap(), PLAYER);
    }

    #[test]
    fn a_renamed_player_gets_its_name_in_mkxp_json() {
        let named = tempfile::tempdir().unwrap();
        fs::write(named.path().join("Uranium.exe"), PLAYER).unwrap();
        fs::write(
            named.path().join("Uranium.ini"),
            "[Game]\nLibrary=RGSS102E.dll\n",
        )
        .unwrap();
        let player = rgss_player(named.path()).unwrap();
        let m = convert(named.path(), &player, "e", "generic", ENGINE, &[], "now").unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(named.path().join("mkxp.json")).unwrap())
                .unwrap();
        assert_eq!(parsed["execName"], "Uranium");
        assert!(parsed.get("midiSoundFont").is_none());
        assert_eq!(m.exe, "Uranium (PokeAccess).exe");
        assert_eq!(fs::read(named.path().join("Uranium.exe")).unwrap(), PLAYER);
        assert_eq!(
            converted_exe(named.path()).as_deref(),
            Some("Uranium (PokeAccess).exe")
        );
        assert_eq!(rgss_player(named.path()).unwrap().exe, "Uranium.exe");
    }

    #[test]
    fn the_bundle_is_its_engine_and_its_extras() {
        let entry = |path: &str, sha: &str| ContentEntry { path: path.to_string(), sha: sha.to_string() };
        let remote = vec![
            entry("assets/engine/e/mkxp-z.exe", "a"),
            entry("assets/engine/e/extras/soundfont.sf2", "b"),
            entry("assets/engine/e/LICENSE.txt", "c"),
            entry("assets/engine/f/mkxp-z.exe", "d"),
        ];
        let b = bundle_in(&remote, "e").unwrap();
        assert_eq!(b.engine.sha, "a");
        assert_eq!(
            b.extras.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            vec!["soundfont.sf2"]
        );
        assert!(bundle_in(&remote, "g").is_none());
        assert_eq!(bundle_dir("e"), "assets/engine/e");
    }

    #[test]
    fn a_missing_mkxp_json_of_a_conversion_comes_back_with_the_engine_keys() {
        let dir = tempfile::tempdir().unwrap();
        rgss_game(dir.path());
        let player = check(dir.path(), &offer()).unwrap();
        let extras = vec![("soundfont.sf2".to_string(), b"sf".to_vec())];
        convert(dir.path(), &player, "e", "insurgence", ENGINE, &extras, "now").unwrap();
        let json = dir.path().join("mkxp.json");
        let written = fs::read(&json).unwrap();
        assert!(!restore_json(dir.path(), None).unwrap(), "con su mkxp.json no se toca");
        fs::remove_file(&json).unwrap();
        assert!(restore_json(dir.path(), None).unwrap());
        assert_eq!(fs::read(&json).unwrap(), written, "vuelve con RGSS1, execName y la soundfont que se añadió");
        fs::remove_file(&json).unwrap();
        assert!(restore_json(dir.path(), Some("other.sf3")).unwrap());
        assert!(fs::read_to_string(&json).unwrap().contains("\"midiSoundFont\": \"other.sf3\""));

        let plain = tempfile::tempdir().unwrap();
        assert!(!restore_json(plain.path(), Some("x.sf2")).unwrap(), "sin registro no hay nada que rehacer");
        assert!(!plain.path().join("mkxp.json").exists());
    }

    #[test]
    fn a_converted_folder_without_its_accessible_exe_names_it() {
        let dir = tempfile::tempdir().unwrap();
        rgss_game(dir.path());
        assert!(missing_accessible_exe(dir.path()).is_none());
        let player = check(dir.path(), &offer()).unwrap();
        convert(dir.path(), &player, "e", "insurgence", ENGINE, &[], "now").unwrap();
        assert!(missing_accessible_exe(dir.path()).is_none());
        fs::remove_file(dir.path().join("Game (PokeAccess).exe")).unwrap();
        assert_eq!(missing_accessible_exe(dir.path()).as_deref(), Some("Game (PokeAccess).exe"));
    }

    fn two_extras() -> Vec<(String, Vec<u8>)> {
        vec![("fluidsynth.dll".to_string(), b"fs".to_vec()), ("soundfont.sf2".to_string(), b"sf".to_vec())]
    }

    #[test]
    fn the_record_reaches_the_disk_pending_before_the_engine_and_only_the_last_step_completes_it() {
        let dir = tempfile::tempdir().unwrap();
        rgss_game(dir.path());
        let before = snapshot(dir.path());
        let player = check(dir.path(), &offer()).unwrap();
        let done = convert(dir.path(), &player, "e", "insurgence", ENGINE, &two_extras(), "now").unwrap();
        assert!(!done.pending && !read_manifest(dir.path()).unwrap().pending);
        let text = fs::read_to_string(manifest_path(dir.path())).unwrap();
        assert!(!text.contains("pending"), "un registro completo se escribe como siempre: {text}");
        restore(dir.path()).unwrap();
        fs::remove_dir_all(accessibility_dir(dir.path())).unwrap();

        let access = dir.path().join("Game (PokeAccess).exe");
        fs::create_dir_all(access.join("sub")).unwrap();
        assert!(convert(dir.path(), &player, "e", "insurgence", ENGINE, &two_extras(), "now").is_err());
        let data = accessibility_dir(dir.path()).join("data");
        assert!(data.is_dir(), "el registro llegó al disco antes que el motor");
        assert_eq!(fs::read_dir(&data).unwrap().count(), 0, "y el fallo lo quita");
        assert!(!dir.path().join("fluidsynth.dll").exists() && !dir.path().join("mkxp.json").exists());
        fs::remove_dir_all(&access).unwrap();
        fs::remove_dir_all(accessibility_dir(dir.path())).unwrap();
        assert_eq!(snapshot(dir.path()), before);
    }

    #[test]
    fn a_conversion_cut_halfway_is_undone_whole_and_leaves_no_file_without_an_owner() {
        let owned = ["Game (PokeAccess).exe", "fluidsynth.dll", "soundfont.sf2", "mkxp.json"];
        let cuts: [(&str, &[&str]); 3] = [
            ("solo el registro", &[]),
            ("el motor a medias", &["Game (PokeAccess).exe"]),
            ("sin el cierre", &owned),
        ];
        for (cut, written) in cuts {
            let dir = tempfile::tempdir().unwrap();
            rgss_game(dir.path());
            let before = snapshot(dir.path());
            let player = check(dir.path(), &offer()).unwrap();
            let mut m = convert(dir.path(), &player, "e", "insurgence", ENGINE, &two_extras(), "now").unwrap();
            m.pending = true;
            write_manifest(dir.path(), &m).unwrap();
            for name in owned {
                if written.contains(&name) {
                    fs::write(dir.path().join(name), b"MZ").unwrap();
                } else {
                    fs::remove_file(dir.path().join(name)).unwrap();
                }
            }
            assert_eq!(converted_exe(dir.path()), None, "{cut}: a medias no hay exe accesible que abrir");
            assert_eq!(missing_accessible_exe(dir.path()).as_deref(), Some("Game (PokeAccess).exe"), "{cut}");
            assert_eq!(check(dir.path(), &offer()).unwrap().exe, "Game.exe", "{cut}: lo que estorba es de la conversión");
            let notes = restore(dir.path()).unwrap();
            assert!(notes.iter().all(|n| !n.starts_with("convert_exe_changed")), "{cut}: {notes:?}");
            fs::remove_dir_all(accessibility_dir(dir.path())).unwrap();
            assert_eq!(snapshot(dir.path()), before, "{cut}: la carpeta vuelve byte a byte");
        }
    }

    #[test]
    fn a_refresh_writes_what_it_is_given_and_the_record_owns_it() {
        let dir = tempfile::tempdir().unwrap();
        rgss_game(dir.path());
        let before = snapshot(dir.path());
        let player = check(dir.path(), &offer()).unwrap();
        convert(dir.path(), &player, "e", "insurgence", ENGINE, &two_extras(), "now").unwrap();
        fs::remove_file(dir.path().join("soundfont.sf2")).unwrap();
        let missing = vec![("soundfont.sf2".to_string(), b"sf".to_vec()), ("libogg.dll".to_string(), b"ogg".to_vec())];
        assert_eq!(refresh(dir.path(), None, &missing).unwrap(), 2);
        assert_eq!(fs::read(dir.path().join("soundfont.sf2")).unwrap(), b"sf");
        let added: Vec<String> = read_manifest(dir.path()).unwrap().added.into_keys().collect();
        assert_eq!(added, ["fluidsynth.dll", "libogg.dll", "soundfont.sf2"]);
        fs::create_dir_all(dir.path().join("broken.dll")).unwrap();
        let newer: &[u8] = b"MZ newer engine preloadScript";
        assert!(refresh(dir.path(), Some(newer), &[("broken.dll".to_string(), b"x".to_vec())]).is_err());
        let noted = read_manifest(dir.path()).unwrap().installed;
        assert_eq!(noted, git_blob_sha1(newer), "lo escrito antes del fallo queda anotado");
        fs::remove_dir_all(dir.path().join("broken.dll")).unwrap();
        assert_eq!(restore(dir.path()).unwrap(), vec![err_key("convert_removed", "Game (PokeAccess).exe")]);
        fs::remove_dir_all(accessibility_dir(dir.path())).unwrap();
        assert_eq!(snapshot(dir.path()), before);
    }

    #[test]
    fn the_soundfont_is_the_first_sf2_or_sf3() {
        assert_eq!(soundfont_in(["fluidsynth.dll", "GeneralUser-GS.SF2", "b.sf3"]), Some("GeneralUser-GS.SF2"));
        assert_eq!(soundfont_in(["fluidsynth.dll"]), None);
    }
}
