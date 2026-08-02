use std::fs;
use std::path::{Path, PathBuf};

const MARKER: &str = "accessibility/preload_access.rb";
const JSON_NAME: &str = "mkxp.json";
const BACKUP_NAME: &str = "mkxp.json.access.bak";

/// The line an installer writes above a `preloadScript` key it created itself.
/// `installer/install.ps1` writes it verbatim, so it is copied byte for byte
/// here: it is the proof that lets the uninstaller take the whole key away
/// again, and a key the player wrote has no such line above it.
const CREATED_BY: &str = "// === MOD DE ACCESIBILIDAD (anadido por el instalador) ===";

/// What recognises that line later. Matching the opening of the banner and not
/// the whole sentence is deliberate: the games registered by hand before the
/// installers existed carry a longer banner (`(lector de pantalla)`, four lines
/// of it), and those are exactly the leftovers an uninstall has to clear too.
const CREATED_BY_PREFIX: &str = "// === MOD DE ACCESIBILIDAD";

pub fn mkxp_json(game_dir: &Path) -> PathBuf {
    game_dir.join(JSON_NAME)
}

/// Where `register` parks the untouched copy of mkxp.json.
///
/// POLICY, one for the whole launcher: the copy is an internal net for the one
/// risky moment, the write that adds the loader, and it is never restored
/// automatically. It lives while the mod is registered and `unregister` deletes
/// it, because by then the live file holds everything the copy did PLUS every
/// setting the player changed since, so restoring it would quietly undo those.
/// `installer/uninstall.ps1` still leaves the file on disk and points the player
/// at it; the two installers only agree once that script deletes it too.
fn backup_of(json: &Path) -> PathBuf {
    json.with_extension("json.access.bak")
}

pub fn has_mkxp_json(game_dir: &Path) -> bool {
    mkxp_json(game_dir).exists()
}

/// Reads mkxp.json the way Game.ini is read: strict UTF-8 first, cp1252 after.
/// A file the player's editor saved in ANSI used to abort the whole install
/// with "stream did not contain valid UTF-8", which names nothing the player
/// can fix; the rewrite that follows turns the file into UTF-8 for good.
fn read_json(path: &Path) -> Result<String, String> {
    fs::read(path)
        .map(|bytes| super::detect::decode_text(&bytes))
        .map_err(|e| super::apply::io_error(JSON_NAME, "leer", &e))
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

/// True when `pos` sits on a whole `//` line, the only comment form mkxp-z
/// ignores and so the only text this module may treat as absent.
fn in_comment_line(text: &str, pos: usize) -> bool {
    let line_start = text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    text[line_start..pos].trim_start().starts_with("//")
}

/// Offset of the `"preloadScript"` key that mkxp-z will read, skipping any
/// commented-out copy. None when every occurrence is commented or absent.
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

/// Offset of the `{` that opens the JSON root, skipping any brace that only
/// exists inside a `//` comment: a file whose header comments show an example
/// took the new key into the comment, and the game then booted with a config
/// its parser chokes on and no loader registered.
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

/// The registered file, or None when this text cannot take the loader. The
/// result is checked instead of trusted: an edit that landed on a commented
/// line reads back as unregistered, and writing it would leave a mute game
/// behind an installer that reported success.
pub fn add_marker(text: &str) -> Option<String> {
    if is_registered(text) {
        return Some(text.to_string());
    }
    let out = if has_key(text) {
        add_to_existing_array(text)
    } else {
        let block = [format!("  {}", CREATED_BY), format!("  \"preloadScript\": [\"{}\"],", MARKER)];
        insert_after_root_brace(text, &block)
    };
    out.filter(|t| is_registered(t))
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
    drop_created_key(&out)
}

/// Clears what an installer itself added around the array: its banner always
/// goes, since the mod that justified it is leaving, and the key goes with it
/// once its last entry is gone. The key only goes when it sits alone on its
/// line AND that line closes with the comma the installers write, because any
/// other shape needs the surrounding commas rebalanced and a wrong guess there
/// is a game that stops booting. A `preloadScript` the player wrote carries no
/// banner and is never touched, empty or not.
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
    let last = if is_emptied_key_line(lines[key]) { key } else { key - 1 };
    lines.iter().enumerate().filter(|&(i, _)| i < top || i > last).map(|(_, l)| *l).collect()
}

/// Where the installer's banner above the key starts. The walk goes up only
/// over whole `//` lines and stops at the first line that is not one, so a
/// comment the player wrote above the banner is never swallowed with it.
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

/// True when the key line has no entries left and closes with a comma, so
/// dropping the whole line cannot leave a dangling one behind.
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
    fs::write(&path, "{}").map_err(|e| super::apply::io_error(JSON_NAME, "crear", &e))
}

pub fn register(game_dir: &Path) -> Result<(), String> {
    let path = mkxp_json(game_dir);
    ensure_json(game_dir)?;
    let text = read_json(&path)?;
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
    let written = match fs::read(&path) {
        Ok(bytes) => write_without_marker(&path, &super::detect::decode_text(&bytes)),
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

/// Applies the backup policy stated on `backup_of`: the copy goes when the mod
/// goes. Best effort on purpose, a copy that refuses to be deleted must not fail
/// an uninstall that already succeeded.
fn drop_backup(json: &Path) {
    let _ = fs::remove_file(backup_of(json));
}

/// Puts the block on its own lines right below the JSON root brace, reusing the
/// line ending the file already had and the newline that followed the brace: a
/// file that comes back with mixed endings, or with a blank line the player
/// never wrote, reads as an installer that damaged it.
fn insert_after_root_brace(text: &str, block: &[String]) -> Option<String> {
    let idx = first_live_brace(text)?;
    let after = &text[idx + 1..];
    let (eol, rest) = match after.strip_prefix("\r\n") {
        Some(r) => ("\r\n", r),
        None => ("\n", after.strip_prefix('\n').unwrap_or(after)),
    };
    let mut out = String::with_capacity(text.len() + 128);
    out.push_str(&text[..=idx]);
    for line in block {
        out.push_str(eol);
        out.push_str(line);
    }
    out.push_str(eol);
    out.push_str(rest);
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

    /// The exact block install.ps1 leaves in the 11 real games that had no
    /// preloadScript of their own.
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

    /// Pokemon Z, registered by hand before the installers existed: a four line
    /// banner, only the first of which names the mod.
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

    /// A comment of the player's directly above the banner is not part of it.
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

    /// Africanvs got its mkxp.json written by hand and the array carries no
    /// trailing comma, so only the banner can go: dropping the line too would
    /// need the commas around it rebalanced.
    #[test]
    fn remove_keeps_a_key_whose_line_does_not_end_in_a_comma() {
        let src = format!("{{\n    {}\n    \"preloadScript\": [\"{}\"]\n}}", CREATED_BY, MARKER);
        let out = remove_marker(&src);
        assert!(!out.contains("MOD DE ACCESIBILIDAD"));
        assert!(out.contains("\"preloadScript\": []"));
        assert!(parse_without_comments(&out)["preloadScript"].as_array().unwrap().is_empty());
    }

    /// Infinite Fusion ships no mkxp.json, so the installer built one over `{}`
    /// and the closing brace shares the key's line.
    #[test]
    fn remove_keeps_a_key_that_shares_its_line_with_the_closing_brace() {
        let src = format!("{{\n    {}\n    \"preloadScript\": [\"{}\"],}}", CREATED_BY, MARKER);
        let out = remove_marker(&src);
        assert!(!out.contains("MOD DE ACCESIBILIDAD"));
        assert!(!out.contains(MARKER));
        assert!(out.contains("\"preloadScript\": [],}"), "{}", out);
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
        register(dir.path()).unwrap();
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
        register(dir.path()).unwrap();
        assert!(backup_of(&json).exists());
        register(dir.path()).unwrap();
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
