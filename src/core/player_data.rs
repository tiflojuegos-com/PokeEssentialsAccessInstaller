use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zip::result::{ZipError, ZipResult};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::apply::io_error;
use super::catalog::Catalog;
use super::installed;
use super::paths::data_dir;
use crate::i18n::{err_key, I18n};

const SETTINGS: &str = "settings.ini";
const VERBOSITY: &str = "verbosity.txt";
const BOUND: [&str; 3] = ["tags.txt", "marks.txt", "map_names.txt"];
const FILES: [(&str, &str); 5] = [
    (SETTINGS, "data_settings"),
    (VERBOSITY, "data_verbosity"),
    (BOUND[0], "data_tags"),
    (BOUND[1], "data_marks"),
    (BOUND[2], "data_map_names"),
];
const RECORDINGS: &str = "recordings";
const MANIFEST: &str = "manifest.json";
const MAX_ENTRY: u64 = 16 << 20;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Manifest {
    #[serde(default)]
    mod_version: String,
    #[serde(default)]
    profile: String,
    #[serde(default)]
    stamps: BTreeMap<String, String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Imported {
    pub now: Vec<&'static str>,
    pub backup: Option<PathBuf>,
    pub merging: Vec<&'static str>,
    pub checking: Vec<&'static str>,
    pub refused: Vec<(&'static str, String)>,
}

impl Imported {
    pub fn lines(&self, i18n: &I18n, cat: Option<&Catalog>) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.now.is_empty() {
            lines.push(i18n.tf("import_now", &listed(i18n, &self.now)));
        }
        if let Some(backup) = &self.backup {
            lines.push(i18n.tf("import_backup", &backup.display().to_string()));
        }
        if !self.merging.is_empty() {
            lines.push(i18n.tf("import_merge", &listed(i18n, &self.merging)));
        }
        if !self.checking.is_empty() {
            lines.push(i18n.tf("import_check", &listed(i18n, &self.checking)));
        }
        let mut owners: Vec<&str> = Vec::new();
        for (_, owner) in &self.refused {
            if !owners.contains(&owner.as_str()) {
                owners.push(owner);
            }
        }
        for owner in owners {
            let labels: Vec<&str> = self.refused.iter().filter(|(_, o)| o == owner).map(|(label, _)| *label).collect();
            let game = i18n.t_err(&owner_title(cat, owner));
            lines.push(i18n.tfn("import_refused", &[&game, &listed(i18n, &labels)]));
        }
        lines
    }
}

pub fn saved(game_dir: &Path) -> Vec<&'static str> {
    let data = data_dir(game_dir);
    FILES.iter().filter(|(name, _)| data.join(name).is_file()).map(|&(_, label)| label).collect()
}

pub fn kept(game_dir: &Path) -> Vec<&'static str> {
    let mut labels = saved(game_dir);
    if fs::read_dir(data_dir(game_dir).join(RECORDINGS)).is_ok_and(|mut found| found.next().is_some()) {
        labels.push("data_recordings");
    }
    labels
}

pub fn listed(i18n: &I18n, labels: &[&str]) -> String {
    labels.iter().map(|label| i18n.t(label)).collect::<Vec<_>>().join(", ")
}

pub fn export_name(name: &str) -> String {
    let safe: String =
        name.trim().chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
    format!("{} - PokeAccess.zip", safe)
}

pub fn export(game_dir: &Path, target: &Path) -> Result<Vec<&'static str>, String> {
    let data = data_dir(game_dir);
    let sealed = installed::read(game_dir);
    let own = installed::specific_profile(game_dir);
    let mut manifest = Manifest {
        mod_version: sealed.as_ref().map(|s| s.mod_version.clone()).unwrap_or_default(),
        profile: sealed.map(|s| s.profile).unwrap_or_default(),
        stamps: BTreeMap::new(),
    };
    let shown = target.display().to_string();
    let failed = |e: &dyn std::fmt::Display| err_key("err_io_write", &format!("{} ({})", shown, e));
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let mut labels = Vec::new();
    for (name, label) in FILES {
        let bytes = match fs::read(data.join(name)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(io_error(&format!("data/{}", name), "read", &e)),
        };
        if let Some(stamp) = BOUND.contains(&name).then(|| stamp_of(&bytes).or_else(|| own.clone())).flatten() {
            manifest.stamps.insert(name.to_string(), stamp);
        }
        add(&mut zip, name, &bytes).map_err(|e| failed(&e))?;
        labels.push(label);
    }
    if labels.is_empty() {
        return Ok(labels);
    }
    let json = serde_json::to_vec_pretty(&manifest).map_err(|e| failed(&e))?;
    add(&mut zip, MANIFEST, &json).map_err(|e| failed(&e))?;
    let bytes = zip.finish().map_err(|e| failed(&e))?.into_inner();
    fs::write(target, bytes).map_err(|e| io_error(&shown, "write", &e))?;
    Ok(labels)
}

fn add(zip: &mut ZipWriter<Cursor<Vec<u8>>>, name: &str, bytes: &[u8]) -> ZipResult<()> {
    zip.start_file(name, SimpleFileOptions::default().compression_method(CompressionMethod::Deflated))?;
    zip.write_all(bytes)?;
    Ok(())
}

pub fn import(game_dir: &Path, source: &Path) -> Result<Imported, String> {
    let shown = source.display().to_string();
    let file = fs::File::open(source).map_err(|e| io_error(&shown, "read", &e))?;
    let mut archive = ZipArchive::new(file).map_err(|_| err_key("import_bad_zip", &shown))?;
    let manifest: Manifest = entry(&mut archive, MANIFEST, &shown)?
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let mut found = Vec::new();
    for (name, label) in FILES {
        if let Some(bytes) = entry(&mut archive, name, &shown)? {
            found.push((name, label, bytes));
        }
    }
    if found.is_empty() {
        return Err(err_key("import_bad_zip", &shown));
    }
    let data = data_dir(game_dir);
    fs::create_dir_all(&data).map_err(|e| io_error("data", "mkdir", &e))?;
    let own = installed::specific_profile(game_dir);
    let mut done = Imported::default();
    for (name, label, bytes) in found {
        if name == SETTINGS {
            done.backup = back_up_settings(&data)?;
            write(&data, name, &bytes)?;
            done.now.push(label);
            continue;
        }
        let stamp = match name {
            VERBOSITY => None,
            _ => stamp_of(&bytes).or_else(|| manifest.stamps.get(name).cloned()),
        };
        match stamp.map(|from| (target_stamp(&data, name, own.as_deref()), from)) {
            Some((Some(mine), from)) if !fits(&from, &mine) => {
                done.refused.push((label, from));
                continue;
            }
            Some((None, _)) => done.checking.push(label),
            _ => done.merging.push(label),
        }
        write(&data, &format!("{}_import.txt", name.trim_end_matches(".txt")), &bytes)?;
    }
    Ok(done)
}

fn entry(archive: &mut ZipArchive<fs::File>, name: &str, shown: &str) -> Result<Option<Vec<u8>>, String> {
    let bad = || err_key("import_bad_zip", shown);
    let file = match archive.by_name(name) {
        Ok(file) => file,
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(_) => return Err(bad()),
    };
    if file.size() > MAX_ENTRY {
        return Err(bad());
    }
    let mut bytes = Vec::new();
    file.take(MAX_ENTRY).read_to_end(&mut bytes).map_err(|_| bad())?;
    Ok(Some(bytes))
}

fn back_up_settings(data: &Path) -> Result<Option<PathBuf>, String> {
    let current = data.join(SETTINGS);
    if !current.is_file() {
        return Ok(None);
    }
    let backup = data.join(format!("{}.bak", SETTINGS));
    fs::copy(&current, &backup).map_err(|e| io_error(&format!("data/{}.bak", SETTINGS), "write", &e))?;
    Ok(Some(backup))
}

fn write(data: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    fs::write(data.join(name), bytes).map_err(|e| io_error(&format!("data/{}", name), "write", &e))
}

fn stamp_of(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    for line in text.trim_start_matches('\u{feff}').lines().map(str::trim).filter(|line| !line.is_empty()) {
        let comment = line.strip_prefix('#')?;
        if let Some(game) = comment.trim_start().strip_prefix("game:").map(str::trim).filter(|g| !g.is_empty()) {
            return Some(game.to_string());
        }
    }
    None
}

fn target_stamp(data: &Path, name: &str, own: Option<&str>) -> Option<String> {
    std::iter::once(name)
        .chain(BOUND.into_iter().filter(|other| *other != name))
        .find_map(|file| fs::read(data.join(file)).ok().and_then(|bytes| stamp_of(&bytes)))
        .or_else(|| own.map(str::to_string))
}

fn fits(stamp: &str, mine: &str) -> bool {
    stamp == mine || (stamp == "generic" && mine.starts_with("generic:"))
}

fn owner_title(cat: Option<&Catalog>, stamp: &str) -> String {
    stamp.strip_prefix("generic:").map_or_else(|| super::ops::profile_title(cat, stamp), str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::paths::accessibility_dir;

    fn tree(root: &Path, files: &[(&str, &str)]) {
        for (rel, text) in files {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    }

    fn stamped(game: &str, line: &str) -> String {
        format!("# PokeAccess: cabecera\n# Comparte este archivo\n# game: {game}\n{line}\n")
    }

    fn sealed(dir: &Path, profile: &str) {
        let mode = if profile == "generic" { "generic" } else { "specific" };
        super::super::apply::seal_installed(dir, "0.6.0", profile, mode, "x86", "now", BTreeMap::new()).unwrap();
    }

    fn zip_of(path: &Path, files: &[(&str, &str)]) {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, text) in files {
            add(&mut zip, name, text.as_bytes()).unwrap();
        }
        fs::write(path, zip.finish().unwrap().into_inner()).unwrap();
    }

    fn names_in(path: &Path) -> Vec<String> {
        let archive = ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        let mut names: Vec<String> = archive.file_names().map(str::to_string).collect();
        names.sort();
        names
    }

    fn read(dir: &Path, name: &str) -> String {
        fs::read_to_string(data_dir(dir).join(name)).unwrap_or_default()
    }

    #[test]
    fn the_players_data_leaves_in_one_zip_with_its_manifest_and_nothing_of_the_installation() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("Anil");
        sealed(&game, "anil");
        tree(&data_dir(&game), &[
            ("settings.ini", "voz=1"),
            ("verbosity.txt", "corto=lectura:brief"),
            ("tags.txt", &stamped("anil", "1:2=Cofre")),
            ("marks.txt", "# sin sello\n3:4=Puerta\n"),
            ("recordings/r1.txt", "grabado"),
            ("diag.txt", "estado"),
            ("hook_loaded.txt", "1"),
        ]);
        let zip = root.path().join("datos.zip");
        let labels = export(&game, &zip).unwrap();
        assert_eq!(labels, vec!["data_settings", "data_verbosity", "data_tags", "data_marks"]);
        assert_eq!(names_in(&zip), ["manifest.json", "marks.txt", "settings.ini", "tags.txt", "verbosity.txt"]);
        let mut archive = ZipArchive::new(fs::File::open(&zip).unwrap()).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&entry(&mut archive, MANIFEST, "datos.zip").unwrap().unwrap()).unwrap();
        assert_eq!(manifest["mod_version"], "0.6.0");
        assert_eq!(manifest["profile"], "anil");
        assert_eq!(manifest["stamps"], serde_json::json!({"tags.txt": "anil", "marks.txt": "anil"}));
        assert_eq!(entry(&mut archive, "tags.txt", "datos.zip").unwrap().unwrap(), stamped("anil", "1:2=Cofre").into_bytes());
        assert_eq!(kept(&game), vec!["data_settings", "data_verbosity", "data_tags", "data_marks", "data_recordings"]);
    }

    #[test]
    fn a_game_without_data_writes_no_zip() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("Nuevo");
        tree(&data_dir(&game), &[("installed.json", "{}"), ("recordings/r1.txt", "x")]);
        assert!(saved(&game).is_empty());
        let zip = root.path().join("datos.zip");
        assert!(export(&game, &zip).unwrap().is_empty());
        assert!(!zip.exists());
    }

    #[test]
    fn data_of_the_same_game_is_left_for_the_mod_to_merge_and_the_settings_replace_the_old_ones() {
        let root = tempfile::tempdir().unwrap();
        let zip = root.path().join("datos.zip");
        zip_of(&zip, &[
            ("manifest.json", r#"{"mod_version":"0.6.0","profile":"anil","stamps":{"tags.txt":"anil"}}"#),
            ("settings.ini", "voz=2"),
            ("verbosity.txt", "corto=lectura:brief"),
            ("tags.txt", &stamped("anil", "1:2=Cofre")),
            ("marks.txt", &stamped("anil", "3:4=Puerta")),
        ]);
        let game = root.path().join("Anil");
        tree(&data_dir(&game), &[("settings.ini", "voz=1"), ("tags.txt", &stamped("anil", "5:6=Mesa"))]);
        let done = import(&game, &zip).unwrap();
        let backup = data_dir(&game).join("settings.ini.bak");
        assert_eq!(done, Imported {
            now: vec!["data_settings"],
            backup: Some(backup),
            merging: vec!["data_verbosity", "data_tags", "data_marks"],
            checking: Vec::new(),
            refused: Vec::new(),
        });
        assert_eq!((read(&game, "settings.ini"), read(&game, "settings.ini.bak")), ("voz=2".into(), "voz=1".into()));
        assert_eq!(read(&game, "verbosity_import.txt"), "corto=lectura:brief");
        assert_eq!(read(&game, "tags_import.txt"), stamped("anil", "1:2=Cofre"));
        assert_eq!(read(&game, "marks_import.txt"), stamped("anil", "3:4=Puerta"), "el sello de tags.txt vale para todo el juego");
        assert_eq!(read(&game, "tags.txt"), stamped("anil", "5:6=Mesa"), "lo fusiona el mod, no el instalador");

        let fresh = root.path().join("Anil limpio");
        sealed(&fresh, "anil");
        let done = import(&fresh, &zip).unwrap();
        assert_eq!((done.backup, done.merging.len()), (None, 3), "sin fichero, el perfil sellado");
    }

    #[test]
    fn another_game_s_bound_data_is_refused_and_named_after_the_game_it_belongs_to() {
        let root = tempfile::tempdir().unwrap();
        let zip = root.path().join("datos.zip");
        zip_of(&zip, &[
            ("verbosity.txt", "corto=lectura:brief"),
            ("tags.txt", &stamped("anil", "1:2=Cofre")),
            ("map_names.txt", &stamped("generic:Mi Juego", "7=Casa")),
        ]);
        let game = root.path().join("Opalo");
        sealed(&game, "opalo");
        let done = import(&game, &zip).unwrap();
        assert_eq!(done.merging, vec!["data_verbosity"]);
        assert_eq!(done.refused, vec![("data_tags", "anil".to_string()), ("data_map_names", "generic:Mi Juego".to_string())]);
        assert!(!data_dir(&game).join("tags_import.txt").exists() && !data_dir(&game).join("map_names_import.txt").exists());
        let cat = Catalog::from_json(r#"{"profiles":[{"key":"anil","display":"Pokemon Anil"}]}"#).unwrap();
        let lines = done.lines(&I18n::new("es"), Some(&cat));
        assert_eq!(lines, vec![
            "Se fusionarán al abrir el juego: esquemas de verbosidad.".to_string(),
            "Rechazados, porque son de Pokemon Anil: etiquetas.".to_string(),
            "Rechazados, porque son de Mi Juego: nombres de mapa.".to_string(),
        ]);
    }

    #[test]
    fn a_generic_game_without_a_stamped_file_keeps_the_bound_data_for_the_mod_to_check() {
        let root = tempfile::tempdir().unwrap();
        let zip = root.path().join("datos.zip");
        zip_of(&zip, &[("tags.txt", &stamped("generic:Mi Juego", "1:2=Cofre")), ("marks.txt", &stamped("anil", "3:4=Puerta"))]);
        let game = root.path().join("Mi Juego");
        sealed(&game, "generic");
        tree(&data_dir(&game), &[("settings.ini", "voz=1")]);
        let done = import(&game, &zip).unwrap();
        assert_eq!(done.checking, vec!["data_tags", "data_marks"]);
        assert!(done.refused.is_empty() && done.merging.is_empty());
        assert_eq!(read(&game, "tags_import.txt"), stamped("generic:Mi Juego", "1:2=Cofre"));
        let line = done.lines(&I18n::new("es"), None).join(" ");
        assert!(line.contains("comprobará") && line.contains("etiquetas, marcadores"), "{line}");

        tree(&data_dir(&game), &[("tags.txt", &stamped("generic:Mi Juego", "9:9=Viejo"))]);
        let done = import(&game, &zip).unwrap();
        assert_eq!((done.merging, done.refused), (vec!["data_tags"], vec![("data_marks", "anil".to_string())]));
        let legacy = root.path().join("antiguo.zip");
        zip_of(&legacy, &[("tags.txt", &stamped("generic", "1:2=Cofre"))]);
        assert_eq!(import(&game, &legacy).unwrap().merging, vec!["data_tags"], "el sello genérico de antes de 0.4.6");
    }

    #[test]
    fn a_zip_without_some_files_brings_the_rest_and_one_without_any_is_not_a_data_file() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("Anil");
        sealed(&game, "anil");
        let partial = root.path().join("parcial.zip");
        zip_of(&partial, &[("marks.txt", "3:4=Puerta\n")]);
        let done = import(&game, &partial).unwrap();
        assert_eq!(done, Imported { merging: vec!["data_marks"], ..Imported::default() }, "sin sello ni manifiesto, lo decide el mod");
        assert!(!data_dir(&game).join("settings.ini").exists());
        let es = I18n::new("es");
        let other = root.path().join("otro.zip");
        zip_of(&other, &[("leeme.txt", "hola")]);
        let err = import(&game, &other).unwrap_err();
        assert!(err.starts_with("import_bad_zip") && es.t_err(&err).contains("otro.zip"), "{err}");
        let text = root.path().join("texto.zip");
        fs::write(&text, "no es un zip").unwrap();
        assert!(import(&game, &text).unwrap_err().starts_with("import_bad_zip"));
        let gone = import(&game, &root.path().join("no_existe.zip")).unwrap_err();
        assert!(gone.starts_with("err_io_read") && es.t_err(&gone).contains("no_existe.zip"), "{gone}");
    }

    #[test]
    fn the_stamp_is_the_game_line_of_the_leading_comments() {
        assert_eq!(stamp_of(stamped("anil", "1=a").as_bytes()).as_deref(), Some("anil"));
        assert_eq!(stamp_of("\u{feff}#game:  generic:Pokemon X  \r\n1=a".as_bytes()).as_deref(), Some("generic:Pokemon X"));
        assert_eq!(stamp_of(b"1=a\n# game: anil\n"), None, "solo la cabecera");
        assert_eq!(stamp_of(b"# game:   \n1=a"), None);
        assert!(fits("anil", "anil") && fits("generic", "generic:Pokemon X"));
        assert!(!fits("anil", "opalo") && !fits("generic:Pokemon X", "generic:Pokemon Y") && !fits("generic:X", "generic"));
    }

    #[test]
    fn an_uninstall_that_keeps_the_data_leaves_what_it_lists() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("Anil");
        sealed(&game, "anil");
        tree(&accessibility_dir(&game), &[
            ("core/a.rb", "a"),
            ("data/settings.ini", "voz=1"),
            ("data/map_names.txt", &stamped("anil", "7=Casa")),
            ("data/recordings/r1.txt", "grabado"),
        ]);
        assert!(super::super::apply::run_uninstall(&game, true).unwrap().is_empty());
        assert!(!installed::is_installed(&game) && !accessibility_dir(&game).join("core").exists());
        let left = kept(&game);
        assert_eq!(left, vec!["data_settings", "data_map_names", "data_recordings"]);
        let es = I18n::new("es");
        let told = es.tfn("uninstall_kept_data", &["Anil", "D:\\Anil\\accessibility\\data", &listed(&es, &left)]);
        assert_eq!(told, "Anil: desinstalado. Se conservan en D:\\Anil\\accessibility\\data: ajustes, nombres de mapa, grabaciones.");
        assert!(super::super::apply::run_uninstall(&game, false).unwrap().is_empty());
        assert!(kept(&game).is_empty());
    }

    #[test]
    fn the_file_is_named_after_the_game_without_what_windows_refuses() {
        assert_eq!(export_name("Pokemon Z (A)"), "Pokemon Z (A) - PokeAccess.zip");
        assert_eq!(export_name(" Añil: 2/3? "), "Añil_ 2_3_ - PokeAccess.zip");
    }
}
