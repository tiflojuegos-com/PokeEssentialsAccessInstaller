pub use imp::notify;

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::sync::OnceLock;

    const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x0800;
    const KIND_ACTION_COMPLETED: i32 = 2;
    const KIND_OTHER: i32 = 4;
    const IMPORTANT_MOST_RECENT: i32 = 1;
    const ACTIVITY: &str = "PokeEssentialsAccess.Progress";

    type HostProvider = unsafe extern "system" fn(isize, *mut *mut c_void) -> i32;
    type RaiseNotification = unsafe extern "system" fn(*mut c_void, i32, i32, *mut u16, *mut u16) -> i32;
    type Release = unsafe extern "system" fn(*mut c_void) -> u32;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryExW(name: *const u16, file: isize, flags: u32) -> isize;
        fn GetProcAddress(module: isize, name: *const u8) -> Option<unsafe extern "system" fn() -> isize>;
    }

    #[link(name = "oleaut32")]
    extern "system" {
        fn SysAllocString(text: *const u16) -> *mut u16;
        fn SysFreeString(text: *mut u16);
    }

    struct Uia {
        host: HostProvider,
        raise: RaiseNotification,
    }

    fn uia() -> Option<&'static Uia> {
        static FOUND: OnceLock<Option<Uia>> = OnceLock::new();
        FOUND
            .get_or_init(|| unsafe {
                let module = LoadLibraryExW(wide("UIAutomationCore.dll").as_ptr(), 0, LOAD_LIBRARY_SEARCH_SYSTEM32);
                if module == 0 {
                    return None;
                }
                let host = GetProcAddress(module, c"UiaHostProviderFromHwnd".as_ptr().cast())?;
                let raise = GetProcAddress(module, c"UiaRaiseNotificationEvent".as_ptr().cast())?;
                Some(Uia {
                    host: std::mem::transmute::<unsafe extern "system" fn() -> isize, HostProvider>(host),
                    raise: std::mem::transmute::<unsafe extern "system" fn() -> isize, RaiseNotification>(raise),
                })
            })
            .as_ref()
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    pub fn notify(window: *mut c_void, text: &str, finished: bool) {
        let Some(uia) = uia().filter(|_| !window.is_null()) else {
            return;
        };
        let kind = if finished { KIND_ACTION_COMPLETED } else { KIND_OTHER };
        unsafe {
            let mut provider = std::ptr::null_mut();
            if (uia.host)(window as isize, &mut provider) < 0 || provider.is_null() {
                return;
            }
            let said = SysAllocString(wide(text).as_ptr());
            let activity = SysAllocString(wide(ACTIVITY).as_ptr());
            if !said.is_null() && !activity.is_null() {
                (uia.raise)(provider, kind, IMPORTANT_MOST_RECENT, said, activity);
            }
            SysFreeString(said);
            SysFreeString(activity);
            let release = *(*(provider as *const *const Release)).add(2);
            release(provider);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn notify(_window: *mut std::ffi::c_void, _text: &str, _finished: bool) {}
}
