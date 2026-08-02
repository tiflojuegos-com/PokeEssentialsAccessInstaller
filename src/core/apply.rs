use std::collections::BTreeMap;
use std::fs;
use std::io;
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

/// The voice library flavour the game needs, read from the header of the
/// executable the folder scan already found. A folder without an executable
/// gets the 32 bit build, which is what an RPG Maker game ships by default.
pub fn arch_of(exe: Option<&Path>) -> String {
    match exe {
        Some(p) => super::detect::pe_arch(p),
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
    let scan = super::detect::scan_exes(game_dir);
    if !super::mkxp::has_mkxp_json(game_dir) && !scan.supports_preload {
        return Err("not_compatible".to_string());
    }
    let arch = arch_of(scan.main_exe.as_deref());
    let version_text = super::github::download_bytes(&super::paths::raw_url("version.json"))?;
    let meta = super::installed::parse_version_json(&String::from_utf8_lossy(&version_text));
    if let Some(m) = &meta {
        if launcher_too_old(&m.min_launcher) {
            return Err(crate::i18n::err_key("err_launcher_too_old", m.min_launcher.trim()));
        }
    }
    let mod_version = meta.map(|m| m.version).unwrap_or_else(|| "0.0.0".to_string());

    let remote = super::github::walk_tree(&repo_dirs_for(profile, &arch))
        .map_err(|e| format!("listar archivos del mod: {}", e))?;

    let previous = super::installed::read(game_dir);
    let prev = previous.as_ref().map(|i| i.files.clone()).unwrap_or_default();
    let prev_version = previous.map(|i| i.mod_version).unwrap_or_default();
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

    let failure = outcome.error.or_else(|| super::mkxp::register(game_dir).err());
    if let Some(e) = failure {
        record_partial(game_dir, &prev_version, profile, profile_mode, &arch, now, files);
        return Err(e);
    }

    let stale = stale_files(&remote, &prev, profile, &arch);
    remove_stale(game_dir, &stale);
    for s in &stale {
        files.remove(s);
    }

    seal_installed(game_dir, &mod_version, profile, profile_mode, &arch, now, files)?;
    Ok(mod_version)
}

/// Records a run that never reached the end: the files that did land on disk,
/// but still the version the game already had, so the list goes on offering the
/// update instead of calling a half written install current and the next run
/// only fetches what is missing. A failure that wrote nothing to a game that had
/// no install leaves no installed.json at all, because there is nothing to
/// resume and the row must keep saying "not installed". Best effort: the run is
/// already failing for another reason and that is the error worth reporting.
fn record_partial(
    game_dir: &Path,
    prev_version: &str,
    profile: &str,
    profile_mode: &str,
    arch: &str,
    now: &str,
    files: BTreeMap<String, String>,
) {
    if files.is_empty() {
        return;
    }
    let _ = seal_installed(game_dir, prev_version, profile, profile_mode, arch, now, files);
}

/// True when version.json demands a launcher newer than this build.
fn launcher_too_old(min_launcher: &str) -> bool {
    super::selfupdate::requires_newer_than(min_launcher, env!("CARGO_PKG_VERSION"))
}

/// Checks a download against the sha announced by the tree before it touches disk.
/// An empty remote sha means the listing carried nothing to compare against.
fn verify_blob(dest_rel: &str, data: &[u8], remote_sha: &str) -> Result<String, String> {
    let sha = super::install::git_blob_sha1(data);
    let expected = remote_sha.trim();
    if !expected.is_empty() && !sha.eq_ignore_ascii_case(expected) {
        return Err(crate::i18n::err_key("err_download_corrupt", dest_rel));
    }
    Ok(sha)
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
            let remote_sha = op.remote_sha.clone();
            let game_dir = game_dir.to_path_buf();
            handles.push(std::thread::spawn(move || -> Result<(String, String), String> {
                let data = super::github::download_bytes(&url)?;
                let sha = verify_blob(&dest_rel, &data, &remote_sha)?;
                write_dest_file(&game_dir, &dest_rel, &data)?;
                Ok((dest_rel, sha))
            }));
        }
        for h in handles {
            match h.join() {
                Ok(Ok((dest_rel, sha))) => {
                    let n = done.fetch_add(1, Ordering::SeqCst) + 1;
                    progress(&dest_rel, n, total);
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
        fs::remove_dir_all(&dir).map_err(|e| io_error("accessibility/", "borrar", &e))?;
    }
    Ok(())
}

pub fn write_dest_file(game_dir: &Path, dest_rel: &str, data: &[u8]) -> Result<(), String> {
    let full = accessibility_dir(game_dir).join(dest_rel.replace('/', &std::path::MAIN_SEPARATOR.to_string()));
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).map_err(|e| io_error(dest_rel, "crear carpeta", &e))?;
    }
    fs::write(&full, data).map_err(|e| io_error(dest_rel, "escribir", &e))
}

/// Turns a write failure into advice the player can act on: close the game when
/// the file is locked, move the game when the folder denies writing. Anything
/// else keeps its literal message. Shared with `mkxp`, so the same class of
/// failure reads the same way whoever touched the disk.
pub(super) fn io_error(dest_rel: &str, action: &str, e: &io::Error) -> String {
    if super::detect::file_locked(e) {
        crate::i18n::err_key("err_write_locked", dest_rel)
    } else if write_denied(e) {
        "no_write_perm".to_string()
    } else {
        format!("{} {}: {}", action, dest_rel, e)
    }
}

/// True when the filesystem refused the write outright, as a read-only or
/// protected folder does. Closing the game would not help here.
fn write_denied(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(5)
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
    fn verify_blob_accepts_the_announced_sha() {
        let data = b"hello\n";
        let sha = super::super::install::git_blob_sha1(data);
        assert_eq!(verify_blob("core/x.rb", data, &sha).unwrap(), sha);
        assert_eq!(verify_blob("core/x.rb", data, &sha.to_uppercase()).unwrap(), sha);
    }

    #[test]
    fn verify_blob_rejects_a_mismatched_sha() {
        let err = verify_blob("core/x.rb", b"hello\n", "0000000000000000000000000000000000000000").unwrap_err();
        let shown = crate::i18n::I18n::new("es").t_err(&err);
        assert!(shown.contains("core/x.rb"));
        assert!(!shown.contains("err_download_corrupt"));
    }

    #[test]
    fn verify_blob_skips_when_the_listing_has_no_sha() {
        assert!(verify_blob("core/x.rb", b"hello\n", "").is_ok());
        assert!(verify_blob("core/x.rb", b"hello\n", "   ").is_ok());
    }

    #[test]
    fn io_error_suggests_closing_the_game_when_locked() {
        for code in [32, 33] {
            let e = io::Error::from_raw_os_error(code);
            let shown = crate::i18n::I18n::new("en").t_err(&io_error("lib/PA3D_steam.dll", "escribir", &e));
            assert!(shown.contains("lib/PA3D_steam.dll"), "codigo {}", code);
            assert!(shown.to_lowercase().contains("close the game"), "codigo {}", code);
        }
    }

    #[test]
    fn io_error_points_at_permissions_when_the_folder_denies_writing() {
        let denied = [
            io::Error::from_raw_os_error(5),
            io::Error::new(io::ErrorKind::PermissionDenied, "acceso denegado"),
        ];
        let i18n = crate::i18n::I18n::new("en");
        for e in denied {
            let shown = i18n.t_err(&io_error("lib/PA3D_steam.dll", "escribir", &e));
            assert_eq!(shown, i18n.t("no_write_perm"));
            assert!(!shown.to_lowercase().contains("close the game"));
        }
    }

    #[test]
    fn install_rejects_a_game_without_mkxp_or_preload_support() {
        let dir = tempfile::tempdir().unwrap();
        let err = run_install(dir.path(), "generic", "generic", "now", |_, _, _| {}).unwrap_err();
        let i18n = crate::i18n::I18n::new("en");
        assert_eq!(i18n.t_err(&err), i18n.t("not_compatible"));
        assert_ne!(i18n.t_err(&err), "not_compatible");
    }

    #[test]
    fn io_error_keeps_the_literal_message_for_other_failures() {
        let e = io::Error::new(io::ErrorKind::NotFound, "no existe");
        let msg = io_error("core/x.rb", "escribir", &e);
        assert!(msg.starts_with("escribir core/x.rb: "));
        assert_eq!(crate::i18n::I18n::new("es").t_err(&msg), msg);
    }

    #[test]
    fn launcher_gate_blocks_only_newer_requirements() {
        assert!(!launcher_too_old(""));
        assert!(!launcher_too_old(env!("CARGO_PKG_VERSION")));
        assert!(!launcher_too_old("0.0.1"));
        assert!(launcher_too_old("999.0.0"));
    }

    #[test]
    fn launcher_gate_message_names_the_required_version() {
        let err = crate::i18n::err_key("err_launcher_too_old", "999.0.0");
        let shown = crate::i18n::I18n::new("es").t_err(&err);
        assert!(shown.contains("999.0.0"));
        assert!(!shown.contains("err_launcher_too_old"));
    }

    fn one_file(dest: &str, sha: &str) -> BTreeMap<String, String> {
        let mut files = BTreeMap::new();
        files.insert(dest.to_string(), sha.to_string());
        files
    }

    #[test]
    fn a_partial_update_keeps_the_old_version_and_the_new_files() {
        use super::super::status::{compute, GameStatus};
        let dir = tempfile::tempdir().unwrap();
        let before = one_file("core/nav/locator.rb", "old");
        seal_installed(dir.path(), "0.8.0", "pokemon_z", "specific", "x64", "before", before).unwrap();

        let half = one_file("core/nav/locator.rb", "new");
        record_partial(dir.path(), "0.8.0", "pokemon_z", "specific", "x64", "now", half);

        let re = super::super::installed::read(dir.path()).unwrap();
        assert_eq!(re.mod_version, "0.8.0");
        assert_eq!(compute(Some(&re.mod_version), "0.9.0"), GameStatus::UpdateAvailable);
        assert_eq!(re.files.get("core/nav/locator.rb").unwrap(), "new");
        assert_eq!(re.installed_at, "now");
    }

    #[test]
    fn a_partial_first_install_is_never_reported_as_up_to_date() {
        use super::super::status::{compute, GameStatus};
        let dir = tempfile::tempdir().unwrap();
        record_partial(dir.path(), "", "generic", "generic", "x86", "now", one_file("boot.rb", "abc"));

        let re = super::super::installed::read(dir.path()).unwrap();
        assert_ne!(re.mod_version, "0.9.0");
        assert_eq!(compute(Some(&re.mod_version), "0.9.0"), GameStatus::UpdateAvailable);
        assert_eq!(re.files.get("boot.rb").unwrap(), "abc");
    }

    #[test]
    fn a_first_install_that_wrote_nothing_stays_not_installed() {
        use super::super::status::{compute, GameStatus};
        let dir = tempfile::tempdir().unwrap();
        record_partial(dir.path(), "", "generic", "generic", "x86", "now", BTreeMap::new());

        assert!(!super::super::installed::is_installed(dir.path()));
        let re = super::super::installed::read(dir.path());
        assert_eq!(compute(re.as_ref().map(|i| i.mod_version.as_str()), "0.9.0"), GameStatus::NotInstalled);
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
