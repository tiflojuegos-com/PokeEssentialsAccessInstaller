use std::fs;
use std::path::Path;

const MARKER: &str = "accessibility/preload_access.rb";

pub fn mkxp_json(game_dir: &Path) -> std::path::PathBuf {
    game_dir.join("mkxp.json")
}

pub fn has_mkxp_json(game_dir: &Path) -> bool {
    mkxp_json(game_dir).exists()
}

fn strip_comment_lines(text: &str) -> String {
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

fn add_to_existing_array(text: &str) -> Option<String> {
    let key_pos = text.find("\"preloadScript\"")?;
    let open = text[key_pos..].find('[')? + key_pos;
    let close = text[open..].find(']')? + open;
    let inner = text[open + 1..close].trim();
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

pub fn remove_marker(text: &str) -> String {
    let key_pos = match text.find("\"preloadScript\"") {
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
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && s.trim_matches('"') != MARKER)
        .collect();
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..open + 1]);
    out.push_str(&kept.join(", "));
    out.push_str(&text[close..]);
    out
}

pub fn ensure_json(game_dir: &Path) -> Result<(), String> {
    let path = mkxp_json(game_dir);
    if path.exists() {
        return Ok(());
    }
    fs::write(&path, "{}").map_err(|e| format!("crear mkxp.json: {}", e))
}

pub fn register(game_dir: &Path) -> Result<(), String> {
    let path = mkxp_json(game_dir);
    ensure_json(game_dir)?;
    let text = fs::read_to_string(&path).map_err(|e| format!("leer mkxp.json: {}", e))?;
    if is_registered(&text) {
        return Ok(());
    }
    let bak = path.with_extension("json.access.bak");
    if !bak.exists() {
        fs::copy(&path, &bak).map_err(|e| format!("copia de seguridad mkxp.json: {}", e))?;
    }
    let updated = add_marker(&text).ok_or_else(|| "mkxp.json sin objeto raiz valido".to_string())?;
    fs::write(&path, updated).map_err(|e| format!("escribir mkxp.json: {}", e))
}

pub fn unregister(game_dir: &Path) -> Result<(), String> {
    let path = mkxp_json(game_dir);
    let bak = path.with_extension("json.access.bak");
    if bak.exists() {
        fs::copy(&bak, &path).map_err(|e| format!("restaurar mkxp.json: {}", e))?;
        let _ = fs::remove_file(&bak);
        return Ok(());
    }
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Ok(()),
    };
    let cleaned = remove_marker(&text);
    fs::write(&path, cleaned).map_err(|e| format!("escribir mkxp.json: {}", e))
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
}
