mod args;
mod data;
mod games;
mod install;
mod list;
mod out;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use args::{Command, Request, COMMANDS, TOPICS};
use games::Target;
use out::Out;

use crate::console;
use crate::core::catalog::Catalog;
use crate::core::config::{Config, GameEntry};
use crate::core::ops::{self, Blocker, Inspection};
use crate::core::source::Source;
use crate::core::{detect, game, selfupdate};
use crate::i18n::{key_of, I18n};

pub const OK: i32 = 0;
pub const FAILED: i32 = 1;
pub const USAGE: i32 = 2;
pub const CANCELLED: i32 = 3;
pub const CANNOT_ASK: i32 = 4;
pub const RUNNING: i32 = 5;
pub const SOME_FAILED: i32 = 6;
pub const INCOMPATIBLE: i32 = 7;
pub const PENDING: i32 = 8;

pub fn window_start(args: &[OsString]) -> Option<Option<PathBuf>> {
    match args {
        [] => Some(None),
        [one] => {
            let path = PathBuf::from(one);
            (path.is_absolute() && path.is_dir()).then_some(Some(path))
        }
        _ => None,
    }
}

pub fn run(args: Vec<OsString>) -> i32 {
    let own_console = console::for_console_run();
    console::catch_interrupts();
    let request = args::parse(args);
    let mut cfg = Config::load();
    let lang = request.lang.clone().unwrap_or_else(|| cfg.resolve_language());
    let out = Out::new(I18n::new(&lang), request.quiet, request.verbose, program_name());
    let code = match &request.problem {
        Some(problem) => {
            out.fail(&out.say(problem));
            let topic = match request.command {
                Command::Help => String::new(),
                command => format!(" {}", command.word()),
            };
            out.fail_line(&out.tf("usage_help_hint", &format!("{} help{}", out.program(), topic)));
            USAGE
        }
        None => dispatch(&out, &request, &mut cfg),
    };
    if request.pause.unwrap_or(own_console) {
        out.pause();
    }
    code
}

fn dispatch(out: &Out, req: &Request, cfg: &mut Config) -> i32 {
    match req.command {
        Command::Install => install::install(out, req, cfg),
        Command::Update => install::update(out, req, cfg),
        Command::Uninstall => install::uninstall(out, req, cfg),
        Command::Check => install::check(out, req, cfg),
        Command::Status => install::status(out, req, cfg),
        Command::Export => data::export(out, req, cfg),
        Command::Import => data::import(out, req, cfg),
        Command::List => list::list(out, req, cfg),
        Command::ListAdd => list::add(out, req, cfg),
        Command::ListRemove => list::remove(out, req, cfg),
        Command::ListSet => list::set(out, req, cfg),
        Command::Play => list::play(out, req, cfg),
        Command::SelfUpdate => self_update(out, req),
        Command::Help => help(out, req.topic.as_deref()),
        Command::Version => {
            out.result(&out.tf("launcher_version", env!("CARGO_PKG_VERSION")));
            OK
        }
    }
}

fn program_name() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.file_stem().map(|stem| stem.to_string_lossy().into_owned()))
        .unwrap_or_else(|| selfupdate::LAUNCHER_ASSET.trim_end_matches(".exe").to_string())
}

fn help(out: &Out, topic: Option<&str>) -> i32 {
    let key = match topic {
        None => Some("help"),
        Some(topic) => COMMANDS
            .iter()
            .map(|(word, _, key)| (*word, *key))
            .chain(TOPICS)
            .find(|(word, _)| word.eq_ignore_ascii_case(topic))
            .map(|(_, key)| key),
    };
    let Some(key) = key else {
        out.fail(&out.tf("usage_unknown_command", topic.unwrap_or_default()));
        out.fail_line(&out.tf("usage_help_hint", &format!("{} help", out.program())));
        return USAGE;
    };
    for line in out.tf(key, out.program()).split(" | ") {
        out.result(line);
    }
    OK
}

fn self_update(out: &Out, req: &Request) -> i32 {
    let update = match selfupdate::check() {
        Ok(Some(update)) => update,
        Ok(None) => {
            out.result(&out.t("launcher_update_none"));
            return OK;
        }
        Err(e) => {
            out.fail(&out.say(&e));
            return FAILED;
        }
    };
    if req.check {
        out.result(&out.tf("launcher_update_found", &update.tag));
        return if req.exit_code { PENDING } else { OK };
    }
    for line in update.notes.lines().map(str::trim).filter(|l| !l.is_empty()) {
        out.info(line);
    }
    if let Err(code) = out.confirm(req.yes, &out.tf("launcher_update_available", &update.tag)) {
        return code;
    }
    match selfupdate::apply(&update) {
        Ok(()) => {
            out.result(&out.tf("launcher_updated", &update.tag));
            OK
        }
        Err(e) => {
            out.fail(&out.say(&e));
            FAILED
        }
    }
}

fn code_of(payload: &str) -> i32 {
    match key_of(payload) {
        "game_running" | "already_running" => RUNNING,
        "not_compatible" | "rgss_player_unsupported" | "convert_markers_missing" | "convert_mkxp_json_present"
        | "convert_exe_exists" | "convert_engine_missing" => INCOMPATIBLE,
        "run_cancelled" => CANCELLED,
        _ => FAILED,
    }
}

fn refuse(out: &Out, blocker: &Blocker) -> i32 {
    out.fail(&out.say(&blocker.message()));
    code_of(&blocker.message())
}

fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default()
}

fn open_source(out: &Out, req: &Request) -> Result<Source, i32> {
    if !req.local {
        let source = Source::github();
        let version = source.available_version();
        out.info(&if version.is_empty() { out.t("origin_github_offline") } else { out.tf("origin_github", &version) });
        return Ok(source);
    }
    let cwd = current_dir();
    let exe_dir = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)).unwrap_or_default();
    let Some(root) = games::mod_folder(req.from.as_deref().map(Path::new), &cwd, &exe_dir) else {
        out.fail(&out.t("local_not_found"));
        return Err(USAGE);
    };
    let engine = req.engine.as_deref().map(|dir| std::path::absolute(cwd.join(dir)).unwrap_or_else(|_| cwd.join(dir)));
    let source = Source::folder(root.clone(), engine).map_err(|e| {
        out.fail(&out.say(&e));
        USAGE
    })?;
    let version = source.available_version();
    let folder = root.display().to_string();
    out.info(&if version.is_empty() {
        out.tf("origin_folder_unknown", &folder)
    } else {
        out.tfn("origin_folder", &[&folder, &version])
    });
    Ok(source)
}

fn catalog(out: &Out, source: &Source) -> Option<Catalog> {
    source.catalog().map_err(|_| out.warn(&out.t("catalog_unavailable"))).ok()
}

fn game_target(out: &Out, req: &Request, cfg: &Config) -> Result<Target, i32> {
    let arg = match &req.game {
        Some(arg) => arg.clone(),
        None => out.pick_folder()?.to_string_lossy().into_owned(),
    };
    games::resolve(&arg, &cfg.games, &current_dir()).map_err(|e| {
        out.fail(&out.say(&e));
        USAGE
    })
}

fn one_game(out: &Out, req: &Request, cfg: &Config) -> Result<(PathBuf, String), i32> {
    match game_target(out, req, cfg)? {
        Target::Listed(i) => match ops::game_dir(&cfg.games[i]) {
            Ok(dir) => Ok((dir, list_names(cfg).swap_remove(i))),
            Err(blocker) => Err(refuse(out, &blocker)),
        },
        Target::Folder(dir) => {
            let name = folder_name(&dir);
            Ok((dir, name))
        }
        Target::All => Err(USAGE),
    }
}

fn folder_name(dir: &Path) -> String {
    dir.file_name().map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into_owned())
}

fn mode_of(profile: &str) -> String {
    let mode = if profile == "generic" { "generic" } else { "specific" };
    mode.to_string()
}

fn known_profile(out: &Out, cat: Option<&Catalog>, asked: &str) -> Result<String, i32> {
    let key = asked.trim().to_lowercase();
    match cat {
        Some(c) if key != "generic" && !c.profiles.iter().any(|p| p.key == key) => {
            let keys: Vec<&str> = std::iter::once("generic").chain(c.specific().map(|p| p.key.as_str())).collect();
            out.fail(&out.tfn("unknown_profile", &[&key, &keys.join(", ")]));
            Err(USAGE)
        }
        _ => Ok(key),
    }
}

fn new_record(out: &Out, req: &Request, cat: Option<&Catalog>, game: &Inspection) -> Result<GameEntry, i32> {
    if let Some(blocker) = game.blocker() {
        return Err(refuse(out, &blocker));
    }
    if game.doubtful() {
        out.confirm(req.yes, &format!("{}: {}", game.game.name(), out.t("preload_missing_warn")))?;
        if req.yes {
            out.warn(&format!("{}: {}", game.game.name(), out.t("forced_without_preload")));
        }
    }
    if let Some(offer) = &game.offer {
        out.confirm(req.yes, &out.tf("convert_confirm", &offer.display))?;
    }
    let (profile, mode) = match game.offer.as_ref().filter(|o| !o.profile.is_empty()) {
        Some(offer) => (offer.profile.clone(), mode_of(&offer.profile)),
        None => chosen_profile(out, req, cat, game)?,
    };
    let mut entry = ops::new_entry(&game.game, &profile, &mode, cat);
    if let Some(name) = &req.name {
        entry.name = name.trim().to_string();
    }
    if let Some(exe) = &req.exe {
        entry.executable = folder_exe(out, &game.game.dir, exe)?;
    } else if detect::root_exe_names(&game.game.dir).is_empty() {
        out.warn(&out.t("invalid_executable"));
    }
    Ok(entry)
}

fn chosen_profile(out: &Out, req: &Request, cat: Option<&Catalog>, game: &Inspection) -> Result<(String, String), i32> {
    if let Some(asked) = &req.profile {
        let key = known_profile(out, cat, asked)?;
        let mode = mode_of(&key);
        return Ok((key, mode));
    }
    if let Some(key) = game.suggested_profile() {
        return Ok((key.to_string(), mode_of(key)));
    }
    let cannot = out.t("profile_needs_choice");
    if req.yes {
        out.fail(&cannot);
        return Err(CANNOT_ASK);
    }
    let keys: Vec<String> = std::iter::once("generic".to_string())
        .chain(cat.map(|c| c.specific().map(|p| p.key.clone()).collect::<Vec<_>>()).unwrap_or_default())
        .collect();
    let names: Vec<String> = keys.iter().map(|key| out.say(&ops::profile_title(cat, key))).collect();
    let picked = &keys[out.choose(&out.t("not_detected"), &names, &cannot)?];
    Ok((picked.clone(), mode_of(picked)))
}

fn folder_exe(out: &Out, dir: &Path, asked: &str) -> Result<String, i32> {
    let exes = detect::root_exe_names(dir);
    pick_exe(&exes, asked).ok_or_else(|| {
        out.fail(&out.tfn("exe_not_in_folder", &[asked, &exes.join(", ")]));
        USAGE
    })
}

fn pick_exe(exes: &[String], asked: &str) -> Option<String> {
    let asked = asked.trim();
    exes.iter()
        .find(|exe| {
            let stem = exe.get(..exe.len().saturating_sub(4)).unwrap_or_default();
            exe.eq_ignore_ascii_case(asked) || stem.eq_ignore_ascii_case(asked)
        })
        .cloned()
}

fn list_names(cfg: &Config) -> Vec<String> {
    game::list_names(&cfg.games)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_failure_ends_the_run_with_its_own_code() {
        let cases = [
            (crate::i18n::err_key("game_running", "Relict"), RUNNING),
            ("already_running".to_string(), RUNNING),
            ("not_compatible".to_string(), INCOMPATIBLE),
            ("rgss_player_unsupported".to_string(), INCOMPATIBLE),
            (crate::i18n::err_key("convert_markers_missing", "Data/Scripts.rxdata"), INCOMPATIBLE),
            ("convert_mkxp_json_present".to_string(), INCOMPATIBLE),
            ("run_cancelled".to_string(), CANCELLED),
            ("no_write_perm".to_string(), FAILED),
            (crate::i18n::err_key("err_list_files", "timeout"), FAILED),
            (crate::i18n::err_key("game_folder_missing", "D:/x"), FAILED),
        ];
        for (payload, code) in cases {
            assert_eq!(code_of(&payload), code, "{payload:?}");
        }
        assert_eq!(code_of(&Blocker::GameRunning("Z".into()).message()), RUNNING);
        assert_eq!(code_of(&Blocker::Incompatible("not_compatible".into()).message()), INCOMPATIBLE);
        assert_eq!(code_of(&Blocker::NoWritePermission.message()), FAILED);
    }

    #[test]
    fn the_window_opens_with_no_arguments_or_a_folder_dropped_on_the_executable_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Game.exe");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(window_start(&[]), Some(None));
        assert_eq!(window_start(&[dir.path().into()]), Some(Some(dir.path().to_path_buf())));
        for line in [vec![OsString::from("install")], vec![file.into()], vec![OsString::from(".")],
            vec![dir.path().into(), OsString::from("--yes")]] {
            assert_eq!(window_start(&line), None, "{line:?}");
        }
    }

    #[test]
    fn an_executable_is_named_whatever_its_case_and_with_or_without_its_extension() {
        let exes = vec!["Game.exe".to_string(), "Uranium.exe".to_string()];
        assert_eq!(pick_exe(&exes, "game.EXE").as_deref(), Some("Game.exe"));
        assert_eq!(pick_exe(&exes, "uranium").as_deref(), Some("Uranium.exe"));
        assert_eq!(pick_exe(&exes, "Patcher.exe"), None);
        assert_eq!(pick_exe(&exes, "Gam"), None);
    }

    #[test]
    fn every_command_and_topic_has_its_help_in_every_language_with_the_same_lines() {
        let keys: Vec<&str> =
            COMMANDS.iter().map(|(_, _, key)| *key).chain(TOPICS.iter().map(|(_, key)| *key)).collect();
        let spanish = I18n::new("es");
        for lang in crate::i18n::LANGS {
            let i18n = I18n::new(lang);
            for key in &keys {
                let text = i18n.t(key);
                assert_ne!(&text, key, "{lang}: falta {key}");
                assert_eq!(text.split(" | ").count(), spanish.t(key).split(" | ").count(), "{lang}: {key}");
            }
        }
        let general = spanish.tf("help", "pea");
        for (word, _, _) in COMMANDS {
            assert!(general.contains(word), "la ayuda general no nombra {word}");
        }
    }
}
