use std::path::{Path, PathBuf};

use super::args::Request;
use super::games::{self, Target};
use super::out::Out;
use super::{
    code_of, current_dir, folder_exe, install, known_profile, list_names, mode_of, new_record, open_source, refuse,
    FAILED, OK, USAGE,
};
use crate::core::batch::{Job, Outcome};
use crate::core::catalog::Catalog;
use crate::core::config::{Config, GameEntry};
use crate::core::ops::{self, Inspection, ScannedGame};
use crate::core::source::Source;
use crate::core::{game, installed};

pub fn list(out: &Out, req: &Request, cfg: &mut Config) -> i32 {
    if cfg.games.is_empty() {
        out.result(&out.t("list_empty"));
        return OK;
    }
    let cat = Catalog::cached();
    for (i, (entry, name)) in cfg.games.iter().zip(list_names(cfg)).enumerate() {
        let dir = Path::new(&entry.path);
        let profile = out.say(&ops::profile_title(cat.as_ref(), &entry.profile));
        let state = match installed::read(dir).filter(|sealed| !sealed.mod_version.is_empty()) {
            _ if !dir.is_dir() => out.t("status_missing"),
            Some(sealed) => out.tfn("list_state_installed", &[&sealed.mod_version, &profile]),
            None => out.tf("list_state_not_installed", &profile),
        };
        let mut line = out.tfn("list_row", &[&(i + 1).to_string(), &name, &state]);
        if req.paths {
            line = format!("{} {}", line, out.tf("list_path", &entry.path));
        }
        out.result(&line);
    }
    OK
}

pub fn add(out: &Out, req: &Request, cfg: &mut Config) -> i32 {
    let arg = match &req.game {
        Some(arg) => arg.clone(),
        None => match out.pick_folder() {
            Ok(dir) => dir.to_string_lossy().into_owned(),
            Err(code) => return code,
        },
    };
    let dir = match games::resolve(&arg, &cfg.games, &current_dir()) {
        Ok(Target::Folder(dir)) => dir,
        Ok(Target::Listed(i)) => {
            out.fail(&format!("{}: {}", list_names(cfg)[i], out.t("duplicate_game")));
            return USAGE;
        }
        Ok(Target::All) => {
            out.fail(&out.tf("game_not_found", &arg));
            return USAGE;
        }
        Err(e) => {
            out.fail(&out.say(&e));
            return USAGE;
        }
    };
    let source = Source::github();
    let cat = source.catalog().ok();
    let game = Inspection::of(dir, cat.as_ref(), &source);
    let entry = match new_record(out, req, cat.as_ref(), &game) {
        Ok(entry) => entry,
        Err(code) => return code,
    };
    let name = game::display_name(&entry);
    let profile = out.say(&ops::profile_title(cat.as_ref(), &entry.profile));
    match cfg.commit(|c| {
        c.upsert_game(entry);
        Ok(())
    }) {
        Ok(()) => {
            out.result(&out.tfn("added_to_list", &[&name, &cfg.games.len().to_string(), &profile]));
            OK
        }
        Err(e) => {
            out.fail(&out.tf("edit_save_failed", &out.say(&e)));
            FAILED
        }
    }
}

pub fn remove(out: &Out, req: &Request, cfg: &mut Config) -> i32 {
    let i = match listed_game(out, req, cfg) {
        Ok(i) => i,
        Err(code) => return code,
    };
    let (path, name) = (cfg.games[i].path.clone(), list_names(cfg).swap_remove(i));
    match cfg.commit(|c| {
        c.remove_game(&path);
        Ok(())
    }) {
        Ok(()) => {
            out.result(&out.tf("list_removed", &name));
            OK
        }
        Err(e) => {
            out.fail(&out.tf("remove_save_failed", &out.say(&e)));
            FAILED
        }
    }
}

pub fn set(out: &Out, req: &Request, cfg: &mut Config) -> i32 {
    if req.profile.is_none() && req.name.is_none() && req.exe.is_none() {
        out.fail(&out.t("usage_set_nothing"));
        return USAGE;
    }
    let i = match listed_game(out, req, cfg) {
        Ok(i) => i,
        Err(code) => return code,
    };
    let name = list_names(cfg).swap_remove(i);
    let mut entry = cfg.games[i].clone();
    if let Some(new_name) = &req.name {
        entry.name = new_name.trim().to_string();
    }
    if let Some(exe) = &req.exe {
        match folder_exe(out, Path::new(&entry.path), exe) {
            Ok(exe) => entry.executable = exe,
            Err(code) => return code,
        }
    }
    let cat = req.profile.as_ref().and_then(|_| Source::github().catalog().ok());
    if let Some(asked) = &req.profile {
        match known_profile(out, cat.as_ref(), asked) {
            Ok(key) => {
                entry.profile_mode = mode_of(&key);
                entry.profile = key;
            }
            Err(code) => return code,
        }
    }
    if !cfg.games[i].profile_changed(&entry) {
        return save_edit(out, cfg, i, entry, &name);
    }
    let source = match open_source(out, req) {
        Ok(source) => source,
        Err(code) => return code,
    };
    if let Err(blocker) = ops::game_dir(&entry).and_then(|dir| ops::check_writable(&dir)) {
        return refuse(out, &blocker);
    }
    out.info(&out.tf("profile_changed", &out.say(&ops::profile_title(cat.as_ref(), &entry.profile))));
    let job = Job::of(&entry, &name);
    let outcomes = install::run_jobs(out, std::slice::from_ref(&job), &source, cat.as_ref(), 1, false);
    if matches!(outcomes[0], Outcome::Installed(_)) {
        let saved = save_edit(out, cfg, i, entry, &name);
        if saved != OK {
            return saved;
        }
    }
    install::finish(out, req, cfg, cat.as_ref(), &[job], &outcomes)
}

fn save_edit(out: &Out, cfg: &mut Config, i: usize, entry: GameEntry, name: &str) -> i32 {
    match cfg.commit(|c| c.edit_game(i, entry).map(|_| ()).map_err(str::to_string)) {
        Ok(()) => {
            out.result(&out.tf("list_saved", name));
            OK
        }
        Err(e) => {
            out.fail(&out.tf("edit_save_failed", &out.say(&e)));
            FAILED
        }
    }
}

pub fn play(out: &Out, req: &Request, cfg: &mut Config) -> i32 {
    let (mut entry, listed) = match &req.game {
        None => match listed_game(out, req, cfg) {
            Ok(i) => (cfg.games[i].clone(), Some(i)),
            Err(code) => return code,
        },
        Some(arg) => match games::resolve(arg, &cfg.games, &current_dir()) {
            Ok(Target::Listed(i)) => (cfg.games[i].clone(), Some(i)),
            Ok(Target::Folder(dir)) => (ops::new_entry(&ScannedGame::of(dir), "", "", None), None),
            Ok(Target::All) => return USAGE,
            Err(e) => {
                out.fail(&out.say(&e));
                return USAGE;
            }
        },
    };
    let dir = PathBuf::from(&entry.path);
    let mut keep = game::adopt_accessible(&mut entry).then(|| entry.executable.clone());
    let chosen = match game::launch_plan(&entry) {
        game::Launch::Blocked(why) => {
            out.fail(&out.say(&why));
            return FAILED;
        }
        _ if req.exe.is_some() => match folder_exe(out, &dir, req.exe.as_deref().unwrap_or_default()) {
            Ok(exe) => Some(exe),
            Err(code) => return code,
        },
        game::Launch::Start => None,
        game::Launch::Choose(exes) => {
            let scanned = ScannedGame::of(dir.clone());
            let known = Catalog::cached().map(|c| c.exes_of(&entry.profile)).unwrap_or_default();
            let safe = game::pick_executable(&scanned.scan, &scanned.dir, &known);
            let items: Vec<String> = exes
                .iter()
                .map(|exe| if Some(exe) == safe.as_ref() { out.tf("recommended", exe) } else { exe.clone() })
                .collect();
            let cannot = out.tf("exe_needs_choice", &exes.join(", "));
            let header = format!("{}: {}", game::display_name(&entry), out.t("pick_exe_prompt"));
            match out.choose(&header, &items, &cannot) {
                Ok(i) => Some(exes[i].clone()),
                Err(code) => return code,
            }
        }
    };
    if let Some(exe) = chosen {
        entry.executable = exe.clone();
        if req.remember {
            keep = Some(exe);
        }
    }
    if let (Some(i), Some(exe)) = (listed, keep) {
        if let Err(e) = cfg.commit(|c| {
            c.games[i].executable = exe;
            Ok(())
        }) {
            out.fail(&out.tf("edit_save_failed", &out.say(&e)));
        }
    }
    match game::launch(&entry) {
        Ok(()) => {
            out.result(&out.tf("launching", &game::display_name(&entry)));
            OK
        }
        Err(e) => {
            out.fail(&out.say(&e));
            code_of(&e)
        }
    }
}

fn listed_game(out: &Out, req: &Request, cfg: &Config) -> Result<usize, i32> {
    if cfg.games.is_empty() {
        out.fail(&out.t("list_empty"));
        return Err(USAGE);
    }
    match &req.game {
        Some(arg) => match games::resolve(arg, &cfg.games, &current_dir()) {
            Ok(Target::Listed(i)) => Ok(i),
            Ok(_) => {
                out.fail(&out.tf("game_not_listed", arg));
                Err(USAGE)
            }
            Err(e) => {
                out.fail(&out.say(&e));
                Err(USAGE)
            }
        },
        None => out.choose(&out.t("choose_game"), &list_names(cfg), &out.t("needs_listed_game")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::detect;

    #[test]
    fn a_game_s_executable_is_the_record_s_the_one_asked_for_or_one_of_the_folder_s() {
        let dir = tempfile::tempdir().unwrap();
        let mut entry = GameEntry {
            path: dir.path().to_string_lossy().into_owned(),
            name: "Royal".into(),
            executable: "Game.exe".into(),
            profile: "generic".into(),
            profile_mode: "generic".into(),
        };
        assert_eq!(game::launch_plan(&entry), game::Launch::Start);
        std::fs::write(dir.path().join("Royal.exe"), b"x").unwrap();
        std::fs::write(dir.path().join("setup.exe"), b"x").unwrap();
        assert_eq!(game::launch_plan(&entry), game::Launch::Choose(vec!["Royal.exe".into(), "setup.exe".into()]));
        let exes = detect::root_exe_names(dir.path());
        assert_eq!(super::super::pick_exe(&exes, "royal").as_deref(), Some("Royal.exe"));
        let scan = detect::scan_exes(dir.path());
        assert_eq!(game::pick_executable(&scan, dir.path(), &[]).as_deref(), Some("Royal.exe"), "el recomendado");
        entry.executable = "Royal.exe".into();
        assert_eq!(game::launch_plan(&entry), game::Launch::Start);
    }
}
