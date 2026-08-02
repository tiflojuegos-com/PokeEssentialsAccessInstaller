use std::fs;
use std::path::{Path, PathBuf};

const MARKER: &str = "accessibility/preload_access.rb";
const JSON_NAME: &str = "mkxp.json";
const BACKUP_NAME: &str = "mkxp.json.access.bak";

pub fn mkxp_json(game_dir: &Path) -> PathBuf {
    game_dir.join(JSON_NAME)
}

/// Where `register` parks the untouched copy of mkxp.json.
fn backup_of(json: &Path) -> PathBuf {
    json.with_extension("json.access.bak")
}

pub fn has_mkxp_json(game_dir: &Path) -> bool {
    mkxp_json(game_dir).exists()
}

/// Drops whole `//` comment lines so the rest of the module (and the title
/// detector) only ever sees the JSON that mkxp-z will actually honour.
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

/// Offset of the `"preloadScript"` key that mkxp-z will read, skipping any
/// commented-out copy. None when every occurrence is commented or absent.
fn find_active_key(text: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = text[from..].find("\"preloadScript\"") {
        let pos = from + rel;
        let line_start = text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
        if !text[line_start..pos].trim_start().starts_with("//") {
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

pub fn add_marker(text: &str) -> Option<String> {
    if is_registered(text) {
        return Some(text.to_string());
    }
    if has_key(text) {
        add_to_existing_array(text)
    } else {
        let line = format!("  \"preloadScript\": [\"{}\"],\n", MARKER);
        insert_after_first_brace(text, &line)
    }
}

/// Puts the marker first in the live array. The tail of the array is kept as it
/// was, because trimming it would move the closing bracket onto a `//` line and
/// break the file for a player who has a commented entry at the end.
fn add_to_existing_array(text: &str) -> Option<String> {
    let key_pos = find_active_key(text)?;
    let open = text[key_pos..].find('[')? + key_pos;
    let close = text[open..].find(']')? + open;
    let inner = text[open + 1..close].trim_start();
    let entry = format!("\"{}\"", MARKER);
    let new_inner = if inner.is_empty() {
        entry
    } else {
        format!("{}, {}", entry, inner)
    };
    let mut out = String::with_capacity(text.len() + new_inner.len());
    out.push_str(&text[..open + 1]);
    out.push_str(&new_inner);
    out.push_str(&text[close..]);
    Some(out)
}

/// True when a comma separated slice of the array holds the marker as its only
/// live value, i.e. once its `//` comment lines are gone.
fn is_marker_entry(entry: &str) -> bool {
    strip_comment_lines(entry).trim().trim_matches('"') == MARKER
}

/// Removes the marker from the live `preloadScript` array, keeping every other
/// slice byte for byte so line breaks and `//` comments survive untouched: a
/// collapsed array would let a comment swallow the closing bracket.
pub fn remove_marker(text: &str) -> String {
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
    let kept: Vec<&str> = inner
        .split(',')
        .filter(|s| !s.is_empty() && !is_marker_entry(s))
        .collect();
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..open + 1]);
    out.push_str(&kept.join(","));
    out.push_str(&text[close..]);
    out
}

pub fn ensure_json(game_dir: &Path) -> Result<(), String> {
    let path = mkxp_json(game_dir);
    if path.exists() {
        return Ok(());
    }
    fs::write(&path, "{}").map_err(|e| super::apply::io_error(JSON_NAME, "crear", &e))
}

pub fn register(game_dir: &Path) -> Result<(), String> {
    let path = mkxp_json(game_dir);
    ensure_json(game_dir)?;
    let text = fs::read_to_string(&path).map_err(|e| super::apply::io_error(JSON_NAME, "leer", &e))?;
    if is_registered(&text) {
        return Ok(());
    }
    let bak = backup_of(&path);
    if !bak.exists() {
        fs::copy(&path, &bak).map_err(|e| super::apply::io_error(BACKUP_NAME, "crear", &e))?;
    }
    let updated = add_marker(&text).ok_or_else(|| "err_mkxp_no_root".to_string())?;
    fs::write(&path, updated).map_err(|e| super::apply::io_error(JSON_NAME, "escribir", &e))
}

/// Takes the marker out of mkxp.json and clears the copy `register` left, so
/// uninstalling leaves the folder as the launcher found it. A game with no
/// mkxp.json at all still gets the copy cleared.
pub fn unregister(game_dir: &Path) -> Result<(), String> {
    let path = mkxp_json(game_dir);
    let written = match fs::read_to_string(&path) {
        Ok(text) => write_without_marker(&path, &text),
        Err(_) => Ok(()),
    };
    drop_backup(&path);
    written
}

/// Rewrites mkxp.json only when the marker really was in it, so a file the
/// launcher never touched keeps its bytes and its timestamp.
fn write_without_marker(path: &Path, text: &str) -> Result<(), String> {
    let cleaned = remove_marker(text);
    if cleaned == text {
        return Ok(());
    }
    fs::write(path, cleaned).map_err(|e| super::apply::io_error(JSON_NAME, "escribir", &e))
}

/// Deletes the copy `register` made. Once the marker is gone the live file
/// already holds everything the copy did plus whatever the player changed since,
/// so keeping it would only leave debris in the game folder and invite a restore
/// that silently undoes those edits. Best effort on purpose: a copy that refuses
/// to go must not fail an uninstall that already succeeded.
fn drop_backup(json: &Path) {
    let _ = fs::remove_file(backup_of(json));
}

fn insert_after_first_brace(text: &str, line: &str) -> Option<String> {
    let idx = text.find('{')?;
    let mut out = String::with_capacity(text.len() + line.len());
    out.push_str(&text[..=idx]);
    out.push('\n');
    out.push_str(line);
    out.push_str(&text[idx + 1..]);
    Some(out)
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
        register(dir.path()).unwrap();
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
        register(dir.path()).unwrap();
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
        let err = register(dir.path()).unwrap_err();
        let shown = crate::i18n::I18n::new("en").t_err(&err);
        assert!(shown.contains(JSON_NAME), "{}", shown);
        assert!(shown.to_lowercase().contains("close the game"), "{}", shown);
    }

    #[test]
    fn register_reports_a_file_without_a_root_object_in_the_players_language() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(mkxp_json(dir.path()), "sin objeto").unwrap();
        let err = register(dir.path()).unwrap_err();
        let i18n = crate::i18n::I18n::new("en");
        assert_eq!(i18n.t_err(&err), i18n.t("err_mkxp_no_root"));
        assert_ne!(i18n.t_err(&err), "err_mkxp_no_root");
    }

    #[test]
    fn register_keeps_the_literal_message_for_other_failures() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("carpeta").join("que").join("no").join("existe");
        let err = register(&missing).unwrap_err();
        assert!(err.starts_with("crear mkxp.json: "), "{}", err);
        assert_eq!(crate::i18n::I18n::new("es").t_err(&err), err);
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
