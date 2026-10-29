//! Fenêtres Win32 : trouver Claude, coller nos fenêtres dessus (fenêtres « possédées »).

use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, BOOL, ERROR_ALREADY_EXISTS, HWND, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetCursorPos, GetWindow, GetWindowLongPtrW, GetWindowTextW, GetWindowThreadProcessId,
    IsIconic, IsWindowVisible, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWLP_HWNDPARENT,
    GW_OWNER, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE,
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
    pub rect: Rect,
    pub scale: f32,
    pub offset: (i32, i32),
    pub modal_open: bool,
    pub focus_modal: bool,
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
    modal: HWND,
    own_pid: u32,
}

unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let s = &mut *(lparam as *mut Search);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == s.own_pid {
        match text_of(hwnd, false).as_str() {
            BUTTON_TITLE => s.button = hwnd,
            MODAL_TITLE => s.modal = hwnd,
            _ => {}
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
        modal: std::ptr::null_mut(),
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

/// Fil de suivi : colle le bouton (et le modal) à la fenêtre de Claude, et quitte
/// l'application quand Claude est fermé.
pub fn track(shared: SharedRef) {
    std::thread::spawn(move || {
        let started = Instant::now();
        let mut last_seen: Option<Instant> = None;
        let mut button_hidden = false;
        loop {
            std::thread::sleep(Duration::from_millis(40));
            let s = search();
            if s.claude.is_null() {
                let gone = match last_seen {
                    Some(t) => t.elapsed() > Duration::from_secs(10),
                    None => started.elapsed() > Duration::from_secs(180),
                };
                if gone {
                    std::process::exit(0);
                }
                continue;
            }
            last_seen = Some(Instant::now());
            let rect = bounds(s.claude);
            let scale = unsafe { GetDpiForWindow(s.claude) } as f32 / 96.0;
            let (offset, modal_open, focus_modal) = {
                let mut g = shared.lock().unwrap();
                g.claude = s.claude as isize;
                g.rect = rect;
                g.scale = scale;
                let f = g.focus_modal && !s.modal.is_null();
                if f {
                    g.focus_modal = false;
                }
                (g.offset, g.modal_open, f)
            };
            if unsafe { IsIconic(s.claude) } != 0 {
                continue;
            }
            if !s.button.is_null() {
                own(s.button, s.claude);
                let x = rect.x + offset.0.clamp(0, (rect.w - (120.0 * scale) as i32).max(0));
                let y = rect.y + offset.1.clamp(0, (rect.h - (32.0 * scale) as i32).max(0));
                unsafe {
                    SetWindowPos(s.button, std::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                    if modal_open && !button_hidden {
                        ShowWindow(s.button, SW_HIDE);
                        button_hidden = true;
                    } else if !modal_open && button_hidden {
                        ShowWindow(s.button, SW_SHOWNOACTIVATE);
                        button_hidden = false;
                    }
                }
            }
            if modal_open && !s.modal.is_null() {
                own(s.modal, s.claude);
                unsafe {
                    SetWindowPos(s.modal, std::ptr::null_mut(), rect.x, rect.y, rect.w, rect.h, SWP_NOZORDER | SWP_NOACTIVATE);
                    if focus_modal {
                        SetForegroundWindow(s.modal);
                    }
                }
            }
        }
    });
}
