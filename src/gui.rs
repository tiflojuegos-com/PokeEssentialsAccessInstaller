use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use wxdragon::prelude::*;

use crate::core::batch::{self, Job, Outcome};
use crate::core::ops::{self, Blocker, Inspection, ScannedGame};
use crate::core::source::Source;
use crate::core::paths::data_dir;
use crate::core::{apply, catalog, config, convert, detect, game, player_data, status};
use crate::i18n::I18n;

enum Msg {
    Booted(Option<catalog::Catalog>, String),
    LauncherUpdate(Option<String>),
    Started(usize),
    Progress(usize, String, u32, u32),
    Finished(usize, Outcome),
    Done(Vec<Outcome>),
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
    last_jobs: Vec<Job>,
    progress: Vec<(u32, u32)>,
    milestone: i32,
    forewarned: Option<PathBuf>,
    pending_edit: Option<(usize, config::GameEntry)>,
    booted: bool,
    dropped: Option<PathBuf>,
    health: HashMap<String, Vec<status::Fault>>,
    health_rx: Option<Receiver<HashMap<String, Vec<status::Fault>>>>,
}

struct Ui {
    frame: Frame,
    status: StatusBar,
    games: ListBox,
    log: TextCtrl,
    gauge: Gauge,
    buttons: Vec<Button>,
}

pub fn run(dropped: Option<PathBuf>) {
    let _ = wxdragon::main(|_| {
        let mut cfg = config::Config::load();
        if cfg.games.iter_mut().fold(false, |changed, g| game::adopt_accessible(g) || changed) {
            if let Err(e) = cfg.save() {
                crate::core::logging::append(&format!("Saving the accessible executables failed: {e}"));
            }
        }
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
            progress: Vec::new(),
            milestone: 0,
            forewarned: None,
            pending_edit: None,
            booted: false,
            dropped,
            health: HashMap::new(),
            health_rx: None,
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

        let btns = WrapSizer::builder(Orientation::Horizontal).build();
        let mk = |lbl: &str| Button::builder(&panel).with_label(lbl).build();
        let play_btn = mk(&app.borrow().i18n.t("play_btn"));
        let add_btn = mk(&app.borrow().i18n.t("add_game_btn"));
        let check_btn = mk(&app.borrow().i18n.t("check_btn"));
        let inst_btn = mk(&app.borrow().i18n.t("install_btn"));
        let updall_btn = mk(&app.borrow().i18n.t("update_all_btn"));
        let prof_btn = mk(&app.borrow().i18n.t("change_profile_btn"));
        let uninst_btn = mk(&app.borrow().i18n.t("uninstall_btn"));
        let export_btn = mk(&app.borrow().i18n.t("export_btn"));
        let import_btn = mk(&app.borrow().i18n.t("import_btn"));
        let remove_btn = mk(&app.borrow().i18n.t("remove_from_list_btn"));
        let opt_btn = mk(&app.borrow().i18n.t("options_btn"));
        let chkupd_btn = mk(&app.borrow().i18n.t("check_launcher_update_btn"));
        for b in [&play_btn, &add_btn, &check_btn, &inst_btn, &updall_btn, &prof_btn, &uninst_btn, &export_btn,
            &import_btn, &remove_btn, &opt_btn, &chkupd_btn] {
            btns.add(b, 0, SizerFlag::All, 4);
        }

        root.add(&heading, 0, SizerFlag::All, 8);
        root.add(&games, 1, SizerFlag::Expand | SizerFlag::All, 8);
        root.add(&log, 1, SizerFlag::Expand | SizerFlag::All, 8);
        root.add(&gauge, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, 8);
        root.add_sizer(&btns, 0, SizerFlag::Expand | SizerFlag::All, 4);
        panel.set_sizer(root, true);

        let ui = Rc::new(Ui {
            frame: frame.clone(),
            status: frame.create_status_bar(1, 0, -1, ""),
            games: games.clone(),
            log: log.clone(),
            gauge: gauge.clone(),
            buttons: vec![
                play_btn.clone(), add_btn.clone(), check_btn.clone(), inst_btn.clone(), updall_btn.clone(),
                prof_btn.clone(), uninst_btn.clone(), export_btn.clone(), import_btn.clone(),
                remove_btn.clone(), opt_btn.clone(), chkupd_btn.clone(),
            ],
        });

        refresh_list(&ui, &app);
        check_health(&app);
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
                pump_health(&ui_c, &app_c);
            });
        }
        timer.start(120, false);
        std::mem::forget(timer);

        bind(&play_btn, &ui, &app, |ui, app| launch_selected(ui, app));
        bind(&add_btn, &ui, &app, |ui, app| add_game(ui, app));
        bind(&check_btn, &ui, &app, |ui, app| check_game(ui, app));
        bind(&inst_btn, &ui, &app, |ui, app| install_selected(ui, app));
        bind(&updall_btn, &ui, &app, |ui, app| update_all(ui, app));
        bind(&prof_btn, &ui, &app, |ui, app| edit_game(ui, app));
        bind(&uninst_btn, &ui, &app, |ui, app| uninstall_selected(ui, app));
        bind(&export_btn, &ui, &app, |ui, app| export_selected(ui, app));
        bind(&import_btn, &ui, &app, |ui, app| import_selected(ui, app));
        bind(&remove_btn, &ui, &app, |ui, app| remove_selected(ui, app));
        bind(&opt_btn, &ui, &app, |ui, app| options_dialog(ui, app));
        bind(&chkupd_btn, &ui, &app, |ui, app| check_launcher_update(ui, app));

        bind_shortcuts_on(&games, &ui, &app, true);
        {
            let ui_c = ui.clone();
            let app_c = app.clone();
            games.on_item_double_clicked(move |ev| {
                if let Some(idx) = ev.get_selection().filter(|&idx| idx >= 0) {
                    ui_c.games.set_selection(idx as u32, true);
                    launch_index(&ui_c, &app_c, idx as usize);
                } else {
                    launch_selected(&ui_c, &app_c);
                }
            });
        }
        #[cfg(windows)]
        if !list_enter::install(&games, &ui, &app) {
            crate::core::logging::append("Could not install native Enter handler for game list");
        }
        bind_shortcuts_on(&log, &ui, &app, false);
        for b in &ui.buttons {
            bind_shortcuts_on(b, &ui, &app, false);
        }
    });
}

fn bind_shortcuts_on<W: WindowEvents>(widget: &W, ui: &Rc<Ui>, app: &Rc<RefCell<App>>, games: bool) {
    let ui_c = ui.clone();
    let app_c = app.clone();
    widget.on_key_down(move |ev| {
        if let WindowEventData::Keyboard(kev) = ev {
            if games && !cfg!(windows) && !kev.control_down() && !kev.alt_down()
                && kev.get_key_code() == Some(13) {
                launch_selected(&ui_c, &app_c);
                return;
            }
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

#[cfg(windows)]
mod list_enter {
    use super::*;
    use std::ffi::c_void;

    type Hwnd = *mut c_void;
    type SubclassProc = unsafe extern "system" fn(Hwnd, u32, usize, isize, usize, usize) -> isize;
    const WM_NCDESTROY: u32 = 0x0082;
    const WM_GETDLGCODE: u32 = 0x0087;
    const WM_KEYDOWN: u32 = 0x0100;
    const WM_CHAR: u32 = 0x0102;
    const VK_RETURN: usize = 0x0d;
    const VK_CONTROL: i32 = 0x11;
    const VK_MENU: i32 = 0x12;
    const DLGC_WANTALLKEYS: isize = 0x0004;
    const EXTENDED_KEY: usize = 1 << 24;
    const REPEAT_KEY: usize = 1 << 30;

    #[link(name = "comctl32")]
    extern "system" {
        fn SetWindowSubclass(hwnd: Hwnd, callback: Option<SubclassProc>, id: usize, data: usize) -> i32;
        fn RemoveWindowSubclass(hwnd: Hwnd, callback: Option<SubclassProc>, id: usize) -> i32;
        fn DefSubclassProc(hwnd: Hwnd, msg: u32, key: usize, flags: isize) -> isize;
    }
    #[link(name = "user32")]
    extern "system" {
        fn GetKeyState(key: i32) -> i16;
    }

    struct EnterHandler {
        ui: Rc<Ui>,
        app: Rc<RefCell<App>>,
    }

    fn main_return(key: usize, flags: isize) -> bool {
        key == VK_RETURN && (flags as usize & EXTENDED_KEY) == 0
    }

    unsafe extern "system" fn handle(
        hwnd: Hwnd, msg: u32, key: usize, flags: isize, id: usize, data: usize,
    ) -> isize {
        if msg == WM_NCDESTROY {
            RemoveWindowSubclass(hwnd, Some(handle), id);
            let result = DefSubclassProc(hwnd, msg, key, flags);
            drop(Box::from_raw(data as *mut EnterHandler));
            return result;
        }

        if msg == WM_GETDLGCODE && key == VK_RETURN {
            return DefSubclassProc(hwnd, msg, key, flags) | DLGC_WANTALLKEYS;
        }
        if msg == WM_KEYDOWN && main_return(key, flags) {
            if (flags as usize & REPEAT_KEY) == 0
                && GetKeyState(VK_CONTROL) >= 0 && GetKeyState(VK_MENU) >= 0
            {
                let handler = &*(data as *const EnterHandler);
                let ui = handler.ui.clone();
                let app = handler.app.clone();
                launch_selected(&ui, &app);
            }
            return 0;
        }
        if msg == WM_CHAR && key == VK_RETURN {
            return 0;
        }
        DefSubclassProc(hwnd, msg, key, flags)
    }

    pub(super) fn install(games: &ListBox, ui: &Rc<Ui>, app: &Rc<RefCell<App>>) -> bool {
        let hwnd = games.get_handle();
        if hwnd.is_null() {
            return false;
        }
        let state = Box::into_raw(Box::new(EnterHandler { ui: ui.clone(), app: app.clone() }));
        if unsafe { SetWindowSubclass(hwnd, Some(handle), 1, state as usize) } == 0 {
            unsafe { drop(Box::from_raw(state)); }
            return false;
        }
        true
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn only_regular_enter_matches() {
            assert!(main_return(VK_RETURN, 0));
            assert!(!main_return(VK_RETURN, EXTENDED_KEY as isize));
            assert!(!main_return(0x20, 0));
        }
    }
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
        'J' => launch_selected(ui, app),
        'A' => add_game(ui, app),
        'K' => check_game(ui, app),
        'I' => install_selected(ui, app),
        'U' => update_all(ui, app),
        'P' => edit_game(ui, app),
        'D' => uninstall_selected(ui, app),
        'E' => export_selected(ui, app),
        'M' => import_selected(ui, app),
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

fn log_line(ui: &Ui, msg: &str) {
    ui.log.append_text(msg);
    ui.log.append_text("\n");
    crate::core::logging::append(msg);
}

fn announce(ui: &Ui, msg: &str) {
    ui.frame.set_status_text(msg, 0);
    ui.status.update();
    log_line(ui, msg);
}

fn announce_end(ui: &Ui, msg: &str) {
    announce(ui, msg);
    crate::uia::notify(ui.frame.get_handle(), msg, true);
}

const MILESTONES: [i32; 3] = [75, 50, 25];

fn milestone(pct: i32, said: i32) -> Option<i32> {
    MILESTONES.into_iter().find(|&m| pct >= m).filter(|&m| m > said && pct < 100)
}

fn tell_progress(ui: &Ui, app: &Rc<RefCell<App>>, pct: i32) {
    let said = {
        let mut a = app.borrow_mut();
        milestone(pct, a.milestone).map(|m| {
            a.milestone = m;
            a.i18n.tf("progress_milestone", &m.to_string())
        })
    };
    if let Some(text) = said {
        crate::uia::notify(ui.frame.get_handle(), &text, false);
    }
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
        let source = Source::github();
        let cat = source.catalog().ok();
        let available = source.available_version();
        let _ = tx.send(Msg::Booted(cat, available));
        let launcher_update = crate::core::selfupdate::check().ok().flatten().map(|u| u.tag);
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
                    a.booted = true;
                }
                refresh_list(ui, app);
                let msg = {
                    let a = app.borrow();
                    let offline = a.cat.is_none() || a.available.trim().is_empty();
                    a.i18n.t(if offline { "ready_offline" } else { "ready" })
                };
                announce(ui, &msg);
                let dropped = app.borrow_mut().dropped.take();
                if let Some(dir) = dropped {
                    add_folder(ui, app, detect::resolve_game_dir(&dir));
                }
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
    let saved = save_change(app, |cfg| cfg.last_notified_tag = tag.to_string());
    let m = app.borrow().i18n.tf("launcher_update_ready", tag);
    announce(ui, &m);
    if let Err(e) = saved {
        log_line(ui, &save_failure(app, "config_save_failed", &e));
    }
}

fn pump(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    let rx_present = app.borrow().rx.is_some();
    if !rx_present {
        return;
    }
    let mut done: Option<Vec<Outcome>> = None;
    let mut crashed = false;
    let mut moved: Option<i32> = None;
    let mut update_checked: Option<Option<crate::core::selfupdate::LauncherUpdate>> = None;
    let mut update_applied: Option<Result<(), String>> = None;
    loop {
        let recv = {
            let a = app.borrow();
            a.rx.as_ref().unwrap().try_recv()
        };
        match recv {
            Ok(Msg::Started(i)) => {
                let header = {
                    let a = app.borrow();
                    (a.last_jobs.len() > 1).then(|| format!("== {} ==", a.last_jobs[i].dir.display()))
                };
                if let Some(h) = header {
                    log_line(ui, &h);
                }
            }
            Ok(Msg::Progress(i, file, done_count, total)) => {
                let pct = {
                    let mut a = app.borrow_mut();
                    a.progress[i] = (done_count, total);
                    run_percent(&a.progress)
                };
                ui.gauge.set_value(pct);
                moved = Some(pct);
                announce(ui, &format!("{} ({}%)", app.borrow().i18n.tf("downloading_file", &file), pct));
            }
            Ok(Msg::Finished(i, outcome)) => {
                let line = {
                    let mut a = app.borrow_mut();
                    a.progress[i] = (1, 1);
                    let pct = run_percent(&a.progress);
                    ui.gauge.set_value(pct);
                    moved = Some(pct);
                    (a.last_jobs.len() > 1).then(|| {
                        let job = &a.last_jobs[i];
                        format!("{}: {}", job.name, outcome_text(&a.i18n, job, &outcome))
                    })
                };
                if let Some(l) = line {
                    log_line(ui, &l);
                }
            }
            Ok(Msg::Done(outcomes)) => {
                done = Some(outcomes);
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
                crashed = true;
                break;
            }
        }
    }
    if let Some(pct) = moved.filter(|_| done.is_none() && !crashed) {
        tell_progress(ui, app, pct);
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
    if crashed {
        let jobs = app.borrow().last_jobs.len();
        done = Some((0..jobs).map(|_| Outcome::Failed("worker_crashed".to_string())).collect());
    }
    if let Some(outcomes) = done {
        app.borrow_mut().rx = None;
        set_busy(ui, app, false);
        finish_run(ui, app, outcomes);
        check_health(app);
    }
}

fn check_health(app: &Rc<RefCell<App>>) {
    let dirs: Vec<String> = app.borrow().cfg.games.iter().map(|g| g.path.clone()).collect();
    let (tx, rx) = std::sync::mpsc::channel();
    app.borrow_mut().health_rx = Some(rx);
    std::thread::spawn(move || {
        let found = dirs
            .into_iter()
            .map(|path| {
                let faults = status::faults(Path::new(&path));
                (path, faults)
            })
            .filter(|(_, faults)| !faults.is_empty())
            .collect();
        let _ = tx.send(found);
    });
}

fn pump_health(ui: &Ui, app: &Rc<RefCell<App>>) {
    let received = match app.borrow().health_rx.as_ref().map(Receiver::try_recv) {
        None | Some(Err(TryRecvError::Empty)) => return,
        Some(received) => received.ok(),
    };
    let changed = {
        let mut a = app.borrow_mut();
        a.health_rx = None;
        match received {
            Some(found) if found != a.health => {
                a.health = found;
                true
            }
            _ => false,
        }
    };
    if changed {
        refresh_list(ui, app);
    }
}

fn run_percent(progress: &[(u32, u32)]) -> i32 {
    let shares: u32 = progress.iter().map(|&(done, total)| if total == 0 { 0 } else { done * 100 / total }).sum();
    (shares / progress.len().max(1) as u32) as i32
}

fn outcome_text(i18n: &I18n, job: &Job, outcome: &Outcome) -> String {
    match outcome {
        Outcome::Installed(done) if !done.changed() => i18n.t("status_uptodate"),
        Outcome::Installed(done) => i18n.tf("done_installed", &done.version),
        Outcome::Skipped => i18n.tf("game_folder_missing", &job.dir.display().to_string()),
        Outcome::Failed(e) => i18n.tf("error", &i18n.t_err(e)),
    }
}

fn finish_run(ui: &Rc<Ui>, app: &Rc<RefCell<App>>, outcomes: Vec<Outcome>) {
    let (jobs, forewarned) = {
        let mut a = app.borrow_mut();
        (std::mem::take(&mut a.last_jobs), a.forewarned.take())
    };
    match (jobs.as_slice(), outcomes.as_slice()) {
        ([], _) => {
            let m = {
                let a = app.borrow();
                a.i18n.tf("error", &a.i18n.t("worker_crashed"))
            };
            announce_end(ui, &m);
            info(ui, app, &m);
            refresh_list(ui, app);
        }
        ([job], [outcome]) => finish_one(ui, app, job, outcome, forewarned.as_deref()),
        _ => finish_many(ui, app, &jobs, &outcomes),
    }
}

fn finish_one(ui: &Rc<Ui>, app: &Rc<RefCell<App>>, job: &Job, outcome: &Outcome, forewarned: Option<&Path>) {
    let jobs = std::slice::from_ref(job);
    let path_notices = if forewarned == Some(job.dir.as_path()) { Vec::new() } else { long_path_notices(app, jobs) };
    let e = match outcome {
        Outcome::Installed(done) => {
            let pending = app.borrow_mut().pending_edit.take();
            let edited_idx = pending.as_ref().map(|(idx, _)| *idx);
            if let Some((idx, entry)) = pending {
                if let Err(e) = save_edit(app, idx, entry) {
                    info(ui, app, &save_failure(app, "edit_save_failed", &e));
                }
            }
            let mut hints = adopt(ui, app, jobs, std::slice::from_ref(outcome));
            hints.extend(path_notices);
            refresh_list(ui, app);
            if let Some(idx) = edited_idx { ui.games.set_selection(idx as u32, true); }
            let m = app.borrow().i18n.tf("done_installed", &done.version);
            announce_end(ui, &m);
            for h in &hints {
                announce(ui, h);
            }
            info(ui, app, &std::iter::once(m).chain(hints).collect::<Vec<_>>().join("\n\n"));
            return;
        }
        Outcome::Skipped => crate::i18n::err_key("game_folder_missing", &job.dir.display().to_string()),
        Outcome::Failed(e) => e.clone(),
    };
    let m = {
        let a = app.borrow();
        a.i18n.tf("error", &a.i18n.t_err(&e))
    };
    let m = std::iter::once(m).chain(path_notices).collect::<Vec<_>>().join("\n\n");
    announce_end(ui, &m);
    let pending = app.borrow_mut().pending_edit.take();
    if let Some((idx, entry)) = pending {
        let warning = app.borrow().i18n.tf("edit_repatch_failed", &m);
        if confirm_invalid(ui, app, &[&warning]) {
            match save_edit(app, idx, entry) {
                Ok(()) => announce(ui, &app.borrow().i18n.t("edit_saved")),
                Err(e) => info(ui, app, &save_failure(app, "edit_save_failed", &e)),
            }
            refresh_list(ui, app);
            ui.games.set_selection(idx as u32, true);
        } else {
            edit_game_with_draft(ui, app, idx, entry);
        }
        return;
    }
    offer_retry(ui, app, &m, jobs.to_vec());
    refresh_list(ui, app);
}

fn finish_many(ui: &Rc<Ui>, app: &Rc<RefCell<App>>, jobs: &[Job], outcomes: &[Outcome]) {
    let mut hints = adopt(ui, app, jobs, outcomes);
    hints.extend(long_path_notices(app, jobs));
    refresh_list(ui, app);
    let failed: Vec<(&Job, &String)> = jobs
        .iter()
        .zip(outcomes)
        .filter_map(|(job, outcome)| match outcome {
            Outcome::Failed(e) => Some((job, e)),
            _ => None,
        })
        .collect();
    let (summary, failures) = {
        let a = app.borrow();
        let failures: Vec<String> = failed
            .iter()
            .map(|(job, e)| format!("{} {}", a.i18n.tf("summary_game_failed", &job.name), a.i18n.t_err(e)))
            .collect();
        (batch::Summary::of(outcomes).line(&a.i18n), failures)
    };
    announce_end(ui, &summary);
    for line in failures.iter().chain(&hints) {
        announce(ui, line);
    }
    let m = std::iter::once(summary).chain(failures).chain(hints).collect::<Vec<_>>().join("\n\n");
    let retry: Vec<Job> = failed.into_iter().map(|(job, _)| job.clone()).collect();
    if retry.is_empty() {
        info(ui, app, &m);
    } else {
        offer_retry(ui, app, &m, retry);
    }
}

fn offer_retry(ui: &Rc<Ui>, app: &Rc<RefCell<App>>, error: &str, jobs: Vec<Job>) {
    let (msg, caption) = {
        let a = app.borrow();
        (format!("{}\n\n{}", error, a.i18n.t("ask_retry")), a.i18n.t("app_title"))
    };
    if ask_yes_no(ui, &msg, &caption) {
        announce(ui, &app.borrow().i18n.t("retrying"));
        spawn_run(ui, app, jobs);
    }
}

fn adopt(ui: &Ui, app: &Rc<RefCell<App>>, jobs: &[Job], outcomes: &[Outcome]) -> Vec<String> {
    let (notices, saved) = {
        let mut a = app.borrow_mut();
        let a = &mut *a;
        let mut notices = Vec::new();
        for (job, outcome) in jobs.iter().zip(outcomes) {
            let one = std::slice::from_ref(job);
            for notice in batch::adopt(&mut a.cfg, a.cat.as_ref(), one, std::slice::from_ref(outcome)) {
                notices.push(of_game(jobs, job, a.i18n.t_err(&notice)));
            }
        }
        let saved = if notices.is_empty() { Ok(()) } else { a.cfg.save() };
        (notices, saved)
    };
    if let Err(e) = saved {
        info(ui, app, &save_failure(app, "edit_save_failed", &e));
    }
    notices
}

fn long_path_notices(app: &Rc<RefCell<App>>, jobs: &[Job]) -> Vec<String> {
    let a = app.borrow();
    jobs.iter()
        .filter_map(|job| convert::long_path_notice(&job.dir).map(|notice| of_game(jobs, job, a.i18n.t_err(&notice))))
        .collect()
}

fn of_game(jobs: &[Job], job: &Job, said: String) -> String {
    if jobs.len() > 1 { format!("{}: {}", job.name, said) } else { said }
}

fn row_label(app: &App, entry: &config::GameEntry, name: &str) -> String {
    let st_txt = app.i18n.t(status::entry_key(entry, &app.available));
    let prof = if entry.profile.is_empty() {
        String::new()
    } else {
        format!(", {}", app.i18n.tf("profile_label", &profile_name(app, &entry.profile)))
    };
    let repair = app
        .health
        .get(&entry.path)
        .and_then(|faults| status::repair_note(&app.i18n, "health_repair", faults))
        .map(|note| format!(". {}", note))
        .unwrap_or_default();
    format!("{} - {}{}{}", name, st_txt, prof, repair)
}

fn profile_name(app: &App, key: &str) -> String {
    app.i18n.t_err(&ops::profile_title(app.cat.as_ref(), key))
}

fn refresh_list(ui: &Ui, app: &Rc<RefCell<App>>) {
    let selected = ui.games.get_selection();
    ui.games.clear();
    let a = app.borrow();
    for (e, name) in a.cfg.games.iter().zip(game::list_names(&a.cfg.games)) {
        ui.games.append(&row_label(&a, e, &name));
    }
    if let Some(idx) = selected.filter(|&idx| idx < ui.games.get_count()) {
        ui.games.set_selection(idx, true);
    }
}

fn selected_index(ui: &Ui) -> Option<usize> {
    ui.games.get_selection().map(|s| s as usize)
}

fn info(ui: &Ui, app: &Rc<RefCell<App>>, msg: &str) {
    let caption = app.borrow().i18n.t("app_title");
    MessageDialog::builder(&ui.frame, msg, &caption).build().show_modal();
}

fn refuse(ui: &Ui, app: &Rc<RefCell<App>>, blocker: &Blocker) {
    let m = app.borrow().i18n.t_err(&blocker.message());
    info(ui, app, &m);
}

fn ensure_closed(ui: &Rc<Ui>, app: &Rc<RefCell<App>>, game: &ScannedGame) -> bool {
    match game.check_closed() {
        Ok(()) => true,
        Err(blocker) => {
            refuse(ui, app, &blocker);
            false
        }
    }
}

fn scan_game(ui: &Rc<Ui>, app: &Rc<RefCell<App>>, dir: PathBuf) -> ScannedGame {
    announce(ui, &app.borrow().i18n.t("checking_games"));
    ScannedGame::of(dir)
}

fn inspect(ui: &Rc<Ui>, app: &Rc<RefCell<App>>, dir: PathBuf) -> Inspection {
    announce(ui, &app.borrow().i18n.t("checking_games"));
    Inspection::of(dir, app.borrow().cat.as_ref(), &Source::github())
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

fn require_game_dir(ui: &Ui, app: &Rc<RefCell<App>>, idx: usize) -> Option<PathBuf> {
    let found = ops::game_dir(&app.borrow().cfg.games[idx]);
    found.map_err(|blocker| refuse(ui, app, &blocker)).ok()
}

fn spawn_run(ui: &Rc<Ui>, app: &Rc<RefCell<App>>, jobs: Vec<Job>) {
    let (tx, rx): (Sender<Msg>, Receiver<Msg>) = std::sync::mpsc::channel();
    {
        let mut a = app.borrow_mut();
        a.rx = Some(rx);
        a.progress = vec![(0, 0); jobs.len()];
        a.milestone = 0;
        a.last_jobs = jobs.clone();
    }
    set_busy(ui, app, true);
    ui.gauge.set_value(0);
    let cat = app.borrow().cat.clone();
    std::thread::spawn(move || {
        let outcomes = batch::run(&jobs, &Source::github(), cat.as_ref(), batch::WINDOW_THREADS, |event| {
            let _ = tx.send(match event {
                batch::Event::Started(i) => Msg::Started(i),
                batch::Event::Progress(i, file, done, total) => Msg::Progress(i, file.to_string(), done, total),
                batch::Event::Finished(i, outcome) => Msg::Finished(i, outcome.clone()),
            });
        });
        let _ = tx.send(Msg::Done(outcomes));
    });
}

fn catalog_ready(ui: &Ui, app: &Rc<RefCell<App>>) -> bool {
    let booted = app.borrow().booted;
    if !booted {
        info(ui, app, &app.borrow().i18n.t("catalog_loading_wait"));
    }
    booted
}

fn pick_game_folder(ui: &Ui, app: &Rc<RefCell<App>>) -> Option<PathBuf> {
    let prompt = app.borrow().i18n.t("pick_folder");
    let dlg = DirDialog::builder(&ui.frame, &prompt, "").build();
    if dlg.show_modal() != ID_OK {
        return None;
    }
    dlg.get_path().map(|p| detect::resolve_game_dir(&PathBuf::from(p)))
}

fn add_game(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    if !catalog_ready(ui, app) {
        return;
    }
    if let Some(dir) = pick_game_folder(ui, app) {
        add_folder(ui, app, dir);
    }
}

fn check_game(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    if !catalog_ready(ui, app) {
        return;
    }
    let Some((dir, listed)) = check_target(ui, app) else {
        return;
    };
    let game = inspect(ui, app, dir);
    let (title, verdict, text) = {
        let a = app.borrow();
        let report = game.report(a.cat.as_ref());
        let verdict = a.i18n.t_err(&report.verdict);
        let lines: Vec<String> = std::iter::once(verdict.clone())
            .chain(report.warning.iter().chain(&report.facts).map(|p| a.i18n.t_err(p)))
            .chain(std::iter::once(a.i18n.tf("list_path", &game.game.dir.display().to_string())))
            .collect();
        let name = listed.unwrap_or_else(|| game.game.name());
        (a.i18n.tf("check_report_title", &name), verdict, lines.join("\n"))
    };
    announce(ui, &verdict);
    show_report(ui, app, &title, &text);
}

fn check_target(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) -> Option<(PathBuf, Option<String>)> {
    if let Some(idx) = selected_index(ui) {
        let name = game::list_names(&app.borrow().cfg.games).swap_remove(idx);
        let (choices, which, caption) = {
            let a = app.borrow();
            let choices = vec![a.i18n.tf("check_selected", &name), a.i18n.t("check_other_folder")];
            (choices, a.i18n.t("check_which"), a.i18n.t("check_caption"))
        };
        let choice_refs: Vec<&str> = choices.iter().map(String::as_str).collect();
        let dlg = SingleChoiceDialog::builder(&ui.frame, &which, &caption, &choice_refs).build();
        if dlg.show_modal() != ID_OK {
            return None;
        }
        if dlg.get_selection() == 0 {
            return require_game_dir(ui, app, idx).map(|dir| (dir, Some(name)));
        }
    }
    pick_game_folder(ui, app).map(|dir| (dir, None))
}

fn show_report(ui: &Ui, app: &Rc<RefCell<App>>, title: &str, text: &str) {
    let close = app.borrow().i18n.t("edit_ok");
    let dlg = Dialog::builder(&ui.frame, title).with_size(640, 360).build();
    let layout = BoxSizer::builder(Orientation::Vertical).build();
    let report = TextCtrl::builder(&dlg)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::WordWrap)
        .build();
    report.set_value(text);
    report.set_name(title);
    layout.add(&report, 1, SizerFlag::Expand | SizerFlag::All, 8);
    let ok = Button::builder(&dlg).with_label(&close).build();
    layout.add(&ok, 0, SizerFlag::All, 6);
    dlg.set_sizer(layout, true);
    dlg.set_escape_id(ok.get_id());
    ok.set_default();
    report.set_insertion_point(0);
    report.set_focus();
    let d = dlg;
    ok.on_click(move |_| d.end_modal(ID_OK));
    dlg.show_modal();
    dlg.destroy();
}

fn add_folder(ui: &Rc<Ui>, app: &Rc<RefCell<App>>, path: PathBuf) {
    let game = inspect(ui, app, path);
    if let Some(blocker) = game.blocker() {
        refuse(ui, app, &blocker);
        return;
    }
    if game.doubtful() && !confirm_preload_warning(ui, app) {
        return;
    }
    let warning = game.long_path_notice().map(|n| app.borrow().i18n.t_err(&n));
    if let Some(o) = &game.offer {
        if !confirm_conversion(ui, app, o, warning.as_deref()) {
            return;
        }
    }
    let (profile, mode) = match &game.offer {
        Some(o) if !o.profile.is_empty() => (Some(o.profile.clone()), "specific".to_string()),
        Some(_) => choose_profile(ui, &game, app, None),
        None => choose_profile(ui, &game, app, warning.as_deref()),
    };
    let profile = match profile {
        Some(p) => p,
        None => return,
    };
    let entry = ops::new_entry(&game.game, &profile, &mode, app.borrow().cat.as_ref());
    if detect::root_exe_names(&game.game.dir).is_empty() && !confirm_invalid(ui, app, &["invalid_executable"]) {
        return;
    }
    let job = Job::of(&entry, &game::display_name(&entry));
    if let Err(e) = save_change(app, |cfg| cfg.upsert_game(entry)) {
        info(ui, app, &save_failure(app, "edit_save_failed", &e));
        return;
    }
    refresh_list(ui, app);
    announce(ui, &app.borrow().i18n.tf("installing", &job.dir.display().to_string()));
    app.borrow_mut().forewarned = warning.is_some().then(|| job.dir.clone());
    spawn_run(ui, app, vec![job]);
}

fn confirm_preload_warning(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) -> bool {
    let (msg, caption) = {
        let a = app.borrow();
        (a.i18n.t("preload_missing_warn"), a.i18n.t("app_title"))
    };
    ask_yes_no(ui, &msg, &caption)
}

fn confirm_conversion(ui: &Rc<Ui>, app: &Rc<RefCell<App>>, offer: &convert::Offer, warning: Option<&str>) -> bool {
    let (msg, caption) = {
        let a = app.borrow();
        let risk = offer.experimental.then(|| a.i18n.tf("convert_experimental", &offer.display));
        let question = a.i18n.tf("convert_confirm", &offer.display);
        (warned(warning, risk.into_iter().chain([question])), a.i18n.t("app_title"))
    };
    ask_yes_no(ui, &msg, &caption)
}

fn warned(warning: Option<&str>, text: impl IntoIterator<Item = String>) -> String {
    warning.map(str::to_string).into_iter().chain(text).collect::<Vec<_>>().join("\n\n")
}

fn ask_yes_no(ui: &Ui, msg: &str, caption: &str) -> bool {
    MessageDialog::builder(&ui.frame, msg, caption)
        .with_style(MessageDialogStyle::OK | MessageDialogStyle::Cancel)
        .build()
        .show_modal()
        == ID_OK
}

fn choose_profile(
    ui: &Rc<Ui>,
    game: &Inspection,
    app: &Rc<RefCell<App>>,
    warning: Option<&str>,
) -> (Option<String>, String) {
    let a = app.borrow();
    let kept_generic = game.sealed_profile.as_deref() == Some("generic");
    let detected = if kept_generic {
        game.detected_profile.clone()
    } else {
        game.suggested_profile().map(str::to_string)
    };

    let generic = a.i18n.t("install_generic");
    let mut choices: Vec<String> =
        detected.iter().map(|key| a.i18n.tf("install_specific", &profile_name(&a, key))).collect();
    choices.insert(if kept_generic { 0 } else { choices.len() }, generic.clone());
    let manual = a.i18n.t("choose_manual");
    let has_specific_profiles = a.cat.as_ref().is_some_and(|c| c.specific().next().is_some());
    if has_specific_profiles {
        choices.push(manual.clone());
    }
    let headline = match (&game.sealed_profile, &detected) {
        _ if kept_generic => a.i18n.t("installed_generic"),
        (Some(key), _) => a.i18n.tf("installed_profile", &profile_name(&a, key)),
        (None, Some(key)) => a.i18n.tf("detected_profile", &profile_name(&a, key)),
        (None, None) => a.i18n.t("not_detected"),
    };
    let prompt = warned(warning, [headline, a.i18n.t("generic_hint")]);
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
            for p in cat.specific() {
                keys.push(p.key.clone());
                names.push(p.display.clone());
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
    let path = match require_game_dir(ui, app, idx) {
        Some(p) => p,
        None => return,
    };
    let job = {
        let a = app.borrow();
        Job::of(&a.cfg.games[idx], &game::display_name(&a.cfg.games[idx]))
    };
    if let Err(blocker) = ops::check_writable(&path) {
        refuse(ui, app, &blocker);
        return;
    }
    let game = scan_game(ui, app, path);
    if !ensure_closed(ui, app, &game) {
        return;
    }
    announce(ui, &app.borrow().i18n.tf("installing", &game.dir.display().to_string()));
    spawn_run(ui, app, vec![job]);
}

fn update_all(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    let jobs = batch::installed_jobs(&app.borrow().cfg);
    if jobs.is_empty() {
        info(ui, app, &app.borrow().i18n.t("nothing_to_update"));
        return;
    }
    announce(ui, &app.borrow().i18n.t("updating_all"));
    spawn_run(ui, app, jobs);
}

fn save_change(app: &Rc<RefCell<App>>, change: impl FnOnce(&mut config::Config)) -> Result<(), String> {
    app.borrow_mut().cfg.commit(|cfg| {
        change(cfg);
        Ok(())
    })
}

fn save_failure(app: &Rc<RefCell<App>>, key: &str, error: &str) -> String {
    let a = app.borrow();
    a.i18n.tf(key, &a.i18n.t_err(error))
}

fn save_edit(app: &Rc<RefCell<App>>, idx: usize, entry: config::GameEntry) -> Result<(), String> {
    app.borrow_mut().cfg.commit(|cfg| cfg.edit_game(idx, entry).map(|_| ()).map_err(str::to_string))
}

fn launch_selected(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    let idx = match require_selection(ui, app) { Some(i) => i, None => return };
    launch_index(ui, app, idx);
}

fn launch_index(ui: &Rc<Ui>, app: &Rc<RefCell<App>>, idx: usize) {
    if app.borrow().busy {
        announce(ui, &app.borrow().i18n.t("working_wait"));
        return;
    }
    if idx >= app.borrow().cfg.games.len() {
        info(ui, app, &app.borrow().i18n.t("no_selection"));
        return;
    }
    crate::core::logging::append(&format!("Launch requested for row {idx}"));
    let mut entry = app.borrow().cfg.games[idx].clone();
    if game::adopt_accessible(&mut entry) {
        let saved = {
            let mut a = app.borrow_mut();
            a.cfg.games[idx].executable = entry.executable.clone();
            a.cfg.save()
        };
        if let Err(e) = saved {
            info(ui, app, &save_failure(app, "edit_save_failed", &e));
        }
    }
    match game::launch_plan(&entry) {
        game::Launch::Start => {}
        game::Launch::Blocked(why) => {
            let m = app.borrow().i18n.t_err(&why);
            info(ui, app, &m);
            return;
        }
        game::Launch::Choose(exes) => {
            let scanned = scan_game(ui, app, PathBuf::from(&entry.path));
            let known = app.borrow().cat.as_ref().map(|c| c.exes_of(&entry.profile)).unwrap_or_default();
            let safe = game::pick_executable(&scanned.scan, &scanned.dir, &known);
            let asks_each_time = entry.executable.is_empty();
            let (exe, remember) = match choose_executable(ui, app, &exes, safe.as_deref(), !asks_each_time) {
                Some(choice) => choice,
                None => {
                    ui.frame.set_status_text(&app.borrow().i18n.t("ready"), 0);
                    return;
                }
            };
            entry.executable = exe;
            if remember {
                let saved = {
                    let mut a = app.borrow_mut();
                    a.cfg.games[idx].executable = entry.executable.clone();
                    a.cfg.save()
                };
                if let Err(e) = saved {
                    info(ui, app, &save_failure(app, "edit_save_failed", &e));
                }
            }
        }
    }
    match game::launch(&entry) {
        Ok(()) => announce(ui, &app.borrow().i18n.tf("launching", &game::display_name(&entry))),
        Err(e) => {
            crate::core::logging::append(&format!("Launch failed: {e}"));
            info(ui, app, &app.borrow().i18n.t_err(&e));
        }
    }
}

fn choose_executable(
    ui: &Ui,
    app: &Rc<RefCell<App>>,
    exes: &[String],
    suggested: Option<&str>,
    remember_first: bool,
) -> Option<(String, bool)> {
    let a = app.borrow();
    let dlg = Dialog::builder(&ui.frame, &a.i18n.t("pick_exe_title")).with_size(520, 360).build();
    let layout = BoxSizer::builder(Orientation::Vertical).build();
    let prompt = StaticText::builder(&dlg).with_label(&a.i18n.t("pick_exe_prompt")).build();
    let list = ListBox::builder(&dlg).build();
    list.set_name(&a.i18n.t("pick_exe_prompt"));
    for e in exes {
        list.append(e);
    }
    let current = suggested.and_then(|s| exes.iter().position(|e| e.eq_ignore_ascii_case(s))).unwrap_or(0);
    list.set_selection(current as u32, true);
    let remember = CheckBox::builder(&dlg).with_label(&a.i18n.t("pick_exe_remember")).with_value(remember_first).build();
    remember.set_name(&a.i18n.t("pick_exe_remember"));
    layout.add(&prompt, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, 8);
    layout.add(&list, 1, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, 8);
    layout.add(&remember, 0, SizerFlag::All, 8);
    let buttons = BoxSizer::builder(Orientation::Horizontal).build();
    let ok = Button::builder(&dlg).with_label(&a.i18n.t("edit_ok")).build();
    let cancel = Button::builder(&dlg).with_label(&a.i18n.t("edit_cancel")).build();
    buttons.add(&ok, 0, SizerFlag::All, 6);
    buttons.add(&cancel, 0, SizerFlag::All, 6);
    layout.add_sizer(&buttons, 0, SizerFlag::All, 6);
    dlg.set_sizer(layout, true);
    dlg.set_escape_id(cancel.get_id());
    drop(a);
    ok.set_default();
    list.set_focus();
    let d = dlg;
    ok.on_click(move |_| d.end_modal(ID_OK));
    let d = dlg;
    cancel.on_click(move |_| d.end_modal(ID_CANCEL));
    let d = dlg;
    list.on_item_double_clicked(move |_| d.end_modal(ID_OK));
    let chosen = if dlg.show_modal() == ID_OK {
        list.get_selection().and_then(|i| exes.get(i as usize)).cloned().map(|e| (e, remember.get_value()))
    } else {
        None
    };
    dlg.destroy();
    chosen
}

fn confirm_invalid(ui: &Ui, app: &Rc<RefCell<App>>, errors: &[&str]) -> bool {
    let a = app.borrow();
    let text = format!("{}\n{}", a.i18n.t("edit_invalid"), errors.iter()
        .map(|e| format!("• {}", a.i18n.t(e))).collect::<Vec<_>>().join("\n"));
    let dlg = Dialog::builder(&ui.frame, &a.i18n.t("edit_title")).with_size(640, 320).build();
    let layout = BoxSizer::builder(Orientation::Vertical).build();
    let label = StaticText::builder(&dlg).with_label(&text).build();
    label.wrap(600);
    layout.add(&label, 1, SizerFlag::Expand | SizerFlag::All, 12);
    let buttons = BoxSizer::builder(Orientation::Horizontal).build();
    let correct = Button::builder(&dlg).with_label(&a.i18n.t("edit_correct")).build();
    let cont = Button::builder(&dlg).with_label(&a.i18n.t("edit_continue")).build();
    buttons.add(&correct, 0, SizerFlag::All, 6);
    buttons.add(&cont, 0, SizerFlag::All, 6);
    layout.add_sizer(&buttons, 0, SizerFlag::All, 6);
    dlg.set_sizer(layout, true);
    dlg.set_escape_id(correct.get_id());
    drop(a);
    correct.set_default();
    correct.set_focus();
    let d = dlg;
    correct.on_click(move |_| d.end_modal(ID_CANCEL));
    let d = dlg;
    cont.on_click(move |_| d.end_modal(ID_OK));
    let confirmed = dlg.show_modal() == ID_OK;
    dlg.destroy();
    confirmed
}

fn edit_game(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    let idx = match require_selection(ui, app) { Some(i) => i, None => return };
    let draft = app.borrow().cfg.games[idx].clone();
    edit_game_with_draft(ui, app, idx, draft);
}

fn edit_game_with_draft(ui: &Rc<Ui>, app: &Rc<RefCell<App>>, idx: usize, mut draft: config::GameEntry) {
    if draft.name.is_empty() {
        draft.name = PathBuf::from(&draft.path).file_name()
            .map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    }
    loop {
        let a = app.borrow();
        let dlg = Dialog::builder(&ui.frame, &a.i18n.t("edit_title")).with_size(600, 390).build();
        let layout = BoxSizer::builder(Orientation::Vertical).build();
        let label = |key: &str| a.i18n.t(key);
        let name_label = StaticText::builder(&dlg).with_label(&label("edit_name")).build();
        let name = TextCtrl::builder(&dlg).build();
        name.set_value(&draft.name);
        name.set_name(&label("edit_name"));
        layout.add(&name_label, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, 8);
        layout.add(&name, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, 8);

        let profile_label = StaticText::builder(&dlg).with_label(&label("edit_profile")).build();
        let profile = Choice::builder(&dlg).build();
        profile.set_name(&label("edit_profile"));
        let mut keys = vec![draft.profile.clone()];
        if !keys.contains(&"generic".to_string()) { keys.push("generic".into()); }
        if let Some(cat) = &a.cat {
            for p in &cat.profiles {
                if !keys.contains(&p.key) { keys.push(p.key.clone()); }
            }
        }
        for key in &keys { profile.append(&profile_name(&a, key)); }
        profile.set_selection(0);
        layout.add(&profile_label, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, 8);
        layout.add(&profile, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, 8);

        let path_label = StaticText::builder(&dlg).with_label(&label("edit_path")).build();
        let path = TextCtrl::builder(&dlg).build();
        path.set_value(&draft.path);
        path.set_name(&label("edit_path"));
        layout.add(&path_label, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, 8);
        layout.add(&path, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, 8);

        let exe_label = StaticText::builder(&dlg).with_label(&label("edit_executable")).build();
        let exe = Choice::builder(&dlg).build();
        exe.set_name(&label("edit_executable"));
        let mut exes = detect::root_exe_names(&PathBuf::from(&draft.path));
        if !draft.executable.is_empty() && !exes.iter().any(|e| e.eq_ignore_ascii_case(&draft.executable)) {
            exes.insert(0, draft.executable.clone());
        }
        exe.append(&label("exe_ask_on_play"));
        for e in &exes { exe.append(e); }
        let current = exes.iter().position(|e| e.eq_ignore_ascii_case(&draft.executable)).map(|i| i + 1).unwrap_or(0);
        exe.set_selection(current as u32);
        layout.add(&exe_label, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, 8);
        layout.add(&exe, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, 8);

        let buttons = BoxSizer::builder(Orientation::Horizontal).build();
        let ok = Button::builder(&dlg).with_label(&label("edit_ok")).build();
        let cancel = Button::builder(&dlg).with_label(&label("edit_cancel")).build();
        buttons.add(&ok, 0, SizerFlag::All, 6);
        buttons.add(&cancel, 0, SizerFlag::All, 6);
        layout.add_sizer(&buttons, 0, SizerFlag::All, 6);
        dlg.set_sizer(layout, true);
        dlg.set_escape_id(cancel.get_id());
        drop(a);
        ok.set_default();
        name.set_focus();
        let d = dlg;
        ok.on_click(move |_| d.end_modal(ID_OK));
        let d = dlg;
        cancel.on_click(move |_| d.end_modal(ID_CANCEL));
        if dlg.show_modal() != ID_OK {
            dlg.destroy();
            return;
        }
        draft.name = name.get_value().trim().to_string();
        draft.path = path.get_value().trim().to_string();
        draft.executable = exe.get_selection().filter(|&i| i > 0)
            .and_then(|i| exes.get(i as usize - 1)).cloned().unwrap_or_default();
        draft.profile = profile.get_selection().and_then(|i| keys.get(i as usize)).cloned().unwrap_or_default();
        draft.profile_mode = if draft.profile == "generic" { "generic" } else { "specific" }.into();
        dlg.destroy();

        let errors = game::validate(&draft);
        let conflict = {
            let mut probe = app.borrow().cfg.clone();
            probe.edit_game(idx, draft.clone()).err()
        };
        if let Some(key) = conflict {
            info(ui, app, &app.borrow().i18n.t(key));
            continue;
        }
        if !errors.is_empty() && !confirm_invalid(ui, app, &errors) { continue; }
        let original = app.borrow().cfg.games[idx].clone();
        let changed = original.profile_changed(&draft);
        if changed && errors.is_empty() {
            let dir = PathBuf::from(&draft.path);
            if !apply::can_write(&dir) {
                if !confirm_invalid(ui, app, &["no_write_perm"]) { continue; }
            } else {
                let scanned = scan_game(ui, app, dir.clone());
                if !ensure_closed(ui, app, &scanned) { continue; }
                app.borrow_mut().pending_edit = Some((idx, draft.clone()));
                announce(ui, &app.borrow().i18n.tf("profile_changed", &draft.profile));
                spawn_run(ui, app, vec![Job::of(&draft, &draft.name)]);
                return;
            }
        }
        match save_edit(app, idx, draft.clone()) {
            Ok(()) => {
                refresh_list(ui, app);
                ui.games.set_selection(idx as u32, true);
                announce(ui, &app.borrow().i18n.t("edit_saved"));
                check_health(app);
            }
            Err(e) => info(ui, app, &save_failure(app, "edit_save_failed", &e)),
        }
        return;
    }
}

fn uninstall_selected(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    let idx = match require_selection(ui, app) {
        Some(i) => i,
        None => return,
    };
    let path = match require_game_dir(ui, app, idx) {
        Some(p) => p,
        None => return,
    };
    let game = scan_game(ui, app, path);
    if !ensure_closed(ui, app, &game) {
        return;
    }
    let name = game::list_names(&app.borrow().cfg.games).swap_remove(idx);
    let Some(keep) = ask_uninstall(ui, app, &name) else {
        return;
    };
    match apply::run_uninstall(&game.dir, keep) {
        Ok(notes) => {
            let kept = if keep { player_data::kept(&game.dir) } else { Vec::new() };
            let (done, lines) = {
                let a = app.borrow();
                let done = if kept.is_empty() {
                    a.i18n.t("done_uninstalled")
                } else {
                    let data = data_dir(&game.dir).display().to_string();
                    a.i18n.tfn("uninstall_kept_data", &[&name, &data, &player_data::listed(&a.i18n, &kept)])
                };
                (done, notes.iter().map(|n| a.i18n.t_err(n)).collect::<Vec<String>>())
            };
            for l in &lines {
                log_line(ui, l);
            }
            announce(ui, &done);
            if !kept.is_empty() || !lines.is_empty() {
                info(ui, app, &std::iter::once(done).chain(lines).collect::<Vec<_>>().join("\n\n"));
            }
        }
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
    check_health(app);
}

fn ask_uninstall(ui: &Ui, app: &Rc<RefCell<App>>, name: &str) -> Option<bool> {
    let a = app.borrow();
    let dlg = Dialog::builder(&ui.frame, &a.i18n.t("uninstall")).with_size(640, 300).build();
    let layout = BoxSizer::builder(Orientation::Vertical).build();
    let label = StaticText::builder(&dlg).with_label(&a.i18n.tf("uninstall_ask", name)).build();
    label.wrap(600);
    layout.add(&label, 1, SizerFlag::Expand | SizerFlag::All, 12);
    let buttons = BoxSizer::builder(Orientation::Horizontal).build();
    let keep = Button::builder(&dlg).with_label(&a.i18n.t("uninstall_keep_btn")).build();
    let delete = Button::builder(&dlg).with_label(&a.i18n.t("uninstall_delete_btn")).build();
    let cancel = Button::builder(&dlg).with_label(&a.i18n.t("edit_cancel")).build();
    for b in [&keep, &delete, &cancel] {
        buttons.add(b, 0, SizerFlag::All, 6);
    }
    layout.add_sizer(&buttons, 0, SizerFlag::All, 6);
    dlg.set_sizer(layout, true);
    dlg.set_escape_id(cancel.get_id());
    drop(a);
    keep.set_default();
    keep.set_focus();
    let d = dlg;
    keep.on_click(move |_| d.end_modal(ID_YES));
    let d = dlg;
    delete.on_click(move |_| d.end_modal(ID_NO));
    let d = dlg;
    cancel.on_click(move |_| d.end_modal(ID_CANCEL));
    let answer = match dlg.show_modal() {
        ID_YES => Some(true),
        ID_NO => Some(false),
        _ => None,
    };
    dlg.destroy();
    answer
}

fn export_selected(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    let Some(idx) = require_selection(ui, app) else {
        return;
    };
    let Some(dir) = require_game_dir(ui, app, idx) else {
        return;
    };
    let name = game::list_names(&app.borrow().cfg.games).swap_remove(idx);
    if player_data::saved(&dir).is_empty() {
        info(ui, app, &app.borrow().i18n.tf("export_nothing", &name));
        return;
    }
    let saving = FileDialogStyle::Save | FileDialogStyle::OverwritePrompt;
    let Some(path) = pick_file(ui, app, "export_title", saving, &player_data::export_name(&name)) else {
        return;
    };
    let m = {
        let a = app.borrow();
        match player_data::export(&dir, Path::new(&path)) {
            Ok(labels) => a.i18n.tfn("export_done", &[&name, &path, &player_data::listed(&a.i18n, &labels)]),
            Err(e) => a.i18n.tf("error", &a.i18n.t_err(&e)),
        }
    };
    announce(ui, &m);
    info(ui, app, &m);
}

fn import_selected(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    let Some(idx) = require_selection(ui, app) else {
        return;
    };
    let Some(dir) = require_game_dir(ui, app, idx) else {
        return;
    };
    let game = scan_game(ui, app, dir);
    if !ensure_closed(ui, app, &game) {
        return;
    }
    let opening = FileDialogStyle::Open | FileDialogStyle::FileMustExist;
    let Some(path) = pick_file(ui, app, "import_title", opening, "") else {
        return;
    };
    let name = game::list_names(&app.borrow().cfg.games).swap_remove(idx);
    let lines: Vec<String> = {
        let a = app.borrow();
        match player_data::import(&game.dir, Path::new(&path)) {
            Ok(done) => std::iter::once(a.i18n.tfn("import_done", &[&name, &path]))
                .chain(done.lines(&a.i18n, a.cat.as_ref()))
                .collect(),
            Err(e) => vec![a.i18n.tf("error", &a.i18n.t_err(&e))],
        }
    };
    for line in &lines {
        announce(ui, line);
    }
    info(ui, app, &lines.join("\n"));
}

fn pick_file(ui: &Ui, app: &Rc<RefCell<App>>, title: &str, style: FileDialogStyle, file: &str) -> Option<String> {
    let dlg = {
        let a = app.borrow();
        FileDialog::builder(&ui.frame)
            .with_message(&a.i18n.t(title))
            .with_default_file(file)
            .with_wildcard(&a.i18n.t("data_zip_filter"))
            .with_style(style)
            .build()
    };
    let picked = if dlg.show_modal() == ID_OK { dlg.get_path() } else { None };
    dlg.destroy();
    picked
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
    if !ask_yes_no(ui, &confirm, &caption) {
        return;
    }
    let path = app.borrow().cfg.games[idx].path.clone();
    if let Err(e) = save_change(app, |cfg| cfg.remove_game(&path)) {
        info(ui, app, &save_failure(app, "remove_save_failed", &e));
        return;
    }
    announce(ui, &app.borrow().i18n.t("removed_from_list"));
    refresh_list(ui, app);
}

fn options_dialog(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    let (choices, title) = {
        let a = app.borrow();
        let names: Vec<String> = crate::i18n::LANGS.iter().map(|code| a.i18n.t(&format!("lang_{}", code))).collect();
        (names, a.i18n.t("language"))
    };
    let choice_refs: Vec<&str> = choices.iter().map(|s| s.as_str()).collect();
    let dlg = SingleChoiceDialog::builder(&ui.frame, &title, &title, &choice_refs).build();
    if dlg.show_modal() != ID_OK {
        return;
    }
    let sel = dlg.get_selection();
    if sel < 0 || sel as usize >= crate::i18n::LANGS.len() {
        return;
    }
    let lang = crate::i18n::LANGS[sel as usize];
    if let Err(e) = save_change(app, |cfg| cfg.set_language(lang)) {
        info(ui, app, &save_failure(app, "config_save_failed", &e));
        return;
    }
    app.borrow_mut().i18n.set_lang(lang);
    info(ui, app, &app.borrow().i18n.t("lang_changed"));
}

fn check_launcher_update(ui: &Rc<Ui>, app: &Rc<RefCell<App>>) {
    let (tx, rx): (Sender<Msg>, Receiver<Msg>) = std::sync::mpsc::channel();
    app.borrow_mut().rx = Some(rx);
    set_busy(ui, app, true);
    announce(ui, &app.borrow().i18n.t("check_launcher_update"));
    std::thread::spawn(move || {
        let _ = tx.send(Msg::UpdateChecked(crate::core::selfupdate::check().ok().flatten()));
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
    if !ask_yes_no(ui, &msg, &caption) {
        return;
    }
    let (tx, rx): (Sender<Msg>, Receiver<Msg>) = std::sync::mpsc::channel();
    app.borrow_mut().rx = Some(rx);
    set_busy(ui, app, true);
    std::thread::spawn(move || {
        let _ = tx.send(Msg::UpdateApplied(crate::core::selfupdate::apply(&u)));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_milestone_is_said_once_only_the_highest_one_reached_and_never_at_the_end() {
        let mut said = 0;
        let heard: Vec<i32> = [3, 10, 26, 27, 49, 50, 50, 80, 99, 100]
            .into_iter()
            .filter_map(|pct| {
                let reached = milestone(pct, said)?;
                said = reached;
                Some(reached)
            })
            .collect();
        assert_eq!(heard, [25, 50, 75]);
        assert_eq!(milestone(100, 0), None, "el final lo dice el aviso del final");
        assert_eq!(milestone(90, 0), Some(75), "un salto dice solo el hito más alto");
    }
}
