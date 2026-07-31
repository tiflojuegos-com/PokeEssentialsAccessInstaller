use std::fs;
use std::path::{Path, PathBuf};

pub fn find_main_exe(game_dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(game_dir).ok()?;
    let mut exes: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x.eq_ignore_ascii_case("exe")).unwrap_or(false))
        .collect();
    exes.sort_by_key(|p| fs::metadata(p).and_then(|m| m.len().try_into().map_err(|_| std::io::Error::from(std::io::ErrorKind::Other))).unwrap_or(0u64));
    exes.pop()
}

pub fn pe_arch(exe: &Path) -> String {
    let data = match fs::read(exe) {
        Ok(d) => d,
        Err(_) => return "x86".to_string(),
    };
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

pub fn supports_preload(game_dir: &Path) -> bool {
    let entries = match fs::read_dir(game_dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    for path in entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x.eq_ignore_ascii_case("exe")).unwrap_or(false))
    {
        if let Ok(data) = fs::read(&path) {
            if data.windows(13).any(|w| w == b"preloadScript") {
                return true;
            }
        }
    }
    false
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
}
