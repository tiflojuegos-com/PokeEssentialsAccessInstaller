use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf, MAIN_SEPARATOR_STR};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use super::catalog::{Catalog, Profile};
use super::convert::{Bundle, Manifest, RgssPlayer};
use super::installed::Installed;
use super::ops::Inspection;
use super::paths::{accessibility_dir, installed_file};
use super::source::{ContentEntry, Source};

pub fn dest_for(repo_path: &str, profile: &str, arch: &str) -> Option<String> {
    let p = repo_path.replace('\\', "/");
    if let Some(rest) = p.strip_prefix("core/") {
        return Some(format!("core/{}", rest));
    }
    if let Some(rest) = p.strip_prefix("lang/") {
        return Some(format!("lang/{}", rest));
    }
    if let Some(rest) = p.strip_prefix("plugins/") {
        return Some(format!("plugins/{}", rest));
    }
    let game_prefix = format!("games/{}/", profile);
    if let Some(rest) = p.strip_prefix(&game_prefix) {
        return Some(format!("game/{}", rest));
    }
    if let Some((name, rest)) = p.strip_prefix("games/").and_then(|r| r.split_once('/')) {
        if name.ends_with("_common") {
            return Some(format!("common/{}/{}", name, rest));
        }
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
        "plugins".to_string(),
        format!("games/{}", profile),
        "loader".to_string(),
        "assets/sounds".to_string(),
        format!("assets/{}", arch),
    ]
}

fn imports_of(manifest: &str) -> Vec<String> {
    let re = regex::Regex::new(r":imports\s*=>\s*%w\[([^\]]*)\]").expect("imports pattern");
    let mut out: Vec<String> = Vec::new();
    if let Some(c) = re.captures(manifest) {
        for name in c[1].split_whitespace() {
            let valid = name.ends_with("_common")
                && name.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_');
            if valid && !out.iter().any(|n| n == name) {
                out.push(name.to_string());
            }
        }
    }
    out
}

fn only_wanted(listed: Vec<ContentEntry>, dirs: &[String], imports: &[String]) -> Vec<ContentEntry> {
    let mut keep: Vec<String> = dirs.to_vec();
    keep.extend(imports.iter().map(|c| format!("games/{}", c)));
    listed
        .into_iter()
        .filter(|e| keep.iter().any(|k| e.path == *k || e.path.starts_with(&format!("{}/", k))))
        .collect()
}

fn manifest_imports(source: &Source, listed: &[ContentEntry], profile: &str) -> Result<Vec<String>, String> {
    let path = format!("games/{}/manifest.rb", profile);
    match listed.iter().find(|e| e.path == path) {
        Some(mf) => Ok(imports_of(&String::from_utf8_lossy(&fetch_checked(source, mf)?))),
        None => Ok(Vec::new()),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileOp {
    pub entry: ContentEntry,
    pub dest_rel: String,
}

pub fn plan_update(
    remote: &[ContentEntry],
    installed_files: &BTreeMap<String, String>,
    profile: &str,
    arch: &str,
) -> Vec<FileOp> {
    let mut ops = Vec::new();
    for e in remote {
        let dest = match dest_for(&e.path, profile, arch) {
            Some(d) => d,
            None => continue,
        };
        let local = installed_files.get(&dest);
        if super::install::needs_update(local, &e.sha) {
            ops.push(FileOp { entry: e.clone(), dest_rel: dest });
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
        if let Some(d) = dest_for(&e.path, profile, arch) {
            wanted.insert(d);
        }
    }
    installed_files
        .keys()
        .filter(|k| !wanted.contains(*k) && !super::install::is_user_data(k))
        .cloned()
        .collect()
}

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

#[derive(Debug, Clone)]
pub struct InstallOutcome {
    pub version: String,
    pub previous: String,
    pub files: usize,
    pub removed: usize,
    pub converted: Option<String>,
    pub kept_profile: Option<String>,
    pub engine_note: Option<String>,
}

impl InstallOutcome {
    pub fn changed(&self) -> bool {
        self.files > 0 || self.removed > 0 || self.converted.is_some() || self.previous != self.version
    }
}

pub fn run_install(
    game: &Inspection,
    profile: &str,
    profile_mode: &str,
    source: &Source,
    cat: Option<&Catalog>,
    now: &str,
    mut progress: impl FnMut(&str, u32, u32),
) -> Result<InstallOutcome, String> {
    let game_dir = game.game.dir.as_path();
    if let Some(reason) = game.incompatible() {
        return Err(reason);
    }
    let player = game.player.clone().transpose()?;
    let offer = game.offer.as_ref();
    let kept_profile = game.kept_profile(profile);
    let (profile, profile_mode) = match &kept_profile {
        Some(p) => (p.as_str(), "specific"),
        None => (profile, profile_mode),
    };
    let arch = arch_of(game.game.scan.main_exe.as_deref());
    let meta = source.mod_version()?;
    if let Some(m) = &meta {
        if launcher_too_old(&m.min_launcher) {
            return Err(crate::i18n::err_key("err_launcher_too_old", m.min_launcher.trim()));
        }
    }
    let mod_version = meta.map(|m| m.version).unwrap_or_else(|| "0.0.0".to_string());

    let listed_profile = cat.and_then(|c| c.profiles.iter().find(|p| p.key == profile));
    let compat = listed_profile.and_then(|p| p.compat.clone()).filter(|c| !c.trim().is_empty());
    let engine =
        offer.map(|o| o.engine.clone()).or_else(|| game.record.as_ref().map(|m| engine_for(listed_profile, m)));
    let mut dirs = repo_dirs_for(profile, &arch);
    for id in engine.iter().chain(game.record.as_ref().map(|m| &m.engine)) {
        let bundle = super::convert::bundle_dir(id);
        if !dirs.contains(&bundle) {
            dirs.push(bundle);
        }
    }
    let mut listing = dirs.clone();
    listing.push("games".to_string());
    let listed = source.list(&listing).map_err(|e| crate::i18n::err_key("err_list_files", &e))?;
    let imports = manifest_imports(source, &listed, profile)?;
    let remote = only_wanted(listed, &dirs, &imports);
    if profile_files(&remote, profile) == 0 {
        return Err(crate::i18n::err_key("err_profile_missing", profile));
    }

    move_loose_data(game_dir);
    let previous = super::installed::read(game_dir);
    let prev = previous.as_ref().map(|i| i.files.clone()).unwrap_or_default();
    let prev_version = previous.map(|i| i.mod_version).unwrap_or_default();
    let ops = plan_update(&remote, &intact_files(game_dir, &prev), profile, &arch);

    let mut files: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in prev.iter() {
        files.insert(k.clone(), v.clone());
    }

    let outcome = copy_files(game_dir, &ops, source, &mut progress);
    let mut written = outcome.written.len();
    for (dest_rel, sha) in &outcome.written {
        files.insert(dest_rel.clone(), sha.clone());
    }

    let mut failure = outcome.error;
    if failure.is_none() {
        failure = missing_deployed(game_dir, &remote, profile, &arch)
            .map(|dest| crate::i18n::err_key("err_deploy_missing", &dest));
    }
    let mut converted = None;
    let mut engine_note = None;
    if failure.is_none() {
        let engine_step = match (offer.zip(player.as_ref()), &game.record, engine.as_deref()) {
            (Some((o, p)), _, _) => convert_game(game_dir, source, &remote, &o.engine, p, profile, now)
                .map(|m| EngineStep { converted: Some(m.exe), ..EngineStep::default() }),
            (None, Some(record), Some(id)) => update_engine(game_dir, source, &remote, record, id, profile, now),
            _ => Ok(EngineStep::default()),
        };
        match engine_step {
            Ok(step) => {
                converted = step.converted;
                engine_note = step.note;
                written += step.files;
            }
            Err(e) => failure = Some(e),
        }
    }
    if failure.is_none() {
        let registered = compat_deployed(game_dir, compat.as_deref())
            .and_then(|_| super::mkxp::register(game_dir, compat.as_deref()));
        if let Err(e) = registered {
            if converted.is_some() {
                let _ = super::convert::restore(game_dir);
            }
            failure = Some(e);
        }
    }
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
    Ok(InstallOutcome {
        version: mod_version,
        previous: prev_version,
        files: written,
        removed: stale.len(),
        converted,
        kept_profile,
        engine_note,
    })
}

const LOOSE_DATA: [&str; 4] = ["settings.ini", "tags.txt", "tags_export.txt", "tags_import.txt"];

fn move_loose_data(game_dir: &Path) {
    let root = accessibility_dir(game_dir);
    let data = root.join("data");
    for name in LOOSE_DATA {
        let (loose, kept) = (root.join(name), data.join(name));
        if loose.is_file() && !kept.exists() && fs::create_dir_all(&data).is_ok() {
            let _ = fs::rename(&loose, &kept);
        }
    }
}

fn missing_deployed(game_dir: &Path, remote: &[ContentEntry], profile: &str, arch: &str) -> Option<String> {
    let root = accessibility_dir(game_dir);
    remote
        .iter()
        .filter_map(|e| dest_for(&e.path, profile, arch))
        .find(|dest| !root.join(dest.replace('/', MAIN_SEPARATOR_STR)).is_file())
}

fn compat_deployed(game_dir: &Path, compat: Option<&str>) -> Result<(), String> {
    match compat {
        Some(c) if !game_dir.join("accessibility").join("game").join(c).is_file() => {
            Err(crate::i18n::err_key("err_compat_missing", c))
        }
        _ => Ok(()),
    }
}

pub(super) fn intact_files(game_dir: &Path, sealed: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let root = accessibility_dir(game_dir);
    sealed
        .iter()
        .filter(|(rel, sha)| {
            fs::read(root.join(rel.replace('/', &std::path::MAIN_SEPARATOR.to_string())))
                .map(|b| super::install::git_blob_sha1(&b).eq_ignore_ascii_case(sha))
                .unwrap_or(false)
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

#[derive(Default)]
struct EngineStep {
    converted: Option<String>,
    files: usize,
    note: Option<String>,
}

fn engine_for(listed: Option<&Profile>, record: &Manifest) -> String {
    listed
        .and_then(|p| p.convert.clone())
        .filter(|e| !e.trim().is_empty())
        .unwrap_or_else(|| record.engine.clone())
}

fn convert_game(
    game_dir: &Path,
    source: &Source,
    remote: &[ContentEntry],
    engine_id: &str,
    player: &RgssPlayer,
    profile: &str,
    now: &str,
) -> Result<Manifest, String> {
    let bundle = super::convert::bundle_in(remote, engine_id)
        .ok_or_else(|| crate::i18n::err_key("convert_engine_missing", engine_id))?;
    let engine = fetch_checked(source, &bundle.engine)?;
    let mut extras = Vec::new();
    for (name, entry) in &bundle.extras {
        extras.push((name.clone(), fetch_checked(source, entry)?));
    }
    super::convert::restore(game_dir)?;
    super::convert::convert(game_dir, player, engine_id, profile, &engine, &extras, now)
}

fn update_engine(
    game_dir: &Path,
    source: &Source,
    remote: &[ContentEntry],
    record: &Manifest,
    wanted: &str,
    profile: &str,
    now: &str,
) -> Result<EngineStep, String> {
    let found = [wanted, record.engine.as_str()]
        .into_iter()
        .find_map(|id| super::convert::bundle_in(remote, id).map(|bundle| (id, bundle)));
    match found {
        Some((id, _)) if record.pending || id != record.engine => {
            let player = super::convert::rgss_player(game_dir).ok_or_else(|| "not_compatible".to_string())?;
            let redone = convert_game(game_dir, source, remote, id, &player, profile, now)?;
            Ok(if record.pending {
                EngineStep { converted: Some(redone.exe), ..EngineStep::default() }
            } else {
                EngineStep {
                    files: 1 + redone.added.len(),
                    note: Some(crate::i18n::err_key("convert_migrated", id)),
                    converted: None,
                }
            })
        }
        Some((_, bundle)) => {
            refresh_engine(game_dir, source, &bundle).map(|files| EngineStep { files, ..EngineStep::default() })
        }
        None if record.pending => Err(crate::i18n::err_key("convert_engine_missing", wanted)),
        None => {
            let files = usize::from(super::convert::restore_json(game_dir, None)?);
            let note = (record.engine != super::ops::LOCAL_ENGINE)
                .then(|| crate::i18n::err_key("convert_engine_gone", &record.engine));
            Ok(EngineStep { files, note, converted: None })
        }
    }
}

fn refresh_engine(game_dir: &Path, source: &Source, bundle: &Bundle) -> Result<usize, String> {
    let soundfont = super::convert::soundfont_in(bundle.extras.iter().map(|(n, _)| n.as_str()));
    let restored = usize::from(super::convert::restore_json(game_dir, soundfont)?);
    let engine = match super::convert::accessible_sha(game_dir) {
        Some(sha) if bundle.engine.sha.eq_ignore_ascii_case(&sha) => None,
        _ => Some(fetch_checked(source, &bundle.engine)?),
    };
    let mut missing = Vec::new();
    for (name, entry) in bundle.extras.iter().filter(|(name, _)| !game_dir.join(name).exists()) {
        missing.push((name.clone(), fetch_checked(source, entry)?));
    }
    Ok(restored + super::convert::refresh(game_dir, engine.as_deref(), &missing)?)
}

fn fetch_checked(source: &Source, entry: &ContentEntry) -> Result<Vec<u8>, String> {
    let data = source.read(entry)?;
    verify_blob(&entry.path, &data, &entry.sha)?;
    Ok(data)
}

fn profile_files(remote: &[ContentEntry], profile: &str) -> usize {
    let prefix = format!("games/{}/", profile);
    remote.iter().filter(|e| e.path.starts_with(&prefix)).count()
}

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

fn launcher_too_old(min_launcher: &str) -> bool {
    super::selfupdate::requires_newer_than(min_launcher, env!("CARGO_PKG_VERSION"))
}

fn verify_blob(dest_rel: &str, data: &[u8], remote_sha: &str) -> Result<String, String> {
    let sha = super::install::git_blob_sha1(data);
    let expected = remote_sha.trim();
    if !expected.is_empty() && !sha.eq_ignore_ascii_case(expected) {
        return Err(crate::i18n::err_key("err_download_corrupt", dest_rel));
    }
    Ok(sha)
}

const MAX_CONCURRENT: usize = 8;

struct CopyOutcome {
    written: Vec<(String, String)>,
    error: Option<String>,
}

fn copy_files(
    game_dir: &Path,
    ops: &[FileOp],
    source: &Source,
    progress: &mut impl FnMut(&str, u32, u32),
) -> CopyOutcome {
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let (tx, rx) = mpsc::channel();
    let mut outcome = CopyOutcome { written: Vec::new(), error: None };
    thread::scope(|s| {
        for _ in 0..MAX_CONCURRENT.min(ops.len()) {
            let tx = tx.clone();
            let (next, failed) = (&next, &failed);
            s.spawn(move || {
                while !failed.load(Ordering::SeqCst) {
                    let Some(op) = ops.get(next.fetch_add(1, Ordering::SeqCst)) else { break };
                    let copied = copy_file(game_dir, op, source);
                    if copied.is_err() {
                        failed.store(true, Ordering::SeqCst);
                    }
                    if tx.send(copied).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        for copied in rx {
            match copied {
                Ok((dest_rel, sha)) => {
                    progress(&dest_rel, outcome.written.len() as u32 + 1, ops.len() as u32);
                    outcome.written.push((dest_rel, sha));
                }
                Err(e) => {
                    outcome.error.get_or_insert(e);
                }
            }
        }
    });
    outcome
}

fn copy_file(game_dir: &Path, op: &FileOp, source: &Source) -> Result<(String, String), String> {
    let data = source.read(&op.entry)?;
    let sha = verify_blob(&op.dest_rel, &data, &op.entry.sha)?;
    write_dest_file(game_dir, &op.dest_rel, &data)?;
    Ok((op.dest_rel.clone(), sha))
}

pub fn run_uninstall(game_dir: &Path, keep_data: bool) -> Result<Vec<String>, String> {
    let mut notes = super::convert::restore(game_dir)?;
    super::mkxp::unregister(game_dir)?;
    let dir = accessibility_dir(game_dir);
    if keep_data {
        move_loose_data(game_dir);
        remove_keeping_data(&dir)?;
    } else {
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| io_error("accessibility/", "delete", &e))?;
        }
        if let Some(copy) = dirs::data_local_dir().and_then(|base| virtual_store_copy(game_dir, &base)) {
            notes.push(crate::i18n::err_key("uninstall_virtualstore", &copy.display().to_string()));
        }
    }
    Ok(notes)
}

fn remove_keeping_data(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    let entries = fs::read_dir(dir).map_err(|e| io_error("accessibility/", "read", &e))?;
    for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
        let removed = match path.file_name() {
            Some(name) if name.eq_ignore_ascii_case("data") => continue,
            _ if path.is_dir() => fs::remove_dir_all(&path),
            _ => fs::remove_file(&path),
        };
        removed.map_err(|e| io_error(&path.display().to_string(), "delete", &e))?;
    }
    match fs::remove_file(dir.join("data").join("installed.json")) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(io_error("data/installed.json", "delete", &e)),
        _ => Ok(()),
    }
}

fn virtual_store_copy(game_dir: &Path, local_app_data: &Path) -> Option<PathBuf> {
    let rel: PathBuf = game_dir.components().filter(|c| matches!(c, Component::Normal(_))).collect();
    let copy = local_app_data.join("VirtualStore").join(rel).join("accessibility");
    copy.is_dir().then_some(copy)
}

pub fn write_dest_file(game_dir: &Path, dest_rel: &str, data: &[u8]) -> Result<(), String> {
    let full = accessibility_dir(game_dir).join(dest_rel.replace('/', &std::path::MAIN_SEPARATOR.to_string()));
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).map_err(|e| io_error(dest_rel, "mkdir", &e))?;
    }
    fs::write(&full, data).map_err(|e| io_error(dest_rel, "write", &e))
}

pub(super) fn io_error(dest_rel: &str, action: &str, e: &io::Error) -> String {
    if super::detect::file_locked(e) {
        crate::i18n::err_key("err_write_locked", dest_rel)
    } else if write_denied(e) {
        "no_write_perm".to_string()
    } else {
        crate::i18n::err_key(&format!("err_io_{}", action), &format!("{} ({})", dest_rel, e))
    }
}

fn write_denied(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(5)
}

pub fn remove_stale(game_dir: &Path, stale: &[String]) {
    let root = accessibility_dir(game_dir);
    for rel in stale {
        let full = root.join(rel.replace('/', &std::path::MAIN_SEPARATOR.to_string()));
        let _ = fs::remove_file(&full);
        let mut dir = full.parent().map(|p| p.to_path_buf());
        while let Some(d) = dir {
            if d == root || !d.starts_with(&root) || fs::remove_dir(&d).is_err() {
                break;
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
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
        fs::create_dir_all(parent).map_err(|e| io_error("data/installed.json", "mkdir", &e))?;
    }
    let json = serde_json::to_string_pretty(&inst)
        .map_err(|e| crate::i18n::err_key("err_io_write", &format!("data/installed.json ({})", e)))?;
    fs::write(path, json).map_err(|e| io_error("data/installed.json", "write", &e))
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

    #[test]
    fn imports_are_read_from_the_manifest_text_once_each() {
        let mf = "{\n  :imports => %w[infinitefusion_common lostie_common infinitefusion_common],\n  :modules => %w[]\n}\n";
        assert_eq!(imports_of(mf), vec!["infinitefusion_common".to_string(), "lostie_common".to_string()]);
        assert!(imports_of("{ :modules => %w[a b], :plugins => %w[luka_title] }").is_empty());
        assert_eq!(imports_of(":imports => %w[../x_common Foo_common anil ok_common]"), vec!["ok_common".to_string()]);
    }

    #[test]
    fn an_imported_common_lands_in_its_own_folder_beside_the_profile() {
        assert_eq!(
            dest_for("games/infinitefusion_common/outfits.rb", "infinitefusion_hoenn", "x64").unwrap(),
            "common/infinitefusion_common/outfits.rb"
        );
        assert_eq!(dest_for("games/infinitefusion_hoenn/manifest.rb", "infinitefusion_hoenn", "x64").unwrap(), "game/manifest.rb");
    }

    #[test]
    fn only_the_profile_and_the_commons_it_imports_are_kept_from_the_listing() {
        let listed = vec![
            entry("core/nav/locator.rb", "s"),
            entry("games/infinitefusion/manifest.rb", "s"),
            entry("games/infinitefusion_common/outfits.rb", "s"),
            entry("games/lostie_common/iv_stars.rb", "s"),
            entry("games/anil/menus.rb", "s"),
            entry("games/catalog.json", "s"),
        ];
        let dirs = repo_dirs_for("infinitefusion", "x64");
        let kept: Vec<String> = only_wanted(listed, &dirs, &["infinitefusion_common".to_string()])
            .into_iter()
            .map(|e| e.path)
            .collect();
        assert_eq!(
            kept,
            vec![
                "core/nav/locator.rb".to_string(),
                "games/infinitefusion/manifest.rb".to_string(),
                "games/infinitefusion_common/outfits.rb".to_string()
            ]
        );
    }

    #[test]
    fn a_common_no_longer_imported_leaves_no_file_or_folder_behind() {
        let dir = tempfile::tempdir().unwrap();
        write_dest_file(dir.path(), "common/infinitefusion_common/outfits.rb", b"x").unwrap();
        write_dest_file(dir.path(), "core/nav/locator.rb", b"x").unwrap();
        let mut installed = BTreeMap::new();
        installed.insert("common/infinitefusion_common/outfits.rb".to_string(), "s".to_string());
        installed.insert("core/nav/locator.rb".to_string(), "s".to_string());
        let stale = stale_files(&[entry("core/nav/locator.rb", "s")], &installed, "anil", "x64");
        assert_eq!(stale, vec!["common/infinitefusion_common/outfits.rb".to_string()]);
        remove_stale(dir.path(), &stale);
        let root = dir.path().join("accessibility");
        assert!(!root.join("common").exists());
        assert!(root.join("core").join("nav").join("locator.rb").exists());
    }

    fn entry(path: &str, sha: &str) -> ContentEntry {
        ContentEntry { path: path.to_string(), sha: sha.to_string() }
    }

    fn install(dir: &Path, source: &Source, cat: Option<&Catalog>) -> Result<InstallOutcome, String> {
        let game = Inspection::of(dir.to_path_buf(), cat, source);
        run_install(&game, "generic", "generic", source, cat, "now", |_, _, _| {})
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
        let err = install(dir.path(), &Source::github(), None).unwrap_err();
        let i18n = crate::i18n::I18n::new("en");
        assert_eq!(i18n.t_err(&err), i18n.t("not_compatible"));
        assert_ne!(i18n.t_err(&err), "not_compatible");
    }

    #[test]
    fn an_rpg_maker_xp_game_without_a_catalog_offer_is_refused_with_its_own_reason() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Game.exe"), b"MZ player").unwrap();
        fs::write(dir.path().join("Game.ini"), "[Game]\nLibrary=RGSS102E.dll\n").unwrap();
        let err = install(dir.path(), &Source::github(), None).unwrap_err();
        assert_eq!(err, "rgss_player_unsupported");
        assert_ne!(crate::i18n::I18n::new("en").t_err(&err), "rgss_player_unsupported");
    }

    #[test]
    fn uninstalling_a_converted_game_leaves_only_its_own_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Game.exe"), b"MZ player").unwrap();
        fs::write(dir.path().join("Game.ini"), "[Game]\nLibrary=RGSS102E.dll\n").unwrap();
        let player = super::super::convert::rgss_player(dir.path()).unwrap();
        super::super::convert::convert(dir.path(), &player, "e", "insurgence", b"MZ engine preloadScript", &[], "now")
            .unwrap();
        super::super::mkxp::register(dir.path(), None).unwrap();
        let notes = run_uninstall(dir.path(), false).unwrap();
        assert_eq!(notes, vec![crate::i18n::err_key("convert_removed", "Game (PokeAccess).exe")]);
        assert_eq!(fs::read(dir.path().join("Game.exe")).unwrap(), b"MZ player");
        let mut left: Vec<String> =
            fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, vec!["Game.exe".to_string(), "Game.ini".to_string()]);
    }

    #[test]
    fn a_profile_with_no_folder_in_the_repo_is_not_installable() {
        let remote = vec![entry("core/nav/locator.rb", "s"), entry("games/anil/menus.rb", "s")];
        assert_eq!(profile_files(&remote, "anil"), 1);
        assert_eq!(profile_files(&remote, "anil2"), 0);
        assert_eq!(profile_files(&remote, ""), 0);
    }

    #[test]
    fn the_missing_profile_error_names_the_profile_in_the_players_language() {
        let err = crate::i18n::err_key("err_profile_missing", "anil2");
        let shown = crate::i18n::I18n::new("es").t_err(&err);
        assert!(shown.contains("anil2"));
        assert!(!shown.contains("err_profile_missing"));
    }

    #[test]
    fn io_error_names_the_action_and_the_file_in_the_players_language() {
        let e = io::Error::new(io::ErrorKind::NotFound, "no existe");
        let msg = io_error("core/x.rb", "write", &e);
        for lang in crate::i18n::LANGS {
            let shown = crate::i18n::I18n::new(lang).t_err(&msg);
            assert!(shown.contains("core/x.rb"), "{}: {}", lang, shown);
            assert!(shown.contains("no existe"), "{}: {}", lang, shown);
            assert!(!shown.contains("err_io_"), "{}: {}", lang, shown);
        }
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

    #[test]
    fn plugins_are_fetched_and_land_next_to_core() {
        assert_eq!(
            dest_for("plugins/item_crafting.rb", "pokemon_z", "x86"),
            Some("plugins/item_crafting.rb".to_string())
        );
        let dirs = repo_dirs_for("pokemon_z", "x86");
        assert!(dirs.contains(&"plugins".to_string()), "plugins must be walked: {:?}", dirs);
        assert_eq!(
            dest_for("plugins/item_crafting.rb", "anil", "x64"),
            dest_for("plugins/item_crafting.rb", "royal", "x86")
        );
        assert_eq!(dest_for("docs/whatever.md", "pokemon_z", "x86"), None);
    }

    #[test]
    fn a_sealed_file_deleted_or_changed_on_disk_is_fetched_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut sealed = BTreeMap::new();
        for (rel, body) in [("core/a.rb", "a"), ("lib/PA3D_steam.dll", "dll"), ("lang/es.txt", "es")] {
            write_dest_file(dir.path(), rel, body.as_bytes()).unwrap();
            sealed.insert(rel.to_string(), super::super::install::git_blob_sha1(body.as_bytes()));
        }
        fs::remove_file(accessibility_dir(dir.path()).join("lib").join("PA3D_steam.dll")).unwrap();
        write_dest_file(dir.path(), "lang/es.txt", b"editado a mano").unwrap();
        let intact = intact_files(dir.path(), &sealed);
        assert_eq!(intact.keys().map(String::as_str).collect::<Vec<_>>(), vec!["core/a.rb"]);
        let remote = vec![
            entry("core/a.rb", &sealed["core/a.rb"]),
            entry("assets/x86/PA3D_steam.dll", &sealed["lib/PA3D_steam.dll"]),
            entry("lang/es.txt", &sealed["lang/es.txt"]),
        ];
        let fetched: Vec<String> =
            plan_update(&remote, &intact, "generic", "x86").into_iter().map(|o| o.dest_rel).collect();
        assert_eq!(fetched, vec!["lib/PA3D_steam.dll".to_string(), "lang/es.txt".to_string()]);
        assert!(plan_update(&remote, &sealed, "generic", "x86").is_empty(), "fiandose del sello no se repara nada");
    }

    fn tree(root: &Path, files: &[(&str, &[u8])]) {
        for (rel, bytes) in files {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
    }

    fn mod_folder(root: &Path) {
        tree(root, &[
            ("version.json", b"{\"version\": \"0.6.0\"}"),
            ("core/manifest.rb", b"core"),
            ("core/nav/locator.rb", b"nav"),
            ("games/catalog.json", b"{\"profiles\": []}"),
            ("games/generic/manifest.rb", b"{ :modules => %w[] }"),
            ("games/insurgence/manifest.rb", b"{ :modules => %w[] }"),
            ("loader/preload_access.rb", b"loader"),
        ]);
    }

    fn mkxp_game(root: &Path) -> std::path::PathBuf {
        let dir = root.join("juego");
        tree(&dir, &[("Game.exe", b"MZ preloadScript")]);
        dir
    }

    #[test]
    fn a_folder_installs_and_the_next_run_copies_only_what_changed_there() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        mod_folder(&repo);
        let dir = mkxp_game(root.path());
        let first = install(&dir, &Source::folder(repo.clone(), None).unwrap(), None).unwrap();
        assert_eq!((first.files, first.version.as_str(), first.previous.as_str()), (4, "0.6.0", ""));
        assert!(first.changed());
        let again = install(&dir, &Source::folder(repo.clone(), None).unwrap(), None).unwrap();
        assert_eq!(again.files, 0);
        assert!(!again.changed());
        fs::write(repo.join("core").join("nav").join("locator.rb"), b"nav 2").unwrap();
        let third = install(&dir, &Source::folder(repo, None).unwrap(), None).unwrap();
        assert_eq!(third.files, 1);
        let sealed = super::super::installed::read(&dir).unwrap();
        assert_eq!(sealed.files["core/nav/locator.rb"], super::super::install::git_blob_sha1(b"nav 2"));
        assert_eq!(fs::read(accessibility_dir(&dir).join("core").join("nav").join("locator.rb")).unwrap(), b"nav 2");
    }

    #[test]
    fn a_file_that_changes_after_the_listing_fails_the_install_and_the_new_version_is_not_sealed() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        mod_folder(&repo);
        let dir = mkxp_game(root.path());
        let source = Source::folder(repo.clone(), None).unwrap();
        source.list(&["core".to_string()]).unwrap();
        fs::write(repo.join("core").join("nav").join("locator.rb"), b"cambiado a medias").unwrap();
        let err = install(&dir, &source, None).unwrap_err();
        assert!(err.starts_with("err_download_corrupt\u{1}"), "{err}");
        let sealed = super::super::installed::read(&dir).map(|i| i.mod_version);
        assert_ne!(sealed.as_deref(), Some("0.6.0"));
    }

    #[test]
    fn the_players_files_an_old_version_left_loose_move_into_data() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        mod_folder(&repo);
        let dir = mkxp_game(root.path());
        let acc = accessibility_dir(&dir);
        tree(&acc, &[("settings.ini", b"old"), ("tags.txt", b"loose"), ("data/tags.txt", b"kept")]);
        install(&dir, &Source::folder(repo, None).unwrap(), None).unwrap();
        assert_eq!(fs::read(acc.join("data").join("settings.ini")).unwrap(), b"old");
        assert!(!acc.join("settings.ini").exists());
        assert_eq!(fs::read(acc.join("data").join("tags.txt")).unwrap(), b"kept", "data/ keeps its own");
        assert!(acc.join("tags.txt").exists());
    }

    #[test]
    fn a_file_missing_after_the_copy_is_named_and_nothing_else() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("juego");
        tree(&accessibility_dir(&dir), &[("core/a.rb", b"a"), ("preload_access.rb", b"p")]);
        let remote = vec![entry("core/a.rb", "s"), entry("loader/preload_access.rb", "s"), entry("core/b.rb", "s"),
            entry("README.md", "s")];
        assert_eq!(missing_deployed(&dir, &remote, "generic", "x86").as_deref(), Some("core/b.rb"));
        assert_eq!(missing_deployed(&dir, &remote[..2], "generic", "x86"), None);
    }

    #[test]
    fn uninstalling_with_keep_data_leaves_only_the_players_data() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        mod_folder(&repo);
        let dir = mkxp_game(root.path());
        install(&dir, &Source::folder(repo, None).unwrap(), None).unwrap();
        let acc = accessibility_dir(&dir);
        tree(&acc, &[("data/settings.ini", b"mine"), ("data/rec/1.txt", b"r"), ("tags.txt", b"loose")]);
        assert!(run_uninstall(&dir, true).unwrap().is_empty());
        let left: Vec<String> =
            fs::read_dir(&acc).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(left, vec!["data".to_string()]);
        assert_eq!(fs::read(acc.join("data").join("settings.ini")).unwrap(), b"mine");
        assert_eq!(fs::read(acc.join("data").join("tags.txt")).unwrap(), b"loose");
        assert!(acc.join("data").join("rec").join("1.txt").is_file());
        assert!(!super::super::installed::is_installed(&dir));
        assert!(!fs::read_to_string(dir.join("mkxp.json")).unwrap_or_default().contains("preload_access"));
        assert!(run_uninstall(&dir, false).unwrap().is_empty());
        assert!(!acc.exists());
    }

    #[cfg(windows)]
    #[test]
    fn a_copy_windows_kept_in_the_virtual_store_is_found_where_windows_puts_it() {
        let base = tempfile::tempdir().unwrap();
        let game = Path::new(r"C:\Program Files (x86)\Pokemon Z");
        assert_eq!(virtual_store_copy(game, base.path()), None);
        let copy = base.path().join("VirtualStore").join("Program Files (x86)").join("Pokemon Z").join("accessibility");
        fs::create_dir_all(&copy).unwrap();
        assert_eq!(virtual_store_copy(game, base.path()), Some(copy));
    }

    #[test]
    fn a_game_on_the_player_is_converted_with_the_engine_the_engine_folder_brings() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        mod_folder(&repo);
        let engine = root.path().join("build");
        tree(&engine, &[("mkxp-z.exe", b"MZ local engine preloadScript"), ("extras/font.sf2", b"sf2")]);
        let dir = root.path().join("Pokemon Insurgence");
        tree(&dir, &[("Game.exe", b"MZ player"), ("Game.ini", b"[Game]\r\nLibrary=RGSS102E.dll\r\n")]);
        let cat = Catalog::from_json(
            r#"{"profiles":[{"key":"insurgence","display":"Pokemon Insurgence","detect":"insurgence","convert":"e1"}]}"#,
        )
        .unwrap();
        let source = Source::folder(repo, Some(engine)).unwrap();
        let done = install(&dir, &source, Some(&cat)).unwrap();
        assert_eq!(done.converted.as_deref(), Some("Game (PokeAccess).exe"));
        assert_eq!(done.kept_profile.as_deref(), Some("insurgence"));
        assert_eq!(fs::read(dir.join("Game (PokeAccess).exe")).unwrap(), b"MZ local engine preloadScript");
        assert_eq!(fs::read(dir.join("font.sf2")).unwrap(), b"sf2");
        assert_eq!(fs::read(dir.join("Game.exe")).unwrap(), b"MZ player");
        assert_eq!(super::super::installed::read(&dir).unwrap().profile, "insurgence");
    }

    fn insurgence_catalog(engine: &str) -> Catalog {
        Catalog::from_json(&format!(
            r#"{{"profiles":[{{"key":"insurgence","display":"Pokemon Insurgence","detect":"insurgence","convert":"{engine}"}}]}}"#
        ))
        .unwrap()
    }

    fn engine_bundle(repo: &Path, id: &str, exe: &[u8], extras: &[(&str, &[u8])]) {
        let bundle = repo.join("assets").join("engine").join(id);
        tree(&bundle, &[("mkxp-z.exe", exe)]);
        tree(&bundle.join("extras"), extras);
    }

    fn insurgence(root: &Path) -> PathBuf {
        let dir = root.join("Pokemon Insurgence");
        tree(&dir, &[("Game.exe", b"MZ player"), ("Game.ini", b"[Game]\r\nLibrary=RGSS102E.dll\r\n")]);
        dir
    }

    fn left_in(dir: &Path) -> Vec<String> {
        let mut left: Vec<String> =
            fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        left
    }

    #[test]
    fn a_conversion_cut_halfway_is_finished_by_the_next_install_whatever_it_left_behind() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        mod_folder(&repo);
        let engine: &[u8] = b"MZ e1 engine preloadScript";
        engine_bundle(&repo, "e1", engine, &[("font.sf2", b"sf2")]);
        let cat = insurgence_catalog("e1");
        let dir = insurgence(root.path());
        let record_file = accessibility_dir(&dir).join("data").join("engine.json");
        let cuts = [
            ("solo el registro", vec![]),
            ("el motor a medias", vec![("Game (PokeAccess).exe", &b"MZ"[..])]),
            ("todo menos el cierre", vec![("Game (PokeAccess).exe", engine), ("font.sf2", b"s"), ("mkxp.json", b"{")]),
        ];
        for (cut, left) in cuts {
            install(&dir, &Source::folder(repo.clone(), None).unwrap(), Some(&cat)).unwrap();
            let mut record: serde_json::Value = serde_json::from_str(&fs::read_to_string(&record_file).unwrap()).unwrap();
            record["pending"] = serde_json::Value::Bool(true);
            fs::write(&record_file, record.to_string()).unwrap();
            fs::remove_file(installed_file(&dir)).unwrap();
            for name in ["Game (PokeAccess).exe", "font.sf2", "mkxp.json", "mkxp.json.access.bak"] {
                fs::remove_file(dir.join(name)).unwrap();
            }
            tree(&dir, &left);
            let done = install(&dir, &Source::folder(repo.clone(), None).unwrap(), Some(&cat)).unwrap();
            assert_eq!(done.converted.as_deref(), Some("Game (PokeAccess).exe"), "{cut}");
            assert_eq!(fs::read(dir.join("Game (PokeAccess).exe")).unwrap(), engine, "{cut}");
            assert_eq!(fs::read(dir.join("font.sf2")).unwrap(), b"sf2", "{cut}");
            let json = fs::read_to_string(dir.join("mkxp.json")).unwrap();
            assert!(super::super::mkxp::is_registered(&json) && json.contains("\"execName\": \"Game\""), "{cut}: {json}");
            let finished = super::super::convert::read_manifest(&dir).unwrap();
            assert!(!finished.pending && finished.added.contains_key("font.sf2"), "{cut}");
            assert!(super::super::installed::is_installed(&dir), "{cut}");
        }
        run_uninstall(&dir, false).unwrap();
        assert_eq!(left_in(&dir), ["Game.exe", "Game.ini"]);
    }

    #[test]
    fn a_converted_game_moves_to_the_engine_the_catalog_names_now_and_one_gone_from_the_mod_is_left_alone() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        mod_folder(&repo);
        engine_bundle(&repo, "e1", b"MZ e1 preloadScript", &[("old.sf2", b"old"), ("fluidsynth.dll", b"fs1")]);
        engine_bundle(&repo, "e2", b"MZ e2 preloadScript", &[("new.sf2", b"new"), ("fluidsynth.dll", b"fs2")]);
        let dir = insurgence(root.path());
        let access = dir.join("Game (PokeAccess).exe");
        let run = |engine: &str| {
            install(&dir, &Source::folder(repo.clone(), None).unwrap(), Some(&insurgence_catalog(engine))).unwrap()
        };
        assert!(run("e1").converted.is_some());
        fs::remove_file(dir.join("old.sf2")).unwrap();
        let repaired = run("e1");
        assert_eq!((repaired.files, repaired.engine_note.as_deref()), (1, None), "solo vuelve el extra que falta");
        assert_eq!(fs::read(dir.join("old.sf2")).unwrap(), b"old");

        let moved = run("e2");
        assert_eq!(moved.engine_note, Some(crate::i18n::err_key("convert_migrated", "e2")));
        assert!(moved.converted.is_none(), "el exe accesible se llama igual: la lista no cambia");
        assert_eq!(fs::read(&access).unwrap(), b"MZ e2 preloadScript");
        assert!(!dir.join("old.sf2").exists(), "el extra que el motor nuevo no trae se va");
        assert_eq!((fs::read(dir.join("new.sf2")).unwrap(), fs::read(dir.join("fluidsynth.dll")).unwrap()),
            (b"new".to_vec(), b"fs2".to_vec()));
        let json = fs::read_to_string(dir.join("mkxp.json")).unwrap();
        assert!(json.contains("\"midiSoundFont\": \"new.sf2\"") && super::super::mkxp::is_registered(&json), "{json}");
        let record = super::super::convert::read_manifest(&dir).unwrap();
        assert_eq!((record.engine.as_str(), record.profile.as_str()), ("e2", "insurgence"));

        assert!(run("e3").engine_note.is_none(), "sin el motor nuevo en el mod sigue con el suyo");
        fs::remove_dir_all(repo.join("assets").join("engine").join("e2")).unwrap();
        let gone = run("e2");
        assert_eq!(gone.engine_note, Some(crate::i18n::err_key("convert_engine_gone", "e2")));
        assert_eq!(fs::read(&access).unwrap(), b"MZ e2 preloadScript");
        assert_eq!(super::super::convert::read_manifest(&dir).unwrap(), record, "ni el motor ni su registro se tocan");
        run_uninstall(&dir, false).unwrap();
        assert_eq!(left_in(&dir), ["Game.exe", "Game.ini"]);
    }
}
