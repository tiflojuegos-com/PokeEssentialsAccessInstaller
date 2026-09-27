use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use super::args::Request;
use super::games::Target;
use super::out::{InOrder, Line, Out};
use super::{
    catalog, code_of, folder_name, game_target, known_profile, list_names, mode_of, new_record, one_game, open_source,
    refuse, FAILED, OK, PENDING, SOME_FAILED,
};
use crate::console;
use crate::core::apply;
use crate::core::batch::{self, Event, Job, Outcome, Summary};
use crate::core::catalog::Catalog;
use crate::core::config::{Config, GameEntry};
use crate::core::ops::{self, Blocker, Inspection};
use crate::core::paths::{accessibility_dir, data_dir};
use crate::core::source::Source;
use crate::core::{detect, game, installed, mkxp, player_data, status};
use crate::i18n::key_of;

const DEFAULT_JOBS: usize = 3;

const LIFE_LINE: Duration = Duration::from_secs(10);

const OFFLINE: [&str; 5] =
    ["err_list_files", "err_download", "err_download_status", "err_rate_limited", "err_rate_limited_short"];

pub fn install(out: &Out, req: &Request, cfg: &mut Config) -> i32 {
    let source = match open_source(out, req) {
        Ok(source) => source,
        Err(code) => return code,
    };
    let cat = catalog(out, &source);
    let target = match game_target(out, req, cfg) {
        Ok(target) => target,
        Err(code) => return code,
    };
    match target {
        Target::All => {
            let jobs: Vec<Job> = cfg.games.iter().zip(list_names(cfg)).map(|(e, name)| Job::of(e, &name)).collect();
            run_all(out, req, cfg, &source, cat.as_ref(), &jobs, "run_start_install")
        }
        Target::Listed(i) => reinstall(out, req, cfg, &source, cat.as_ref(), i),
        Target::Folder(dir) => {
            let game = Inspection::of(dir, cat.as_ref(), &source);
            let entry = match new_record(out, req, cat.as_ref(), &game) {
                Ok(entry) => entry,
                Err(code) => return code,
            };
            if let Err(e) = cfg.commit(|c| {
                c.upsert_game(entry.clone());
                Ok(())
            }) {
                out.fail(&out.tf("edit_save_failed", &out.say(&e)));
                return FAILED;
            }
            let name = game::display_name(&entry);
            let profile = out.say(&ops::profile_title(cat.as_ref(), &entry.profile));
            out.info(&out.tfn("added_to_list", &[&name, &cfg.games.len().to_string(), &profile]));
            run_one(out, req, cfg, &source, cat.as_ref(), &Job::of(&entry, &name))
        }
    }
}

fn reinstall(out: &Out, req: &Request, cfg: &mut Config, source: &Source, cat: Option<&Catalog>, i: usize) -> i32 {
    let mut entry = cfg.games[i].clone();
    let dir = match ops::game_dir(&entry) {
        Ok(dir) => dir,
        Err(blocker) => return refuse(out, &blocker),
    };
    if let Err(blocker) = ops::check_writable(&dir) {
        return refuse(out, &blocker);
    }
    if let Some(asked) = &req.profile {
        match known_profile(out, cat, asked) {
            Ok(key) => {
                entry.profile_mode = mode_of(&key);
                entry.profile = key;
            }
            Err(code) => return code,
        }
    }
    if let Some(name) = &req.name {
        entry.name = name.trim().to_string();
    }
    let job = Job::of(&entry, &list_names(cfg)[i]);
    let outcome = run_jobs(out, std::slice::from_ref(&job), source, cat, 1, false).remove(0);
    if matches!(outcome, Outcome::Installed(_)) && entry != cfg.games[i] {
        if let Err(e) = cfg.commit(|c| c.edit_game(i, entry).map(|_| ()).map_err(str::to_string)) {
            out.fail(&out.tf("edit_save_failed", &out.say(&e)));
        }
    }
    finish(out, req, cfg, cat, &[job], &[outcome])
}

pub fn update(out: &Out, req: &Request, cfg: &mut Config) -> i32 {
    let source = match open_source(out, req) {
        Ok(source) => source,
        Err(code) => return code,
    };
    let cat = catalog(out, &source);
    let job = match game_target(out, req, cfg) {
        Err(code) => return code,
        Ok(Target::All) => {
            let jobs = batch::installed_jobs(cfg);
            if jobs.is_empty() {
                out.result(&out.t("nothing_to_update"));
                return OK;
            }
            return run_all(out, req, cfg, &source, cat.as_ref(), &jobs, "run_start_update");
        }
        Ok(Target::Listed(i)) => match ops::game_dir(&cfg.games[i]) {
            Ok(_) => Job::of(&cfg.games[i], &list_names(cfg)[i]),
            Err(blocker) => return refuse(out, &blocker),
        },
        Ok(Target::Folder(dir)) => {
            let sealed = installed::read(&dir);
            Job {
                name: folder_name(&dir),
                profile: sealed.as_ref().map(|s| s.profile.clone()).unwrap_or_default(),
                profile_mode: sealed.map(|s| s.profile_mode).unwrap_or_default(),
                dir,
            }
        }
    };
    if !installed::is_installed(&job.dir) {
        out.fail(&out.tfn("not_installed_update", &[&job.name, out.program()]));
        return FAILED;
    }
    run_one(out, req, cfg, &source, cat.as_ref(), &job)
}

pub fn uninstall(out: &Out, req: &Request, cfg: &mut Config) -> i32 {
    let (dir, name) = match one_game(out, req, cfg) {
        Ok(found) => found,
        Err(code) => return code,
    };
    if detect::folder_running(&dir) {
        return refuse(out, &Blocker::GameRunning(name));
    }
    let registered =
        std::fs::read(mkxp::mkxp_json(&dir)).is_ok_and(|b| mkxp::is_registered(&String::from_utf8_lossy(&b)));
    if !accessibility_dir(&dir).exists() && !registered {
        out.result(&out.tf("uninstall_no_mod", &name));
        return OK;
    }
    if !req.keep_data {
        if let Err(code) = out.confirm(req.yes, &format!("{}: {}", name, out.t("confirm_uninstall"))) {
            return code;
        }
    }
    match apply::run_uninstall(&dir, req.keep_data) {
        Ok(notes) => {
            for note in &notes {
                out.info(&out.say(note));
            }
            let kept = if req.keep_data { player_data::kept(&dir) } else { Vec::new() };
            if kept.is_empty() {
                out.result(&out.tf("uninstall_done", &name));
            } else {
                let data = data_dir(&dir).display().to_string();
                out.result(&out.tfn("uninstall_kept_data", &[&name, &data, &player_data::listed(&out.i18n, &kept)]));
            }
            OK
        }
        Err(e) => {
            out.fail(&format!("{}: {}", name, out.say(&e)));
            code_of(&e)
        }
    }
}

pub fn check(out: &Out, req: &Request, cfg: &mut Config) -> i32 {
    let source = match open_source(out, req) {
        Ok(source) => source,
        Err(code) => return code,
    };
    let cat = catalog(out, &source);
    if req.all() {
        return check_all(out, cfg, &source, cat.as_ref());
    }
    let (dir, name) = match one_game(out, req, cfg) {
        Ok(found) => found,
        Err(code) => return code,
    };
    out.info(&out.tfn("check_title", &[&name, &dir.display().to_string()]));
    let game = Inspection::of(dir, cat.as_ref(), &source);
    let report = game.report(cat.as_ref());
    for fact in &report.facts {
        out.info(&out.say(fact));
    }
    if let Some(warning) = &report.warning {
        out.warn(&out.say(warning));
    }
    out.result(&out.say(&report.verdict));
    game.blocker().map_or(OK, |b| code_of(&b.message()))
}

fn check_all(out: &Out, cfg: &Config, source: &Source, cat: Option<&Catalog>) -> i32 {
    if cfg.games.is_empty() {
        out.result(&out.t("list_empty"));
        return OK;
    }
    let names = list_names(cfg);
    let total = names.len().to_string();
    out.info(&out.tf("run_start_check", &total));
    let (mut installable, mut skipped, mut refused) = (0, 0, Vec::new());
    for (i, entry) in cfg.games.iter().enumerate() {
        let name = out.tfn("item_of", &[&(i + 1).to_string(), &total, &names[i]]);
        let Ok(dir) = ops::game_dir(entry) else {
            out.result(&out.tf("result_skipped", &name));
            skipped += 1;
            continue;
        };
        let game = Inspection::of(dir, cat, source);
        let report = game.report(cat);
        for fact in &report.facts {
            out.detail(&format!("{}: {}", name, out.say(fact)));
        }
        out.result(&format!("{}: {}", name, out.say(&report.verdict)));
        match game.blocker() {
            Some(blocker) => refused.push((&names[i], blocker.message())),
            None => installable += 1,
        }
    }
    let counts = [
        ("summary_games", names.len()),
        ("summary_installable", installable),
        ("summary_skipped", skipped),
        ("summary_not_installable", refused.len()),
    ];
    let parts: Vec<String> =
        counts.iter().filter(|(_, n)| *n > 0).map(|(key, n)| out.tf(key, &n.to_string())).collect();
    out.result(&out.tf("summary", &parts.join(". ")));
    for (name, why) in &refused {
        out.fail_line(&format!("{} {}", out.tf("summary_game_failed", name), out.say(why)));
    }
    if refused.is_empty() { OK } else { SOME_FAILED }
}

pub fn status(out: &Out, req: &Request, cfg: &mut Config) -> i32 {
    let source = match open_source(out, req) {
        Ok(source) => source,
        Err(code) => return code,
    };
    let available = source.available_version();
    let cat = source.catalog().ok();
    let rows: Vec<(String, GameEntry)> = match req.game.as_deref().map(|_| game_target(out, req, cfg)) {
        None | Some(Ok(Target::All)) => list_names(cfg)
            .into_iter()
            .zip(&cfg.games)
            .enumerate()
            .map(|(i, (name, entry))| (format!("{}. {}", i + 1, name), entry.clone()))
            .collect(),
        Some(Ok(Target::Listed(i))) => vec![(list_names(cfg).swap_remove(i), cfg.games[i].clone())],
        Some(Ok(Target::Folder(dir))) => {
            let profile = installed::read(&dir).map(|s| s.profile).unwrap_or_default();
            let entry = GameEntry {
                path: dir.to_string_lossy().into_owned(),
                name: folder_name(&dir),
                executable: String::new(),
                profile_mode: mode_of(&profile),
                profile,
            };
            vec![(folder_name(&dir), entry)]
        }
        Some(Err(code)) => return code,
    };
    if rows.is_empty() {
        out.result(&out.t("list_empty"));
        return OK;
    }
    let mut pending = false;
    for (name, entry) in &rows {
        let dir = Path::new(&entry.path);
        let key = status::entry_key(entry, &available);
        let sealed = installed::read(dir).filter(|s| !s.mod_version.is_empty());
        let line = match sealed.filter(|_| key != "status_missing") {
            Some(sealed) => {
                let profile = out.say(&ops::profile_title(cat.as_ref(), &entry.profile));
                out.tfn("status_installed", &[name, &out.t(key), &sealed.mod_version, &profile])
            }
            None => out.tfn("status_of", &[name, &out.t(key)]),
        };
        let faults = status::faults(dir);
        match status::repair_note(&out.i18n, "cli_health_repair", &faults) {
            Some(note) => out.result(&format!("{} {}", line, note)),
            None => out.result(&line),
        }
        pending |= matches!(key, "status_update" | "status_outdated") || !faults.is_empty();
    }
    if req.exit_code && pending { PENDING } else { OK }
}

fn run_one(out: &Out, req: &Request, cfg: &mut Config, source: &Source, cat: Option<&Catalog>, job: &Job) -> i32 {
    let outcomes = run_jobs(out, std::slice::from_ref(job), source, cat, 1, false);
    finish(out, req, cfg, cat, std::slice::from_ref(job), &outcomes)
}

fn run_all(
    out: &Out,
    req: &Request,
    cfg: &mut Config,
    source: &Source,
    cat: Option<&Catalog>,
    jobs: &[Job],
    start: &str,
) -> i32 {
    if jobs.is_empty() {
        out.result(&out.t("list_empty"));
        return OK;
    }
    let threads = req.jobs.unwrap_or(DEFAULT_JOBS).min(jobs.len());
    out.info(&out.tfn(start, &[&threads.to_string(), &jobs.len().to_string()]));
    let outcomes = run_jobs(out, jobs, source, cat, threads, true);
    finish(out, req, cfg, cat, jobs, &outcomes)
}

pub(super) fn finish(
    out: &Out,
    req: &Request,
    cfg: &mut Config,
    cat: Option<&Catalog>,
    jobs: &[Job],
    outcomes: &[Outcome],
) -> i32 {
    let before = cfg.games.clone();
    for (job, outcome) in jobs.iter().zip(outcomes).filter(|(_, o)| matches!(o, Outcome::Installed(_))) {
        let one = std::slice::from_ref(job);
        for notice in batch::adopt(cfg, cat, one, std::slice::from_ref(outcome)) {
            out.info(&format!("{}: {}", job.name, out.say(&notice)));
        }
        for notice in batch::long_path_notices(one) {
            out.warn(&format!("{}: {}", job.name, out.say(&notice)));
        }
    }
    if cfg.games != before {
        if let Err(e) = cfg.save() {
            out.fail(&out.tf("edit_save_failed", &out.say(&e)));
        }
    }
    if jobs.len() > 1 {
        out.result(&Summary::of(outcomes).line(&out.i18n));
        for (job, outcome) in jobs.iter().zip(outcomes) {
            if let Outcome::Failed(e) = outcome {
                out.fail_line(&format!("{} {}", out.tf("summary_game_failed", &job.name), out.say(e)));
            }
        }
    }
    let failures: Vec<&String> = outcomes
        .iter()
        .filter_map(|o| match o {
            Outcome::Failed(e) => Some(e),
            _ => None,
        })
        .collect();
    if !req.local && failures.iter().any(|e| OFFLINE.contains(&key_of(e))) {
        out.info(&out.tf("hint_local", out.program()));
    }
    match failures.first() {
        _ if console::stop_asked() => out.cancelled(),
        None => OK,
        Some(_) if jobs.len() > 1 => SOME_FAILED,
        Some(e) => code_of(e),
    }
}

struct Board {
    order: InOrder,
    running: BTreeSet<usize>,
    files: Vec<(u32, u32)>,
    finished: usize,
}

pub(super) fn run_jobs(
    out: &Out,
    jobs: &[Job],
    source: &Source,
    cat: Option<&Catalog>,
    threads: usize,
    numbered: bool,
) -> Vec<Outcome> {
    let board = Mutex::new(Board {
        order: InOrder::default(),
        running: BTreeSet::new(),
        files: vec![(0, 0); jobs.len()],
        finished: 0,
    });
    let done = AtomicBool::new(false);
    console::soft_stop(true);
    let outcomes = std::thread::scope(|s| {
        s.spawn(|| watch(out, jobs, source, &board, &done, numbered));
        let outcomes = batch::run(jobs, source, cat, threads, |event| hear(out, jobs, cat, &board, event, numbered));
        done.store(true, Ordering::SeqCst);
        outcomes
    });
    console::soft_stop(false);
    outcomes
}

fn hear(out: &Out, jobs: &[Job], cat: Option<&Catalog>, board: &Mutex<Board>, event: Event<'_>, numbered: bool) {
    let mut board = board.lock().unwrap_or_else(|e| e.into_inner());
    match event {
        Event::Started(i) => {
            board.running.insert(i);
        }
        Event::Progress(i, file, done, total) => {
            if done == 1 && !numbered {
                out.info(&out.tfn("files_to_copy", &[&jobs[i].name, &total.to_string()]));
            }
            board.files[i] = (done, total);
            out.detail(&format!("{}: {}", jobs[i].name, file));
        }
        Event::Finished(i, outcome) => {
            board.running.remove(&i);
            board.finished += 1;
            let name = if numbered {
                out.tfn("item_of", &[&(i + 1).to_string(), &jobs.len().to_string(), &jobs[i].name])
            } else {
                jobs[i].name.clone()
            };
            for (to_stderr, line) in board.order.finish(i, result_line(out, cat, &jobs[i], outcome, &name)) {
                if to_stderr {
                    out.fail_line(&line);
                } else {
                    out.result(&line);
                }
            }
        }
    }
}

fn watch(out: &Out, jobs: &[Job], source: &Source, board: &Mutex<Board>, done: &AtomicBool, numbered: bool) {
    while !done.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(200));
        if console::stop_asked() && !source.stopped() {
            source.stop();
            out.info(&out.t("stopping"));
        }
        if out.quiet() || out.silence() < LIFE_LINE {
            continue;
        }
        let board = board.lock().unwrap_or_else(|e| e.into_inner());
        let line = match board.files[0] {
            _ if numbered => {
                let going: Vec<&str> = board.running.iter().map(|&i| jobs[i].name.as_str()).collect();
                out.tfn("lifeline_games", &[&board.finished.to_string(), &jobs.len().to_string(), &going.join(", ")])
            }
            (copied, total) if total > 0 => {
                out.tfn("lifeline_files", &[&jobs[0].name, &copied.to_string(), &total.to_string()])
            }
            _ => out.tf("lifeline_working", &jobs[0].name),
        };
        out.info(&line);
    }
}

fn result_line(out: &Out, cat: Option<&Catalog>, job: &Job, outcome: &Outcome, name: &str) -> Line {
    match outcome {
        Outcome::Installed(done) if !done.changed() => (false, out.tfn("result_current", &[name, &done.version])),
        Outcome::Installed(done) if done.previous.is_empty() => {
            let profile = done.kept_profile.as_deref().unwrap_or(&job.profile);
            let profile = out.say(&ops::profile_title(cat, profile));
            (false, out.tfn("result_installed", &[name, &done.version, &profile]))
        }
        Outcome::Installed(done) if done.previous != done.version => {
            (false, out.tfn("result_updated", &[name, &done.previous, &done.version, &done.files.to_string()]))
        }
        Outcome::Installed(done) => {
            (false, out.tfn("result_repaired", &[name, &done.version, &done.files.to_string()]))
        }
        Outcome::Skipped => (false, out.tf("result_skipped", name)),
        Outcome::Failed(e) => (true, out.tf("error", &format!("{}: {}", name, out.say(e)))),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::core::apply::InstallOutcome;
    use crate::i18n::I18n;

    fn job(name: &str) -> Job {
        Job { dir: PathBuf::from(name), profile: "generic".into(), profile_mode: "generic".into(), name: name.into() }
    }

    fn installed(previous: &str, version: &str, files: usize) -> Outcome {
        Outcome::Installed(InstallOutcome {
            version: version.into(),
            previous: previous.into(),
            files,
            removed: 0,
            converted: None,
            kept_profile: None,
            engine_note: None,
        })
    }

    #[test]
    fn each_end_of_a_game_has_its_line_and_only_a_failure_goes_to_stderr() {
        let out = Out::new(I18n::new("es"), false, false, "pea".into());
        let line = |outcome: &Outcome| result_line(&out, None, &job("Anil"), outcome, "Anil");
        assert_eq!(line(&installed("", "0.6.0", 800)), (false, "Anil: instalado. Mod 0.6.0, perfil genérico.".into()));
        let updated = "Anil: actualizado de 0.5.0 a 0.6.0. Archivos copiados: 12.";
        assert_eq!(line(&installed("0.5.0", "0.6.0", 12)), (false, updated.into()));
        assert_eq!(line(&installed("0.6.0", "0.6.0", 0)), (false, "Anil: al día, mod 0.6.0.".into()));
        assert!(line(&installed("0.6.0", "0.6.0", 3)).1.contains("Archivos copiados: 3"));
        assert!(!line(&Outcome::Skipped).0);
        let failed = line(&Outcome::Failed(crate::i18n::err_key("game_running", "Anil")));
        assert!(failed.0 && failed.1.starts_with("Error: Anil: ") && failed.1.contains("abierto"), "{}", failed.1);
    }
}
