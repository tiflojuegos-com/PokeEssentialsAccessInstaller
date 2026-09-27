use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};

use super::catalog::Catalog;
use super::convert::{ENGINE_FILE, ENGINE_ROOT, EXTRAS_DIR};
use super::install::git_blob_sha1;
use super::installed::{parse_version_json, ModVersion};
use crate::i18n::err_key;

const MAX_READS: usize = 8;

const MOD_MARKS: [&str; 4] = ["version.json", "core/manifest.rb", "games/catalog.json", "loader/preload_access.rb"];

#[derive(Debug, Clone, PartialEq)]
pub struct ContentEntry {
    pub path: String,
    pub sha: String,
}

pub struct Source {
    origin: Origin,
    version: OnceLock<Result<Option<ModVersion>, String>>,
    slots: Slots,
    stopped: AtomicBool,
}

enum Origin {
    GitHub {
        tree: OnceLock<Result<Vec<ContentEntry>, String>>,
    },
    Folder {
        root: PathBuf,
        engine: Option<PathBuf>,
        walked: Mutex<HashMap<String, Vec<ContentEntry>>>,
    },
}

impl Source {
    pub fn github() -> Source {
        Source::new(Origin::GitHub { tree: OnceLock::new() })
    }

    pub fn folder(root: PathBuf, engine: Option<PathBuf>) -> Result<Source, String> {
        if let Some(mark) = MOD_MARKS.iter().map(|m| on_disk(&root, m)).find(|p| !p.is_file()) {
            return Err(err_key("err_not_mod_folder", &mark.display().to_string()));
        }
        if let Some(exe) = engine.as_ref().map(|e| e.join(ENGINE_FILE)).filter(|p| !p.is_file()) {
            return Err(err_key("err_not_engine_folder", &exe.display().to_string()));
        }
        Ok(Source::new(Origin::Folder { root, engine, walked: Mutex::default() }))
    }

    fn new(origin: Origin) -> Source {
        Source { origin, version: OnceLock::new(), slots: Slots::new(MAX_READS), stopped: AtomicBool::new(false) }
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }

    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    pub fn engine_dir(&self) -> Option<&Path> {
        match &self.origin {
            Origin::Folder { engine, .. } => engine.as_deref(),
            Origin::GitHub { .. } => None,
        }
    }

    pub fn mod_version(&self) -> Result<Option<ModVersion>, String> {
        self.version
            .get_or_init(|| {
                let bytes = match &self.origin {
                    Origin::GitHub { .. } => super::github::download_bytes(&super::paths::raw_url("version.json"))?,
                    Origin::Folder { root, .. } => read_file(&on_disk(root, "version.json"), "version.json")?,
                };
                Ok(parse_version_json(&String::from_utf8_lossy(&bytes)))
            })
            .clone()
    }

    pub fn available_version(&self) -> String {
        self.mod_version().ok().flatten().map(|m| m.version).unwrap_or_default()
    }

    pub fn catalog(&self) -> Result<Catalog, String> {
        match &self.origin {
            Origin::GitHub { .. } => Catalog::fetch(),
            Origin::Folder { root, .. } => {
                let bytes = read_file(&on_disk(root, "games/catalog.json"), "games/catalog.json")?;
                Catalog::from_json(&String::from_utf8_lossy(&bytes))
            }
        }
    }

    pub fn list(&self, prefixes: &[String]) -> Result<Vec<ContentEntry>, String> {
        match &self.origin {
            Origin::GitHub { tree } => {
                let tree = tree.get_or_init(super::github::fetch_tree).as_ref().map_err(String::clone)?;
                let mut listed: Vec<ContentEntry> =
                    tree.iter().filter(|e| under_prefix(&e.path, prefixes)).cloned().collect();
                listed.sort_by(|a, b| a.path.cmp(&b.path));
                Ok(listed)
            }
            Origin::Folder { root, engine, walked } => {
                let mut walked = walked.lock().unwrap_or_else(|e| e.into_inner());
                let mut listed = BTreeMap::new();
                for prefix in prefixes {
                    if !walked.contains_key(prefix) {
                        let found = walk_prefix(root, engine.as_deref(), prefix)?;
                        walked.insert(prefix.clone(), found);
                    }
                    for e in &walked[prefix] {
                        listed.insert(e.path.clone(), e.clone());
                    }
                }
                Ok(listed.into_values().collect())
            }
        }
    }

    pub fn read(&self, entry: &ContentEntry) -> Result<Vec<u8>, String> {
        let _slot = self.slots.take();
        if self.stopped() {
            return Err("run_cancelled".to_string());
        }
        match &self.origin {
            Origin::GitHub { .. } => super::github::download_bytes(&super::paths::raw_url(&entry.path)),
            Origin::Folder { root, engine, .. } => {
                let file = match (engine, bundle_file(&entry.path)) {
                    (Some(engine), Some(rest)) => on_disk(engine, rest),
                    _ => on_disk(root, &entry.path),
                };
                read_file(&file, &entry.path)
            }
        }
    }
}

pub fn is_mod_folder(root: &Path) -> bool {
    MOD_MARKS.iter().all(|mark| on_disk(root, mark).is_file())
}

fn under_prefix(path: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|p| path == p || path.strip_prefix(p.as_str()).is_some_and(|rest| rest.starts_with('/')))
}

fn on_disk(base: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(base.to_path_buf(), |path, part| path.join(part))
}

fn bundle_file(path: &str) -> Option<&str> {
    let (_, rest) = path.strip_prefix(ENGINE_ROOT)?.strip_prefix('/')?.split_once('/')?;
    Some(rest)
}

fn is_bundle(prefix: &str) -> bool {
    prefix
        .strip_prefix(ENGINE_ROOT)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|id| !id.is_empty() && !id.contains('/'))
}

fn walk_prefix(root: &Path, engine: Option<&Path>, prefix: &str) -> Result<Vec<ContentEntry>, String> {
    let mut found = Vec::new();
    match engine {
        Some(engine) if is_bundle(prefix) => {
            add_file(&engine.join(ENGINE_FILE), format!("{}/{}", prefix, ENGINE_FILE), &mut found)?;
            for (name, file) in files_in(&engine.join(EXTRAS_DIR))? {
                add_file(&file, format!("{}/{}/{}", prefix, EXTRAS_DIR, name), &mut found)?;
            }
        }
        _ => walk(&on_disk(root, prefix), prefix, &mut found)?,
    }
    Ok(found)
}

fn walk(path: &Path, rel: &str, found: &mut Vec<ContentEntry>) -> Result<(), String> {
    if path.is_file() {
        return add_file(path, rel.to_string(), found);
    }
    for entry in read_dir(path, rel)? {
        let child = entry.path();
        walk(&child, &format!("{}/{}", rel, entry.file_name().to_string_lossy()), found)?;
    }
    Ok(())
}

fn files_in(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let rel = dir.display().to_string();
    Ok(read_dir(dir, &rel)?
        .into_iter()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .filter(|(_, p)| p.is_file())
        .collect())
}

fn read_dir(dir: &Path, rel: &str) -> Result<Vec<fs::DirEntry>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    fs::read_dir(dir)
        .and_then(|entries| entries.collect::<io::Result<Vec<_>>>())
        .map_err(|e| err_key("err_io_read", &format!("{} ({})", rel, e)))
}

fn add_file(path: &Path, rel: String, found: &mut Vec<ContentEntry>) -> Result<(), String> {
    if path.is_file() {
        let sha = git_blob_sha1(&read_file(path, &rel)?);
        found.push(ContentEntry { path: rel, sha });
    }
    Ok(())
}

fn read_file(path: &Path, rel: &str) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| err_key("err_io_read", &format!("{} ({})", rel, e)))
}

struct Slots {
    free: Mutex<usize>,
    freed: Condvar,
}

struct Slot<'a>(&'a Slots);

impl Slots {
    fn new(count: usize) -> Slots {
        Slots { free: Mutex::new(count), freed: Condvar::new() }
    }

    fn take(&self) -> Slot<'_> {
        let mut free = self.free.lock().unwrap_or_else(|e| e.into_inner());
        while *free == 0 {
            free = self.freed.wait(free).unwrap_or_else(|e| e.into_inner());
        }
        *free -= 1;
        Slot(self)
    }
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        *self.0.free.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        self.0.freed.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(root: &Path, files: &[(&str, &str)]) {
        for (rel, text) in files {
            let path = on_disk(root, rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    }

    fn mod_folder(root: &Path) {
        tree(root, &[
            ("version.json", "{\"version\": \"0.6.0\", \"min_launcher\": \"0.2.0\"}"),
            ("core/manifest.rb", "hello\n"),
            ("core/nav/locator.rb", "loc"),
            ("games/catalog.json", r#"{"profiles":[{"key":"anil","display":"Pokemon Anil","titles":[],"exes":[]}]}"#),
            ("games/anil/manifest.rb", "{ :imports => %w[anil_common] }"),
            ("games/anil_common/extra.rb", "x"),
            ("games/opalo/menus.rb", "o"),
            ("loader/preload_access.rb", "p"),
            ("assets/engine/e1/mkxp-z.exe", "MZ repo engine"),
            ("launcher/target/release/big.pdb", "pdb"),
            ("test/run_all.rb", "t"),
        ]);
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn paths(entries: &[ContentEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.path.as_str()).collect()
    }

    #[test]
    fn a_folder_lists_what_is_asked_once_each_with_the_blob_id_git_gives() {
        let dir = tempfile::tempdir().unwrap();
        mod_folder(dir.path());
        let source = Source::folder(dir.path().to_path_buf(), None).unwrap();
        let listed = source.list(&strings(&["core", "games", "games/anil", "loader", "assets/x64"])).unwrap();
        assert_eq!(
            paths(&listed),
            vec![
                "core/manifest.rb", "core/nav/locator.rb", "games/anil/manifest.rb", "games/anil_common/extra.rb",
                "games/catalog.json", "games/opalo/menus.rb", "loader/preload_access.rb",
            ]
        );
        assert_eq!(listed[0].sha, "ce013625030ba8dba906f756967f9e9ca394464a", "git hash-object de \"hello\\n\"");
        assert_eq!(source.read(&listed[1]).unwrap(), b"loc");
    }

    #[test]
    fn a_run_lists_a_folder_once_and_reads_it_as_it_is_now() {
        let dir = tempfile::tempdir().unwrap();
        mod_folder(dir.path());
        let source = Source::folder(dir.path().to_path_buf(), None).unwrap();
        let first = source.list(&strings(&["core"])).unwrap();
        fs::write(on_disk(dir.path(), "core/nav/locator.rb"), "changed").unwrap();
        fs::write(on_disk(dir.path(), "core/new.rb"), "new").unwrap();
        assert_eq!(source.list(&strings(&["core"])).unwrap(), first, "la lista es la de la primera vez");
        assert_eq!(source.read(&first[1]).unwrap(), b"changed");
        let next_run = Source::folder(dir.path().to_path_buf(), None).unwrap();
        assert_eq!(next_run.list(&strings(&["core"])).unwrap().len(), 3);
    }

    #[test]
    fn an_engine_folder_stands_in_for_every_bundle_with_its_engine_and_extras_only() {
        let dir = tempfile::tempdir().unwrap();
        mod_folder(dir.path());
        let engine = dir.path().join("build");
        tree(&engine, &[("mkxp-z.exe", "MZ local engine"), ("extras/fluidsynth.dll", "fs"), ("obj/x.obj", "o"),
            ("extras/sub/deep.txt", "d")]);
        let source = Source::folder(dir.path().to_path_buf(), Some(engine)).unwrap();
        let listed = source.list(&strings(&["assets/engine/e1", "assets/engine/local"])).unwrap();
        assert_eq!(
            paths(&listed),
            vec![
                "assets/engine/e1/extras/fluidsynth.dll", "assets/engine/e1/mkxp-z.exe",
                "assets/engine/local/extras/fluidsynth.dll", "assets/engine/local/mkxp-z.exe",
            ]
        );
        assert_eq!(source.read(&listed[1]).unwrap(), b"MZ local engine");
        assert_eq!(listed[1].sha, git_blob_sha1(b"MZ local engine"));
        assert!(source.engine_dir().is_some());
    }

    #[test]
    fn only_a_folder_with_the_mod_and_an_engine_folder_with_its_engine_are_taken() {
        let dir = tempfile::tempdir().unwrap();
        mod_folder(dir.path());
        assert!(is_mod_folder(dir.path()));
        fs::remove_file(on_disk(dir.path(), "loader/preload_access.rb")).unwrap();
        assert!(!is_mod_folder(dir.path()));
        let err = Source::folder(dir.path().to_path_buf(), None).err().unwrap();
        assert!(err.starts_with("err_not_mod_folder\u{1}") && err.ends_with("preload_access.rb"), "{err}");
        mod_folder(dir.path());
        let err = Source::folder(dir.path().to_path_buf(), Some(dir.path().join("core"))).err().unwrap();
        assert!(err.starts_with("err_not_engine_folder\u{1}") && err.ends_with("mkxp-z.exe"), "{err}");
        let shown = crate::i18n::I18n::new("es").t_err(&err);
        assert!(shown.contains("mkxp-z.exe") && !shown.contains("err_not_engine_folder"), "{shown}");
    }

    #[test]
    fn a_stopped_run_reads_no_more_files() {
        let dir = tempfile::tempdir().unwrap();
        mod_folder(dir.path());
        let source = Source::folder(dir.path().to_path_buf(), None).unwrap();
        let listed = source.list(&strings(&["core"])).unwrap();
        assert!(source.read(&listed[0]).is_ok() && !source.stopped());
        source.stop();
        assert!(source.stopped());
        assert_eq!(source.read(&listed[0]), Err("run_cancelled".to_string()));
    }

    #[test]
    fn a_folder_gives_its_own_version_and_catalog() {
        let dir = tempfile::tempdir().unwrap();
        mod_folder(dir.path());
        let source = Source::folder(dir.path().to_path_buf(), None).unwrap();
        assert_eq!(source.available_version(), "0.6.0");
        assert_eq!(source.mod_version().unwrap().unwrap().min_launcher, "0.2.0");
        assert_eq!(source.catalog().unwrap().display_of("anil"), "Pokemon Anil");
    }

    #[test]
    fn github_serves_every_listing_of_a_run_from_the_one_tree_it_fetched() {
        let blob = |path: &str| ContentEntry { path: path.to_string(), sha: format!("sha of {path}") };
        let tree = vec![blob("games/anil/menus.rb"), blob("core/x.rb"), blob("README.md"), blob("games/opalo/m.rb")];
        let source = Source::new(Origin::GitHub { tree: OnceLock::from(Ok(tree)) });
        let listed = source.list(&strings(&["games/anil", "core", "games"])).unwrap();
        assert_eq!(paths(&listed), vec!["core/x.rb", "games/anil/menus.rb", "games/opalo/m.rb"]);
        assert_eq!(listed[0].sha, "sha of core/x.rb");
        assert!(source.engine_dir().is_none());
        let offline = Source::new(Origin::GitHub { tree: OnceLock::from(Err("err_tree_truncated".to_string())) });
        assert_eq!(offline.list(&strings(&["core"])), Err("err_tree_truncated".to_string()));
    }

    #[test]
    fn the_whole_tree_is_filtered_by_folder_never_by_a_partial_name() {
        let prefixes = strings(&["core", "games/pokemon_z"]);
        assert!(under_prefix("core/x.rb", &prefixes));
        assert!(under_prefix("core", &prefixes));
        assert!(under_prefix("games/pokemon_z/menu.rb", &prefixes));
        assert!(!under_prefix("corefoo/x.rb", &prefixes));
        assert!(!under_prefix("games/pokemon_zeta/menu.rb", &prefixes));
    }

    #[test]
    fn only_a_bundle_folder_is_served_by_the_engine_folder() {
        assert!(is_bundle("assets/engine/mkxp-z-1.3.0-x86"));
        assert!(!is_bundle("assets/engine"));
        assert!(!is_bundle("assets/engine/"));
        assert!(!is_bundle("assets/engineer/x"));
        assert!(!is_bundle("assets/engine/a/b"));
        assert_eq!(bundle_file("assets/engine/e/extras/sf.sf2"), Some("extras/sf.sf2"));
        assert_eq!(bundle_file("assets/x64/PA3D.dll"), None);
    }

    #[test]
    fn no_more_than_the_slots_are_taken_at_once() {
        let slots = Slots::new(2);
        let busy = std::sync::atomic::AtomicUsize::new(0);
        let most = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..6 {
                s.spawn(|| {
                    let _slot = slots.take();
                    let now = busy.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    most.fetch_max(now, std::sync::atomic::Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    busy.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                });
            }
        });
        assert_eq!(most.into_inner(), 2);
    }
}
