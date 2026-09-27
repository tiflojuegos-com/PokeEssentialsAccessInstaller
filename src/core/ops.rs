use std::path::{Path, PathBuf};

use super::catalog::{Catalog, Profile};
use super::config::GameEntry;
use super::convert::{self, Manifest, Offer, RgssPlayer};
use super::detect::{self, ExeScan};
use super::source::Source;
use crate::i18n::err_key;

pub const LOCAL_ENGINE: &str = "local";

pub struct ScannedGame {
    pub dir: PathBuf,
    pub scan: ExeScan,
}

impl ScannedGame {
    pub fn of(dir: PathBuf) -> ScannedGame {
        let scan = detect::scan_exes(&dir);
        ScannedGame { dir, scan }
    }

    pub fn name(&self) -> String {
        self.dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.dir.to_string_lossy().to_string())
    }

    pub fn check_closed(&self) -> Result<(), Blocker> {
        if detect::folder_running(&self.dir) {
            Err(Blocker::GameRunning(self.name()))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Blocker {
    FolderMissing(PathBuf),
    NoWritePermission,
    GameRunning(String),
    Incompatible(String),
    ConversionImpossible(String),
}

impl Blocker {
    pub fn message(&self) -> String {
        match self {
            Blocker::FolderMissing(dir) => err_key("game_folder_missing", &dir.display().to_string()),
            Blocker::NoWritePermission => "no_write_perm".to_string(),
            Blocker::GameRunning(name) => err_key("game_running", name),
            Blocker::Incompatible(reason) | Blocker::ConversionImpossible(reason) => reason.clone(),
        }
    }
}

pub fn game_dir(entry: &GameEntry) -> Result<PathBuf, Blocker> {
    let dir = PathBuf::from(&entry.path);
    if dir.is_dir() {
        Ok(dir)
    } else {
        Err(Blocker::FolderMissing(dir))
    }
}

pub fn check_writable(dir: &Path) -> Result<(), Blocker> {
    if super::apply::can_write(dir) {
        Ok(())
    } else {
        Err(Blocker::NoWritePermission)
    }
}

pub struct Inspection {
    pub game: ScannedGame,
    pub has_mkxp_json: bool,
    pub record: Option<Manifest>,
    pub offer: Option<Offer>,
    pub player: Option<Result<RgssPlayer, String>>,
    pub sealed_profile: Option<String>,
    pub detected_profile: Option<String>,
    pub writable: bool,
}

impl Inspection {
    pub fn of(dir: PathBuf, cat: Option<&Catalog>, source: &Source) -> Inspection {
        let game = ScannedGame::of(dir);
        let has_mkxp_json = super::mkxp::has_mkxp_json(&game.dir);
        let detected = cat.and_then(|c| detected(c, &game));
        let offer = if has_mkxp_json || game.scan.supports_preload {
            None
        } else {
            cat.and_then(|c| convert::offer_for(c, &game.dir)).or_else(|| local_offer(&game, detected, source))
        };
        let player = offer.as_ref().map(|o| convert::check(&game.dir, o));
        let detected_profile = detected.map(|p| p.key.clone());
        let record = convert::read_manifest(&game.dir);
        let sealed_profile = super::installed::sealed_profile(&game.dir);
        let writable = super::apply::can_write(&game.dir);
        Inspection { game, has_mkxp_json, record, offer, player, sealed_profile, detected_profile, writable }
    }

    pub fn native(&self) -> bool {
        self.has_mkxp_json || self.game.scan.supports_preload
    }

    pub fn pending(&self) -> bool {
        self.record.as_ref().is_some_and(|m| m.pending)
    }

    pub fn incompatible(&self) -> Option<String> {
        (!self.native() && self.offer.is_none() && !self.pending()).then(|| convert::incompatible(&self.game.dir))
    }

    pub fn blocker(&self) -> Option<Blocker> {
        if let Some(reason) = self.incompatible() {
            return Some(Blocker::Incompatible(reason));
        }
        if !self.writable {
            return Some(Blocker::NoWritePermission);
        }
        if let Err(running) = self.game.check_closed() {
            return Some(running);
        }
        match &self.player {
            Some(Err(reason)) => Some(Blocker::ConversionImpossible(reason.clone())),
            _ => None,
        }
    }

    pub fn doubtful(&self) -> bool {
        self.has_mkxp_json && !self.game.scan.supports_preload && self.record.is_none()
    }

    pub fn suggested_profile(&self) -> Option<&str> {
        self.sealed_profile.as_deref().or(self.detected_profile.as_deref())
    }

    pub fn kept_profile(&self, asked: &str) -> Option<String> {
        self.offer
            .as_ref()
            .map(|o| o.profile.clone())
            .or_else(|| self.record.as_ref().map(|m| m.profile.clone()))
            .filter(|p| !p.trim().is_empty() && p != asked)
    }

    pub fn long_path_notice(&self) -> Option<String> {
        convert::path_notice(convert::path_len(&self.game.dir), self.record.is_some() || self.offer.is_some())
    }

    pub fn report(&self, cat: Option<&Catalog>) -> Report {
        let player = convert::rgss_player(&self.game.dir).filter(|_| !self.native());
        let mut facts = vec![if self.has_mkxp_json { "check_json_yes" } else { "check_json_no" }.to_string()];
        facts.push(match &player {
            _ if self.native() => "check_engine_mkxp".to_string(),
            Some(p) => err_key("check_engine_player", &p.library),
            None => "check_engine_unknown".to_string(),
        });
        facts.push(match self.game.scan.main_exe.as_deref().filter(|_| self.game.scan.supports_preload) {
            Some(exe) => err_key("check_preload_yes", &exe.file_name().unwrap_or_default().to_string_lossy()),
            None => "check_preload_no".to_string(),
        });
        if let Some(sealed) = &self.sealed_profile {
            facts.push(err_key("check_profile_sealed", &profile_title(cat, sealed)));
        }
        match &self.detected_profile {
            Some(key) if self.sealed_profile.as_ref() != Some(key) => {
                facts.push(err_key("check_profile_detected", &profile_title(cat, key)))
            }
            None if self.sealed_profile.is_none() => facts.push("check_profile_none".to_string()),
            _ => {}
        }
        if player.is_some() {
            facts.push(match (&self.offer, &self.player) {
                (Some(_), Some(Err(reason))) => err_key("check_convert_blocked", reason),
                (Some(offer), _) => err_key("check_convert_yes", &offer.engine),
                (None, _) => "check_convert_no".to_string(),
            });
        }
        Report { facts, warning: self.long_path_notice(), verdict: self.verdict() }
    }

    fn verdict(&self) -> String {
        match self.blocker() {
            Some(Blocker::Incompatible(reason) | Blocker::ConversionImpossible(reason)) => {
                err_key("check_incompatible", &reason)
            }
            Some(other) => err_key("check_blocked", &other.message()),
            None if self.pending() => "check_pending".to_string(),
            None => match &self.player {
                Some(Ok(player)) => err_key("check_convertible", &convert::access_exe_name(player)),
                _ if self.record.is_some() && convert::missing_accessible_exe(&self.game.dir).is_some() => {
                    "check_exe_missing".to_string()
                }
                _ if self.doubtful() => "check_doubtful".to_string(),
                _ => "check_compatible".to_string(),
            },
        }
    }
}

pub struct Report {
    pub facts: Vec<String>,
    pub warning: Option<String>,
    pub verdict: String,
}

fn detected<'c>(cat: &'c Catalog, game: &ScannedGame) -> Option<&'c Profile> {
    let exe = game.scan.main_exe.as_deref();
    let titles = detect::game_titles(&game.dir);
    let hay = detect::folder_and_exe_string(&game.dir, exe);
    let exe_name = exe.and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
    cat.detect(&titles, &hay, exe_name.as_deref())
}

fn local_offer(game: &ScannedGame, detected: Option<&Profile>, source: &Source) -> Option<Offer> {
    source.engine_dir()?;
    convert::rgss_player(&game.dir)?;
    Some(Offer {
        engine: LOCAL_ENGINE.to_string(),
        markers: Vec::new(),
        profile: String::new(),
        display: detected.map(|p| p.display.clone()).unwrap_or_else(|| game.name()),
    })
}

pub fn profile_title(cat: Option<&Catalog>, key: &str) -> String {
    if key == "generic" {
        return "profile_generic".to_string();
    }
    cat.map(|c| c.display_of(key)).unwrap_or_else(|| key.to_string())
}

pub fn new_entry(game: &ScannedGame, profile: &str, profile_mode: &str, cat: Option<&Catalog>) -> GameEntry {
    let known = cat.map(|c| c.exes_of(profile)).unwrap_or_default();
    GameEntry {
        name: game.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        executable: super::game::pick_executable(&game.scan, &game.dir, &known).unwrap_or_default(),
        path: game.dir.to_string_lossy().to_string(),
        profile: profile.to_string(),
        profile_mode: profile_mode.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use super::*;
    use crate::i18n::I18n;

    fn catalog() -> Catalog {
        Catalog::from_json(
            r#"{"profiles":[
                {"key":"insurgence","display":"Pokemon Insurgence","titles":[],"detect":"insurgence","exes":[],
                 "convert":"e1","markers":["Data/Scripts.rxdata"]},
                {"key":"anil","display":"Pokemon Anil","titles":["pokemon anil"],"detect":"anil","exes":["Anil.exe"]},
                {"key":"generic","display":"Generico","titles":[],"exes":[]}
            ]}"#,
        )
        .unwrap()
    }

    fn player_folder(dir: &Path) {
        fs::write(dir.join("Game.exe"), b"MZ player").unwrap();
        fs::write(dir.join("Game.ini"), "[Game]\r\nLibrary=RGSS102E.dll\r\n").unwrap();
    }

    fn named(root: &Path, name: &str) -> PathBuf {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_game_on_its_own_mkxp_z_has_nothing_in_the_way_and_offers_the_profile_it_carries_first() {
        let root = tempfile::tempdir().unwrap();
        let dir = named(root.path(), "Pokemon Anil");
        fs::write(dir.join("Game.exe"), b"MZ preloadScript").unwrap();
        let cat = catalog();
        let game = Inspection::of(dir.clone(), Some(&cat), &Source::github());
        assert!(game.native() && game.offer.is_none() && !game.doubtful());
        assert_eq!(game.blocker(), None);
        assert_eq!(game.suggested_profile(), Some("anil"));
        super::super::apply::seal_installed(&dir, "0.5.0", "insurgence", "specific", "x86", "now", BTreeMap::new())
            .unwrap();
        let sealed = Inspection::of(dir, Some(&cat), &Source::github());
        assert_eq!(sealed.suggested_profile(), Some("insurgence"));
        assert_eq!(sealed.detected_profile.as_deref(), Some("anil"));
    }

    #[test]
    fn a_game_sealed_with_the_generic_profile_keeps_it_as_the_answer_over_the_one_recognised() {
        let root = tempfile::tempdir().unwrap();
        let dir = named(root.path(), "Pokemon Anil");
        fs::write(dir.join("Game.exe"), b"MZ preloadScript").unwrap();
        super::super::apply::seal_installed(&dir, "0.5.0", "generic", "generic", "x86", "now", BTreeMap::new())
            .unwrap();
        let cat = catalog();
        let game = Inspection::of(dir, Some(&cat), &Source::github());
        assert_eq!(game.suggested_profile(), Some("generic"));
        assert_eq!(game.detected_profile.as_deref(), Some("anil"));
        let es = I18n::new("es");
        let facts: Vec<String> = game.report(Some(&cat)).facts.iter().map(|f| es.t_err(f)).collect();
        assert!(facts.contains(&es.tf("check_profile_sealed", &es.t("profile_generic"))), "{facts:?}");
        assert!(facts.contains(&es.tf("check_profile_detected", "Pokemon Anil")), "{facts:?}");
    }

    #[test]
    fn a_conversion_cut_halfway_is_told_as_such_and_nothing_stands_in_the_way_of_finishing_it() {
        let root = tempfile::tempdir().unwrap();
        let dir = named(root.path(), "Pokemon Insurgence");
        player_folder(&dir);
        fs::create_dir_all(dir.join("Data")).unwrap();
        fs::write(dir.join("Data").join("Scripts.rxdata"), b"x").unwrap();
        let player = convert::rgss_player(&dir).unwrap();
        convert::convert(&dir, &player, "e1", "insurgence", b"MZ engine preloadScript", &[], "now").unwrap();
        let record = dir.join("accessibility").join("data").join("engine.json");
        let mut json: serde_json::Value = serde_json::from_str(&fs::read_to_string(&record).unwrap()).unwrap();
        json["pending"] = serde_json::Value::Bool(true);
        fs::write(&record, json.to_string()).unwrap();
        fs::write(dir.join("Game (PokeAccess).exe"), b"MZ").unwrap();
        fs::remove_file(dir.join("mkxp.json")).unwrap();
        let cat = catalog();
        for cat in [Some(&cat), None] {
            let game = Inspection::of(dir.clone(), cat, &Source::github());
            assert!(game.pending() && game.blocker().is_none() && game.incompatible().is_none());
            assert_eq!(game.report(cat).verdict, "check_pending");
        }
        fs::remove_file(&record).unwrap();
        let leftover = Inspection::of(dir, Some(&cat), &Source::github());
        assert!(matches!(leftover.blocker(), Some(Blocker::ConversionImpossible(e)) if e.starts_with("convert_exe_exists")));
    }

    #[test]
    fn each_reason_to_refuse_a_folder_is_given_as_the_window_gives_it() {
        let root = tempfile::tempdir().unwrap();
        let cat = catalog();
        let empty = named(root.path(), "vacia");
        let blocker = |dir: &Path| Inspection::of(dir.to_path_buf(), Some(&cat), &Source::github()).blocker();
        assert_eq!(blocker(&empty), Some(Blocker::Incompatible("not_compatible".into())));
        let player = named(root.path(), "Juego raro");
        player_folder(&player);
        assert_eq!(blocker(&player), Some(Blocker::Incompatible("rgss_player_unsupported".into())));
        let insurgence = named(root.path(), "Pokemon Insurgence");
        player_folder(&insurgence);
        let refused = blocker(&insurgence).unwrap();
        assert!(matches!(&refused, Blocker::ConversionImpossible(e) if e.starts_with("convert_markers_missing")));
        let shown = I18n::new("es").t_err(&refused.message());
        assert!(shown.contains("Data/Scripts.rxdata"), "{shown}");
        fs::create_dir_all(insurgence.join("Data")).unwrap();
        fs::write(insurgence.join("Data").join("Scripts.rxdata"), b"x").unwrap();
        let convertible = Inspection::of(insurgence, Some(&cat), &Source::github());
        assert_eq!(convertible.blocker(), None);
        assert_eq!(convertible.offer.as_ref().map(|o| o.engine.as_str()), Some("e1"));
        assert_eq!(convertible.kept_profile("generic").as_deref(), Some("insurgence"));
    }

    #[cfg(windows)]
    #[test]
    fn an_open_game_is_named_in_the_notice() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = tempfile::tempdir().unwrap();
        let dir = named(root.path(), "Relict");
        fs::write(dir.join("Game.exe"), b"MZ preloadScript").unwrap();
        let _open = fs::OpenOptions::new().read(true).share_mode(1).open(dir.join("Game.exe")).unwrap();
        let game = Inspection::of(dir, None, &Source::github());
        assert_eq!(game.blocker(), Some(Blocker::GameRunning("Relict".into())));
        let shown = I18n::new("en").t_err(&game.blocker().unwrap().message());
        assert_eq!(shown, I18n::new("en").tf("game_running", "Relict"));
    }

    #[test]
    fn an_mkxp_json_without_preload_asks_first_unless_a_conversion_made_it() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Game.exe"), b"MZ player").unwrap();
        fs::write(dir.path().join("mkxp.json"), "{\n}").unwrap();
        let game = Inspection::of(dir.path().to_path_buf(), None, &Source::github());
        assert!(game.doubtful() && game.native() && game.blocker().is_none());
        let converted = Inspection { record: Some(record("uranium")), ..game };
        assert!(!converted.doubtful());
    }

    #[test]
    fn a_missing_folder_and_a_folder_that_refuses_writing_are_told_apart() {
        let root = tempfile::tempdir().unwrap();
        let gone = root.path().join("movida");
        let entry = GameEntry {
            path: gone.to_string_lossy().into_owned(),
            name: String::new(),
            executable: String::new(),
            profile: "generic".into(),
            profile_mode: "generic".into(),
        };
        assert_eq!(game_dir(&entry), Err(Blocker::FolderMissing(gone.clone())));
        assert!(I18n::new("es").t_err(&Blocker::FolderMissing(gone.clone()).message()).contains("movida"));
        assert_eq!(check_writable(&gone), Err(Blocker::NoWritePermission));
        assert_eq!(check_writable(root.path()), Ok(()));
    }

    fn record(profile: &str) -> Manifest {
        Manifest {
            engine: "e".into(),
            profile: profile.into(),
            exe: "Uranium (PokeAccess).exe".into(),
            installed: String::new(),
            added: BTreeMap::new(),
            mkxp_json: "created".into(),
            converted_at: String::new(),
            pending: false,
        }
    }

    fn bare(record: Option<Manifest>, offer: Option<Offer>) -> Inspection {
        Inspection {
            game: ScannedGame { dir: PathBuf::from("x"), scan: ExeScan { main_exe: None, supports_preload: false } },
            has_mkxp_json: false,
            record,
            offer,
            player: None,
            sealed_profile: None,
            detected_profile: None,
            writable: true,
        }
    }

    #[test]
    fn a_converted_game_keeps_the_profile_its_conversion_needs() {
        assert_eq!(bare(Some(record("uranium")), None).kept_profile("generic"), Some("uranium".to_string()));
        assert_eq!(bare(Some(record("uranium")), None).kept_profile("uranium"), None);
        assert_eq!(bare(Some(record(" ")), None).kept_profile("generic"), None, "un registro sin perfil no impone ninguno");
        assert_eq!(bare(None, None).kept_profile("generic"), None);
        let offer = Offer {
            engine: "e".into(),
            markers: Vec::new(),
            profile: "insurgence".into(),
            display: "Pokemon Insurgence".into(),
        };
        assert_eq!(bare(None, Some(offer)).kept_profile("generic"), Some("insurgence".to_string()));
    }

    #[test]
    fn an_engine_folder_offers_its_conversion_to_any_game_on_the_player_without_imposing_a_profile() {
        let root = tempfile::tempdir().unwrap();
        let repo = named(root.path(), "repo");
        for (rel, text) in [("version.json", "{}"), ("core/manifest.rb", ""), ("games/catalog.json", "{}"),
            ("loader/preload_access.rb", "")] {
            let path = repo.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let engine = named(root.path(), "build");
        fs::write(engine.join("mkxp-z.exe"), b"MZ engine preloadScript").unwrap();
        let local = Source::folder(repo, Some(engine)).unwrap();
        let game_dir = named(root.path(), "Juego raro");
        player_folder(&game_dir);
        let cat = catalog();
        let game = Inspection::of(game_dir.clone(), Some(&cat), &local);
        let offer = game.offer.as_ref().unwrap();
        assert_eq!((offer.engine.as_str(), offer.display.as_str()), (LOCAL_ENGINE, "Juego raro"));
        assert_eq!(game.blocker(), None);
        assert_eq!(game.kept_profile("generic"), None);
        assert!(Inspection::of(game_dir, Some(&cat), &Source::github()).offer.is_none());
    }

    #[test]
    fn the_report_says_what_the_folder_runs_and_how_the_mod_goes_in() {
        let root = tempfile::tempdir().unwrap();
        let cat = catalog();
        let es = I18n::new("es");
        let report = |dir: &Path| Inspection::of(dir.to_path_buf(), Some(&cat), &Source::github()).report(Some(&cat));
        let told = |r: &Report| r.facts.iter().chain([&r.verdict]).map(|p| es.t_err(p)).collect::<Vec<_>>().join(" | ");
        let anil = named(root.path(), "Pokemon Anil");
        fs::write(anil.join("Game.exe"), b"MZ preloadScript").unwrap();
        let native = report(&anil);
        assert_eq!(native.verdict, "check_compatible");
        assert!(native.facts.contains(&"check_engine_mkxp".to_string()), "{:?}", native.facts);
        let text = told(&native);
        assert!(text.contains("Game.exe") && text.contains("Pokemon Anil") && !text.contains("check_"), "{text}");
        let insurgence = named(root.path(), "Pokemon Insurgence");
        player_folder(&insurgence);
        let blocked = report(&insurgence);
        assert!(blocked.verdict.starts_with("check_incompatible\u{1}convert_markers_missing"), "{}", blocked.verdict);
        assert!(blocked.facts.iter().any(|f| f.starts_with("check_convert_blocked")));
        assert!(blocked.facts.iter().any(|f| f.starts_with("check_engine_player\u{1}RGSS102E.dll")));
        fs::create_dir_all(insurgence.join("Data")).unwrap();
        fs::write(insurgence.join("Data").join("Scripts.rxdata"), b"x").unwrap();
        let convertible = report(&insurgence);
        assert_eq!(convertible.verdict, err_key("check_convertible", "Game (PokeAccess).exe"));
        assert!(convertible.facts.contains(&err_key("check_convert_yes", "e1")));
        let empty = report(&named(root.path(), "vacia"));
        assert_eq!(empty.verdict, err_key("check_incompatible", "not_compatible"));
        assert!(told(&empty).contains(&es.t("not_compatible")));
    }

    #[test]
    fn a_folder_about_to_be_converted_is_warned_by_the_limit_of_the_engine_it_will_run() {
        let root = tempfile::tempdir().unwrap();
        let cat = catalog();
        let pad = 140usize.saturating_sub(root.path().to_string_lossy().encode_utf16().count() + 20).max(1);
        let long = named(root.path(), &format!("Pokemon Insurgence {}", "x".repeat(pad)));
        assert!((127..=200).contains(&convert::path_len(&long)), "{}", convert::path_len(&long));
        player_folder(&long);
        fs::create_dir_all(long.join("Data")).unwrap();
        fs::write(long.join("Data").join("Scripts.rxdata"), b"x").unwrap();
        let converted = Inspection::of(long.clone(), Some(&cat), &Source::github());
        assert!(converted.offer.is_some());
        assert_eq!(converted.long_path_notice(), None, "el motor del mod lee hasta 200");
        fs::write(long.join("Game.exe"), b"MZ preloadScript").unwrap();
        let native = Inspection::of(long, Some(&cat), &Source::github());
        assert!(native.long_path_notice().is_some_and(|n| n.starts_with("long_path_own_engine")));
    }

    #[test]
    fn the_generic_profile_is_named_in_the_players_language_and_the_others_as_the_catalog_names_them() {
        let cat = catalog();
        let es = I18n::new("es");
        assert_eq!(es.t_err(&profile_title(Some(&cat), "generic")), es.t("profile_generic"));
        assert_eq!(profile_title(Some(&cat), "anil"), "Pokemon Anil");
        assert_eq!(profile_title(None, "anil"), "anil");
    }

    #[test]
    fn a_new_record_is_named_after_its_folder_and_starts_from_the_exe_the_catalog_knows() {
        let root = tempfile::tempdir().unwrap();
        let dir = named(root.path(), "Anil 2");
        fs::write(dir.join("Anil.exe"), b"MZ player").unwrap();
        fs::write(dir.join("Anil Classic.exe"), b"MZ player").unwrap();
        let game = ScannedGame::of(dir.clone());
        let cat = catalog();
        let entry = new_entry(&game, "anil", "specific", Some(&cat));
        assert_eq!((entry.name.as_str(), entry.executable.as_str()), ("Anil 2", "Anil.exe"));
        assert_eq!((entry.profile.as_str(), entry.profile_mode.as_str()), ("anil", "specific"));
        assert_eq!(entry.path, dir.to_string_lossy());
        assert_eq!(new_entry(&game, "generic", "generic", None).executable, "", "sin catálogo pregunta al jugar");
    }
}
