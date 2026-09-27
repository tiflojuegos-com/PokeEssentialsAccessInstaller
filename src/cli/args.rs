use std::ffi::OsString;

use crate::i18n::{err_key, LANGS};

const MAX_JOBS: usize = 16;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Command {
    Install,
    Update,
    Uninstall,
    Check,
    Status,
    Export,
    Import,
    List,
    ListAdd,
    ListRemove,
    ListSet,
    Play,
    SelfUpdate,
    #[default]
    Help,
    Version,
}

pub const COMMANDS: [(&str, Command, &str); 12] = [
    ("install", Command::Install, "help_install"),
    ("update", Command::Update, "help_update"),
    ("uninstall", Command::Uninstall, "help_uninstall"),
    ("check", Command::Check, "help_check"),
    ("status", Command::Status, "help_status"),
    ("export", Command::Export, "help_export"),
    ("import", Command::Import, "help_import"),
    ("list", Command::List, "help_list"),
    ("play", Command::Play, "help_play"),
    ("self-update", Command::SelfUpdate, "help_self_update"),
    ("help", Command::Help, "help"),
    ("version", Command::Version, "help_version"),
];

pub const TOPICS: [(&str, &str); 2] = [("local", "help_local"), ("exit-codes", "help_exit_codes")];

const LIST_COMMANDS: [(&str, Command); 3] =
    [("add", Command::ListAdd), ("remove", Command::ListRemove), ("set", Command::ListSet)];

impl Command {
    fn named(word: &str) -> Option<Command> {
        COMMANDS.iter().find(|(name, _, _)| name.eq_ignore_ascii_case(word)).map(|(_, command, _)| *command)
    }

    pub fn word(self) -> &'static str {
        let base = match self {
            Command::ListAdd | Command::ListRemove | Command::ListSet => Command::List,
            other => other,
        };
        COMMANDS.iter().find(|(_, command, _)| *command == base).map_or("help", |(name, _, _)| name)
    }

    fn takes_local(self) -> bool {
        matches!(self, Command::Install | Command::Update | Command::Uninstall | Command::Check | Command::Status)
    }

    fn takes_game(self) -> bool {
        self.takes_local()
            || self.takes_file()
            || matches!(self, Command::ListAdd | Command::ListRemove | Command::ListSet | Command::Play)
    }

    fn takes_file(self) -> bool {
        matches!(self, Command::Export | Command::Import)
    }

    fn takes_all(self) -> bool {
        matches!(self, Command::Install | Command::Update | Command::Check | Command::Status)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flag {
    Yes,
    Lang,
    Pause,
    NoPause,
    Quiet,
    Verbose,
    Jobs,
    From,
    Engine,
    Profile,
    Name,
    Exe,
    KeepData,
    Paths,
    Remember,
    CheckOnly,
    ExitCode,
    Help,
    Version,
}

const FLAGS: [(&str, &str, Flag, bool); 19] = [
    ("--yes", "-y", Flag::Yes, false),
    ("--lang", "", Flag::Lang, true),
    ("--pause", "", Flag::Pause, false),
    ("--no-pause", "", Flag::NoPause, false),
    ("--quiet", "-q", Flag::Quiet, false),
    ("--verbose", "-v", Flag::Verbose, false),
    ("--jobs", "", Flag::Jobs, true),
    ("--from", "", Flag::From, true),
    ("--engine", "", Flag::Engine, true),
    ("--profile", "", Flag::Profile, true),
    ("--name", "", Flag::Name, true),
    ("--exe", "", Flag::Exe, true),
    ("--keep-data", "", Flag::KeepData, false),
    ("--paths", "", Flag::Paths, false),
    ("--remember", "", Flag::Remember, false),
    ("--check", "", Flag::CheckOnly, false),
    ("--exit-code", "", Flag::ExitCode, false),
    ("--help", "-h", Flag::Help, false),
    ("--version", "", Flag::Version, false),
];

impl Flag {
    fn fits(self, command: Command) -> bool {
        use Command::*;
        match self {
            Flag::Yes | Flag::Lang | Flag::Pause | Flag::NoPause | Flag::Quiet | Flag::Verbose | Flag::Help
            | Flag::Version => true,
            Flag::Jobs => matches!(command, Install | Update),
            Flag::From | Flag::Engine => command.takes_local(),
            Flag::Profile | Flag::Name => matches!(command, Install | ListAdd | ListSet),
            Flag::Exe => matches!(command, ListAdd | ListSet | Play),
            Flag::KeepData => command == Uninstall,
            Flag::Paths => command == List,
            Flag::Remember => command == Play,
            Flag::CheckOnly => command == SelfUpdate,
            Flag::ExitCode => matches!(command, Status | SelfUpdate),
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Request {
    pub command: Command,
    pub local: bool,
    pub game: Option<String>,
    pub file: Option<String>,
    pub topic: Option<String>,
    pub yes: bool,
    pub lang: Option<String>,
    pub pause: Option<bool>,
    pub quiet: bool,
    pub verbose: bool,
    pub jobs: Option<usize>,
    pub from: Option<String>,
    pub engine: Option<String>,
    pub profile: Option<String>,
    pub name: Option<String>,
    pub exe: Option<String>,
    pub keep_data: bool,
    pub paths: bool,
    pub remember: bool,
    pub check: bool,
    pub exit_code: bool,
    pub help: bool,
    pub version: bool,
    pub problem: Option<String>,
}

impl Request {
    pub fn all(&self) -> bool {
        self.game.as_deref().is_some_and(|g| g.eq_ignore_ascii_case("all"))
    }

    fn refuse(&mut self, key: &str, arg: &str) {
        self.problem.get_or_insert_with(|| err_key(key, arg));
    }

    fn set(&mut self, flag: Flag, value: String) {
        match flag {
            Flag::Yes => self.yes = true,
            Flag::Lang => match LANGS.iter().find(|lang| lang.eq_ignore_ascii_case(&value)) {
                Some(lang) => self.lang = Some(lang.to_string()),
                None => self.refuse("usage_bad_lang", &value),
            },
            Flag::Pause => self.pause = Some(true),
            Flag::NoPause => self.pause = Some(false),
            Flag::Quiet => self.quiet = true,
            Flag::Verbose => self.verbose = true,
            Flag::Jobs => match value.parse::<usize>() {
                Ok(n) if (1..=MAX_JOBS).contains(&n) => self.jobs = Some(n),
                _ => self.refuse("usage_bad_jobs", &value),
            },
            Flag::From => self.from = Some(value),
            Flag::Engine => self.engine = Some(value),
            Flag::Profile => self.profile = Some(value),
            Flag::Name => self.name = Some(value),
            Flag::Exe => self.exe = Some(value),
            Flag::KeepData => self.keep_data = true,
            Flag::Paths => self.paths = true,
            Flag::Remember => self.remember = true,
            Flag::CheckOnly => self.check = true,
            Flag::ExitCode => self.exit_code = true,
            Flag::Help => self.help = true,
            Flag::Version => self.version = true,
        }
    }

    fn read_words(&mut self, words: Vec<String>) {
        let mut words = words.into_iter().peekable();
        self.local = words.next_if(|w| w.eq_ignore_ascii_case("local")).is_some();
        self.command = match words.next() {
            Some(word) => Command::named(&word).unwrap_or_else(|| {
                self.refuse("usage_unknown_command", &word);
                Command::Help
            }),
            None if self.version => Command::Version,
            None => {
                if std::mem::take(&mut self.local) {
                    self.topic = Some("local".to_string());
                }
                Command::Help
            }
        };
        if self.command == Command::List {
            let sub = words.peek().and_then(|w| LIST_COMMANDS.iter().find(|(name, _)| name.eq_ignore_ascii_case(w)));
            if let Some((_, command)) = sub {
                self.command = *command;
                words.next();
            }
        }
        if self.local && !self.command.takes_local() {
            self.refuse("usage_local", self.command.word());
        }
        if self.command == Command::Help {
            self.topic = words.next().or(self.topic.take());
        } else if self.command.takes_game() {
            self.game = words.next();
            if self.command.takes_file() {
                self.file = words.next();
            }
        }
        if let Some(extra) = words.next() {
            self.refuse("usage_extra_argument", &extra);
        }
        if !self.help && self.command.takes_file() {
            if self.game.is_none() {
                self.refuse("usage_needs_game", self.command.word());
            } else if self.command == Command::Import && self.file.is_none() {
                self.refuse("usage_needs_file", self.command.word());
            }
        }
        if self.all() && !self.command.takes_all() {
            self.refuse("usage_no_all", self.command.word());
        }
        if self.help && self.command != Command::Help {
            self.topic = Some(self.command.word().to_string());
            self.command = Command::Help;
        }
    }

    fn check_flags(&mut self, flags: &[(Flag, String)]) {
        if self.command == Command::Help {
            return;
        }
        for (flag, name) in flags {
            if !flag.fits(self.command) {
                self.refuse("usage_option_not_here", name);
            } else if matches!(flag, Flag::From | Flag::Engine) && !self.local {
                self.refuse("usage_needs_local", name);
            }
        }
        if self.all() && (self.profile.is_some() || self.name.is_some()) {
            self.refuse("usage_not_with_all", if self.profile.is_some() { "--profile" } else { "--name" });
        }
    }
}

pub fn parse(args: Vec<OsString>) -> Request {
    let mut request = Request::default();
    let mut words = Vec::new();
    let mut flags = Vec::new();
    let mut tokens = mend(args).into_iter();
    while let Some(token) = tokens.next() {
        if token.is_empty() {
            continue;
        }
        if token == "/?" {
            request.help = true;
            continue;
        }
        if !token.starts_with('-') || token.len() == 1 {
            words.push(token);
            continue;
        }
        let (name, inline) = match token.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name.to_string(), Some(value.to_string())),
            _ => (token.clone(), None),
        };
        let found = FLAGS.iter().find(|(long, short, _, _)| name.eq_ignore_ascii_case(long) || name == *short);
        let Some(&(long, _, flag, takes_value)) = found else {
            request.refuse("usage_unknown_option", &name);
            continue;
        };
        let value = match (takes_value, inline) {
            (true, inline) => inline.or_else(|| tokens.next()),
            (false, None) => Some(String::new()),
            (false, Some(_)) => None,
        };
        match value {
            Some(value) => {
                request.set(flag, value);
                flags.push((flag, long.to_string()));
            }
            None if takes_value => request.refuse("usage_missing_value", long),
            None => request.refuse("usage_unknown_option", &token),
        }
    }
    request.read_words(words);
    request.check_flags(&flags);
    request
}

pub fn mend(args: Vec<OsString>) -> Vec<String> {
    let mut out = Vec::new();
    for arg in args {
        let arg = arg.to_string_lossy().into_owned();
        let (head, tail) = arg.split_once('"').unwrap_or((arg.as_str(), ""));
        let head = head.trim_end();
        if head.ends_with(':') && arg.contains('"') {
            out.push(format!("{}\\", head));
        } else if !head.is_empty() || !arg.contains('"') {
            out.push(head.to_string());
        }
        out.extend(mend(tail.split_whitespace().map(OsString::from).collect()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(line: &[&str]) -> Request {
        parse(line.iter().map(OsString::from).collect())
    }

    fn problem_key(line: &[&str]) -> String {
        let request = read(line);
        crate::i18n::key_of(request.problem.as_deref().unwrap_or("")).to_string()
    }

    #[test]
    fn a_bat_without_a_folder_and_a_drive_root_it_quoted_are_read_as_meant() {
        let cases: [(&[&str], &[&str]); 7] = [
            (&[], &[]),
            (&[""], &[""]),
            (&["install", "", "--pause"], &["install", "", "--pause"]),
            (&["F:\""], &["F:\\"]),
            (&["F:\" --pause"], &["F:\\", "--pause"]),
            (&["D:\\Juegos\\Anil\" --yes --pause"], &["D:\\Juegos\\Anil", "--yes", "--pause"]),
            (&["D:\\Juegos\\Pokemon Z  "], &["D:\\Juegos\\Pokemon Z"]),
        ];
        for (given, meant) in cases {
            assert_eq!(mend(given.iter().map(OsString::from).collect()), meant.to_vec(), "{given:?}");
        }
        let r = read(&["install", "", "--pause"]);
        assert_eq!((r.game, r.pause, r.problem), (None, Some(true), None));
        let r = read(&["list", "set", "3", "--name", "", "--exe", "Game.exe"]);
        assert_eq!((r.name.as_deref(), r.exe.as_deref(), r.problem), (Some(""), Some("Game.exe"), None));
    }

    #[test]
    fn each_line_is_read_as_its_command_game_and_options() {
        let r = read(&["local", "install", "D:\\Juegos\\Anil", "--profile=anil", "-y", "--from", "D:\\mod"]);
        assert_eq!((r.command, r.local, r.game.as_deref()), (Command::Install, true, Some("D:\\Juegos\\Anil")));
        assert_eq!((r.profile.as_deref(), r.yes, r.from.as_deref()), (Some("anil"), true, Some("D:\\mod")));
        assert_eq!(r.problem, None);
        let r = read(&["update", "ALL", "--jobs", "4", "-q", "--lang", "EN", "--no-pause"]);
        assert_eq!((r.command, r.all(), r.jobs, r.quiet), (Command::Update, true, Some(4), true));
        assert_eq!((r.lang.as_deref(), r.pause), (Some("en"), Some(false)));
        let r = read(&["list", "add", "D:\\Juegos\\Relict", "--exe", "Game.exe", "--name=Relict"]);
        assert_eq!((r.command, r.game.as_deref()), (Command::ListAdd, Some("D:\\Juegos\\Relict")));
        assert_eq!((r.exe.as_deref(), r.name.as_deref()), (Some("Game.exe"), Some("Relict")));
        assert_eq!(read(&["list", "remove", "7"]).game.as_deref(), Some("7"));
        assert_eq!(read(&["list", "--paths"]).command, Command::List);
        assert!(read(&["uninstall", "2", "--keep-data"]).keep_data);
        assert_eq!(read(&["self-update", "--check", "--exit-code"]).command, Command::SelfUpdate);
        assert!(read(&["play", "Pokemon Z", "--exe", "Game.exe", "--remember"]).remember);
        assert_eq!(read(&["status"]).game, None);
        let r = read(&["export", "Pokemon Z", "D:\\Copias\\z.zip", "--yes"]);
        assert_eq!((r.command, r.game.as_deref(), r.file.as_deref()), (Command::Export, Some("Pokemon Z"), Some("D:\\Copias\\z.zip")));
        assert_eq!((r.yes, r.problem), (true, None));
        let r = read(&["export", "2"]);
        assert_eq!((r.game.as_deref(), r.file, r.problem), (Some("2"), None, None));
        let r = read(&["import", "2", "z.zip"]);
        assert_eq!((r.command, r.game.as_deref(), r.file.as_deref()), (Command::Import, Some("2"), Some("z.zip")));
        assert_eq!(read(&["import", "--help"]).topic.as_deref(), Some("import"), "la ayuda no pide el juego");
        let r = read(&["local"]);
        assert_eq!((r.command, r.topic.as_deref(), r.local, r.problem), (Command::Help, Some("local"), false, None));
    }

    #[test]
    fn help_is_asked_in_every_way_the_help_says() {
        for line in [&["help"][..], &["/?"], &["-h"], &["--help"], &[], &["--lang", "en"]] {
            let r = read(line);
            assert_eq!((r.command, r.topic.as_deref(), r.problem.as_deref()), (Command::Help, None, None), "{line:?}");
        }
        for line in [&["help", "install"][..], &["install", "/?"], &["install", "--help"], &["install", "-h"]] {
            let r = read(line);
            assert_eq!((r.command, r.topic.as_deref()), (Command::Help, Some("install")), "{line:?}");
        }
        assert_eq!(read(&["uninstall", "--keep-data", "/?"]).problem, None, "la ayuda no mira las opciones");
        assert_eq!(read(&["list", "add", "--help"]).topic.as_deref(), Some("list"));
        assert_eq!(read(&["version"]).command, Command::Version);
        assert_eq!(read(&["--version"]).command, Command::Version);
    }

    #[test]
    fn a_line_that_cannot_run_says_why() {
        let cases: [(&[&str], &str); 20] = [
            (&["export"], "usage_needs_game"),
            (&["import"], "usage_needs_game"),
            (&["import", "1"], "usage_needs_file"),
            (&["export", "all"], "usage_no_all"),
            (&["import", "1", "z.zip", "otro"], "usage_extra_argument"),
            (&["local", "export", "1"], "usage_local"),
            (&["instalar"], "usage_unknown_command"),
            (&["install", "--perfil", "x"], "usage_unknown_option"),
            (&["install", "--si"], "usage_unknown_option"),
            (&["install", "--profile"], "usage_missing_value"),
            (&["status", "--keep-data"], "usage_option_not_here"),
            (&["install", "--from", "D:\\mod"], "usage_needs_local"),
            (&["local", "list"], "usage_local"),
            (&["local", "play"], "usage_local"),
            (&["install", "a", "b"], "usage_extra_argument"),
            (&["uninstall", "all"], "usage_no_all"),
            (&["install", "all", "--profile", "anil"], "usage_not_with_all"),
            (&["update", "all", "--jobs", "0"], "usage_bad_jobs"),
            (&["status", "--lang", "xx"], "usage_bad_lang"),
            (&["version", "--yes=no"], "usage_unknown_option"),
        ];
        for (line, key) in cases {
            assert_eq!(problem_key(line), key, "{line:?}");
        }
        assert_eq!(read(&["uninstall", "todos"]).problem, None, "todos no es all: es un juego que se llama así");
    }

    #[test]
    fn every_command_has_its_help_and_a_word_that_names_it_back() {
        for (word, command, key) in COMMANDS {
            assert!(key == "help" || key == format!("help_{}", word.replace('-', "_")), "{word}: {key}");
            assert_eq!(Command::named(word), Some(command));
            assert_eq!(command.word(), word);
        }
        for (_, command) in LIST_COMMANDS {
            assert_eq!(command.word(), "list");
        }
    }

    #[test]
    fn the_command_reference_names_every_command_list_command_and_option() {
        let reference = include_str!("../../commands.txt");
        let words: Vec<&str> = reference.split(|c: char| !c.is_alphanumeric() && c != '-').collect();
        for (word, _, _) in COMMANDS {
            assert!(words.contains(&word), "commands.txt no nombra la orden {word}");
        }
        for (word, _) in LIST_COMMANDS {
            assert!(reference.contains(&format!("\nlist {word} ")), "commands.txt no nombra list {word}");
        }
        for (long, short, _, _) in FLAGS {
            for name in [long, short].into_iter().filter(|name| !name.is_empty()) {
                assert!(words.contains(&name), "commands.txt no nombra la opción {name}");
            }
        }
    }
}
