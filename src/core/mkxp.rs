use std::fs;
use std::path::{Path, PathBuf};

const MARKER: &str = "accessibility/preload_access.rb";
const JSON_NAME: &str = "mkxp.json";
const BACKUP_NAME: &str = "mkxp.json.access.bak";

const CREATED_BY: &str = "// === MOD DE ACCESIBILIDAD (anadido por el instalador) ===";

const CREATED_BY_PREFIX: &str = "// === MOD DE ACCESIBILIDAD";

const COMPAT_PREFIX: &str = "accessibility/game/";

pub fn mkxp_json(game_dir: &Path) -> PathBuf {
    game_dir.join(JSON_NAME)
}

fn backup_of(json: &Path) -> PathBuf {
    json.with_extension("json.access.bak")
}

pub fn has_mkxp_json(game_dir: &Path) -> bool {
    mkxp_json(game_dir).exists()
}

fn read_json(path: &Path) -> Result<String, String> {
    fs::read(path)
        .map(|bytes| super::detect::decode_text(&bytes))
        .map_err(|e| super::apply::io_error(JSON_NAME, "read", &e))
}

pub(super) fn strip_comment_lines(text: &str) -> String {
    text.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<&str>>()
        .join("\n")
}

fn array_range(text: &str) -> Option<(usize, usize)> {
    let key_pos = text.find("\"preloadScript\"")?;
    let open = text[key_pos..].find('[')? + key_pos;
    let close = text[open..].find(']')? + open;
    Some((open, close))
}

fn in_comment_line(text: &str, pos: usize) -> bool {
    let line_start = text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    text[line_start..pos].trim_start().starts_with("//")
}

fn find_active_key(text: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = text[from..].find("\"preloadScript\"") {
        let pos = from + rel;
        if !in_comment_line(text, pos) {
            return Some(pos);
        }
        from = pos + 1;
    }
    None
}

fn first_live_brace(text: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = text[from..].find('{') {
        let pos = from + rel;
        if !in_comment_line(text, pos) {
            return Some(pos);
        }
        from = pos + 1;
    }
    None
}

fn array_entries(inner: &str) -> Vec<String> {
    inner
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn is_registered(text: &str) -> bool {
    let active = strip_comment_lines(text);
    match array_range(&active) {
        Some((open, close)) => array_entries(&active[open + 1..close]).iter().any(|e| e == MARKER),
        None => false,
    }
}

fn has_key(text: &str) -> bool {
    strip_comment_lines(text).contains("\"preloadScript\"")
}

pub fn compat_entry(file: &str) -> String {
    format!("{}{}", COMPAT_PREFIX, file)
}

fn is_compat_value(v: &str) -> bool {
    v.starts_with(COMPAT_PREFIX) && v.ends_with(".rb")
}

fn mod_entries(text: &str) -> Vec<String> {
    let active = strip_comment_lines(text);
    match array_range(&active) {
        Some((open, close)) => array_entries(&active[open + 1..close])
            .into_iter()
            .filter(|e| e == MARKER || is_compat_value(e))
            .collect(),
        None => Vec::new(),
    }
}

fn loader_entries(compat: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = compat.map(compat_entry).into_iter().collect();
    out.push(MARKER.to_string());
    out
}

pub fn is_registered_with(text: &str, compat: Option<&str>) -> bool {
    mod_entries(text) == loader_entries(compat)
}

#[cfg(test)]
pub fn add_marker(text: &str) -> Option<String> {
    add_loader(text, None)
}

pub fn add_loader(text: &str, compat: Option<&str>) -> Option<String> {
    if is_registered_with(text, compat) {
        return Some(text.to_string());
    }
    let clean = remove_entries(text, is_compat_entry);
    let out = if is_registered(&clean) {
        match compat {
            Some(c) => insert_before_marker(&clean, &compat_entry(c)),
            None => Some(clean),
        }
    } else if has_key(&clean) {
        add_to_existing_array(&clean, &loader_entries(compat))
    } else {
        let quoted: Vec<String> = loader_entries(compat).iter().map(|e| format!("\"{}\"", e)).collect();
        insert_after_root_brace(&clean, CREATED_BY, &format!("\"preloadScript\": [{}]", quoted.join(", ")))
    };
    out.filter(|t| is_registered_with(t, compat))
}

fn add_to_existing_array(text: &str, entries: &[String]) -> Option<String> {
    let key_pos = find_active_key(text)?;
    let open = text[key_pos..].find('[')? + key_pos;
    let close = text[open..].find(']')? + open;
    let inner = text[open + 1..close].trim_start();
    let head = entries.iter().map(|e| format!("\"{}\"", e)).collect::<Vec<String>>().join(", ");
    let new_inner = if inner.is_empty() {
        head
    } else {
        format!("{}, {}", head, inner)
    };
    let mut out = String::with_capacity(text.len() + new_inner.len());
    out.push_str(&text[..open + 1]);
    out.push_str(&new_inner);
    out.push_str(&text[close..]);
    Some(out)
}

fn insert_before_marker(text: &str, entry: &str) -> Option<String> {
    let key_pos = find_active_key(text)?;
    let open = text[key_pos..].find('[')? + key_pos;
    let close = text[open..].find(']')? + open;
    let literal = format!("\"{}\"", MARKER);
    let mut from = open;
    while let Some(rel) = text[from..close].find(&literal) {
        let pos = from + rel;
        if !in_comment_line(text, pos) {
            return Some(format!("{}\"{}\", {}", &text[..pos], entry, &text[pos..]));
        }
        from = pos + 1;
    }
    None
}

fn is_marker_entry(entry: &str) -> bool {
    let live = strip_comment_lines(entry);
    let v = live.trim().trim_matches('"');
    v == MARKER || is_compat_value(v)
}

fn is_compat_entry(entry: &str) -> bool {
    is_compat_value(strip_comment_lines(entry).trim().trim_matches('"'))
}

fn remove_entries(text: &str, hit: fn(&str) -> bool) -> String {
    let key_pos = match find_active_key(text) {
        Some(p) => p,
        None => return text.to_string(),
    };
    let open = match text[key_pos..].find('[') {
        Some(p) => p + key_pos,
        None => return text.to_string(),
    };
    let close = match text[open..].find(']') {
        Some(p) => p + open,
        None => return text.to_string(),
    };
    let inner = &text[open + 1..close];
    if !inner.split(',').any(hit) {
        return text.to_string();
    }
    let kept: Vec<&str> = inner.split(',').filter(|s| !s.is_empty() && !hit(s)).collect();
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..open + 1]);
    out.push_str(&kept.join(","));
    out.push_str(&text[close..]);
    out
}

pub fn remove_marker(text: &str) -> String {
    drop_created_key(&remove_entries(text, is_marker_entry))
}

fn drop_created_key(text: &str) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let key = match lines.iter().position(|l| l.trim_start().starts_with("\"preloadScript\"")) {
        Some(i) => i,
        None => return text.to_string(),
    };
    let top = match banner_start(&lines, key) {
        Some(i) => i,
        None => return text.to_string(),
    };
    let last = if is_emptied_key_line(lines[key]) || is_emptied_last_key(&lines, key) { key } else { key - 1 };
    lines.iter().enumerate().filter(|&(i, _)| i < top || i > last).map(|(_, l)| *l).collect()
}

fn is_emptied_last_key(lines: &[&str], key: usize) -> bool {
    let re = regex::Regex::new(r#"^"preloadScript"\s*:\s*\[\s*\]\s*$"#)
        .expect("the emptied last key pattern is a literal and always compiles");
    re.is_match(lines[key].trim()) && closes_object(&lines[key + 1..].concat())
}

fn banner_start(lines: &[&str], key: usize) -> Option<usize> {
    let mut top = None;
    let mut i = key;
    while i > 0 {
        i -= 1;
        let line = lines[i].trim_start();
        if !line.starts_with("//") {
            break;
        }
        if line.starts_with(CREATED_BY_PREFIX) {
            top = Some(i);
        }
    }
    top
}

fn is_emptied_key_line(line: &str) -> bool {
    regex::Regex::new(r#"^"preloadScript"\s*:\s*\[\s*\]\s*,$"#)
        .expect("the emptied array pattern is a literal and always compiles")
        .is_match(line.trim())
}

pub fn ensure_json(game_dir: &Path) -> Result<(), String> {
    let path = mkxp_json(game_dir);
    if path.exists() {
        return Ok(());
    }
    fs::write(&path, "{}").map_err(|e| super::apply::io_error(JSON_NAME, "create", &e))
}

pub fn register(game_dir: &Path, compat: Option<&str>) -> Result<(), String> {
    let path = mkxp_json(game_dir);
    ensure_json(game_dir)?;
    let text = read_json(&path)?;
    if is_registered_with(&text, compat) {
        return Ok(());
    }
    let bak = backup_of(&path);
    if !bak.exists() {
        fs::copy(&path, &bak).map_err(|e| super::apply::io_error(BACKUP_NAME, "create", &e))?;
    }
    let updated = add_loader(&text, compat).ok_or_else(|| "err_mkxp_no_root".to_string())?;
    fs::write(&path, updated).map_err(|e| super::apply::io_error(JSON_NAME, "write", &e))
}

pub fn unregister(game_dir: &Path) -> Result<(), String> {
    let path = mkxp_json(game_dir);
    let written = match fs::read(&path) {
        Ok(bytes) => write_without_marker(&path, &super::detect::decode_text(&bytes)),
        Err(_) => Ok(()),
    };
    if written.is_ok() && created_by_launcher(&path) && is_empty_object(&path) {
        let _ = fs::remove_file(&path);
    }
    drop_backup(&path);
    written
}

fn created_by_launcher(json: &Path) -> bool {
    fs::read(backup_of(json))
        .map(|b| super::detect::decode_text(&b).trim() == "{}")
        .unwrap_or(false)
}

fn is_empty_object(json: &Path) -> bool {
    fs::read(json)
        .map(|b| strip_comment_lines(&super::detect::decode_text(&b)).split_whitespace().collect::<String>() == "{}")
        .unwrap_or(false)
}

fn write_without_marker(path: &Path, text: &str) -> Result<(), String> {
    let cleaned = remove_marker(text);
    if cleaned == text {
        return Ok(());
    }
    fs::write(path, cleaned).map_err(|e| super::apply::io_error(JSON_NAME, "write", &e))
}

fn drop_backup(json: &Path) {
    let _ = fs::remove_file(backup_of(json));
}

fn insert_after_root_brace(text: &str, banner: &str, key_line: &str) -> Option<String> {
    let idx = first_live_brace(text)?;
    let after = &text[idx + 1..];
    let (eol, rest) = match after.strip_prefix("\r\n") {
        Some(r) => ("\r\n", r),
        None => ("\n", after.strip_prefix('\n').unwrap_or(after)),
    };
    let comma = if closes_object(rest) { "" } else { "," };
    let mut out = String::with_capacity(text.len() + 128);
    out.push_str(&text[..=idx]);
    for line in [banner, &format!("{}{}", key_line, comma)] {
        out.push_str(eol);
        out.push_str("  ");
        out.push_str(line);
    }
    out.push_str(eol);
    out.push_str(rest);
    Some(out)
}

fn closes_object(rest: &str) -> bool {
    strip_comment_lines(rest).trim_start().starts_with('}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_active_registration() {
        assert!(is_registered("{\n  \"preloadScript\": [\"accessibility/preload_access.rb\"]\n}"));
    }

    #[test]
    fn ignores_commented_registration() {
        assert!(!is_registered("{\n  // \"preloadScript\": [\"accessibility/preload_access.rb\"]\n}"));
        assert!(!is_registered("{\n  \"rgssVersion\": 1\n}"));
    }

    #[test]
    fn adds_key_when_absent() {
        let out = add_marker("{\n  \"rgssVersion\": 1\n}").unwrap();
        assert!(is_registered(&out));
    }

    #[test]
    fn adds_to_existing_array_without_duplicating_key() {
        let src = "{\n  \"preloadScript\": [\"user.rb\"]\n}";
        let out = add_marker(src).unwrap();
        assert!(is_registered(&out));
        assert!(out.contains("user.rb"));
        assert_eq!(out.matches("\"preloadScript\"").count(), 1);
    }

    #[test]
    fn compat_goes_right_ahead_of_the_loader() {
        let out = add_loader("{\n  \"rgssVersion\": 1\n}", Some("compat.rb")).unwrap();
        let json: serde_json::Value = serde_json::from_str(&strip_comment_lines(&out)).unwrap();
        assert_eq!(json["preloadScript"], serde_json::json!(["accessibility/game/compat.rb", MARKER]));
        assert_eq!(add_loader(&out, Some("compat.rb")).unwrap(), out);
    }

    #[test]
    fn compat_joins_a_loader_already_registered_without_moving_lines() {
        let src = "{\n  \"preloadScript\": [\n    // \"old.rb\",\n    \"accessibility/preload_access.rb\",\n    \"user.rb\"\n  ]\n}";
        let out = add_loader(src, Some("compat.rb")).unwrap();
        assert!(is_registered_with(&out, Some("compat.rb")));
        assert_eq!(out.lines().count(), src.lines().count());
        let json: serde_json::Value = serde_json::from_str(&strip_comment_lines(&out)).unwrap();
        assert_eq!(json["preloadScript"], serde_json::json!(["accessibility/game/compat.rb", MARKER, "user.rb"]));
    }

    #[test]
    fn another_profiles_compat_leaves_the_array() {
        let src = "{ \"preloadScript\": [\"accessibility/game/compat.rb\", \"accessibility/preload_access.rb\"] }";
        let out = add_loader(src, None).unwrap();
        assert!(!out.contains("compat.rb"));
        assert!(is_registered_with(&out, None));
        let other = add_loader(src, Some("other.rb")).unwrap();
        let json: serde_json::Value = serde_json::from_str(&other).unwrap();
        assert_eq!(json["preloadScript"], serde_json::json!(["accessibility/game/other.rb", MARKER]));
    }

    #[test]
    fn remove_takes_the_compat_with_the_loader() {
        let src = "{ \"preloadScript\": [\"accessibility/game/compat.rb\", \"accessibility/preload_access.rb\", \"user.rb\"] }";
        let json: serde_json::Value = serde_json::from_str(&remove_marker(src)).unwrap();
        assert_eq!(json["preloadScript"], serde_json::json!(["user.rb"]));
    }

    #[test]
    fn register_writes_the_compat_first_and_unregister_clears_both() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(JSON_NAME), "{\n  \"rgssVersion\": 1\n}\n").unwrap();
        register(dir.path(), Some("compat.rb")).unwrap();
        let text = fs::read_to_string(dir.path().join(JSON_NAME)).unwrap();
        assert!(is_registered_with(&text, Some("compat.rb")));
        unregister(dir.path()).unwrap();
        let back = fs::read_to_string(dir.path().join(JSON_NAME)).unwrap();
        assert!(!back.contains("accessibility/"));
    }

    #[test]
    fn add_marker_idempotent() {
        let src = "{\n  \"preloadScript\": [\"accessibility/preload_access.rb\"]\n}";
        let out = add_marker(src).unwrap();
        assert_eq!(out.matches(MARKER).count(), 1);
    }

    #[test]
    fn remove_keeps_other_scripts_single_line() {
        let src = "{ \"preloadScript\": [\"accessibility/preload_access.rb\", \"user.rb\"], \"x\": 1 }";
        let out = remove_marker(src);
        assert!(!out.contains(MARKER));
        assert!(out.contains("user.rb"));
        assert!(out.contains("\"x\": 1"));
    }

    #[test]
    fn remove_empties_array_when_only_marker() {
        let src = "{ \"preloadScript\": [\"accessibility/preload_access.rb\"] }";
        let out = remove_marker(src);
        assert!(!out.contains(MARKER));
        assert!(out.contains("\"preloadScript\": []"));
    }

    #[test]
    fn is_registered_multiline_array() {
        let src = "{\n  \"preloadScript\": [\n    \"user.rb\",\n    \"accessibility/preload_access.rb\"\n  ]\n}";
        assert!(is_registered(src));
    }

    #[test]
    fn add_marker_idempotent_multiline() {
        let src = "{\n  \"preloadScript\": [\n    \"accessibility/preload_access.rb\"\n  ]\n}";
        let out = add_marker(src).unwrap();
        assert_eq!(out.matches(MARKER).count(), 1);
    }

    fn parse_without_comments(text: &str) -> serde_json::Value {
        serde_json::from_str(&strip_comment_lines(text))
            .unwrap_or_else(|e| panic!("mkxp.json ya no es JSON valido ({}):\n{}", e, text))
    }

    #[test]
    fn remove_keeps_commented_entries_on_their_own_line() {
        let src = "{\n  \"preloadScript\": [\n    \"accessibility/preload_access.rb\",\n    // \"desactivado.rb\",\n    \"mi_script.rb\"\n  ]\n}";
        let out = remove_marker(src);
        assert!(!out.contains(MARKER));
        assert!(out.contains("\n    // \"desactivado.rb\","));
        assert!(out.contains("\n    \"mi_script.rb\""));
        let commented = out.lines().find(|l| l.trim_start().starts_with("//")).unwrap();
        assert!(!commented.contains(']'), "el comentario se traga el cierre del array:\n{}", out);
    }

    #[test]
    fn remove_leaves_valid_json_when_entries_are_commented() {
        let src = "{\n  \"preloadScript\": [\n    \"accessibility/preload_access.rb\",\n    // \"desactivado.rb\",\n    \"mi_script.rb\"\n  ],\n  \"rgssVersion\": 1\n}";
        let out = remove_marker(src);
        let json = parse_without_comments(&out);
        assert_eq!(json["preloadScript"].as_array().unwrap().len(), 1);
        assert_eq!(json["preloadScript"][0], "mi_script.rb");
        assert_eq!(json["rgssVersion"], 1);
    }

    #[test]
    fn remove_leaves_valid_json_when_the_marker_is_last() {
        let src = "{\n  \"preloadScript\": [\n    // \"viejo.rb\",\n    \"mi_script.rb\",\n    \"accessibility/preload_access.rb\"\n  ]\n}";
        let out = remove_marker(src);
        assert!(!out.contains(MARKER));
        let json = parse_without_comments(&out);
        assert_eq!(json["preloadScript"][0], "mi_script.rb");
        assert_eq!(json["preloadScript"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn remove_does_not_delete_substring_lookalike() {
        let src = "{ \"preloadScript\": [\"accessibility/preload_access.rb.bak\"] }";
        let out = remove_marker(src);
        assert!(out.contains("accessibility/preload_access.rb.bak"));
    }

    #[test]
    fn is_registered_ignores_commented_multiline() {
        let src = "{\n  // \"preloadScript\": [\"accessibility/preload_access.rb\"],\n  \"rgssVersion\": 1\n}";
        assert!(!is_registered(src));
    }

    #[test]
    fn add_inserts_into_real_array_not_commented_one() {
        let src = "{\n  // \"preloadScript\": [\"old.rb\"],\n  \"preloadScript\": [\"user.rb\"]\n}";
        let out = add_marker(src).unwrap();
        assert!(is_registered(&out));
        assert_eq!(out.matches(MARKER).count(), 1);
        assert!(out.contains("// \"preloadScript\": [\"old.rb\"],"));
        let commented = out.lines().find(|l| l.trim_start().starts_with("//")).unwrap();
        assert!(!commented.contains(MARKER));
        assert!(out.contains("user.rb"));
    }

    #[test]
    fn add_creates_key_when_only_commented_one_exists() {
        let src = "{\n  // \"preloadScript\": [\"old.rb\"],\n  \"rgssVersion\": 1\n}";
        let out = add_marker(src).unwrap();
        assert!(is_registered(&out));
        assert!(out.contains("// \"preloadScript\": [\"old.rb\"],"));
        let commented = out.lines().find(|l| l.trim_start().starts_with("//")).unwrap();
        assert!(!commented.contains(MARKER));
    }

    #[test]
    fn add_leaves_valid_json_when_the_last_entry_is_commented() {
        let src = "{\n  \"preloadScript\": [\n    \"mi_script.rb\",\n    \"otro.rb\"\n    // \"viejo.rb\"\n  ],\n  \"rgssVersion\": 1\n}";
        let out = add_marker(src).unwrap();
        assert!(is_registered(&out));
        let commented = out.lines().find(|l| l.trim_start().starts_with("//")).unwrap();
        assert!(!commented.contains(']'), "el comentario se traga el cierre del array:\n{}", out);
        let json = parse_without_comments(&out);
        assert_eq!(json["preloadScript"].as_array().unwrap().len(), 3);
        assert_eq!(json["rgssVersion"], 1);
    }

    fn ps_installed() -> String {
        format!(
            "{{\n    {}\n    \"preloadScript\": [\"{}\"],\n    \"rgssVersion\": 1\n}}",
            CREATED_BY, MARKER
        )
    }

    #[test]
    fn remove_takes_the_comment_and_the_key_the_installer_created() {
        let out = remove_marker(&ps_installed());
        assert!(!out.contains(MARKER));
        assert!(!out.contains("preloadScript"), "queda una clave huerfana:\n{}", out);
        assert!(!out.contains("MOD DE ACCESIBILIDAD"), "queda el comentario huerfano:\n{}", out);
        let json = parse_without_comments(&out);
        assert_eq!(json["rgssVersion"], 1);
        assert!(json.get("preloadScript").is_none());
    }

    #[test]
    fn remove_keeps_a_key_the_installer_created_while_it_holds_player_scripts() {
        let src = ps_installed().replace(
            &format!("[\"{}\"]", MARKER),
            &format!("[\"{}\", \"mi_script.rb\"]", MARKER),
        );
        let out = remove_marker(&src);
        assert!(!out.contains(MARKER));
        assert!(!out.contains("MOD DE ACCESIBILIDAD"), "el comentario ya no describe nada:\n{}", out);
        let json = parse_without_comments(&out);
        assert_eq!(json["preloadScript"][0], "mi_script.rb");
        assert_eq!(json["preloadScript"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn remove_leaves_an_empty_key_the_player_wrote_alone() {
        let src = "{\n  \"preloadScript\": [],\n  \"rgssVersion\": 1\n}";
        assert_eq!(remove_marker(src), src);
    }

    #[test]
    fn remove_takes_the_whole_handwritten_banner() {
        let src = format!(
            "{{\r\n    // === MOD DE ACCESIBILIDAD (lector de pantalla) ===\r\n    // Carga el mod sin tocar Scripts.rxdata.\r\n    // Para desinstalar, borra esta linea.\r\n    \"preloadScript\": [\"{}\"],\r\n\r\n    \"rgssVersion\": 1\r\n}}",
            MARKER
        );
        let out = remove_marker(&src);
        assert!(!out.contains("MOD DE ACCESIBILIDAD"), "queda banner huerfano:\n{}", out);
        assert!(!out.contains("preloadScript"), "queda la clave vacia:\n{}", out);
        assert!(!out.contains("Scripts.rxdata"), "queda media explicacion del mod:\n{}", out);
        assert_eq!(parse_without_comments(&out)["rgssVersion"], 1);
    }

    #[test]
    fn remove_does_not_swallow_a_comment_above_the_banner() {
        let src = format!(
            "{{\n    // no me borres\n    {}\n    \"preloadScript\": [\"{}\"],\n    \"rgssVersion\": 1\n}}",
            CREATED_BY, MARKER
        );
        let out = remove_marker(&src);
        assert!(out.contains("// no me borres"));
        assert!(!out.contains("MOD DE ACCESIBILIDAD"));
        assert!(!out.contains("preloadScript"));
    }

    #[test]
    fn remove_takes_a_last_key_that_has_no_trailing_comma() {
        let src = format!("{{
    {}
    \"preloadScript\": [\"{}\"]
}}", CREATED_BY, MARKER);
        let out = remove_marker(&src);
        assert!(!out.contains("MOD DE ACCESIBILIDAD"));
        assert!(!out.contains("preloadScript"), "{}", out);
        assert!(parse_without_comments(&out).as_object().unwrap().is_empty());
    }

    #[test]
    fn remove_keeps_a_comma_less_key_that_is_followed_by_more() {
        let src = format!(
            "{{
    {}
    \"preloadScript\": [\"{}\"]
    \"rgssVersion\": 1
}}",
            CREATED_BY, MARKER
        );
        let out = remove_marker(&src);
        assert!(!out.contains("MOD DE ACCESIBILIDAD"));
        assert!(out.contains("\"preloadScript\": []"), "{}", out);
    }

    #[test]
    fn a_file_built_over_an_empty_object_is_valid_json_both_ways() {
        let out = add_marker("{}").unwrap();
        assert!(is_registered(&out));
        assert!(!out.contains("],"), "coma colgante:\n{}", out);
        assert_eq!(parse_without_comments(&out)["preloadScript"][0], MARKER);
        let back = remove_marker(&out);
        assert!(!back.contains("preloadScript"), "queda la clave vacia:\n{}", back);
        assert!(!back.contains("MOD DE ACCESIBILIDAD"), "{}", back);
        assert!(parse_without_comments(&back).as_object().unwrap().is_empty());
    }

    #[test]
    fn a_file_of_only_comments_gets_no_dangling_comma() {
        let src = "{\n    // Lines starting with '//' are comments.\n    // \"windowTitle\": \"X\",\n}";
        let out = add_marker(src).unwrap();
        assert!(!out.contains("],"), "{}", out);
        assert_eq!(parse_without_comments(&out)["preloadScript"][0], MARKER);
        assert!(out.contains("// Lines starting with"));
    }

    #[test]
    fn remove_repairs_the_legacy_dangling_comma_before_the_closing_brace() {
        let src = format!("{{\n    {}\n    \"preloadScript\": [\"{}\"],\n}}", CREATED_BY, MARKER);
        let out = remove_marker(&src);
        assert!(!out.contains("MOD DE ACCESIBILIDAD"));
        assert!(!out.contains("preloadScript"), "{}", out);
        assert!(parse_without_comments(&out).as_object().unwrap().is_empty());
    }

    #[test]
    fn add_uses_the_root_brace_not_one_inside_a_comment() {
        let src = "// ejemplo: { \"windowTitle\": \"X\" }\n{\n  \"rgssVersion\": 1\n}";
        let out = add_marker(src).unwrap();
        assert!(is_registered(&out));
        let json = parse_without_comments(&out);
        assert_eq!(json["preloadScript"][0], MARKER);
        assert_eq!(json["rgssVersion"], 1);
    }

    #[test]
    fn add_refuses_a_file_whose_only_brace_is_commented_out() {
        assert!(add_marker("// { \"windowTitle\": \"X\" }\n").is_none());
        assert!(add_marker("sin objeto").is_none());
    }

    #[test]
    fn register_reads_an_ansi_file_and_leaves_it_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = b"{\n  \"windowTitle\": \"Pok".to_vec();
        bytes.push(0xE9);
        bytes.extend_from_slice(b"mon ");
        bytes.push(0xD3);
        bytes.extend_from_slice(b"palo\"\n}");
        fs::write(mkxp_json(dir.path()), &bytes).unwrap();
        register(dir.path(), None).unwrap();
        let after = fs::read_to_string(mkxp_json(dir.path())).expect("el archivo queda en UTF-8");
        assert!(is_registered(&after));
        assert!(after.contains("Pok\u{e9}mon \u{d3}palo"), "{}", after);
        assert_eq!(fs::read(backup_of(&mkxp_json(dir.path()))).unwrap(), bytes);
    }

    #[test]
    fn the_backup_lives_exactly_as_long_as_the_registration() {
        let dir = tempfile::tempdir().unwrap();
        let json = mkxp_json(dir.path());
        fs::write(&json, "{\n  \"rgssVersion\": 1\n}").unwrap();
        register(dir.path(), None).unwrap();
        assert!(backup_of(&json).exists());
        register(dir.path(), None).unwrap();
        assert!(backup_of(&json).exists(), "una reinstalacion no debe perder la copia");
        unregister(dir.path()).unwrap();
        assert!(!backup_of(&json).exists(), "la copia sobrevive a la desinstalacion");
    }

    #[test]
    fn a_round_trip_leaves_the_file_as_it_was_found() {
        let src = "{\n  \"rgssVersion\": 1,\n  \"smoothScaling\": true\n}";
        let registered = add_marker(src).unwrap();
        assert!(is_registered(&registered));
        assert_eq!(remove_marker(&registered), src);
    }

    #[test]
    fn remove_does_not_touch_commented_key() {
        let src = "{\n  // \"preloadScript\": [\"accessibility/preload_access.rb\"],\n  \"rgssVersion\": 1\n}";
        assert_eq!(remove_marker(src), src);
    }

    #[test]
    fn remove_targets_real_array_after_commented_one() {
        let src = "{\n  // \"preloadScript\": [\"x.rb\"],\n  \"preloadScript\": [\"accessibility/preload_access.rb\", \"user.rb\"]\n}";
        let out = remove_marker(src);
        assert!(!is_registered(&out));
        assert!(out.contains("// \"preloadScript\": [\"x.rb\"],"));
        assert!(out.contains("user.rb"));
    }

    #[test]
    fn register_writes_backup_once() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(mkxp_json(dir.path()), "{\n  \"rgssVersion\": 1\n}").unwrap();
        register(dir.path(), None).unwrap();
        let bak = backup_of(&mkxp_json(dir.path()));
        assert!(bak.exists());
        assert_eq!(bak.file_name().unwrap(), BACKUP_NAME);
        assert_eq!(fs::read_to_string(&bak).unwrap(), "{\n  \"rgssVersion\": 1\n}");
        assert!(is_registered(&fs::read_to_string(mkxp_json(dir.path())).unwrap()));
    }

    #[test]
    fn unregister_is_surgical_and_drops_the_backup() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(mkxp_json(dir.path()), "{\n  \"rgssVersion\": 1\n}").unwrap();
        register(dir.path(), None).unwrap();
        let with_player_edit = fs::read_to_string(mkxp_json(dir.path()))
            .unwrap()
            .replace("\"rgssVersion\": 1", "\"rgssVersion\": 1,\n  \"smoothScaling\": true");
        fs::write(mkxp_json(dir.path()), &with_player_edit).unwrap();
        unregister(dir.path()).unwrap();
        let after = fs::read_to_string(mkxp_json(dir.path())).unwrap();
        assert!(!after.contains(MARKER));
        assert!(after.contains("\"smoothScaling\": true"));
        assert!(!backup_of(&mkxp_json(dir.path())).exists());
    }

    #[test]
    fn unregister_drops_a_backup_left_by_an_older_install() {
        let dir = tempfile::tempdir().unwrap();
        let json = mkxp_json(dir.path());
        fs::write(&json, "{ \"rgssVersion\": 1 }").unwrap();
        fs::write(backup_of(&json), "{ \"rgssVersion\": 1 }").unwrap();
        unregister(dir.path()).unwrap();
        assert!(!backup_of(&json).exists());
        assert_eq!(fs::read_to_string(&json).unwrap(), "{ \"rgssVersion\": 1 }");
    }

    #[cfg(windows)]
    #[test]
    fn register_reports_a_locked_json_in_the_players_language() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let json = mkxp_json(dir.path());
        fs::write(&json, "{\n  \"rgssVersion\": 1\n}").unwrap();
        let _hold = fs::OpenOptions::new().read(true).share_mode(1).open(&json).unwrap();
        let err = register(dir.path(), None).unwrap_err();
        let shown = crate::i18n::I18n::new("en").t_err(&err);
        assert!(shown.contains(JSON_NAME), "{}", shown);
        assert!(shown.to_lowercase().contains("close the game"), "{}", shown);
    }

    #[test]
    fn register_reports_a_file_without_a_root_object_in_the_players_language() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(mkxp_json(dir.path()), "sin objeto").unwrap();
        let err = register(dir.path(), None).unwrap_err();
        let i18n = crate::i18n::I18n::new("en");
        assert_eq!(i18n.t_err(&err), i18n.t("err_mkxp_no_root"));
        assert_ne!(i18n.t_err(&err), "err_mkxp_no_root");
    }

    #[test]
    fn register_names_the_file_it_could_not_create_in_the_players_language() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("carpeta").join("que").join("no").join("existe");
        let err = register(&missing, None).unwrap_err();
        let shown = crate::i18n::I18n::new("de").t_err(&err);
        assert!(shown.contains("mkxp.json"), "{}", shown);
        assert!(!shown.contains("err_io_"), "{}", shown);
        assert!(shown.starts_with("Fehler beim Erstellen"), "{}", shown);
    }

    #[test]
    fn unregister_removes_the_mkxp_json_the_launcher_created_and_keeps_the_games_own() {
        let dir = tempfile::tempdir().unwrap();
        let json = mkxp_json(dir.path());
        register(dir.path(), None).unwrap();
        assert!(json.exists() && backup_of(&json).exists());
        unregister(dir.path()).unwrap();
        assert!(!json.exists(), "el fichero que creo el instalador debe irse con el mod");
        assert!(!backup_of(&json).exists());

        let own = tempfile::tempdir().unwrap();
        let own_json = mkxp_json(own.path());
        fs::write(&own_json, "{\n  \"rgssVersion\": 1\n}").unwrap();
        register(own.path(), None).unwrap();
        unregister(own.path()).unwrap();
        assert_eq!(fs::read_to_string(&own_json).unwrap(), "{\n  \"rgssVersion\": 1\n}");
    }

    #[test]
    fn unregister_leaves_a_commented_multiline_file_bootable() {
        let dir = tempfile::tempdir().unwrap();
        let src = "// mkxp.json de ejemplo\n{\n  \"windowTitle\": \"Mi juego\",\n  \"preloadScript\": [\n    \"accessibility/preload_access.rb\",\n    // \"desactivado.rb\",\n    \"mi_script.rb\"\n  ]\n}";
        fs::write(mkxp_json(dir.path()), src).unwrap();
        unregister(dir.path()).unwrap();
        let after = fs::read_to_string(mkxp_json(dir.path())).unwrap();
        assert!(!after.contains(MARKER));
        assert!(after.contains("// \"desactivado.rb\","));
        let json = parse_without_comments(&after);
        assert_eq!(json["preloadScript"][0], "mi_script.rb");
        assert_eq!(json["windowTitle"], "Mi juego");
    }

    #[test]
    fn unregister_without_marker_or_file_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        unregister(dir.path()).unwrap();
        fs::write(mkxp_json(dir.path()), "{ \"rgssVersion\": 1 }").unwrap();
        unregister(dir.path()).unwrap();
        assert_eq!(fs::read_to_string(mkxp_json(dir.path())).unwrap(), "{ \"rgssVersion\": 1 }");
    }

    #[test]
    fn unregister_drops_the_backup_even_without_an_mkxp_json() {
        let dir = tempfile::tempdir().unwrap();
        let json = mkxp_json(dir.path());
        fs::write(backup_of(&json), "{}").unwrap();
        unregister(dir.path()).unwrap();
        assert!(!backup_of(&json).exists());
        assert!(!json.exists());
    }
}
