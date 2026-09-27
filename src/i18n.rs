use std::collections::HashMap;

const ERR_SEP: char = '\u{1}';

pub const LANGS: [&str; 6] = ["es", "en", "fr", "pt", "de", "pl"];

const FILES: [&str; LANGS.len()] = [
    include_str!("../lang/es.txt"),
    include_str!("../lang/en.txt"),
    include_str!("../lang/fr.txt"),
    include_str!("../lang/pt.txt"),
    include_str!("../lang/de.txt"),
    include_str!("../lang/pl.txt"),
];

pub fn err_key(key: &str, arg: &str) -> String {
    format!("{}{}{}", key, ERR_SEP, arg)
}

pub fn key_of(payload: &str) -> &str {
    payload.split(ERR_SEP).next().unwrap_or(payload)
}

pub struct I18n {
    lang: String,
    tables: HashMap<&'static str, HashMap<&'static str, &'static str>>,
}

fn known(lang: &str) -> String {
    if LANGS.contains(&lang) {
        lang.to_string()
    } else {
        "es".to_string()
    }
}

fn entries(file: &'static str) -> impl Iterator<Item = (&'static str, &'static str)> {
    file.trim_start_matches('\u{feff}')
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(|line| match line.split_once('=') {
            Some((key, text)) => (key.trim(), text),
            None => (line.trim(), ""),
        })
}

impl I18n {
    pub fn new(lang: &str) -> I18n {
        let tables = LANGS.into_iter().zip(FILES).map(|(code, file)| (code, entries(file).collect())).collect();
        I18n { lang: known(lang), tables }
    }

    pub fn set_lang(&mut self, lang: &str) {
        self.lang = known(lang);
    }

    pub fn t(&self, key: &str) -> String {
        self.tables
            .get(self.lang.as_str())
            .and_then(|m| m.get(key))
            .or_else(|| self.tables.get("es").and_then(|m| m.get(key)))
            .copied()
            .unwrap_or(key)
            .to_string()
    }

    pub fn tf(&self, key: &str, arg: &str) -> String {
        self.t(key).replace("{}", arg)
    }

    pub fn tfn(&self, key: &str, args: &[&str]) -> String {
        let text = self.t(key);
        let mut parts = text.split("{}");
        let mut out = parts.next().unwrap_or_default().to_string();
        for (part, arg) in parts.zip(args.iter().chain(std::iter::repeat(&""))) {
            out.push_str(arg);
            out.push_str(part);
        }
        out
    }

    pub fn t_err(&self, payload: &str) -> String {
        match payload.split_once(ERR_SEP) {
            Some((key, arg)) => self.tf(key, &self.t_err(arg)),
            None => self.t(payload),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::path::PathBuf;

    use regex::Regex;

    use super::*;

    const BUILT_KEYS: [&str; 11] = [
        "lang_es", "lang_en", "lang_fr", "lang_pt", "lang_de", "lang_pl",
        "err_io_mkdir", "err_io_write", "err_io_delete", "err_io_read", "err_io_create",
    ];

    fn production_code() -> String {
        let mut code = String::new();
        let mut dirs = vec![PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))];
        while let Some(dir) = dirs.pop() {
            for path in fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().path()) {
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let mut closing: Option<String> = None;
                    for line in fs::read_to_string(&path).unwrap().lines() {
                        let code_line = line.trim_start();
                        if let Some(end) = &closing {
                            if line == end {
                                closing = None;
                            }
                        } else if code_line == "mod tests {" {
                            closing = Some(format!("{}}}", &line[..line.len() - code_line.len()]));
                        } else if !code_line.starts_with("//") && !code_line.starts_with("#[") {
                            code.push_str(line);
                            code.push('\n');
                        }
                    }
                }
            }
        }
        code
    }

    fn spanish_keys() -> HashSet<&'static str> {
        entries(FILES[0]).map(|(key, _)| key).collect()
    }

    #[test]
    fn falls_back_to_key_then_spanish() {
        let i = I18n::new("en");
        assert_eq!(i.t("uninstall"), "Uninstall");
        assert_eq!(i.t("clave_inexistente"), "clave_inexistente");
    }

    #[test]
    fn spanish_default_and_switch() {
        let mut i = I18n::new("es");
        assert_eq!(i.t("uninstall"), "Desinstalar");
        i.set_lang("en");
        assert_eq!(i.t("uninstall"), "Uninstall");
        i.set_lang("pl");
        assert_eq!(i.t("uninstall"), "Odinstaluj");
    }

    #[test]
    fn format_arg() {
        let i = I18n::new("es");
        assert_eq!(i.tf("profile_label", "Pokemon Z"), "perfil: Pokemon Z");
    }

    #[test]
    fn unknown_lang_defaults_spanish() {
        let i = I18n::new("xx");
        assert_eq!(i.t("uninstall"), "Desinstalar");
    }

    #[test]
    fn every_listed_language_has_its_own_table() {
        let names = ["Español", "English", "Français", "Português", "Deutsch", "Polski"];
        for (lang, name) in LANGS.into_iter().zip(names) {
            assert_eq!(I18n::new(lang).t(&format!("lang_{}", lang)), name, "sin su tabla: {}", lang);
        }
    }

    #[test]
    fn t_err_resolves_bare_key() {
        let i = I18n::new("en");
        assert_eq!(i.t_err("status_unknown"), "Unknown version (offline)");
    }

    #[test]
    fn t_err_resolves_key_with_argument() {
        let i = I18n::new("es");
        let msg = i.t_err(&err_key("err_download_corrupt", "core/nav/locator.rb"));
        assert!(msg.contains("core/nav/locator.rb"));
        assert!(!msg.contains("err_download_corrupt"));
        assert!(!msg.contains('\u{1}'));
    }

    #[test]
    fn t_err_resolves_a_wrapped_error_in_the_active_language() {
        let nested = err_key("err_list_files", &err_key("err_download", "timeout"));
        assert_eq!(I18n::new("en").t_err(&nested), "Could not get the mod's file list: Download failed: timeout");
        let bare = err_key("err_selfupdate_download", "err_rate_limited_short");
        assert_eq!(I18n::new("de").t_err(&bare),
                   "Der neue Installer konnte nicht heruntergeladen werden: GitHub begrenzt gerade die Anfragen. Warte ein wenig und versuche es erneut.");
    }

    #[test]
    fn t_err_passes_literal_messages_through() {
        let i = I18n::new("es");
        assert_eq!(i.t_err("descarga estado 404"), "descarga estado 404");
    }

    #[test]
    fn every_key_is_translated_in_every_language() {
        let es = spanish_keys();
        for (lang, file) in LANGS.into_iter().zip(FILES).skip(1) {
            let table: HashSet<&str> = entries(file).map(|(key, _)| key).collect();
            let mut missing: Vec<&str> = es.symmetric_difference(&table).copied().collect();
            missing.sort_unstable();
            assert!(missing.is_empty(), "claves sin pareja es/{}: {:?}", lang, missing);
        }
    }

    #[test]
    fn placeholders_survive_every_translation() {
        let es: HashMap<&str, &str> = entries(FILES[0]).collect();
        for (lang, file) in LANGS.into_iter().zip(FILES).skip(1) {
            for (k, v) in entries(file) {
                let want = es.get(k).map_or(0, |text| text.matches("{}").count());
                assert_eq!(v.matches("{}").count(), want, "hueco {{}} perdido o sobrante en {}:{}", lang, k);
            }
        }
    }

    #[test]
    fn every_line_is_a_key_and_its_text() {
        for (lang, file) in LANGS.into_iter().zip(FILES) {
            for (key, text) in entries(file) {
                let well_named = !key.is_empty() && key.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
                assert!(well_named, "{}: clave mal escrita o línea sin '=': {:?}", lang, key);
                assert!(!text.is_empty() && text == text.trim(), "{}:{}: texto vacío o con espacios en los bordes", lang, key);
            }
        }
    }

    #[test]
    fn no_file_writes_a_key_twice() {
        for (lang, file) in LANGS.into_iter().zip(FILES) {
            let mut seen = HashSet::new();
            let twice: Vec<&str> = entries(file).map(|(key, _)| key).filter(|key| !seen.insert(*key)).collect();
            assert!(twice.is_empty(), "{}: claves repetidas {:?}", lang, twice);
        }
    }

    #[test]
    fn every_key_is_used_by_the_code() {
        let code = production_code();
        let mut unused: Vec<&str> = spanish_keys().into_iter()
            .filter(|key| !BUILT_KEYS.contains(key) && !code.contains(&format!("\"{}\"", key)))
            .collect();
        unused.sort_unstable();
        assert!(unused.is_empty(), "claves que el código no usa: {:?}", unused);
    }

    #[test]
    fn every_key_the_code_names_exists() {
        let code = production_code();
        let called = Regex::new(r#"\b(?:t|tf|t_err|err_key)\(\s*"([^"]*)""#).unwrap();
        let snake_case = Regex::new(r#""([a-z][a-z0-9]*(?:_[a-z0-9]+)+)""#).unwrap();
        let named = called.captures_iter(&code).chain(snake_case.captures_iter(&code)).map(|c| c[1].to_string());
        let keys = spanish_keys();
        let mut missing: Vec<String> = named.chain(BUILT_KEYS.map(String::from))
            .filter(|key| !keys.contains(key.as_str()))
            .collect();
        missing.sort_unstable();
        missing.dedup();
        assert!(missing.is_empty(), "claves que no existen: {:?}", missing);
    }

    #[test]
    fn edit_controls_and_errors_are_translated_for_every_language() {
        for lang in LANGS {
            let i = I18n::new(lang);
            for key in ["change_profile_btn", "edit_title", "edit_name", "edit_profile", "edit_path",
                "edit_executable", "edit_ok", "edit_cancel", "edit_correct", "edit_continue",
                "edit_invalid", "invalid_game_path", "invalid_executable", "launching", "play_btn",
                "already_running", "exe_ask_on_play", "pick_exe_title", "pick_exe_prompt", "pick_exe_remember",
                "convert_play_hint", "convert_profile_kept", "convert_exe_missing", "catalog_loading_wait"] {
                assert_ne!(i.t(key), key, "{lang}: {key}");
            }
        }
        assert!(I18n::new("es").t("change_profile_btn").contains("Editar"));
        assert!(I18n::new("en").t("change_profile_btn").contains("Edit"));
    }
}
