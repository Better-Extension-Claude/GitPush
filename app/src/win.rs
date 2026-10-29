//! Fenêtres Win32 : trouver Claude, coller nos fenêtres dessus (fenêtres « possédées »).

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, BOOL, ERROR_ALREADY_EXISTS, HWND, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumWindows, GetClassNameW, GetMessageW, SetTimer, EVENT_OBJECT_LOCATIONCHANGE, MSG, OBJID_WINDOW,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_TIMER, GetCursorPos, GetWindow, GetWindowLongPtrW, GetWindowTextW, GetWindowThreadProcessId,
    IsIconic, IsWindowVisible, SetWindowLongPtrW, SetWindowPos, GWLP_HWNDPARENT,
    GW_OWNER, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
};

pub const BUTTON_TITLE: &str = "GitPush-bouton";
pub const MODAL_TITLE: &str = "GitPush";

#[derive(Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Default)]
pub struct Shared {
    pub claude: isize,
    pub button: isize,
    pub rect: Rect,
    pub scale: f32,
    pub offset: (i32, i32),
}

pub type SharedRef = Arc<Mutex<Shared>>;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Une seule instance : le hook de Claude peut nous lancer plusieurs fois.
pub fn single_instance() -> bool {
    let name = wide("Local\\GitPush-overlay");
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
        !h.is_null() && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

fn text_of(hwnd: HWND, class: bool) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe {
        if class {
            GetClassNameW(hwnd, buf.as_mut_ptr(), 256)
        } else {
            GetWindowTextW(hwnd, buf.as_mut_ptr(), 256)
        }
    };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn exe_of(pid: u32) -> String {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..len as usize]).to_lowercase()
    }
}

struct Search {
    claude: HWND,
    button: HWND,
    own_pid: u32,
}

unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let s = &mut *(lparam as *mut Search);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == s.own_pid {
        if text_of(hwnd, false) == BUTTON_TITLE {
            s.button = hwnd;
        }
        return 1;
    }
    if s.claude.is_null()
        && IsWindowVisible(hwnd) != 0
        && GetWindow(hwnd, GW_OWNER).is_null()
        && text_of(hwnd, false) == "Claude"
        && text_of(hwnd, true) == "Chrome_WidgetWin_1"
        && exe_of(pid).ends_with("\\claude.exe")
    {
        s.claude = hwnd;
    }
    1
}

fn search() -> Search {
    let mut s = Search {
        claude: std::ptr::null_mut(),
        button: std::ptr::null_mut(),
        own_pid: unsafe { GetCurrentProcessId() },
    };
    unsafe { EnumWindows(Some(visit), &mut s as *mut Search as LPARAM) };
    s
}

fn bounds(hwnd: HWND) -> Rect {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as _,
            &mut r as *mut RECT as *mut c_void,
            std::mem::size_of::<RECT>() as u32,
        );
    }
    Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
}

fn own(hwnd: HWND, owner: HWND) {
    unsafe {
        if GetWindowLongPtrW(hwnd, GWLP_HWNDPARENT) != owner as isize {
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner as isize);
        }
    }
}

pub fn cursor() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

/// Place le bouton sur Claude. Appelé sans tenir le verrou : `SetWindowPos` attend le fil
/// de l'interface, qui peut lui-même attendre le verrou.
fn place_button(button: HWND, rect: Rect, offset: (i32, i32), scale: f32) {
    let x = rect.x + offset.0.clamp(0, (rect.w - (120.0 * scale) as i32).max(0));
    let y = rect.y + offset.1.clamp(0, (rect.h - (32.0 * scale) as i32).max(0));
    unsafe { SetWindowPos(button, std::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) };
}

/// Déplace le bouton tout de suite (glisser au clic droit), sans attendre le fil de suivi.
pub fn move_button(shared: &SharedRef, offset: (i32, i32)) {
    let (button, rect, scale) = {
        let mut g = shared.lock().unwrap();
        g.offset = offset;
        (g.button, g.rect, g.scale)
    };
    if button != 0 {
        place_button(button as HWND, rect, offset, scale);
    }
}

struct Tracker {
    shared: SharedRef,
    started: Instant,
    last_seen: Option<Instant>,
    claude: HWND,
    button: HWND,
    hook: HWINEVENTHOOK,
}

thread_local! {
    static TRACKER: RefCell<Option<Tracker>> = const { RefCell::new(None) };
}

impl Tracker {
    /// Recherche périodique : Claude (ouvert, fermé, recréé) et nos propres fenêtres.
    fn tick(&mut self) {
        let s = search();
        if s.claude.is_null() {
            let gone = match self.last_seen {
                Some(t) => t.elapsed() > Duration::from_secs(10),
                None => self.started.elapsed() > Duration::from_secs(180),
            };
            if gone {
                std::process::exit(0);
            }
            return;
        }
        self.last_seen = Some(Instant::now());
        if s.claude != self.claude {
            // Suivi instantané : Windows nous prévient à chaque déplacement de Claude.
            unsafe {
                if !self.hook.is_null() {
                    UnhookWinEvent(self.hook);
                }
                let mut pid = 0u32;
                GetWindowThreadProcessId(s.claude, &mut pid);
                self.hook = SetWinEventHook(
                    EVENT_OBJECT_LOCATIONCHANGE,
                    EVENT_OBJECT_LOCATIONCHANGE,
                    std::ptr::null_mut(),
                    Some(on_location),
                    pid,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                );
            }
        }
        self.claude = s.claude;
        self.button = s.button;
        if !s.button.is_null() {
            own(s.button, s.claude);
        }
        self.place();
    }

    fn place(&mut self) {
        if self.claude.is_null() || unsafe { IsIconic(self.claude) } != 0 {
            return;
        }
        let rect = bounds(self.claude);
        let scale = unsafe { GetDpiForWindow(self.claude) } as f32 / 96.0;
        let offset = {
            let mut g = self.shared.lock().unwrap();
            g.claude = self.claude as isize;
            g.button = self.button as isize;
            g.rect = rect;
            g.scale = scale;
            g.offset
        };
        if !self.button.is_null() {
            place_button(self.button, rect, offset, scale);
        }
    }
}

unsafe extern "system" fn on_location(_: HWINEVENTHOOK, _: u32, hwnd: HWND, id_object: i32, id_child: i32, _: u32, _: u32) {
    if id_object != OBJID_WINDOW || id_child != 0 {
        return;
    }
    TRACKER.with(|t| {
        if let Ok(mut t) = t.try_borrow_mut() {
            if let Some(t) = t.as_mut() {
                if t.claude == hwnd {
                    t.place();
                }
            }
        }
    });
}

/// Fil de suivi : colle le bouton à la fenêtre de Claude, et quitte
/// l'application quand Claude est fermé.
pub fn track(shared: SharedRef) {
    std::thread::spawn(move || {
        TRACKER.with(|t| {
            *t.borrow_mut() = Some(Tracker {
                shared,
                started: Instant::now(),
                last_seen: None,
                claude: std::ptr::null_mut(),
                button: std::ptr::null_mut(),
                hook: std::ptr::null_mut(),
            })
        });
        unsafe {
            SetTimer(std::ptr::null_mut(), 0, 50, None);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                if msg.message == WM_TIMER {
                    TRACKER.with(|t| t.borrow_mut().as_mut().map(Tracker::tick));
                } else {
                    DispatchMessageW(&msg);
                }
            }
        }
    });
}
