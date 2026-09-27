use super::args::Request;
use super::out::Out;
use super::{code_of, current_dir, one_game, refuse, OK, USAGE};
use crate::core::catalog::Catalog;
use crate::core::config::Config;
use crate::core::ops::Blocker;
use crate::core::{detect, player_data};

pub fn export(out: &Out, req: &Request, cfg: &Config) -> i32 {
    let (dir, name) = match one_game(out, req, cfg) {
        Ok(found) => found,
        Err(code) => return code,
    };
    if player_data::saved(&dir).is_empty() {
        out.result(&out.tf("export_nothing", &name));
        return OK;
    }
    let target = match req.file.as_deref().map(|file| current_dir().join(file)) {
        Some(path) if path.is_dir() => path.join(player_data::export_name(&name)),
        Some(path) if path.extension().is_none() => path.with_extension("zip"),
        Some(path) => path,
        None => current_dir().join(player_data::export_name(&name)),
    };
    let shown = target.display().to_string();
    if target.exists() {
        if let Err(code) = out.confirm(req.yes, &out.tf("export_overwrite", &shown)) {
            return code;
        }
    }
    match player_data::export(&dir, &target) {
        Ok(labels) => {
            out.result(&out.tfn("export_done", &[&name, &shown, &player_data::listed(&out.i18n, &labels)]));
            OK
        }
        Err(e) => {
            out.fail(&format!("{}: {}", name, out.say(&e)));
            code_of(&e)
        }
    }
}

pub fn import(out: &Out, req: &Request, cfg: &Config) -> i32 {
    let (dir, name) = match one_game(out, req, cfg) {
        Ok(found) => found,
        Err(code) => return code,
    };
    let file = current_dir().join(req.file.as_deref().unwrap_or_default());
    let shown = file.display().to_string();
    if !file.is_file() {
        out.fail(&out.tf("file_not_found", &shown));
        return USAGE;
    }
    if detect::folder_running(&dir) {
        return refuse(out, &Blocker::GameRunning(name));
    }
    match player_data::import(&dir, &file) {
        Ok(done) => {
            out.result(&out.tfn("import_done", &[&name, &shown]));
            for line in done.lines(&out.i18n, Catalog::cached().as_ref()) {
                out.result(&line);
            }
            OK
        }
        Err(e) => {
            out.fail(&format!("{}: {}", name, out.say(&e)));
            code_of(&e)
        }
    }
}
