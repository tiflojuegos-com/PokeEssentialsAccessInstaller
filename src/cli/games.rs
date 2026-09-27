use std::path::{Path, PathBuf};

use crate::core::config::{same_path, GameEntry};
use crate::core::{detect, game, source};
use crate::i18n::err_key;

const EXE_LEVELS: usize = 5;

#[derive(Debug, PartialEq)]
pub enum Target {
    All,
    Listed(usize),
    Folder(PathBuf),
}

pub fn resolve(arg: &str, games: &[GameEntry], cwd: &Path) -> Result<Target, String> {
    if arg.eq_ignore_ascii_case("all") {
        return Ok(Target::All);
    }
    if arg.bytes().all(|b| b.is_ascii_digit()) {
        return match arg.parse::<usize>() {
            Ok(n) if (1..=games.len()).contains(&n) => Ok(Target::Listed(n - 1)),
            _ => Err(err_key("game_number_out", arg)),
        };
    }
    if arg.contains(['\\', '/', ':']) {
        return folder(&cwd.join(arg), games);
    }
    match by_name(arg, games) {
        Some(found) => found,
        None if cwd.join(arg).is_dir() => folder(&cwd.join(arg), games),
        None => Err(err_key("game_not_found", arg)),
    }
}

fn folder(path: &Path, games: &[GameEntry]) -> Result<Target, String> {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    if !path.is_dir() {
        return Err(err_key("folder_not_found", &path.display().to_string()));
    }
    let dir = detect::resolve_game_dir(&path);
    let listed = games.iter().position(|g| same_path(&g.path, &dir.to_string_lossy()));
    Ok(listed.map_or(Target::Folder(dir), Target::Listed))
}

fn by_name(arg: &str, games: &[GameEntry]) -> Option<Result<Target, String>> {
    let names = game::list_names(games);
    let wanted = arg.to_lowercase();
    let called = |i: &usize| {
        names[*i].to_lowercase() == wanted || game::display_name(&games[*i]).to_lowercase() == wanted
    };
    let mut found: Vec<usize> = (0..games.len()).filter(called).collect();
    if found.is_empty() {
        found = (0..games.len()).filter(|i| names[*i].to_lowercase().contains(&wanted)).collect();
    }
    match found.as_slice() {
        [] => None,
        [one] => Some(Ok(Target::Listed(*one))),
        many => {
            let told: Vec<String> = many.iter().map(|i| format!("{}. {}", i + 1, names[*i])).collect();
            Some(Err(err_key("game_ambiguous", &told.join(", "))))
        }
    }
}

pub fn mod_folder(from: Option<&Path>, cwd: &Path, exe_dir: &Path) -> Option<PathBuf> {
    if let Some(dir) = from {
        let dir = cwd.join(dir);
        return Some(std::path::absolute(&dir).unwrap_or(dir));
    }
    cwd.ancestors()
        .chain(exe_dir.ancestors().take(EXE_LEVELS + 1))
        .find(|dir| source::is_mod_folder(dir))
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn entry(path: &Path, name: &str) -> GameEntry {
        GameEntry {
            path: path.to_string_lossy().into_owned(),
            name: name.into(),
            executable: String::new(),
            profile: "generic".into(),
            profile_mode: "generic".into(),
        }
    }

    fn game_folder(root: &Path, name: &str) -> PathBuf {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Game.exe"), b"MZ").unwrap();
        dir
    }

    #[test]
    fn a_game_is_named_by_path_number_name_part_of_it_or_all() {
        let root = tempfile::tempdir().unwrap();
        let anil = game_folder(root.path(), "Pokemon Anil");
        let z_a = game_folder(&root.path().join("A"), "Pokemon Z");
        let z_b = game_folder(&root.path().join("B"), "Pokemon Z");
        let relict = root.path().join("Relict");
        let games = vec![entry(&anil, ""), entry(&z_a, ""), entry(&z_b, ""), entry(&relict, "Relict")];
        let cwd = root.path();
        assert_eq!(resolve("all", &games, cwd), Ok(Target::All));
        assert_eq!(resolve("ALL", &games, cwd), Ok(Target::All));
        assert_eq!(resolve("2", &games, cwd), Ok(Target::Listed(1)));
        assert!(resolve("5", &games, cwd).unwrap_err().starts_with("game_number_out"));
        assert!(resolve("0", &games, cwd).unwrap_err().starts_with("game_number_out"));
        assert_eq!(resolve(&anil.to_string_lossy(), &games, cwd), Ok(Target::Listed(0)));
        assert_eq!(resolve(&format!("{}\\", anil.display()), &games, cwd), Ok(Target::Listed(0)));
        assert_eq!(resolve("pokemon anil", &games, cwd), Ok(Target::Listed(0)));
        assert_eq!(resolve("anil", &games, cwd), Ok(Target::Listed(0)));
        assert_eq!(resolve("Pokemon Z (B)", &games, cwd), Ok(Target::Listed(2)));
        assert_eq!(resolve("relict", &games, cwd), Ok(Target::Listed(3)));
        let ambiguous = resolve("pokemon z", &games, cwd).unwrap_err();
        assert!(ambiguous.starts_with("game_ambiguous") && ambiguous.contains("2. Pokemon Z (A)"), "{ambiguous}");
        assert!(ambiguous.contains("3. Pokemon Z (B)"), "{ambiguous}");
        assert!(resolve("uranium", &games, cwd).unwrap_err().starts_with("game_not_found"));
        assert!(resolve("D:\\no\\existe", &games, cwd).unwrap_err().starts_with("folder_not_found"));
    }

    #[test]
    fn a_folder_off_the_list_is_the_game_folder_it_holds() {
        let root = tempfile::tempdir().unwrap();
        let outer = root.path().join("Nuevo");
        let inner = game_folder(&outer, "JUEGO");
        assert_eq!(resolve(&outer.to_string_lossy(), &[], root.path()), Ok(Target::Folder(inner.clone())));
        assert_eq!(resolve("Nuevo", &[], root.path()), Ok(Target::Folder(inner)), "una carpeta bajo la actual");
        let listed = [entry(&outer.join("JUEGO"), "")];
        assert_eq!(resolve(&outer.to_string_lossy(), &listed, root.path()), Ok(Target::Listed(0)));
    }

    #[test]
    fn local_finds_the_mod_in_the_zip_or_in_the_repository_and_nowhere_else() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("PokeEssentialsAccess");
        for mark in ["version.json", "core/manifest.rb", "games/catalog.json", "loader/preload_access.rb"] {
            let path = repo.join(mark);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
        let elsewhere = root.path().join("otra");
        fs::create_dir_all(&elsewhere).unwrap();
        let release = repo.join("launcher").join("target").join("x86_64-pc-windows-msvc").join("release");
        for exe_dir in [repo.clone(), release] {
            assert_eq!(mod_folder(None, &elsewhere, &exe_dir), Some(repo.clone()), "{}", exe_dir.display());
        }
        assert_eq!(mod_folder(None, &repo.join("games").join("anil"), &elsewhere), Some(repo.clone()));
        assert_eq!(mod_folder(None, &elsewhere, &elsewhere), None);
        assert_eq!(mod_folder(Some(Path::new("otra")), root.path(), &elsewhere), Some(elsewhere.clone()));
    }
}
