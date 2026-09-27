use std::sync::atomic::{AtomicBool, Ordering};

pub use imp::{catch_interrupts, desktop_visible, for_console_run, leave_for_gui, window};

static SOFT_STOP: AtomicBool = AtomicBool::new(false);

static STOP_ASKED: AtomicBool = AtomicBool::new(false);

pub fn soft_stop(on: bool) {
    SOFT_STOP.store(on, Ordering::SeqCst);
}

pub fn stop_asked() -> bool {
    STOP_ASKED.load(Ordering::SeqCst)
}

#[cfg(windows)]
mod imp {
    use std::num::NonZeroIsize;
    use std::sync::atomic::Ordering;

    use super::{SOFT_STOP, STOP_ASKED};

    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const SW_HIDE: i32 = 0;
    const CTRL_BREAK_EVENT: u32 = 1;
    const UOI_FLAGS: i32 = 1;
    const WSF_VISIBLE: u32 = 1;

    #[repr(C)]
    #[derive(Default)]
    struct UserObjectFlags {
        _inherit: i32,
        _reserved: i32,
        flags: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetConsoleWindow() -> isize;
        fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
        fn FreeConsole() -> i32;
        fn AllocConsole() -> i32;
        fn GetStdHandle(which: u32) -> isize;
        fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
    }

    #[link(name = "user32")]
    extern "system" {
        fn ShowWindow(window: isize, show: i32) -> i32;
        fn GetProcessWindowStation() -> isize;
        fn GetUserObjectInformationW(object: isize, index: i32, info: *mut UserObjectFlags, length: u32,
            needed: *mut u32) -> i32;
    }

    pub fn leave_for_gui() {
        unsafe {
            let window = GetConsoleWindow();
            if window == 0 {
                return;
            }
            if processes() == 1 {
                ShowWindow(window, SW_HIDE);
            }
            FreeConsole();
        }
    }

    pub fn for_console_run() -> bool {
        unsafe {
            let output = GetStdHandle(STD_OUTPUT_HANDLE);
            if GetConsoleWindow() == 0 && (output == 0 || output == -1) {
                AllocConsole();
            }
        }
        processes() == 1
    }

    fn processes() -> u32 {
        let mut list = [0u32; 4];
        unsafe { GetConsoleProcessList(list.as_mut_ptr(), list.len() as u32) }
    }

    pub fn catch_interrupts() {
        unsafe {
            SetConsoleCtrlHandler(Some(on_interrupt), 1);
        }
    }

    unsafe extern "system" fn on_interrupt(event: u32) -> i32 {
        if event > CTRL_BREAK_EVENT {
            return 0;
        }
        if SOFT_STOP.load(Ordering::SeqCst) && !STOP_ASKED.swap(true, Ordering::SeqCst) {
            return 1;
        }
        std::process::exit(crate::cli::CANCELLED)
    }

    pub fn window() -> Option<NonZeroIsize> {
        NonZeroIsize::new(unsafe { GetConsoleWindow() })
    }

    pub fn desktop_visible() -> bool {
        let mut info = UserObjectFlags::default();
        let mut needed = 0u32;
        let size = std::mem::size_of::<UserObjectFlags>() as u32;
        let read =
            unsafe { GetUserObjectInformationW(GetProcessWindowStation(), UOI_FLAGS, &mut info, size, &mut needed) };
        read != 0 && info.flags & WSF_VISIBLE != 0
    }
}

#[cfg(not(windows))]
mod imp {
    use std::num::NonZeroIsize;

    pub fn leave_for_gui() {}

    pub fn for_console_run() -> bool {
        false
    }

    pub fn catch_interrupts() {}

    pub fn window() -> Option<NonZeroIsize> {
        None
    }

    pub fn desktop_visible() -> bool {
        false
    }
}
