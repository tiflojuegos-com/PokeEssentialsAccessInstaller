use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::github::ContentEntry;
use super::installed::Installed;
use super::paths::{accessibility_dir, installed_file};

pub fn dest_for(repo_path: &str, profile: &str, arch: &str) -> Option<String> {
    let p = repo_path.replace('\\', "/");
    if let Some(rest) = p.strip_prefix("core/") {
        return Some(format!("core/{}", rest));
    }
    if let Some(rest) = p.strip_prefix("lang/") {
        return Some(format!("lang/{}", rest));
    }
    let game_prefix = format!("games/{}/", profile);
    if let Some(rest) = p.strip_prefix(&game_prefix) {
        return Some(format!("game/{}", rest));
    }
    if p == "loader/boot.rb" {
        return Some("boot.rb".to_string());
    }
    if p == "loader/preload_access.rb" {
        return Some("preload_access.rb".to_string());
    }
    if let Some(rest) = p.strip_prefix("assets/sounds/") {
        return Some(format!("sounds/{}", rest));
    }
    let arch_prefix = format!("assets/{}/", arch);
    if let Some(rest) = p.strip_prefix(&arch_prefix) {
        return Some(format!("lib/{}", rest));
    }
    None
}

pub fn repo_dirs_for(profile: &str, arch: &str) -> Vec<String> {
    vec![
        "core".to_string(),
        "lang".to_string(),
        format!("games/{}", profile),
        "loader".to_string(),
        "assets/sounds".to_string(),
        format!("assets/{}", arch),
    ]
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileOp {
    pub repo_path: String,
    pub dest_rel: String,
    pub download_url: String,
    pub remote_sha: String,
}

pub fn plan_update(
    remote: &[ContentEntry],
    installed_files: &BTreeMap<String, String>,
    profile: &str,
    arch: &str,
) -> Vec<FileOp> {
    let mut ops = Vec::new();
    for e in remote {
        if e.kind != "file" {
            continue;
        }
        let dest = match dest_for(&e.path, profile, arch) {
            Some(d) => d,
            None => continue,
        };
        let local = installed_files.get(&dest);
        if super::install::needs_update(local, &e.sha) {
            if let Some(url) = &e.download_url {
                ops.push(FileOp {
                    repo_path: e.path.clone(),
                    dest_rel: dest,
                    download_url: url.clone(),
                    remote_sha: e.sha.clone(),
                });
            }
        }
    }
    ops
}

pub fn stale_files(
    remote: &[ContentEntry],
    installed_files: &BTreeMap<String, String>,
    profile: &str,
    arch: &str,
) -> Vec<String> {
    let mut wanted = std::collections::BTreeSet::new();
    for e in remote {
        if e.kind == "file" {
            if let Some(d) = dest_for(&e.path, profile, arch) {
                wanted.insert(d);
            }
        }
    }
    installed_files
        .keys()
        .filter(|k| !wanted.contains(*k) && !super::install::is_user_data(k))
        .cloned()
        .collect()
}

pub fn detect_arch(game_dir: &Path) -> String {
    let exe = super::detect::find_main_exe(game_dir);
    match exe {
        Some(p) => super::detect::pe_arch(&p),
        None => "x86".to_string(),
    }
}

pub fn can_write(game_dir: &Path) -> bool {
    let probe = game_dir.join(".pokeessentialsaccess_write_test");
    match fs::write(&probe, b"") {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

pub fn run_install(
    game_dir: &Path,
    profile: &str,
    profile_mode: &str,
    now: &str,
    mut progress: impl FnMut(&str, u32, u32),
) -> Result<String, String> {
    if !super::mkxp::has_mkxp_json(game_dir) {
        return Err("El juego no tiene mkxp.json: no es compatible.".to_string());
    }
    let arch = detect_arch(game_dir);
    let version_text = super::github::download_bytes(&super::paths::raw_url("version.json"))?;
    let mod_version = super::installed::parse_version_json(&String::from_utf8_lossy(&version_text))
        .map(|m| m.version)
        .unwrap_or_else(|| "0.0.0".to_string());

    let remote = super::github::walk_tree(&repo_dirs_for(profile, &arch))
        .map_err(|e| format!("listar archivos del mod: {}", e))?;

    let prev = super::installed::read(game_dir).map(|i| i.files).unwrap_or_default();
    let ops = plan_update(&remote, &prev, profile, &arch);
    let total = ops.len() as u32;

    let mut files: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in prev.iter() {
        files.insert(k.clone(), v.clone());
    }

    let done = std::sync::atomic::AtomicU32::new(0);
    let outcome = download_ops_parallel(game_dir, &ops, &done, total, &mut progress);
    for (dest_rel, sha) in &outcome.written {
        files.insert(dest_rel.clone(), sha.clone());
    }
    if let Some(e) = outcome.error {
        let _ = seal_installed(game_dir, &mod_version, profile, profile_mode, &arch, now, files);
        return Err(e);
    }

    super::mkxp::register(game_dir)?;

    let stale = stale_files(&remote, &prev, profile, &arch);
    remove_stale(game_dir, &stale);
    for s in &stale {
        files.remove(s);
    }

    seal_installed(game_dir, &mod_version, profile, profile_mode, &arch, now, files)?;
    Ok(mod_version)
}

const MAX_CONCURRENT: usize = 8;

struct DownloadOutcome {
    written: Vec<(String, String)>,
    error: Option<String>,
}

fn download_ops_parallel(
    game_dir: &Path,
    ops: &[FileOp],
    done: &std::sync::atomic::AtomicU32,
    total: u32,
    progress: &mut impl FnMut(&str, u32, u32),
) -> DownloadOutcome {
    use std::sync::atomic::Ordering;

    let mut written: Vec<(String, String)> = Vec::new();
    let mut error: Option<String> = None;

    for batch in ops.chunks(MAX_CONCURRENT) {
        let mut handles = Vec::with_capacity(batch.len());
        for op in batch {
            let url = op.download_url.clone();
            let dest_rel = op.dest_rel.clone();
            let game_dir = game_dir.to_path_buf();
            handles.push(std::thread::spawn(move || -> Result<(String, String), String> {
                let data = super::github::download_bytes(&url)?;
                write_dest_file(&game_dir, &dest_rel, &data)?;
                Ok((dest_rel, super::install::git_blob_sha1(&data)))
            }));
        }
        for h in handles {
            match h.join() {
                Ok(Ok((dest_rel, sha))) => {
                    let n = done.fetch_add(1, Ordering::SeqCst) + 1;
                    progress(&dest_rel, n.saturating_sub(1), total);
                    written.push((dest_rel, sha));
                }
                Ok(Err(e)) => {
                    if error.is_none() {
                        error = Some(e);
                    }
                }
                Err(_) => {
                    if error.is_none() {
                        error = Some("una descarga se interrumpio de forma inesperada".to_string());
                    }
                }
            }
        }
        if error.is_some() {
            break;
        }
    }

    DownloadOutcome { written, error }
}

pub fn run_uninstall(game_dir: &Path) -> Result<(), String> {
    super::mkxp::unregister(game_dir)?;
    let dir = accessibility_dir(game_dir);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("borrar accessibility/: {}", e))?;
    }
    Ok(())
}

pub fn write_dest_file(game_dir: &Path, dest_rel: &str, data: &[u8]) -> Result<(), String> {
    let full = accessibility_dir(game_dir).join(dest_rel.replace('/', &std::path::MAIN_SEPARATOR.to_string()));
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("crear carpeta {}: {}", dest_rel, e))?;
    }
    fs::write(&full, data).map_err(|e| format!("escribir {}: {}", dest_rel, e))
}

pub fn remove_stale(game_dir: &Path, stale: &[String]) {
    for rel in stale {
        let full = accessibility_dir(game_dir).join(rel.replace('/', &std::path::MAIN_SEPARATOR.to_string()));
        let _ = fs::remove_file(full);
    }
}

pub fn seal_installed(
    game_dir: &Path,
    mod_version: &str,
    profile: &str,
    profile_mode: &str,
    voice_arch: &str,
    installed_at: &str,
    files: BTreeMap<String, String>,
) -> Result<(), String> {
    let inst = Installed {
        mod_version: mod_version.to_string(),
        profile: profile.to_string(),
        profile_mode: profile_mode.to_string(),
        voice_arch: voice_arch.to_string(),
        installed_at: installed_at.to_string(),
        files,
    };
    let path = installed_file(game_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("crear data/: {}", e))?;
    }
    let json = serde_json::to_string_pretty(&inst).map_err(|e| format!("serializar installed: {}", e))?;
    fs::write(path, json).map_err(|e| format!("escribir installed.json: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_write_true_on_writable_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(can_write(dir.path()));
        assert!(!dir.path().join(".pokeessentialsaccess_write_test").exists());
    }

    #[test]
    fn can_write_false_on_missing_dir() {
        assert!(!can_write(std::path::Path::new("Z:/definitely/not/here/xyz")));
    }

    #[test]
    fn dest_mapping() {
        assert_eq!(dest_for("core/nav/locator.rb", "pokemon_z", "x64").unwrap(), "core/nav/locator.rb");
        assert_eq!(dest_for("lang/es.txt", "pokemon_z", "x64").unwrap(), "lang/es.txt");
        assert_eq!(dest_for("games/pokemon_z/pause_menu.rb", "pokemon_z", "x64").unwrap(), "game/pause_menu.rb");
        assert_eq!(dest_for("loader/boot.rb", "pokemon_z", "x64").unwrap(), "boot.rb");
        assert_eq!(dest_for("loader/preload_access.rb", "pokemon_z", "x64").unwrap(), "preload_access.rb");
        assert_eq!(dest_for("assets/sounds/48000/step.ogg", "pokemon_z", "x64").unwrap(), "sounds/48000/step.ogg");
        assert_eq!(dest_for("assets/x64/PA3D_steam.dll", "pokemon_z", "x64").unwrap(), "lib/PA3D_steam.dll");
    }

    #[test]
    fn other_profile_game_files_are_ignored() {
        assert!(dest_for("games/reminiscencia/menus.rb", "pokemon_z", "x64").is_none());
        assert!(dest_for("assets/x86/PA3D_steam.dll", "pokemon_z", "x64").is_none());
        assert!(dest_for("test/run_all.rb", "pokemon_z", "x64").is_none());
        assert!(dest_for("README.md", "pokemon_z", "x64").is_none());
        assert!(dest_for("games/catalog.json", "pokemon_z", "x64").is_none());
    }

    fn entry(path: &str, sha: &str) -> ContentEntry {
        ContentEntry {
            name: path.rsplit('/').next().unwrap().to_string(),
            path: path.to_string(),
            kind: "file".to_string(),
            sha: sha.to_string(),
            download_url: Some(format!("https://x/{}", path)),
        }
    }

    #[test]
    fn plan_only_downloads_changed_and_mapped() {
        let remote = vec![
            entry("core/nav/locator.rb", "newsha"),
            entry("lang/es.txt", "samesha"),
            entry("games/pokemon_z/pause_menu.rb", "brandnew"),
            entry("README.md", "whatever"),
        ];
        let mut installed = BTreeMap::new();
        installed.insert("core/nav/locator.rb".to_string(), "oldsha".to_string());
        installed.insert("lang/es.txt".to_string(), "samesha".to_string());

        let ops = plan_update(&remote, &installed, "pokemon_z", "x64");
        let dests: Vec<&str> = ops.iter().map(|o| o.dest_rel.as_str()).collect();
        assert!(dests.contains(&"core/nav/locator.rb"));
        assert!(dests.contains(&"game/pause_menu.rb"));
        assert!(!dests.contains(&"lang/es.txt"));
        assert_eq!(ops.len(), 2);
    }

    #[test]
    fn stale_detects_removed_mod_files_only() {
        let remote = vec![entry("core/nav/locator.rb", "s")];
        let mut installed = BTreeMap::new();
        installed.insert("core/nav/locator.rb".to_string(), "s".to_string());
        installed.insert("core/old/removed.rb".to_string(), "s".to_string());
        installed.insert("data/settings.ini".to_string(), "s".to_string());

        let stale = stale_files(&remote, &installed, "pokemon_z", "x64");
        assert!(stale.contains(&"core/old/removed.rb".to_string()));
        assert!(!stale.contains(&"data/settings.ini".to_string()));
        assert_eq!(stale.len(), 1);
    }

    #[test]
    fn write_dest_creates_nested_and_writes() {
        let dir = tempfile::tempdir().unwrap();
        write_dest_file(dir.path(), "core/nav/locator.rb", b"hello").unwrap();
        let f = dir.path().join("accessibility").join("core").join("nav").join("locator.rb");
        assert!(f.exists());
        assert_eq!(fs::read(&f).unwrap(), b"hello");
    }

    #[test]
    fn remove_stale_deletes_files() {
        let dir = tempfile::tempdir().unwrap();
        write_dest_file(dir.path(), "core/old/removed.rb", b"x").unwrap();
        let f = dir.path().join("accessibility").join("core").join("old").join("removed.rb");
        assert!(f.exists());
        remove_stale(dir.path(), &vec!["core/old/removed.rb".to_string()]);
        assert!(!f.exists());
    }

    #[test]
    fn seal_and_reread_installed() {
        let dir = tempfile::tempdir().unwrap();
        let mut files = BTreeMap::new();
        files.insert("core/nav/locator.rb".to_string(), "abc".to_string());
        seal_installed(dir.path(), "0.8.1", "pokemon_z", "specific", "x64", "now", files).unwrap();
        let re = super::super::installed::read(dir.path()).unwrap();
        assert_eq!(re.mod_version, "0.8.1");
        assert_eq!(re.profile, "pokemon_z");
        assert_eq!(re.profile_mode, "specific");
        assert_eq!(re.files.get("core/nav/locator.rb").unwrap(), "abc");
    }
}
