use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Everything one pass over the folder's executables can answer: which one the
/// game boots from and whether any of them is an mkxp-z build. The pass reads
/// whole executables, so a caller that needs both answers asks once and carries
/// the result instead of walking 100 MB of disk again.
pub struct ExeScan {
    pub main_exe: Option<PathBuf>,
    pub supports_preload: bool,
}

impl ExeScan {
    /// True when the main executable is held open, which on Windows only happens
    /// while the game itself is running. False when there is no exe.
    pub fn game_running(&self) -> bool {
        self.main_exe.as_deref().map(exe_locked).unwrap_or(false)
    }
}

/// Picks the executable the game boots from: the biggest mkxp-z build, else
/// Game.exe, else the biggest one. Answering "is there any mkxp-z build here"
/// falls out of the same walk, so both questions cost one scan.
pub fn scan_exes(game_dir: &Path) -> ExeScan {
    let mut exes = exe_paths(game_dir);
    exes.sort_by_key(|p| fs::metadata(p).map(|m| m.len()).unwrap_or(0));
    if let Some(p) = exes.iter().rev().find(|p| exe_contains_preload(p)) {
        return ExeScan { main_exe: Some(p.clone()), supports_preload: true };
    }
    let game_exe = exes
        .iter()
        .find(|p| p.file_name().map(|n| n.eq_ignore_ascii_case("Game.exe")).unwrap_or(false))
        .cloned();
    ExeScan { main_exe: game_exe.or_else(|| exes.pop()), supports_preload: false }
}

/// The folder the game really lives in: `dir` itself when it holds an executable or an mkxp.json, else
/// the single child folder that does. A zip that extracts to `<Name>\JUEGO\` leaves the player pointing
/// the folder dialog one level too high, and "no executable here" is the wrong answer to give them.
pub fn resolve_game_dir(dir: &Path) -> PathBuf {
    if !exe_paths(dir).is_empty() || super::mkxp::has_mkxp_json(dir) {
        return dir.to_path_buf();
    }
    let mut candidates: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir() && (!exe_paths(p).is_empty() || super::mkxp::has_mkxp_json(p)))
            .collect(),
        Err(_) => Vec::new(),
    };
    if candidates.len() == 1 {
        return candidates.remove(0);
    }
    dir.to_path_buf()
}

fn exe_paths(game_dir: &Path) -> Vec<PathBuf> {
    let entries = match fs::read_dir(game_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x.eq_ignore_ascii_case("exe")).unwrap_or(false))
        .collect()
}

/// The single definition of "another process is holding this file": a Windows
/// sharing or lock violation. Access denied is a permissions problem, not a
/// lock, and must not be reported as one.
pub fn file_locked(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(32) | Some(33))
}

/// True when the executable cannot be opened for writing because it is locked.
/// Any other failure, including a read-only folder, counts as not running.
fn exe_locked(exe: &Path) -> bool {
    match fs::OpenOptions::new().write(true).open(exe) {
        Ok(_) => false,
        Err(e) => file_locked(&e),
    }
}

/// Reads only the head of the executable: the PE header sits within the first kilobytes, and reading a
/// 200 MB game binary whole to look at two bytes of it was the launcher's biggest allocation.
pub fn pe_arch(exe: &Path) -> String {
    use std::io::Read;
    let mut data = Vec::new();
    let read = fs::File::open(exe).and_then(|f| f.take(1 << 20).read_to_end(&mut data));
    if read.is_err() {
        return "x86".to_string();
    }
    machine_from_pe(&data).unwrap_or_else(|| "x86".to_string())
}

fn machine_from_pe(data: &[u8]) -> Option<String> {
    if data.len() < 0x40 {
        return None;
    }
    let pe_off = u32::from_le_bytes([data[0x3C], data[0x3D], data[0x3E], data[0x3F]]) as usize;
    let machine_off = pe_off + 4;
    if data.len() < machine_off + 2 {
        return None;
    }
    let machine = u16::from_le_bytes([data[machine_off], data[machine_off + 1]]);
    Some(if machine == 0x8664 { "x64".to_string() } else { "x86".to_string() })
}

/// Scans the executable for the `preloadScript` literal, which is what tells a
/// real mkxp-z build apart from a plain RPG Maker one. Unreadable files say no.
fn exe_contains_preload(path: &Path) -> bool {
    use std::io::Read;
    const NEEDLE: &[u8] = b"preloadScript";
    let mut f = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = vec![0u8; 1 << 20];
    let mut carry: Vec<u8> = Vec::new();
    loop {
        let n = match f.read(&mut buf) {
            Ok(0) => return false,
            Ok(n) => n,
            Err(_) => return false,
        };
        let mut hay = carry;
        hay.extend_from_slice(&buf[..n]);
        if hay.windows(NEEDLE.len()).any(|w| w == NEEDLE) {
            return true;
        }
        let keep = NEEDLE.len() - 1;
        carry = hay[hay.len().saturating_sub(keep)..].to_vec();
    }
}

/// Every name the folder declares, in the order the profile detector should
/// trust them: mkxp.json's `windowTitle`/`title` first, then Game.ini's
/// `Title`. Both are returned rather than only the first because mkxp-z ships
/// its template with `"windowTitle": "Custom Title"`, and a build that kept the
/// placeholder used to answer that string and hide the real name sitting in
/// Game.ini, so a renamed folder silently fell through to the generic profile.
/// Commented-out keys never count.
pub fn game_titles(game_dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(t) = mkxp_title(game_dir) {
        out.push(t);
    }
    if let Some(t) = ini_title(game_dir) {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

fn mkxp_title(game_dir: &Path) -> Option<String> {
    let bytes = fs::read(super::mkxp::mkxp_json(game_dir)).ok()?;
    let text = super::mkxp::strip_comment_lines(&decode_text(&bytes));
    let re = regex::Regex::new(r#"(?i)"(?:window)?title"\s*:\s*"([^"]+)""#)
        .expect("the window title pattern is a literal and always compiles");
    let t = re.captures(&text)?[1].trim().to_string();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

fn ini_title(game_dir: &Path) -> Option<String> {
    let bytes = fs::read(game_dir.join("Game.ini")).ok()?;
    for line in decode_text(&bytes).lines() {
        let l = line.trim_start_matches('\u{feff}').trim();
        let (key, rest) = match l.split_once('=') {
            Some(kv) => kv,
            None => continue,
        };
        if key.trim().eq_ignore_ascii_case("title") {
            let t = rest.trim().to_string();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

/// Reads a game's config text as UTF-8 and falls back to cp1252, the ANSI code
/// page RPG Maker wrote on a Western Windows, so accents and typographic
/// punctuation both survive. The PowerShell installer decodes Game.ini the same
/// way in `installer/install.ps1` (`Get-GameTitle`): change one and change the
/// other, or the two installers will read the same title differently. mkxp.json
/// goes through here too, which the PowerShell side does NOT do yet: strict
/// UTF-8 there aborted the whole install on an ANSI file with "stream did not
/// contain valid UTF-8", a message nobody can act on.
pub(super) fn decode_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| cp1252_char(b)).collect(),
    }
}

/// The 32 code points where cp1252 departs from latin-1; everything outside
/// 0x80-0x9F is the same in both. The five slots cp1252 leaves undefined (0x81,
/// 0x8D, 0x8F, 0x90, 0x9D) keep the byte's own value as a control character,
/// which is what Windows and .NET's code page 1252 also do, so a title that
/// survives one installer survives the other.
const CP1252_HIGH: [char; 32] = [
    '\u{20ac}', '\u{81}', '\u{201a}', '\u{192}', '\u{201e}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{2c6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8d}', '\u{17d}', '\u{8f}',
    '\u{90}', '\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{2dc}', '\u{2122}', '\u{161}', '\u{203a}', '\u{153}', '\u{9d}', '\u{17e}', '\u{178}',
];

fn cp1252_char(b: u8) -> char {
    match b {
        0x80..=0x9f => CP1252_HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

pub fn folder_and_exe_string(game_dir: &Path, exe: Option<&Path>) -> String {
    let folder = game_dir.to_string_lossy().to_string();
    let name = exe
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    format!("{} {}", folder, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_and_exe_join() {
        let s = folder_and_exe_string(Path::new("D:/Games/ReminiscenciaV2"), Some(Path::new("D:/Games/ReminiscenciaV2/Game.exe")));
        assert!(s.to_lowercase().contains("reminiscencia"));
        assert!(s.to_lowercase().contains("game.exe"));
    }

    #[test]
    fn scan_prefers_the_preload_capable_exe_over_the_biggest() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("engine.exe"), b"xx preloadScript yy").unwrap();
        fs::write(dir.path().join("huge.exe"), vec![0u8; 4096]).unwrap();
        let scan = scan_exes(dir.path());
        assert_eq!(scan.main_exe.unwrap().file_name().unwrap(), "engine.exe");
        assert!(scan.supports_preload);
    }

    #[test]
    fn scan_falls_back_to_game_exe() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Game.exe"), b"tiny").unwrap();
        fs::write(dir.path().join("huge.exe"), vec![0u8; 4096]).unwrap();
        let exe = scan_exes(dir.path()).main_exe.unwrap();
        assert_eq!(exe.file_name().unwrap(), "Game.exe");
    }

    #[test]
    fn scan_falls_back_to_the_biggest_exe() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("small.exe"), b"tiny").unwrap();
        fs::write(dir.path().join("huge.exe"), vec![0u8; 4096]).unwrap();
        let exe = scan_exes(dir.path()).main_exe.unwrap();
        assert_eq!(exe.file_name().unwrap(), "huge.exe");
    }

    #[test]
    fn one_scan_answers_both_the_main_exe_and_preload_support() {
        let dir = tempfile::tempdir().unwrap();
        let empty = scan_exes(dir.path());
        assert!(empty.main_exe.is_none());
        assert!(!empty.supports_preload);
        assert!(!empty.game_running());

        fs::write(dir.path().join("Game.exe"), vec![0u8; 4096]).unwrap();
        let plain = scan_exes(dir.path());
        assert_eq!(plain.main_exe.unwrap().file_name().unwrap(), "Game.exe");
        assert!(!plain.supports_preload);

        fs::write(dir.path().join("mkxp-z.exe"), b"xx preloadScript yy").unwrap();
        let mkxp = scan_exes(dir.path());
        assert_eq!(mkxp.main_exe.unwrap().file_name().unwrap(), "mkxp-z.exe");
        assert!(mkxp.supports_preload);
    }

    fn only_title(dir: &Path) -> String {
        let titles = game_titles(dir);
        assert_eq!(titles.len(), 1, "titulos: {:?}", titles);
        titles[0].clone()
    }

    #[test]
    fn game_title_reads_ansi_accents() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = b"[Game]\r\nTitle=Pok".to_vec();
        bytes.push(0xE9);
        bytes.extend_from_slice(b"mon A");
        bytes.push(0xF1);
        bytes.extend_from_slice(b"il\r\n");
        fs::write(dir.path().join("Game.ini"), bytes).unwrap();
        assert_eq!(only_title(dir.path()), "Pok\u{e9}mon A\u{f1}il");
    }

    #[test]
    fn game_title_reads_cp1252_punctuation_not_control_chars() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = b"[Game]\r\nTitle=Pok".to_vec();
        bytes.push(0xE9);
        bytes.extend_from_slice(b"mon ");
        bytes.push(0x92);
        bytes.extend_from_slice(b"98 ");
        bytes.push(0x97);
        bytes.push(0x20);
        bytes.push(0x93);
        bytes.extend_from_slice(b"Edici");
        bytes.push(0xF3);
        bytes.push(0x6E);
        bytes.push(0x94);
        bytes.extend_from_slice(b"\r\n");
        fs::write(dir.path().join("Game.ini"), bytes).unwrap();
        let title = only_title(dir.path());
        assert_eq!(title, "Pokémon ’98 — “Edición”");
        assert!(!title.chars().any(|c| c.is_control()));
    }

    #[test]
    fn cp1252_maps_the_whole_high_range_and_leaves_the_rest_alone() {
        assert_eq!(cp1252_char(0x80), '€');
        assert_eq!(cp1252_char(0x85), '…');
        assert_eq!(cp1252_char(0x96), '–');
        assert_eq!(cp1252_char(0x9f), 'Ÿ');
        for undefined in [0x81u8, 0x8d, 0x8f, 0x90, 0x9d] {
            assert_eq!(cp1252_char(undefined), undefined as char);
        }
        for same_as_latin1 in [0x41u8, 0x7f, 0xa0, 0xe9, 0xf1, 0xff] {
            assert_eq!(cp1252_char(same_as_latin1), same_as_latin1 as char);
        }
    }

    #[test]
    fn game_title_reads_utf8_accents() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Game.ini"), "[Game]\nTitle=Pokémon Añil\n").unwrap();
        assert_eq!(only_title(dir.path()), "Pokémon Añil");
    }

    #[test]
    fn game_title_reads_mkxp_window_title() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("mkxp.json"), "{\n  \"windowTitle\": \"Pokemon Reminiscencia\"\n}").unwrap();
        assert_eq!(only_title(dir.path()), "Pokemon Reminiscencia");
    }

    #[test]
    fn game_title_mkxp_key_is_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("mkxp.json"), "{\n  \"Title\": \"Pokemon Opalo\"\n}").unwrap();
        assert_eq!(only_title(dir.path()), "Pokemon Opalo");
    }

    #[test]
    fn game_title_ignores_a_commented_mkxp_key() {
        let dir = tempfile::tempdir().unwrap();
        let json = "{\n  // \"windowTitle\": \"Plantilla sin tocar\",\n  \"pathCache\": false\n}";
        fs::write(dir.path().join("mkxp.json"), json).unwrap();
        assert!(game_titles(dir.path()).is_empty());
        fs::write(dir.path().join("Game.ini"), "[Game]\nTitle=Pokemon Z\n").unwrap();
        assert_eq!(only_title(dir.path()), "Pokemon Z");
    }

    #[test]
    fn game_title_key_is_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Game.ini"), "[Game]\ntitle = Opalo\n").unwrap();
        assert_eq!(only_title(dir.path()), "Opalo");
    }

    #[test]
    fn game_title_ignores_other_keys() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Game.ini"), "[Game]\nSubtitle=No\nTitleFoo=No\n").unwrap();
        assert!(game_titles(dir.path()).is_empty());
    }

    #[test]
    fn the_mkxp_placeholder_does_not_hide_the_name_in_game_ini() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("mkxp.json"), "{\n  \"windowTitle\": \"Custom Title\"\n}").unwrap();
        fs::write(dir.path().join("Game.ini"), "[Game]\nTitle=Pokemon Royal\n").unwrap();
        assert_eq!(game_titles(dir.path()), vec!["Custom Title".to_string(), "Pokemon Royal".to_string()]);
    }

    #[test]
    fn the_same_title_in_both_files_is_offered_once() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("mkxp.json"), "{\n  \"windowTitle\": \"Pokemon Opalo\"\n}").unwrap();
        fs::write(dir.path().join("Game.ini"), "[Game]\nTitle=Pokemon Opalo\n").unwrap();
        assert_eq!(game_titles(dir.path()), vec!["Pokemon Opalo".to_string()]);
    }

    #[test]
    fn an_ansi_mkxp_json_still_yields_its_title() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = b"{\n  \"windowTitle\": \"Pok".to_vec();
        bytes.push(0xE9);
        bytes.extend_from_slice(b"mon ");
        bytes.push(0xD3);
        bytes.extend_from_slice(b"palo\"\n}");
        fs::write(dir.path().join("mkxp.json"), bytes).unwrap();
        assert_eq!(only_title(dir.path()), "Pok\u{e9}mon \u{d3}palo");
    }

    #[test]
    fn a_wrapper_folder_resolves_to_the_one_child_that_holds_the_game() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("JUEGO");
        fs::create_dir_all(&inner).unwrap();
        fs::create_dir_all(dir.path().join("Manual")).unwrap();
        fs::write(inner.join("Game.exe"), b"bytes").unwrap();
        assert_eq!(resolve_game_dir(dir.path()), inner);
        assert_eq!(resolve_game_dir(&inner), inner, "a real game folder is itself");
    }

    #[test]
    fn a_folder_with_several_game_children_or_none_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(resolve_game_dir(dir.path()), dir.path());
        for name in ["A", "B"] {
            let d = dir.path().join(name);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("Game.exe"), b"bytes").unwrap();
        }
        assert_eq!(resolve_game_dir(dir.path()), dir.path());
    }

    #[test]
    fn game_running_false_when_closed_or_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!scan_exes(dir.path()).game_running());
        fs::write(dir.path().join("Game.exe"), b"bytes").unwrap();
        assert!(!scan_exes(dir.path()).game_running());
    }

    #[cfg(windows)]
    #[test]
    fn game_running_true_when_exe_locked() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Game.exe");
        fs::write(&exe, b"bytes").unwrap();
        let _hold = fs::OpenOptions::new().read(true).share_mode(1).open(&exe).unwrap();
        assert!(scan_exes(dir.path()).game_running());
    }
}
