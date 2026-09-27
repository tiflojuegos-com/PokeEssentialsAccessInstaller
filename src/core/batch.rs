use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use super::apply::{self, InstallOutcome};
use super::catalog::Catalog;
use super::config::{Config, GameEntry};
use super::ops::{self, Inspection};
use super::source::Source;
use crate::i18n::I18n;

pub const WINDOW_THREADS: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub dir: PathBuf,
    pub profile: String,
    pub profile_mode: String,
    pub name: String,
}

impl Job {
    pub fn of(entry: &GameEntry, name: &str) -> Job {
        Job {
            dir: PathBuf::from(&entry.path),
            profile: entry.profile.clone(),
            profile_mode: entry.profile_mode.clone(),
            name: name.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Outcome {
    Installed(InstallOutcome),
    Skipped,
    Failed(String),
}

pub enum Event<'a> {
    Started(usize),
    Progress(usize, &'a str, u32, u32),
    Finished(usize, &'a Outcome),
}

#[derive(Debug, Default, PartialEq)]
pub struct Summary {
    pub games: usize,
    pub installed: usize,
    pub updated: usize,
    pub current: usize,
    pub skipped: usize,
    pub failed: usize,
}

impl Summary {
    pub fn of(outcomes: &[Outcome]) -> Summary {
        let mut s = Summary { games: outcomes.len(), ..Summary::default() };
        for outcome in outcomes {
            match outcome {
                Outcome::Installed(o) if o.previous.is_empty() => s.installed += 1,
                Outcome::Installed(o) if o.changed() => s.updated += 1,
                Outcome::Installed(_) => s.current += 1,
                Outcome::Skipped => s.skipped += 1,
                Outcome::Failed(_) => s.failed += 1,
            }
        }
        s
    }

    pub fn line(&self, i18n: &I18n) -> String {
        let counts = [
            ("summary_games", self.games),
            ("summary_installed", self.installed),
            ("summary_updated", self.updated),
            ("summary_current", self.current),
            ("summary_skipped", self.skipped),
            ("summary_failed", self.failed),
        ];
        let parts: Vec<String> =
            counts.iter().filter(|(_, n)| *n > 0).map(|(key, n)| i18n.tf(key, &n.to_string())).collect();
        i18n.tf("summary", &parts.join(". "))
    }
}

pub fn run(
    jobs: &[Job],
    source: &Source,
    cat: Option<&Catalog>,
    threads: usize,
    on_event: impl Fn(Event<'_>) + Sync,
) -> Vec<Outcome> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| format!("unix:{}", d.as_secs()))
        .unwrap_or_default();
    let next = AtomicUsize::new(0);
    let mut outcomes: Vec<Option<Outcome>> = jobs.iter().map(|_| None).collect();
    thread::scope(|s| {
        let workers: Vec<_> = (0..threads.clamp(1, jobs.len().max(1)))
            .map(|_| {
                s.spawn(|| {
                    let mut finished = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::SeqCst);
                        let Some(job) = jobs.get(i) else { break };
                        on_event(Event::Started(i));
                        let outcome = install(job, source, cat, &now, |file, done, total| {
                            on_event(Event::Progress(i, file, done, total))
                        });
                        on_event(Event::Finished(i, &outcome));
                        finished.push((i, outcome));
                    }
                    finished
                })
            })
            .collect();
        for worker in workers {
            for (i, outcome) in worker.join().unwrap_or_default() {
                outcomes[i] = Some(outcome);
            }
        }
    });
    outcomes.into_iter().map(|o| o.unwrap_or_else(|| Outcome::Failed("worker_crashed".to_string()))).collect()
}

fn install(
    job: &Job,
    source: &Source,
    cat: Option<&Catalog>,
    now: &str,
    progress: impl FnMut(&str, u32, u32),
) -> Outcome {
    if source.stopped() {
        return Outcome::Failed("run_cancelled".to_string());
    }
    if !job.dir.is_dir() {
        return Outcome::Skipped;
    }
    let game = Inspection::of(job.dir.clone(), cat, source);
    if let Err(running) = game.game.check_closed() {
        return Outcome::Failed(running.message());
    }
    match apply::run_install(&game, &job.profile, &job.profile_mode, source, cat, now, progress) {
        Ok(installed) => Outcome::Installed(installed),
        Err(e) => Outcome::Failed(e),
    }
}

pub fn installed_jobs(cfg: &Config) -> Vec<Job> {
    cfg.games
        .iter()
        .zip(super::game::list_names(&cfg.games))
        .filter(|(e, _)| super::installed::is_installed(&PathBuf::from(&e.path)))
        .map(|(e, name)| Job::of(e, &name))
        .collect()
}

pub fn adopt(cfg: &mut Config, cat: Option<&Catalog>, jobs: &[Job], outcomes: &[Outcome]) -> Vec<String> {
    let installed: Vec<(&Job, &InstallOutcome)> = jobs
        .iter()
        .zip(outcomes)
        .filter_map(|(job, outcome)| match outcome {
            Outcome::Installed(o) => Some((job, o)),
            _ => None,
        })
        .collect();
    let mut notices = Vec::new();
    for (job, o) in &installed {
        if let Some(exe) = &o.converted {
            cfg.set_executable(&job.dir.to_string_lossy(), exe);
            notices.push(crate::i18n::err_key("convert_play_hint", exe));
        }
    }
    for (job, o) in &installed {
        if let Some(profile) = &o.kept_profile {
            cfg.set_profile(&job.dir.to_string_lossy(), profile);
            notices.push(crate::i18n::err_key("convert_profile_kept", &ops::profile_title(cat, profile)));
        }
    }
    notices.extend(installed.iter().filter_map(|(_, o)| o.engine_note.clone()));
    notices
}

pub fn long_path_notices(jobs: &[Job]) -> Vec<String> {
    jobs.iter().filter_map(|job| super::convert::long_path_notice(&job.dir)).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;
    use std::sync::Mutex;

    use super::*;
    use crate::i18n::I18n;

    fn source(root: &Path) -> Source {
        for (rel, text) in [
            ("version.json", "{\"version\": \"0.6.0\"}"),
            ("core/manifest.rb", "core"),
            ("core/nav/locator.rb", "nav"),
            ("games/catalog.json", r#"{"profiles":[{"key":"anil","display":"Pokemon Anil"}]}"#),
            ("games/anil/manifest.rb", "{ :modules => %w[] }"),
            ("loader/preload_access.rb", "loader"),
            ("loader/boot.rb", "boot"),
        ] {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        Source::folder(root.to_path_buf(), None).unwrap()
    }

    fn game(root: &Path, name: &str) -> Job {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Game.exe"), b"MZ preloadScript").unwrap();
        Job { dir, profile: "anil".into(), profile_mode: "specific".into(), name: name.into() }
    }

    fn installed(o: &Outcome) -> &InstallOutcome {
        match o {
            Outcome::Installed(i) => i,
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn each_game_gets_its_outcome_in_the_list_order_and_one_failing_stops_none() {
        let root = tempfile::tempdir().unwrap();
        let src = source(&root.path().join("mod"));
        let mut jobs = vec![game(root.path(), "A"), game(root.path(), "gone"), game(root.path(), "B")];
        fs::remove_dir_all(&jobs[1].dir).unwrap();
        let odd = root.path().join("odd");
        fs::create_dir_all(&odd).unwrap();
        jobs.push(Job { dir: odd, ..jobs[0].clone() });
        let events = Mutex::new(Vec::new());
        let outcomes = run(&jobs, &src, None, 3, |e| {
            let seen = match e {
                Event::Started(i) => format!("start {i}"),
                Event::Progress(i, _, done, total) => format!("file {i} {done}/{total}"),
                Event::Finished(i, _) => format!("end {i}"),
            };
            events.lock().unwrap().push(seen);
        });
        assert_eq!(installed(&outcomes[0]).files, 5);
        assert!(matches!(outcomes[1], Outcome::Skipped));
        assert_eq!(installed(&outcomes[2]).version, "0.6.0");
        assert!(matches!(&outcomes[3], Outcome::Failed(e) if e == "not_compatible"));
        let expected = Summary { games: 4, installed: 2, skipped: 1, failed: 1, ..Summary::default() };
        assert_eq!(Summary::of(&outcomes), expected);
        let events = events.into_inner().unwrap();
        for i in 0..4 {
            assert_eq!(events.iter().filter(|e| **e == format!("start {i}")).count(), 1);
            assert_eq!(events.iter().filter(|e| **e == format!("end {i}")).count(), 1);
        }
        assert!(events.contains(&"file 0 5/5".to_string()));
        let again = run(&jobs[..1], &src, None, 3, |_| {});
        assert_eq!(installed(&again[0]).files, 0);
        assert_eq!(Summary::of(&again), Summary { games: 1, current: 1, ..Summary::default() });
    }

    #[test]
    fn a_stopped_run_starts_no_game_and_its_summary_names_only_what_happened() {
        let root = tempfile::tempdir().unwrap();
        let src = source(&root.path().join("mod"));
        let jobs = vec![game(root.path(), "A"), game(root.path(), "B")];
        src.stop();
        let outcomes = run(&jobs, &src, None, 2, |_| {});
        assert!(outcomes.iter().all(|o| matches!(o, Outcome::Failed(e) if e == "run_cancelled")));
        assert!(!jobs[0].dir.join("accessibility").exists());
        assert_eq!(Summary::of(&outcomes).line(&I18n::new("es")), "Resumen. Juegos: 2. Con error: 2.");
    }

    #[test]
    fn no_more_games_than_the_threads_run_at_once() {
        let root = tempfile::tempdir().unwrap();
        let src = source(&root.path().join("mod"));
        let jobs: Vec<Job> = (0..6).map(|i| game(root.path(), &format!("G{i}"))).collect();
        let busy = AtomicUsize::new(0);
        let most = AtomicUsize::new(0);
        let outcomes = run(&jobs, &src, None, 2, |e| match e {
            Event::Started(_) => {
                let now = busy.fetch_add(1, Ordering::SeqCst) + 1;
                most.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
            Event::Finished(..) => {
                busy.fetch_sub(1, Ordering::SeqCst);
            }
            Event::Progress(..) => {}
        });
        assert_eq!(outcomes.len(), 6);
        assert!(outcomes.iter().all(|o| matches!(o, Outcome::Installed(_))));
        assert_eq!(most.into_inner(), 2);
    }

    #[cfg(windows)]
    #[test]
    fn an_open_game_fails_alone_and_says_which_one_it_is() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = tempfile::tempdir().unwrap();
        let src = source(&root.path().join("mod"));
        let jobs = vec![game(root.path(), "Relict"), game(root.path(), "Anil")];
        let _open = fs::OpenOptions::new().read(true).share_mode(1).open(jobs[0].dir.join("Game.exe")).unwrap();
        let outcomes = run(&jobs, &src, None, 2, |_| {});
        match &outcomes[0] {
            Outcome::Failed(e) => assert_eq!(I18n::new("es").t_err(e), I18n::new("es").tf("game_running", "Relict")),
            other => panic!("{:?}", other),
        }
        assert!(matches!(outcomes[1], Outcome::Installed(_)));
    }

    #[test]
    fn only_the_listed_games_with_the_mod_are_updated_and_by_their_list_name() {
        let root = tempfile::tempdir().unwrap();
        let entry = |dir: &Path, name: &str| GameEntry {
            path: dir.to_string_lossy().into_owned(),
            name: name.into(),
            executable: String::new(),
            profile: "anil".into(),
            profile_mode: "specific".into(),
        };
        let with_mod = root.path().join("A");
        apply::seal_installed(&with_mod, "0.5.0", "anil", "specific", "x86", "now", BTreeMap::new()).unwrap();
        let without = root.path().join("B");
        fs::create_dir_all(&without).unwrap();
        let mut cfg = Config::default();
        cfg.games = vec![entry(&without, "Anil"), entry(&with_mod, "Anil"), entry(&root.path().join("C"), "C")];
        let jobs = installed_jobs(&cfg);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].dir, with_mod);
        assert!(jobs[0].name.starts_with("Anil ("), "{}", jobs[0].name);
    }

    #[test]
    fn a_conversion_points_its_record_at_the_accessible_exe_and_keeps_its_profile() {
        let conversion = InstallOutcome {
            version: "0.6.0".into(),
            previous: String::new(),
            files: 3,
            removed: 0,
            converted: Some("Game (PokeAccess).exe".into()),
            kept_profile: Some("insurgence".into()),
            engine_note: None,
        };
        let job = |path: &str| Job {
            dir: PathBuf::from(path),
            profile: "generic".into(),
            profile_mode: "generic".into(),
            name: path.into(),
        };
        let jobs = [job("D:/Juegos/Insurgence"), job("D:/Juegos/Roto")];
        let mut cfg = Config::default();
        for j in &jobs {
            cfg.upsert_game(GameEntry {
                path: j.dir.to_string_lossy().into_owned(),
                name: String::new(),
                executable: "Game.exe".into(),
                profile: "generic".into(),
                profile_mode: "generic".into(),
            });
        }
        let notices = adopt(&mut cfg, None, &jobs, &[Outcome::Installed(conversion), Outcome::Failed("x".into())]);
        assert_eq!(cfg.games[0].executable, "Game (PokeAccess).exe");
        assert_eq!((cfg.games[0].profile.as_str(), cfg.games[0].profile_mode.as_str()), ("insurgence", "specific"));
        assert_eq!(cfg.games[1].executable, "Game.exe");
        let es = I18n::new("es");
        let shown: Vec<String> = notices.iter().map(|n| es.t_err(n)).collect();
        assert_eq!(shown, vec![es.tf("convert_play_hint", "Game (PokeAccess).exe"), es.tf("convert_profile_kept", "insurgence")]);
    }
}
