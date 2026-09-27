use std::collections::BTreeMap;
use std::io::{self, BufRead, IsTerminal, Write};
use std::num::NonZeroIsize;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};

use super::{CANCELLED, CANNOT_ASK};
use crate::console;
use crate::i18n::{key_of, I18n};

const CONSOLE_TWINS: [(&str, &str); 4] = [
    ("convert_play_hint", "cli_play_hint"),
    ("convert_exe_missing", "cli_exe_missing"),
    ("err_profile_missing", "cli_profile_missing"),
    ("err_launcher_too_old", "cli_launcher_too_old"),
];

pub struct Out {
    pub i18n: I18n,
    quiet: bool,
    verbose: bool,
    program: String,
    last_said: Mutex<Instant>,
}

impl Out {
    pub fn new(i18n: I18n, quiet: bool, verbose: bool, program: String) -> Out {
        Out { i18n, quiet, verbose, program, last_said: Mutex::new(Instant::now()) }
    }

    pub fn program(&self) -> &str {
        &self.program
    }

    pub fn quiet(&self) -> bool {
        self.quiet
    }

    pub fn t(&self, key: &str) -> String {
        self.i18n.t(key)
    }

    pub fn tf(&self, key: &str, arg: &str) -> String {
        self.i18n.tf(key, arg)
    }

    pub fn tfn(&self, key: &str, args: &[&str]) -> String {
        self.i18n.tfn(key, args)
    }

    pub fn say(&self, payload: &str) -> String {
        let key = key_of(payload);
        match CONSOLE_TWINS.iter().find(|(window, _)| *window == key) {
            Some((window, console)) => self.i18n.t_err(&payload.replacen(window, console, 1)),
            None => self.i18n.t_err(payload),
        }
    }

    pub fn info(&self, line: &str) {
        if !self.quiet {
            self.write(false, line);
        }
    }

    pub fn result(&self, line: &str) {
        self.write(false, line);
    }

    pub fn detail(&self, line: &str) {
        if self.verbose {
            self.write(false, line);
        }
    }

    pub fn warn(&self, text: &str) {
        if !self.quiet {
            self.write(false, &self.tf("warning", text));
        }
    }

    pub fn fail(&self, text: &str) {
        self.write(true, &self.tf("error", text));
    }

    pub fn fail_line(&self, line: &str) {
        self.write(true, line);
    }

    pub fn silence(&self) -> Duration {
        self.last_said.lock().unwrap_or_else(|e| e.into_inner()).elapsed()
    }

    fn write(&self, to_stderr: bool, line: &str) {
        let mut last = self.last_said.lock().unwrap_or_else(|e| e.into_inner());
        let _ = if to_stderr {
            writeln!(io::stderr().lock(), "{}", line)
        } else {
            writeln!(io::stdout().lock(), "{}", line)
        };
        *last = Instant::now();
    }

    pub fn confirm(&self, yes: bool, question: &str) -> Result<(), i32> {
        if yes {
            return Ok(());
        }
        if !interactive() {
            self.fail(&self.tf("ask_needs_yes", question));
            return Err(CANNOT_ASK);
        }
        match self.ask(&self.tf("ask_yes_no", question)) {
            Some(answer) if is_yes(&answer, &self.t("yes_letter")) => Ok(()),
            _ => Err(self.cancelled()),
        }
    }

    pub fn choose(&self, header: &str, items: &[String], cannot: &str) -> Result<usize, i32> {
        if !interactive() {
            self.fail(cannot);
            return Err(CANNOT_ASK);
        }
        self.result(header);
        for (i, item) in items.iter().enumerate() {
            self.result(&format!("{}. {}", i + 1, item));
        }
        let mut prompt = self.tf("choose_number", &items.len().to_string());
        while let Some(answer) = self.ask(&prompt).filter(|a| !a.trim().is_empty()) {
            match answer.trim().parse::<usize>() {
                Ok(n) if (1..=items.len()).contains(&n) => return Ok(n - 1),
                _ => prompt = self.tf("choose_invalid", &items.len().to_string()),
            }
        }
        Err(self.cancelled())
    }

    pub fn pick_folder(&self) -> Result<PathBuf, i32> {
        if !interactive() {
            self.fail(&self.t("needs_game"));
            return Err(CANNOT_ASK);
        }
        let picked = if console::desktop_visible() {
            self.info(&self.t("picker_opening"));
            let dialog = rfd::FileDialog::new().set_title(self.t("pick_folder"));
            match console::window() {
                Some(owner) => dialog.set_parent(&ConsoleOwner(owner)).pick_folder(),
                None => dialog.pick_folder(),
            }
        } else {
            self.ask(&self.t("type_folder"))
                .map(|typed| PathBuf::from(typed.trim().trim_matches('"')))
                .filter(|path| !path.as_os_str().is_empty())
        };
        picked.ok_or_else(|| self.cancelled())
    }

    pub fn pause(&self) {
        if interactive() {
            let _ = self.ask(&self.t("pause_prompt"));
        }
    }

    pub fn cancelled(&self) -> i32 {
        self.result(&self.t("cancelled"));
        CANCELLED
    }

    fn ask(&self, prompt: &str) -> Option<String> {
        {
            let mut last = self.last_said.lock().unwrap_or_else(|e| e.into_inner());
            let mut stdout = io::stdout().lock();
            let _ = write!(stdout, "{} ", prompt);
            let _ = stdout.flush();
            *last = Instant::now();
        }
        let mut line = String::new();
        match io::stdin().lock().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim_end_matches(['\r', '\n']).to_string()),
        }
    }
}

pub fn interactive() -> bool {
    io::stdin().is_terminal()
}

fn is_yes(answer: &str, letter: &str) -> bool {
    let first = answer.trim().chars().next().map(|c| c.to_lowercase().to_string());
    first.is_some_and(|c| c == "y" || c == letter.to_lowercase())
}

struct ConsoleOwner(NonZeroIsize);

impl HasWindowHandle for ConsoleOwner {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = RawWindowHandle::Win32(Win32WindowHandle::new(self.0));
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for ConsoleOwner {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::windows())
    }
}

pub type Line = (bool, String);

#[derive(Default)]
pub struct InOrder {
    waiting: BTreeMap<usize, Line>,
    next: usize,
}

impl InOrder {
    pub fn finish(&mut self, i: usize, line: Line) -> Vec<Line> {
        self.waiting.insert(i, line);
        let mut ready = Vec::new();
        while let Some(line) = self.waiting.remove(&self.next) {
            ready.push(line);
            self.next += 1;
        }
        ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_that_end_out_of_order_are_said_in_the_order_of_the_games() {
        let mut order = InOrder::default();
        let line = |text: &str| (false, text.to_string());
        assert!(order.finish(2, line("tercero")).is_empty());
        assert!(order.finish(1, (true, "segundo".into())).is_empty());
        assert_eq!(order.finish(0, line("primero")), vec![line("primero"), (true, "segundo".into()), line("tercero")]);
        assert_eq!(order.finish(3, line("cuarto")), vec![line("cuarto")]);
    }

    #[test]
    fn yes_is_the_letter_of_the_language_or_y_and_anything_else_is_no() {
        for (lang, yes) in [("es", "sí"), ("es", "S"), ("en", "yes"), ("fr", "oui"), ("pt", "sim"), ("de", "Ja"),
            ("pl", "tak"), ("de", "y")] {
            assert!(is_yes(yes, &I18n::new(lang).t("yes_letter")), "{lang}: {yes}");
        }
        for (lang, no) in [("es", ""), ("es", "no"), ("en", "n"), ("fr", "non"), ("de", "nein"), ("pl", "nie"),
            ("es", "   ")] {
            assert!(!is_yes(no, &I18n::new(lang).t("yes_letter")), "{lang}: {no:?}");
        }
    }

    #[test]
    fn a_message_that_names_a_button_of_the_window_is_told_in_the_console_s_words() {
        let out = Out::new(I18n::new("es"), false, false, "pea".into());
        let hint = out.say(&crate::i18n::err_key("convert_play_hint", "Game (PokeAccess).exe"));
        assert!(hint.contains("Game (PokeAccess).exe") && hint.contains("play") && !hint.contains("botón"), "{hint}");
        for (window, console) in CONSOLE_TWINS {
            assert_ne!(out.t(window), out.t(console));
        }
        assert_eq!(out.say("no_write_perm"), out.t("no_write_perm"));
    }
}
