use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender};

use wxdragon::prelude::*;

use crate::core::{apply, catalog, config, detect, installed, status};
use crate::i18n::I18n;

enum Msg {
    Booted(Option<catalog::Catalog>, String),
    LauncherUpdate(Option<String>),
    Progress(String, u32, u32),
    Line(String),
    Done(Result<String, String>),
    UpdateChecked(Option<crate::core::selfupdate::LauncherUpdate>),
    UpdateApplied(Result<(), String>),
}

struct App {
    cfg: config::Config,
    cat: Option<catalog::Catalog>,
    available: String,
    i18n: I18n,
    busy: bool,
    rx: Option<Receiver<Msg>>,
    boot_rx: Option<Receiver<Msg>>,
    last_jobs: Vec<(PathBuf, String, String)>,
}

struct Ui {
    frame: Frame,
    status: StatusBar,
    games: ListBox,
    log: TextCtrl,
    gauge: Gauge,
    buttons: Vec<Button>,
}

/// A game folder together with the one scan of its executables, so a single
/// action never reads the same 100 MB file twice.
struct ScannedGame {
    dir: PathBuf,
    scan: detect::ExeScan,
}

impl ScannedGame {
    fn of(dir: PathBuf) -> ScannedGame {
        let scan = detect::scan_exes(&dir);
        ScannedGame { dir, scan }
    }

    fn name(&self) -> String {
        self.dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.dir.to_string_lossy().to_string())
    }
}

pub fn run() {
    let _ = wxdragon::main(|_| {
        let cfg = config::Config::load();
        let i18n = I18n::new(&cfg.resolve_language());

        let app = Rc::new(RefCell::new(App {
            cfg,
            cat: None,
            available: String::new(),
            i18n,
            busy: false,
            rx: None,
            boot_rx: None,
            last_jobs: Vec::new(),
        }));

        let title = app.borrow().i18n.t("app_title");
        let frame = Frame::builder().with_title(&title).with_size(Size::new(720, 540)).build();
        let panel = Panel::builder(&frame).build();
        let root = BoxSizer::builder(Orientation::Vertical).build();

        let heading = StaticText::builder(&panel).with_label(&app.borrow().i18n.t("my_games")).build();
        let games = ListBox::builder(&panel).build();
        games.set_name(&app.borrow().i18n.t("my_games"));
        let log = TextCtrl::builder(&panel)
            .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
            .build();
        log.set_name(&app.borrow().i18n.t("log_label"));
        let gauge = Gauge::builder(&panel).with_range(100).build();
        gauge.set_name(&app.borrow().i18n.t("progress"));

        let btns = BoxSizer::builder(Orientation::Horizontal).build();
        let mk = |lbl: &str| Button::builder(&panel).with_label(lbl).build();
        let add_btn = mk(&app.borrow().i18n.t("add_game_btn"));
        let inst_btn = mk(&app.borrow().i18n.t("install_btn"));
        let updall_btn = mk(&app.borrow().i18n.t("update_all_btn"));
        let prof_btn = mk(&app.borrow().i18n.t("change_profile_btn"));
        let uninst_btn = mk(&app.borrow().i18n.t("uninstall_btn"));
        let remove_btn = mk(&app.borrow().i18n.t("remove_from_list_btn"));
        let opt_btn = mk(&app.borrow().i18n.t("options_btn"));
        let chkupd_btn = mk(&app.borrow().i18n.t("check_launcher_update_btn"));
        for b in [&add_btn, &inst_btn, &updall_btn, &prof_btn, &uninst_btn, &remove_btn, &opt_btn, &chkupd_btn] {
            btns.add(b, 0, SizerFlag::All, 4);
        }

        root.add(&heading, 0, SizerFlag::All, 8);
        root.add(&games, 1, SizerFlag::Expand | SizerFlag::All, 8);
        root.add(&log, 1, SizerFlag::Expand | SizerFlag::All, 8);
        root.add(&gauge, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, 8);
        root.add_sizer(&btns, 0, SizerFlag::All, 4);
        panel.set_sizer(root, true);

        let ui = Rc::new(Ui {
            frame: frame.clone(),
            status: frame.create_status_bar(1, 0, -1, ""),
            games: games.clone(),
            log: log.clone(),
            gauge: gauge.clone(),
            buttons: vec![
                add_btn.clone(), inst_btn.clone(), updall_btn.clone(),
                prof_btn.clone(), uninst_btn.clone(), remove_btn.clone(),
                opt_btn.clone(), chkupd_btn.clone(),
            ],
        });

        refresh_list(&ui, &app);
        frame.show(true);
        frame.centre();
        ui.games.set_focus();
        announce(&ui, &app.borrow().i18n.t("loading_catalog"));
        spawn_boot(&app);

        let timer = Timer::new(&frame);
        {
            let app_c = app.clone();
            let ui_c = ui.clone();
            timer.on_tick(move |_| {
                pump_boot(&ui_c, &app_c);
                pump(&ui_c, &app_c);
            });
        }
        timer.start(120, false);
        std::mem::forget(timer);

        bind(&add_btn, &ui, &app, |ui, app| add_game(ui, app));
        bind(&inst_btn, &ui, &app, |ui, app| install_selected(ui, app));
        bind(&updall_btn, &ui, &app, |ui, app| update_all(ui, app));
        bind(&prof_btn, &ui, &app, |ui, app| change_profile(ui, app));
        bind(&uninst_btn, &ui, &app, |ui, app| uninstall_selected(ui, app));
        bind(&remove_btn, &ui, &app, |ui, app| remove_selected(ui, app));
        bind(&opt_btn, &ui, &app, |ui, app| options_dialog(ui, app));
        bind(&chkupd_btn, &ui, &app, |ui, app| check_launcher_update(ui, app));

        bind_shortcuts_on(&games, &ui, &app);
        bind_shortcuts_on(&log, &ui, &app);
        for b in &ui.buttons {
            bind_shortcuts_on(b, &ui, &app);
        }
    });
}

fn bind_shortcuts_on<W: WindowEvents>(widget: &W, ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    let ui_c = ui.clone();
    let app_c = app.clone();
    widget.on_key_down(move |ev| {
        if let WindowEventData::Keyboard(kev) = ev {
            if kev.control_down() {
                let letter = normalize_letter(kev.get_key_code().unwrap_or(0));
                if dispatch_shortcut(letter, &ui_c, &app_c) {
                    return;
                }
            }
            kev.event.skip(true);
        }
    });
}

fn normalize_letter(code: i32) -> char {
    match code {
        1..=26 => (b'A' + (code as u8 - 1)) as char,
        65..=90 | 97..=122 => (code as u8 as char).to_ascii_uppercase(),
        _ => '\0',
    }
}

fn dispatch_shortcut(letter: char, ui: &Rc<Ui>, app: &Rc<RefCell<App>>) -> bool {
    if app.borrow().busy {
        return false;
    }
    match letter {
        'A' => add_game(ui, app),
        'I' => install_selected(ui, app),
        'U' => update_all(ui, app),
        'P' => change_profile(ui, app),
        'D' => uninstall_selected(ui, app),
        'Q' => remove_selected(ui, app),
        'O' => options_dialog(ui, app),
        'B' => check_launcher_update(ui, app),
        _ => return false,
    }
    true
}

fn bind<F>(btn: &Button, ui: &Rc<Ui>, app: &Rc<RefCell<App>>, f: F)
where
    F: Fn(&Rc<Ui>, &Rc<RefCell<App>>) + 'static,
{
    let ui_c = ui.clone();
    let app_c = app.clone();
    btn.on_click(move |_| {
        if app_c.borrow().busy {
            return;
        }
        f(&ui_c, &app_c);
    });
}

fn fetch_available_version() -> String {
    match crate::core::github::download_bytes(&crate::core::paths::raw_url("version.json")) {
        Ok(b) => installed::parse_version_json(&String::from_utf8_lossy(&b))
            .map(|m| m.version)
            .unwrap_or_default(),
        Err(_) => String::new(),
    }
}

fn log_line(ui: &Ui, msg: &str) {
    ui.log.append_text(msg);
    ui.log.append_text("\n");
    crate::core::logging::append(msg);
}

/// Says it in the status bar and the log, repainting the bar right away so the
/// message is already on screen when a long step blocks the window.
fn announce(ui: &Ui, msg: &str) {
    ui.frame.set_status_text(msg, 0);
    ui.status.update();
    log_line(ui, msg);
}

fn set_busy(ui: &Ui, app: &Rc<RefCell<App>>, busy: bool) {
    app.borrow_mut().busy = busy;
    for b in &ui.buttons {
        b.enable(!busy);
    }
    let status = if busy {
        app.borrow().i18n.t("working_wait")
    } else {
        app.borrow().i18n.t("ready")
    };
    ui.frame.set_status_text(&status, 0);
    if !busy {
        ui.gauge.set_value(0);
    }
}

fn spawn_boot(app: &Rc<RefCell<App>>) {
    let (tx, rx): (Sender<Msg>, Receiver<Msg>) = std::sync::mpsc::channel();
    app.borrow_mut().boot_rx = Some(rx);
    std::thread::spawn(move || {
        let cat = catalog::Catalog::fetch().ok();
        let available = fetch_available_version();
        let _ = tx.send(Msg::Booted(cat, available));
        let launcher_update = crate::core::selfupdate::check().map(|u| u.tag);
        let _ = tx.send(Msg::LauncherUpdate(launcher_update));
    });
}

fn pump_boot(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    if app.borrow().boot_rx.is_none() {
        return;
    }
    loop {
        let msg = {
            let a = app.borrow();
            a.boot_rx.as_ref().unwrap().try_recv().ok()
        };
        match msg {
            Some(Msg::Booted(cat, available)) => {
                {
                    let mut a = app.borrow_mut();
                    a.cat = cat;
                    a.available = available;
                }
                refresh_list(ui, app);
                let msg = {
                    let a = app.borrow();
                    let offline = a.cat.is_none() || a.available.trim().is_empty();
                    a.i18n.t(if offline { "ready_offline" } else { "ready" })
                };
                announce(ui, &msg);
            }
            Some(Msg::LauncherUpdate(update)) => {
                app.borrow_mut().boot_rx = None;
                if let Some(tag) = update {
                    notify_launcher_update(ui, app, &tag);
                }
                break;
            }
            _ => break,
        }
    }
}

fn notify_launcher_update(ui: &Rc<Ui>, app: &Rc<RefCell<App>>, tag: &str) {
    let already = app.borrow().cfg.last_notified_tag == tag;
    if already {
        return;
    }
    {
        let mut a = app.borrow_mut();
        a.cfg.last_notified_tag = tag.to_string();
        let _ = a.cfg.save();
    }
    let m = app.borrow().i18n.tf("launcher_update_ready", tag);
    announce(ui, &m);
}

fn pump(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    use std::sync::mpsc::TryRecvError;
    let rx_present = app.borrow().rx.is_some();
    if !rx_present {
        return;
    }
    let mut done: Option<Result<String, String>> = None;
    let mut update_checked: Option<Option<crate::core::selfupdate::LauncherUpdate>> = None;
    let mut update_applied: Option<Result<(), String>> = None;
    loop {
        let recv = {
            let a = app.borrow();
            a.rx.as_ref().unwrap().try_recv()
        };
        match recv {
            Ok(Msg::Progress(file, done_count, total)) => {
                let line = if total > 0 {
                    let pct = (done_count * 100 / total) as i32;
                    ui.gauge.set_value(pct);
                    format!("{} ({}%)", app.borrow().i18n.tf("downloading_file", &file), pct)
                } else {
                    app.borrow().i18n.tf("downloading_file", &file)
                };
                announce(ui, &line);
            }
            Ok(Msg::Line(l)) => log_line(ui, &l),
            Ok(Msg::Done(r)) => {
                done = Some(r);
                break;
            }
            Ok(Msg::UpdateChecked(u)) => {
                update_checked = Some(u);
                break;
            }
            Ok(Msg::UpdateApplied(r)) => {
                update_applied = Some(r);
                break;
            }
            Ok(Msg::Booted(_, _)) | Ok(Msg::LauncherUpdate(_)) => {}
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                done = Some(Err(app.borrow().i18n.t("worker_crashed")));
                break;
            }
        }
    }
    if let Some(u) = update_checked {
        app.borrow_mut().rx = None;
        set_busy(ui, app, false);
        handle_update_checked(ui, app, u);
        return;
    }
    if let Some(r) = update_applied {
        app.borrow_mut().rx = None;
        set_busy(ui, app, false);
        match r {
            Ok(_) => {
                if let Err(e) = crate::core::selfupdate::restart() {
                    info(ui, app, &app.borrow().i18n.tf("restart_failed", &e));
                } else {
                    info(ui, app, &app.borrow().i18n.t("launcher_update_applied"));
                    std::process::exit(0);
                }
            }
            Err(e) => {
                let m = {
                    let a = app.borrow();
                    a.i18n.tf("error", &a.i18n.t_err(&e))
                };
                info(ui, app, &m);
            }
        }
        return;
    }
    if let Some(res) = done {
        app.borrow_mut().rx = None;
        set_busy(ui, app, false);
        refresh_list(ui, app);
        match res {
            Ok(v) => {
                app.borrow_mut().last_jobs.clear();
                let m = if v.is_empty() {
                    app.borrow().i18n.t("done_uninstalled")
                } else {
                    app.borrow().i18n.tf("done_installed", &v)
                };
                announce(ui, &m);
                info(ui, app, &m);
            }
            Err(e) => {
                let m = {
                    let a = app.borrow();
                    a.i18n.tf("error", &a.i18n.t_err(&e))
                };
                announce(ui, &m);
                match ask_retry(ui, app, &m) {
                    Retry::Yes => {
                        let jobs = app.borrow().last_jobs.clone();
                        announce(ui, &app.borrow().i18n.t("retrying"));
                        spawn_installs(ui, app, jobs);
                    }
                    Retry::No => {}
                    Retry::NotOffered => info(ui, app, &m),
                }
            }
        }
    }
}

/// What the player answered to the retry dialog, or that it never appeared.
enum Retry {
    Yes,
    No,
    NotOffered,
}

/// Offers to run the failed jobs again, showing the error inside the question.
/// `NotOffered` means no dialog was shown, so the caller still owes the player
/// the error message.
fn ask_retry(ui: &Rc<Ui>, app: &Rc<RefCell<App>>, error: &str) -> Retry {
    if app.borrow().last_jobs.is_empty() {
        return Retry::NotOffered;
    }
    let (msg, caption) = {
        let a = app.borrow();
        (format!("{}\n\n{}", error, a.i18n.t("ask_retry")), a.i18n.t("app_title"))
    };
    let dlg = MessageDialog::builder(&ui.frame, &msg, &caption)
        .with_style(MessageDialogStyle::YesNo)
        .build();
    if dlg.show_modal() == ID_YES {
        Retry::Yes
    } else {
        Retry::No
    }
}

fn row_label(app: &App, entry: &config::GameEntry) -> String {
    let dir = PathBuf::from(&entry.path);
    let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| entry.path.clone());
    let inst = installed::read(&dir);
    let st = status::compute(inst.as_ref().map(|i| i.mod_version.as_str()), &app.available);
    let st_txt = app.i18n.t(status::status_key(st));
    let prof = if entry.profile.is_empty() {
        String::new()
    } else {
        format!(", {}", app.i18n.tf("profile_label", &profile_name(app, &entry.profile)))
    };
    format!("{} - {}{}", name, st_txt, prof)
}

/// The profile as the player should hear it: the generic one has a translated
/// name, a specific one is named by the catalog and falls back to its key when
/// the catalog could not be fetched.
fn profile_name(app: &App, key: &str) -> String {
    if key == "generic" {
        return app.i18n.t("profile_generic");
    }
    app.cat.as_ref().map(|c| c.display_of(key)).unwrap_or_else(|| key.to_string())
}

fn refresh_list(ui: &Ui, app: &Rc<RefCell<App>>) {
    ui.games.clear();
    let a = app.borrow();
    for e in &a.cfg.games {
        ui.games.append(&row_label(&a, e));
    }
}

fn selected_index(ui: &Ui) -> Option<usize> {
    ui.games.get_selection().map(|s| s as usize)
}

/// Shows a modal notice captioned with the app name in the active language.
fn info(ui: &Ui, app: &Rc<RefCell<App>>, msg: &str) {
    let caption = app.borrow().i18n.t("app_title");
    MessageDialog::builder(&ui.frame, msg, &caption).build().show_modal();
}

/// Blocks an action while a game executable is locked, i.e. the game is still open.
fn ensure_closed(ui: &Rc<Ui>, app: &Rc<RefCell<App>>, games: &[ScannedGame]) -> bool {
    match games.iter().find(|g| g.scan.game_running()) {
        Some(g) => {
            info(ui, app, &app.borrow().i18n.tf("game_running", &g.name()));
            false
        }
        None => true,
    }
}

/// Reads a folder's executables once, saying so first: the scan walks whole
/// executables and can take seconds on a slow disk, and an unexplained silence
/// is the worst thing that can happen to someone using a screen reader.
fn scan_game(ui: &Rc<Ui>, app: &Rc<RefCell<App>>, dir: PathBuf) -> ScannedGame {
    announce(ui, &app.borrow().i18n.t("checking_games"));
    ScannedGame::of(dir)
}

fn require_selection(ui: &Ui, app: &Rc<RefCell<App>>) -> Option<usize> {
    match selected_index(ui) {
        Some(i) => Some(i),
        None => {
            info(ui, app, &app.borrow().i18n.t("no_selection"));
            None
        }
    }
}

fn spawn_installs(ui: &Rc<Ui>, app: &Rc<RefCell<App>>, jobs: Vec<(PathBuf, String, String)>) {
    let (tx, rx): (Sender<Msg>, Receiver<Msg>) = std::sync::mpsc::channel();
    {
        let mut a = app.borrow_mut();
        a.rx = Some(rx);
        a.last_jobs = jobs.clone();
    }
    set_busy(ui, app, true);
    ui.gauge.set_value(0);
    let multi = jobs.len() > 1;
    std::thread::spawn(move || {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| format!("unix:{}", d.as_secs()))
            .unwrap_or_default();
        let mut last: Result<String, String> = Ok(String::new());
        for (path, profile, mode) in jobs {
            if multi {
                let _ = tx.send(Msg::Line(format!("== {} ==", path.display())));
            }
            let txp = tx.clone();
            let r = apply::run_install(&path, &profile, &mode, &now, move |file, done_count, total| {
                let _ = txp.send(Msg::Progress(file.to_string(), done_count, total));
            });
            match r {
                Ok(v) => last = Ok(v),
                Err(e) => {
                    last = Err(e);
                    break;
                }
            }
        }
        let _ = tx.send(Msg::Done(last));
    });
}

fn add_game(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    let prompt = app.borrow().i18n.t("pick_folder");
    let dlg = DirDialog::builder(&ui.frame, &prompt, "").build();
    if dlg.show_modal() != ID_OK {
        return;
    }
    let path = match dlg.get_path() {
        Some(p) => detect::resolve_game_dir(&PathBuf::from(p)),
        None => return,
    };
    let game = scan_game(ui, app, path);
    let has_mkxp_json = crate::core::mkxp::has_mkxp_json(&game.dir);
    if !has_mkxp_json && !game.scan.supports_preload {
        info(ui, app, &app.borrow().i18n.t("not_compatible"));
        return;
    }
    if !apply::can_write(&game.dir) {
        info(ui, app, &app.borrow().i18n.t("no_write_perm"));
        return;
    }
    if !ensure_closed(ui, app, std::slice::from_ref(&game)) {
        return;
    }
    if has_mkxp_json && !game.scan.supports_preload && !confirm_preload_warning(ui, app) {
        return;
    }
    let sealed = installed::specific_profile(&game.dir);
    let (profile, mode) = choose_profile(ui, &game, app, sealed);
    let profile = match profile {
        Some(p) => p,
        None => return,
    };
    let path = game.dir;
    {
        let mut a = app.borrow_mut();
        a.cfg.upsert_game(config::GameEntry {
            path: path.to_string_lossy().to_string(),
            profile: profile.clone(),
            profile_mode: mode.clone(),
        });
        let _ = a.cfg.save();
    }
    refresh_list(ui, app);
    announce(ui, &app.borrow().i18n.tf("installing", &path.display().to_string()));
    spawn_installs(ui, app, vec![(path, profile, mode)]);
}

/// Warns that the executable shows no preloadScript support and asks whether to go on.
fn confirm_preload_warning(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) -> bool {
    let (msg, caption) = {
        let a = app.borrow();
        (a.i18n.t("preload_missing_warn"), a.i18n.t("app_title"))
    };
    let dlg = MessageDialog::builder(&ui.frame, &msg, &caption)
        .with_style(MessageDialogStyle::YesNo)
        .build();
    dlg.show_modal() == ID_YES
}

/// Offers a profile for the folder and returns it with its mode. `sealed` is
/// the profile the folder already carries: when there is one it becomes the
/// default answer, ahead of whatever the catalog would guess, so accepting the
/// dialog can never downgrade a game that is already installed.
fn choose_profile(
    ui: &Rc<Ui>,
    game: &ScannedGame,
    app: &Rc<RefCell<App>>,
    sealed: Option<String>,
) -> (Option<String>, String) {
    let a = app.borrow();
    let exe = game.scan.main_exe.as_deref();
    let game_titles = detect::game_titles(&game.dir);
    let hay = detect::folder_and_exe_string(&game.dir, exe);
    let exe_name = exe.and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string());
    let detected = sealed.clone().or_else(|| {
        a.cat
            .as_ref()
            .and_then(|c| c.detect(&game_titles, &hay, exe_name.as_deref()))
            .map(|p| p.key.clone())
    });

    let mut choices: Vec<String> = Vec::new();
    if let Some(key) = &detected {
        let disp = a.cat.as_ref().map(|c| c.display_of(key)).unwrap_or_else(|| key.clone());
        choices.push(a.i18n.tf("install_specific", &disp));
    }
    choices.push(a.i18n.t("install_generic"));
    let manual = a.i18n.t("choose_manual");
    let has_specific_profiles =
        a.cat.as_ref().map(|c| c.profiles.iter().any(|p| p.key != "generic")).unwrap_or(false);
    if has_specific_profiles {
        choices.push(manual.clone());
    }
    let headline = if let Some(key) = &detected {
        let disp = a.cat.as_ref().map(|c| c.display_of(key)).unwrap_or_else(|| key.clone());
        let said = if sealed.is_some() { "installed_profile" } else { "detected_profile" };
        a.i18n.tf(said, &disp)
    } else {
        a.i18n.t("not_detected")
    };
    let prompt = format!("{}\n\n{}", headline, a.i18n.t("generic_hint"));
    let generic = a.i18n.t("install_generic");
    let caption = a.i18n.t("choose_profile_title");
    drop(a);

    let choice_refs: Vec<&str> = choices.iter().map(|s| s.as_str()).collect();
    let dlg = SingleChoiceDialog::builder(&ui.frame, &prompt, &caption, &choice_refs).build();
    if dlg.show_modal() != ID_OK {
        return (None, String::new());
    }
    let sel = dlg.get_selection();
    if sel < 0 {
        return (None, String::new());
    }
    let picked = &choices[sel as usize];
    if *picked == manual {
        choose_profile_manual(ui, app)
    } else if *picked == generic {
        (Some("generic".to_string()), "generic".to_string())
    } else {
        (detected, "specific".to_string())
    }
}

fn choose_profile_manual(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) -> (Option<String>, String) {
    let (keys, names, title, caption) = {
        let a = app.borrow();
        let mut keys: Vec<String> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        if let Some(cat) = a.cat.as_ref() {
            for p in &cat.profiles {
                if p.key != "generic" {
                    keys.push(p.key.clone());
                    names.push(p.display.clone());
                }
            }
        }
        (keys, names, a.i18n.t("manual_profile_title"), a.i18n.t("choose_profile_title"))
    };
    if keys.is_empty() {
        return (None, String::new());
    }
    let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let dlg = SingleChoiceDialog::builder(&ui.frame, &title, &caption, &name_refs).build();
    if dlg.show_modal() != ID_OK {
        return (None, String::new());
    }
    let sel = dlg.get_selection();
    if sel < 0 || sel as usize >= keys.len() {
        return (None, String::new());
    }
    (Some(keys[sel as usize].clone()), "specific".to_string())
}

fn install_selected(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    let idx = match require_selection(ui, app) {
        Some(i) => i,
        None => return,
    };
    let (path, profile, mode) = {
        let a = app.borrow();
        let e = &a.cfg.games[idx];
        (PathBuf::from(&e.path), e.profile.clone(), e.profile_mode.clone())
    };
    if !apply::can_write(&path) {
        info(ui, app, &app.borrow().i18n.t("no_write_perm"));
        return;
    }
    let game = scan_game(ui, app, path);
    if !ensure_closed(ui, app, std::slice::from_ref(&game)) {
        return;
    }
    let path = game.dir;
    announce(ui, &app.borrow().i18n.tf("installing", &path.display().to_string()));
    spawn_installs(ui, app, vec![(path, profile, mode)]);
}

fn update_all(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    let jobs: Vec<(PathBuf, String, String)> = {
        let a = app.borrow();
        a.cfg
            .games
            .iter()
            .filter(|e| installed::is_installed(&PathBuf::from(&e.path)))
            .map(|e| (PathBuf::from(&e.path), e.profile.clone(), e.profile_mode.clone()))
            .collect()
    };
    if jobs.is_empty() {
        info(ui, app, &app.borrow().i18n.t("nothing_to_update"));
        return;
    }
    announce(ui, &app.borrow().i18n.t("checking_games"));
    let games: Vec<ScannedGame> = jobs.iter().map(|(p, _, _)| ScannedGame::of(p.clone())).collect();
    if !ensure_closed(ui, app, &games) {
        return;
    }
    announce(ui, &app.borrow().i18n.t("updating_all"));
    spawn_installs(ui, app, jobs);
}

fn change_profile(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    let idx = match require_selection(ui, app) {
        Some(i) => i,
        None => return,
    };
    let path = PathBuf::from(&app.borrow().cfg.games[idx].path);
    let game = scan_game(ui, app, path);
    if !ensure_closed(ui, app, std::slice::from_ref(&game)) {
        return;
    }
    let (profile, mode) = choose_profile(ui, &game, app, None);
    let profile = match profile {
        Some(p) => p,
        None => return,
    };
    {
        let mut a = app.borrow_mut();
        let e = &mut a.cfg.games[idx];
        e.profile = profile.clone();
        e.profile_mode = mode.clone();
        let _ = a.cfg.save();
    }
    announce(ui, &app.borrow().i18n.tf("profile_changed", &profile));
    spawn_installs(ui, app, vec![(game.dir, profile, mode)]);
}

fn uninstall_selected(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    let idx = match require_selection(ui, app) {
        Some(i) => i,
        None => return,
    };
    let path = PathBuf::from(&app.borrow().cfg.games[idx].path);
    let game = scan_game(ui, app, path);
    if !ensure_closed(ui, app, std::slice::from_ref(&game)) {
        return;
    }
    let (confirm, caption) = {
        let a = app.borrow();
        (a.i18n.t("confirm_uninstall"), a.i18n.t("uninstall"))
    };
    let dlg = MessageDialog::builder(&ui.frame, &confirm, &caption)
        .with_style(MessageDialogStyle::YesNo)
        .build();
    if dlg.show_modal() != ID_YES {
        return;
    }
    match apply::run_uninstall(&game.dir) {
        Ok(_) => announce(ui, &app.borrow().i18n.t("done_uninstalled")),
        Err(e) => {
            let m = {
                let a = app.borrow();
                a.i18n.tf("error", &a.i18n.t_err(&e))
            };
            announce(ui, &m);
            info(ui, app, &m);
        }
    }
    refresh_list(ui, app);
}

fn remove_selected(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    let idx = match require_selection(ui, app) {
        Some(i) => i,
        None => return,
    };
    let (confirm, caption) = {
        let a = app.borrow();
        (a.i18n.t("confirm_remove"), a.i18n.t("remove_from_list"))
    };
    let dlg = MessageDialog::builder(&ui.frame, &confirm, &caption)
        .with_style(MessageDialogStyle::YesNo)
        .build();
    if dlg.show_modal() != ID_YES {
        return;
    }
    let path = app.borrow().cfg.games[idx].path.clone();
    {
        let mut a = app.borrow_mut();
        a.cfg.remove_game(&path);
        let _ = a.cfg.save();
    }
    announce(ui, &app.borrow().i18n.t("removed_from_list"));
    refresh_list(ui, app);
}

fn options_dialog(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    let (lang_es, lang_en, title) = {
        let a = app.borrow();
        (a.i18n.t("lang_es"), a.i18n.t("lang_en"), a.i18n.t("language"))
    };
    let choices = vec![lang_es.clone(), lang_en.clone()];
    let choice_refs: Vec<&str> = choices.iter().map(|s| s.as_str()).collect();
    let dlg = SingleChoiceDialog::builder(&ui.frame, &title, &title, &choice_refs).build();
    if dlg.show_modal() != ID_OK {
        return;
    }
    let sel = dlg.get_selection();
    if sel < 0 {
        return;
    }
    let lang = if choices[sel as usize] == lang_es { "es" } else { "en" };
    {
        let mut a = app.borrow_mut();
        a.i18n.set_lang(lang);
        a.cfg.set_language(lang);
        let _ = a.cfg.save();
    }
    info(ui, app, &app.borrow().i18n.t("lang_changed"));
}

fn check_launcher_update(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    let (tx, rx): (Sender<Msg>, Receiver<Msg>) = std::sync::mpsc::channel();
    app.borrow_mut().rx = Some(rx);
    set_busy(ui, app, true);
    announce(ui, &app.borrow().i18n.t("check_launcher_update"));
    std::thread::spawn(move || {
        let _ = tx.send(Msg::UpdateChecked(crate::core::selfupdate::check()));
    });
}

fn handle_update_checked(
    ui: &Rc<Ui>,
    app: &Rc<RefCell<App>>,
    update: Option<crate::core::selfupdate::LauncherUpdate>,
) {
    let u = match update {
        Some(u) => u,
        None => {
            info(ui, app, &app.borrow().i18n.t("launcher_update_none"));
            return;
        }
    };
    let (mut msg, caption) = {
        let a = app.borrow();
        (a.i18n.tf("launcher_update_available", &u.tag), a.i18n.t("launcher_update_title"))
    };
    if !u.notes.trim().is_empty() {
        msg.push_str("\n\n");
        msg.push_str(u.notes.trim());
    }
    let dlg = MessageDialog::builder(&ui.frame, &msg, &caption)
        .with_style(MessageDialogStyle::YesNo)
        .build();
    if dlg.show_modal() != ID_YES {
        return;
    }
    let (tx, rx): (Sender<Msg>, Receiver<Msg>) = std::sync::mpsc::channel();
    app.borrow_mut().rx = Some(rx);
    set_busy(ui, app, true);
    std::thread::spawn(move || {
        let _ = tx.send(Msg::UpdateApplied(crate::core::selfupdate::apply(&u)));
    });
}
