use std::path::{Component, Path, PathBuf};
use std::process::Command;

use super::config::GameEntry;
use super::{convert, detect};

pub fn executable_path(entry: &GameEntry) -> Result<PathBuf, &'static str> {
    let name = Path::new(&entry.executable);
    if name.components().count() != 1 || !matches!(name.components().next(), Some(Component::Normal(_)))
        || !entry.executable.to_ascii_lowercase().ends_with(".exe")
        || entry.executable.contains('/') || entry.executable.contains('\\')
    {
        return Err("invalid_executable");
    }
    Ok(Path::new(&entry.path).join(name))
}

pub fn validate(entry: &GameEntry) -> Vec<&'static str> {
    let mut errors = Vec::new();
    if entry.name.trim().is_empty() {
        errors.push("invalid_name");
    }
    if !Path::new(&entry.path).is_dir() {
        errors.push("invalid_game_path");
    }
    if entry.profile.trim().is_empty() {
        errors.push("invalid_profile");
    }
    if !entry.executable.is_empty() && !executable_path(entry).is_ok_and(|path| path.is_file()) {
        errors.push("invalid_executable");
    }
    errors
}

#[derive(Debug, PartialEq)]
pub enum Launch {
    Start,
    Blocked(String),
    Choose(Vec<String>),
}

pub fn launch_plan(entry: &GameEntry) -> Launch {
    let dir = Path::new(&entry.path);
    if let Some(exe) = convert::missing_accessible_exe(dir) {
        return Launch::Blocked(crate::i18n::err_key("convert_exe_missing", &exe));
    }
    let exes = detect::root_exe_names(dir);
    if exes.is_empty() || executable_path(entry).is_ok_and(|p| p.is_file()) {
        Launch::Start
    } else {
        Launch::Choose(exes)
    }
}

pub fn pick_executable(scan: &detect::ExeScan, dir: &Path, known_exes: &[String]) -> Option<String> {
    if let Some(exe) = convert::converted_exe(dir) {
        return Some(exe);
    }
    if scan.supports_preload {
        if let Some(name) = scan.main_exe.as_deref().and_then(|p| p.file_name()) {
            return Some(name.to_string_lossy().into_owned());
        }
    }
    detect::launch_exe(dir, known_exes)
}

pub fn adopt_accessible(entry: &mut GameEntry) -> bool {
    match convert::adoptable_exe(Path::new(&entry.path), &entry.executable) {
        Some(exe) => {
            entry.executable = exe;
            true
        }
        None => false,
    }
}

pub fn display_name(entry: &GameEntry) -> String {
    let name = entry.name.trim();
    if !name.is_empty() {
        return name.to_string();
    }
    Path::new(&entry.path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| entry.path.clone())
}

pub fn list_names(entries: &[GameEntry]) -> Vec<String> {
    let names: Vec<String> = entries.iter().map(display_name).collect();
    names
        .iter()
        .zip(entries)
        .map(|(name, e)| {
            let twins: Vec<&GameEntry> =
                entries.iter().zip(&names).filter(|(_, n)| n.eq_ignore_ascii_case(name)).map(|(t, _)| t).collect();
            if twins.len() > 1 {
                format!("{} ({})", name, telling_folder(e, &twins))
            } else {
                name.clone()
            }
        })
        .collect()
}

fn telling_folder(entry: &GameEntry, twins: &[&GameEntry]) -> String {
    let holder = |e: &GameEntry| {
        Path::new(&e.path).parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_lowercase())
    };
    let mine = holder(entry);
    let alone = mine.is_some() && twins.iter().filter(|t| holder(t) == mine).count() == 1;
    match Path::new(&entry.path).parent().and_then(|p| p.file_name()) {
        Some(name) if alone => name.to_string_lossy().into_owned(),
        _ => entry.path.clone(),
    }
}

pub fn launch(entry: &GameEntry) -> Result<(), String> {
    if !Path::new(&entry.path).is_dir() {
        return Err("invalid_game_path".into());
    }
    let exe = executable_path(entry).map_err(str::to_string)?;
    if !exe.is_file() {
        return Err("invalid_executable".into());
    }
    if detect::exe_running(&exe) {
        return Err("already_running".into());
    }
    Command::new(&exe)
        .current_dir(&entry.path)
        .spawn()
        .map(|_| ())
        .map_err(|e| crate::i18n::err_key("launch_failed", &e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(dir: &Path) -> GameEntry {
        GameEntry { path: dir.to_string_lossy().into_owned(), name: "My game".into(), executable: "Game.exe".into(), profile: "generic".into(), profile_mode: "generic".into() }
    }

    #[test]
    fn validates_all_fields_and_blocks_escaping_the_game_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = entry(dir.path());
        std::fs::write(dir.path().join("Game.exe"), b"test").unwrap();
        assert!(validate(&e).is_empty());
        for bad in ["../Game.exe", "sub\\Game.exe", "C:\\Game.exe", "Missing.exe", "not.exe.txt"] {
            e.executable = bad.into();
            assert!(validate(&e).contains(&"invalid_executable"), "{bad}");
        }
        e.executable.clear();
        assert!(validate(&e).is_empty(), "sin ejecutable se pregunta al jugar");
        e.executable = "Game.exe".into();
        e.name = " ".into();
        e.profile.clear();
        e.path = dir.path().join("missing").to_string_lossy().into_owned();
        assert_eq!(validate(&e), vec!["invalid_name", "invalid_game_path", "invalid_profile", "invalid_executable"]);
    }

    #[test]
    fn two_records_listed_alike_are_told_apart_by_their_folder() {
        let mk = |path: &str, name: &str| GameEntry { path: path.into(), name: name.into(), executable: String::new(), profile: "generic".into(), profile_mode: "generic".into() };
        let games = [mk("D:/A/Pokemon Z", ""), mk("E:/B/pokemon z", " "), mk("C:/Juegos/Uranium", "Uranium")];
        assert_eq!(list_names(&games), vec!["Pokemon Z (A)", "pokemon z (B)", "Uranium"]);
        let alike = [mk("D:/Juegos/Pokemon Z", ""), mk("E:/Juegos/Pokemon Z", ""), mk("F:/Otros/Pokemon Z", "")];
        assert_eq!(
            list_names(&alike),
            vec!["Pokemon Z (D:/Juegos/Pokemon Z)", "Pokemon Z (E:/Juegos/Pokemon Z)", "Pokemon Z (Otros)"],
            "la ruta entera solo donde las carpetas que los contienen se llaman igual"
        );
        assert_eq!(display_name(&games[1]), "pokemon z");
    }

    #[test]
    fn a_game_starts_from_its_record_asks_when_the_exe_is_gone_and_never_opens_a_conversion_without_its_exe() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = entry(dir.path());
        assert_eq!(launch_plan(&e), Launch::Start, "sin ejecutables no hay nada que preguntar");
        std::fs::write(dir.path().join("Royal.exe"), b"x").unwrap();
        assert_eq!(launch_plan(&e), Launch::Choose(vec!["Royal.exe".to_string()]));
        e.executable = "Royal.exe".into();
        assert_eq!(launch_plan(&e), Launch::Start);
        std::fs::write(dir.path().join("Game.exe"), b"MZ player").unwrap();
        std::fs::write(dir.path().join("Game.ini"), "[Game]\r\nLibrary=RGSS102E.dll\r\n").unwrap();
        let player = convert::rgss_player(dir.path()).unwrap();
        convert::convert(dir.path(), &player, "e", "insurgence", b"MZ engine preloadScript", &[], "now").unwrap();
        std::fs::remove_file(dir.path().join("Game (PokeAccess).exe")).unwrap();
        let blocked = launch_plan(&e);
        assert_eq!(blocked, Launch::Blocked(crate::i18n::err_key("convert_exe_missing", "Game (PokeAccess).exe")));
    }

    #[test]
    fn missing_executable_does_not_launch() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(launch(&entry(dir.path())).unwrap_err(), "invalid_executable");
    }

    #[test]
    fn failed_process_start_is_reported_to_the_ui() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Game.exe"), b"not a Windows executable").unwrap();
        let error = launch(&entry(dir.path())).unwrap_err();
        assert!(error.starts_with("launch_failed\u{1}"), "{error}");
    }

    #[test]
    fn the_executable_that_loads_the_mod_wins_over_a_plain_game_exe() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Game.exe"), b"RGSS player").unwrap();
        std::fs::write(dir.path().join("Engine.exe"), b"xx preloadScript yy").unwrap();
        let scan = detect::scan_exes(dir.path());
        assert_eq!(pick_executable(&scan, dir.path(), &[]).as_deref(), Some("Engine.exe"));
    }

    #[cfg(windows)]
    #[test]
    fn a_running_game_is_not_started_twice() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Game.exe");
        std::fs::write(&exe, b"bytes").unwrap();
        let _hold = std::fs::OpenOptions::new().read(true).share_mode(1).open(&exe).unwrap();
        assert_eq!(launch(&entry(dir.path())).unwrap_err(), "already_running");
    }

    #[test]
    fn the_only_game_beside_a_tool_is_picked_and_two_candidates_ask() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("setup.exe"), b"x").unwrap();
        std::fs::write(dir.path().join("Royal.exe"), b"x").unwrap();
        let scan = detect::scan_exes(dir.path());
        assert_eq!(pick_executable(&scan, dir.path(), &[]).as_deref(), Some("Royal.exe"));
        std::fs::write(dir.path().join("Royal Classic.exe"), b"x").unwrap();
        let scan = detect::scan_exes(dir.path());
        assert!(pick_executable(&scan, dir.path(), &[]).is_none());
    }

    type Exe = (&'static str, usize, bool);

    fn fill(dir: &Path, exes: &[Exe]) {
        for (name, kb, preload) in exes {
            let mut bytes = if *preload { b"MZ preloadScript".to_vec() } else { b"MZ player".to_vec() };
            bytes.resize(*kb, 0);
            std::fs::write(dir.join(name), bytes).unwrap();
        }
    }

    #[test]
    fn every_known_game_folder_starts_without_asking_and_from_the_right_exe() {
        let folders: [(&str, &[Exe], &[&str], &str); 17] = [
            ("africanvs", &[("animmaker.exe", 10, false), ("Editor.exe", 68, false), ("extendtext.exe", 5, false),
                            ("Game.exe", 11545, true), ("GameOld.exe", 68, false)], &[], "Game.exe"),
            ("anil", &[("Game.exe", 20569, true)], &[], "Game.exe"),
            ("awakening", &[("animmaker.exe", 10, false), ("extendtext.exe", 5, false), ("Game.exe", 11448, true),
                            ("Gamea.exe", 68, false), ("MapMaker.exe", 128, false), ("Positioner.exe", 68, false)],
             &[], "Game.exe"),
            ("emerald", &[("animmaker.exe", 10, false), ("extendtext.exe", 5, false), ("Game.exe", 14387, true)],
             &[], "Game.exe"),
            ("fire ash", &[("Game.exe", 15994, true)], &[], "Game.exe"),
            ("infinite fusion", &[("Game-performance.exe", 14824, true), ("Game.exe", 17925, true)], &[], "Game.exe"),
            ("if hoenn", &[("InfiniteFusion2-performance.exe", 14804, true), ("InfiniteFusion2.exe", 17999, true)],
             &["InfiniteFusion2.exe"], "InfiniteFusion2.exe"),
            ("insurgence", &[("Game.exe", 68, false)], &[], "Game.exe"),
            ("opalo", &[("Game.exe", 11448, true)], &[], "Game.exe"),
            ("uranium", &[("Patcher.exe", 628, false), ("Uranium.exe", 108, false)], &["Uranium.exe"], "Uranium.exe"),
            ("z 2.18", &[("Game.exe", 11448, true)], &[], "Game.exe"),
            ("armonia", &[("animmaker.exe", 10, false), ("Armonia_Juego.exe", 11448, true), ("Game.exe", 68, false)],
             &["Armonia_Juego.exe"], "Armonia_Juego.exe"),
            ("realidea", &[("Game (old).exe", 144, false), ("Game.exe", 11448, true)], &[], "Game.exe"),
            ("reborn", &[("Game.exe", 18857, true)], &[], "Game.exe"),
            ("rejuvenation", &[("Rejuvenation.exe", 20902, true)], &[], "Rejuvenation.exe"),
            ("relict", &[("Game.exe", 18914, true)], &[], "Game.exe"),
            ("soulstones 2", &[("animmaker.exe", 10, false), ("extendtext.exe", 5, false), ("Game.exe", 16200, true)],
             &[], "Game.exe"),
        ];
        for (game, exes, catalog, want) in folders {
            let dir = tempfile::tempdir().unwrap();
            fill(dir.path(), exes);
            let known: Vec<String> = catalog.iter().map(|e| e.to_string()).collect();
            let scan = detect::scan_exes(dir.path());
            assert_eq!(pick_executable(&scan, dir.path(), &known).as_deref(), Some(want), "{game}");
            assert_eq!(pick_executable(&scan, dir.path(), &[]).as_deref(), Some(want), "{game} sin catálogo");
        }
    }

    #[test]
    fn a_record_left_on_the_player_of_a_console_conversion_moves_to_the_accessible_exe_and_no_other_does() {
        let dir = tempfile::tempdir().unwrap();
        fill(dir.path(), &[("Patcher.exe", 628, false), ("Uranium.exe", 108, false)]);
        std::fs::write(dir.path().join("Uranium.ini"), "[Game]\r\nLibrary=RGSS102E.dll\r\n").unwrap();
        let mut e = entry(dir.path());
        e.executable = "uranium.EXE".into();
        assert!(!adopt_accessible(&mut e), "sin convertir no cambia");
        let player = convert::rgss_player(dir.path()).unwrap();
        convert::convert(dir.path(), &player, "e", "uranium", b"MZ engine preloadScript", &[], "now").unwrap();

        assert!(adopt_accessible(&mut e));
        assert_eq!(e.executable, "Uranium (PokeAccess).exe");
        assert!(!adopt_accessible(&mut e), "ya en el exe accesible");

        for other in ["Patcher.exe", ""] {
            e.executable = other.into();
            assert!(!adopt_accessible(&mut e), "{other:?}");
            assert_eq!(e.executable, other);
        }

        std::fs::remove_file(dir.path().join("Uranium (PokeAccess).exe")).unwrap();
        e.executable = "Uranium.exe".into();
        assert!(!adopt_accessible(&mut e), "sin el exe accesible no cambia");
    }

    #[test]
    fn a_converted_game_starts_from_its_accessible_exe() {
        let cases: [(&str, &[Exe], &str, &[&str], &str); 2] = [
            ("insurgence", &[("Game.exe", 68, false)], "Game.ini", &[], "Game (PokeAccess).exe"),
            ("uranium", &[("Patcher.exe", 628, false), ("Uranium.exe", 108, false)], "Uranium.ini", &["Uranium.exe"],
             "Uranium (PokeAccess).exe"),
        ];
        for (game, exes, ini, catalog, want) in cases {
            let dir = tempfile::tempdir().unwrap();
            fill(dir.path(), exes);
            std::fs::write(dir.path().join(ini), "[Game]\r\nLibrary=RGSS102E.dll\r\n").unwrap();
            let player = convert::rgss_player(dir.path()).unwrap();
            convert::convert(dir.path(), &player, "e", game, b"MZ engine preloadScript", &[], "now").unwrap();
            let known: Vec<String> = catalog.iter().map(|e| e.to_string()).collect();
            let scan = detect::scan_exes(dir.path());
            assert_eq!(pick_executable(&scan, dir.path(), &known).as_deref(), Some(want), "{game}");
            let blind = detect::ExeScan { main_exe: None, supports_preload: false };
            assert_eq!(pick_executable(&blind, dir.path(), &known).as_deref(), Some(want), "{game}: manda el registro");
        }
    }
}
