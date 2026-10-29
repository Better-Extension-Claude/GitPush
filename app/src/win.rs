//! Bouton flottant : une fenêtre Win32 native, « possédée » par Claude, seule sur son fil.
//! Sans moteur graphique ni rendu à attendre, elle se replace dès que Windows signale que
//! Claude a bougé. La fenêtre GitPush elle-même est un autre processus (`--modal`).

use crate::data::Config;
use std::cell::RefCell;
use std::ffi::c_void;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, BOOL, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GdiFlush, GetDC, ReleaseDC,
    SelectObject, SetBkMode, SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, BITMAPINFO,
    BITMAPINFOHEADER, BLENDFUNCTION, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CENTER,
    DT_SINGLELINE, DT_VCENTER, OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows, FindWindowW,
    GetClassNameW, GetCursorPos, GetMessageW, GetWindow, GetWindowTextW, GetWindowThreadProcessId,
    IsIconic, IsWindow, IsWindowVisible, LoadCursorW, PostMessageW, RegisterClassW, SetForegroundWindow, SetTimer,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, UpdateLayeredWindow, ASFW_ANY,
    EVENT_OBJECT_LOCATIONCHANGE, EVENT_SYSTEM_MOVESIZEEND, EVENT_SYSTEM_MOVESIZESTART, GWLP_HWNDPARENT, GW_OWNER, IDC_HAND, MA_NOACTIVATE, MSG, OBJID_WINDOW, SWP_NOACTIVATE,
    SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
    WM_APP, WM_CAPTURECHANGED, WM_CLOSE, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_RBUTTONDOWN,
    WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};

pub const BUTTON_TITLE: &str = "GitPush-bouton";
pub const MODAL_TITLE: &str = "GitPush";
pub const DEFAULT_OFFSET: (i32, i32) = (300, 44);

const BUTTON_W: f32 = 118.0;
const BUTTON_H: f32 = 30.0;
const WM_MOUSELEAVE: u32 = 0x02A3;
/// Envoyé par la fenêtre GitPush : « Replacer le bouton ».
const WM_RESET: u32 = WM_APP + 1;

#[derive(Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Une seule instance par rôle (bouton, fenêtre) : le hook de Claude peut nous lancer plusieurs fois.
pub fn single_instance(name: &str) -> bool {
    let name = wide(name);
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

unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let found = &mut *(lparam as *mut HWND);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid != GetCurrentProcessId()
        && IsWindowVisible(hwnd) != 0
        && GetWindow(hwnd, GW_OWNER).is_null()
        && text_of(hwnd, false) == "Claude"
        && text_of(hwnd, true) == "Chrome_WidgetWin_1"
        && exe_of(pid).ends_with("\\claude.exe")
    {
        *found = hwnd;
        return 0;
    }
    1
}

fn find_claude() -> HWND {
    let mut found: HWND = std::ptr::null_mut();
    unsafe { EnumWindows(Some(visit), &mut found as *mut HWND as LPARAM) };
    found
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

fn scale_of(hwnd: HWND) -> f32 {
    (unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0).max(1.0)
}

/// Fenêtre de Claude et son échelle, pour centrer la fenêtre GitPush dessus.
pub fn claude_rect() -> Option<(Rect, f32)> {
    let c = find_claude();
    (!c.is_null()).then(|| (bounds(c), scale_of(c)))
}

fn button_window() -> HWND {
    let title = wide(BUTTON_TITLE);
    unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) }
}

/// « Replacer le bouton » depuis la fenêtre GitPush.
pub fn reset_button() {
    let b = button_window();
    if !b.is_null() {
        unsafe { PostMessageW(b, WM_RESET, 0, 0) };
    }
}

/// « Quitter GitPush » : ferme aussi le bouton.
pub fn quit_button() {
    let b = button_window();
    if !b.is_null() {
        unsafe { PostMessageW(b, WM_CLOSE, 0, 0) };
    }
}

/// Ramène au premier plan une fenêtre GitPush déjà ouverte.
pub fn focus_modal() {
    let title = wide(MODAL_TITLE);
    unsafe {
        let m = FindWindowW(std::ptr::null(), title.as_ptr());
        if !m.is_null() {
            SetForegroundWindow(m);
        }
    }
}

/// Boîte de dialogue Windows « choisir un dossier » ; bloque jusqu'au choix.
pub fn pick_folder(title: &str) -> Option<String> {
    use windows_sys::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
    use windows_sys::Win32::UI::Shell::{
        SHBrowseForFolderW, SHGetPathFromIDListW, BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW,
    };
    let title = wide(title);
    let owner_title = wide("Installer GitPush");
    let mut name = [0u16; 260];
    unsafe {
        CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        let info = BROWSEINFOW {
            hwndOwner: FindWindowW(std::ptr::null(), owner_title.as_ptr()),
            pidlRoot: std::ptr::null_mut(),
            pszDisplayName: name.as_mut_ptr(),
            lpszTitle: title.as_ptr(),
            ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
            lpfn: None,
            lParam: 0,
            iImage: 0,
        };
        let pidl = SHBrowseForFolderW(&info);
        if pidl.is_null() {
            return None;
        }
        let mut path = [0u16; 1024];
        let ok = SHGetPathFromIDListW(pidl, path.as_mut_ptr());
        CoTaskMemFree(pidl as *const c_void);
        let len = path.iter().position(|&c| c == 0).unwrap_or(0);
        (ok != 0 && len > 0).then(|| String::from_utf16_lossy(&path[..len]))
    }
}

fn cursor() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

fn save_offset(offset: Option<(i32, i32)>) {
    // Relit la config : la fenêtre GitPush l'a peut-être modifiée entre-temps.
    let mut cfg = Config::load();
    cfg.offset = offset;
    cfg.save();
}

/// Dessine la pastille « ↑ GitPush » (anticrénelée, coins transparents) dans la fenêtre.
fn render(hwnd: HWND, scale: f32, hover: bool) {
    let w = (BUTTON_W * scale).round() as i32;
    let h = (BUTTON_H * scale).round() as i32;
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        let dc = CreateCompatibleDC(screen);
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = w;
        bi.bmiHeader.biHeight = -h;
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bmp = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if bmp.is_null() || bits.is_null() {
            DeleteDC(dc);
            ReleaseDC(std::ptr::null_mut(), screen);
            return;
        }
        let old_bmp = SelectObject(dc, bmp);
        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (w * h) as usize);
        let fill: u32 = if hover { 0xe38b6e } else { 0xd97757 };
        px.fill(fill);

        let face = wide("Segoe UI");
        let font = CreateFontW(
            -(13.5 * scale).round() as i32,
            0,
            0,
            0,
            700,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            ANTIALIASED_QUALITY as u32,
            DEFAULT_PITCH as u32,
            face.as_ptr(),
        );
        let old_font = SelectObject(dc, font);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, 0x00ff_ffff);
        let mut rc = RECT { left: 0, top: 0, right: w, bottom: h };
        let mut text = wide("↑  GitPush");
        DrawTextW(dc, text.as_mut_ptr(), -1, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        GdiFlush();

        // Forme de pilule : couverture calculée par distance, puis alpha prémultiplié.
        let radius = h as f32 / 2.0 - scale;
        let (cy, x0, x1) = (h as f32 / 2.0, h as f32 / 2.0, w as f32 - h as f32 / 2.0);
        for y in 0..h {
            for x in 0..w {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let d = ((fx - fx.clamp(x0, x1)).powi(2) + (fy - cy).powi(2)).sqrt() - radius;
                let a = (0.5 - d).clamp(0.0, 1.0);
                let i = (y * w + x) as usize;
                let p = px[i];
                let ch = |shift: u32| ((((p >> shift) & 0xff) as f32 * a).round() as u32) << shift;
                px[i] = ((a * 255.0).round() as u32) << 24 | ch(16) | ch(8) | ch(0);
            }
        }

        let size = SIZE { cx: w, cy: h };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        UpdateLayeredWindow(hwnd, screen, std::ptr::null(), &size, dc, &src, 0, &blend, ULW_ALPHA);

        SelectObject(dc, old_font);
        DeleteObject(font);
        SelectObject(dc, old_bmp);
        DeleteObject(bmp);
        DeleteDC(dc);
        ReleaseDC(std::ptr::null_mut(), screen);
    }
}

struct Overlay {
    hwnd: HWND,
    claude: HWND,
    hooks: [HWINEVENTHOOK; 2],
    /// Claude est déplacé ou redimensionné à la souris : bouton caché jusqu'au lâcher.
    is_moving: bool,
    offset: (i32, i32),
    scale: f32,
    is_hover: bool,
    is_pressed: bool,
    is_shown: bool,
    drag: Option<((i32, i32), (i32, i32))>,
    started: Instant,
    last_seen: Option<Instant>,
    last_search: Instant,
}

thread_local! {
    static OVERLAY: RefCell<Option<Overlay>> = const { RefCell::new(None) };
}

impl Overlay {
    /// Toutes les 100 ms : Claude toujours là ? (recherche complète au plus toutes les 2 s)
    fn tick(&mut self) {
        let alive = !self.claude.is_null()
            && unsafe { IsWindow(self.claude) != 0 && IsWindowVisible(self.claude) != 0 };
        if !alive || self.last_search.elapsed() > Duration::from_secs(2) {
            self.last_search = Instant::now();
            let c = find_claude();
            if c != self.claude {
                self.attach(c);
            }
        }
        if self.claude.is_null() {
            if self.is_shown {
                unsafe { ShowWindow(self.hwnd, SW_HIDE) };
                self.is_shown = false;
            }
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
        self.place();
        if !self.is_shown && !self.is_moving {
            unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };
            self.is_shown = true;
        }
    }

    /// Nouvelle fenêtre Claude : on s'y accroche et on écoute ses déplacements.
    fn attach(&mut self, claude: HWND) {
        unsafe {
            for hook in &mut self.hooks {
                if !hook.is_null() {
                    UnhookWinEvent(*hook);
                    *hook = std::ptr::null_mut();
                }
            }
            self.is_moving = false;
            self.claude = claude;
            if claude.is_null() {
                return;
            }
            SetWindowLongPtrW(self.hwnd, GWLP_HWNDPARENT, claude as isize);
            let mut pid = 0u32;
            GetWindowThreadProcessId(claude, &mut pid);
            // Déplacements sans la souris (agrandir, ancrer) : le bouton suit. Pendant un
            // glisser à la souris, il se cache pour ne rien demander à Windows en même temps.
            let ranges = [
                (EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE),
                (EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND),
            ];
            for (hook, (from, to)) in self.hooks.iter_mut().zip(ranges) {
                *hook = SetWinEventHook(
                    from,
                    to,
                    std::ptr::null_mut(),
                    Some(on_event),
                    pid,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                );
            }
        }
    }

    fn place(&mut self) {
        if self.claude.is_null() || self.is_moving || unsafe { IsIconic(self.claude) } != 0 {
            return;
        }
        let rect = bounds(self.claude);
        let scale = scale_of(self.claude);
        if scale != self.scale {
            self.scale = scale;
            render(self.hwnd, scale, self.is_hover);
        }
        let (w, h) = ((BUTTON_W * scale).round() as i32, (BUTTON_H * scale).round() as i32);
        let x = rect.x + self.offset.0.clamp(0, (rect.w - w).max(0));
        let y = rect.y + self.offset.1.clamp(0, (rect.h - h).max(0));
        unsafe { SetWindowPos(self.hwnd, std::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) };
    }

    fn set_hover(&mut self, hover: bool) {
        if hover != self.is_hover {
            self.is_hover = hover;
            render(self.hwnd, self.scale, hover);
        }
    }

    fn message(&mut self, msg: u32, lparam: LPARAM) -> Option<LRESULT> {
        let inside = || {
            let (x, y) = ((lparam & 0xffff) as i16 as f32, ((lparam >> 16) & 0xffff) as i16 as f32);
            x >= 0.0 && y >= 0.0 && x < BUTTON_W * self.scale && y < BUTTON_H * self.scale
        };
        match msg {
            WM_TIMER => self.tick(),
            WM_MOUSEACTIVATE => return Some(MA_NOACTIVATE as LRESULT),
            WM_MOUSEMOVE => {
                if !self.is_hover {
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: self.hwnd,
                        dwHoverTime: 0,
                    };
                    unsafe { TrackMouseEvent(&mut tme) };
                    self.set_hover(true);
                }
                // Clic droit maintenu : le bouton suit la souris.
                if let Some((start, offset)) = self.drag {
                    let now = cursor();
                    self.offset = (offset.0 + now.0 - start.0, offset.1 + now.1 - start.1);
                    self.place();
                }
            }
            WM_MOUSELEAVE => {
                if self.drag.is_none() {
                    self.set_hover(false);
                }
            }
            WM_LBUTTONDOWN => self.is_pressed = true,
            WM_LBUTTONUP => {
                if std::mem::take(&mut self.is_pressed) && inside() {
                    open_modal();
                }
            }
            WM_RBUTTONDOWN => {
                self.drag = Some((cursor(), self.offset));
                unsafe { SetCapture(self.hwnd) };
            }
            WM_RBUTTONUP => {
                if self.drag.take().is_some() {
                    unsafe { ReleaseCapture() };
                    save_offset(Some(self.offset));
                }
            }
            WM_CAPTURECHANGED => {
                if self.drag.take().is_some() {
                    save_offset(Some(self.offset));
                }
            }
            WM_RESET => {
                self.offset = DEFAULT_OFFSET;
                save_offset(None);
                self.place();
            }
            WM_CLOSE => std::process::exit(0),
            _ => return None,
        }
        Some(0)
    }
}

/// Ouvre la fenêtre GitPush dans son propre processus.
fn open_modal() {
    crate::debug("bouton : ouverture");
    unsafe { AllowSetForegroundWindow(ASFW_ANY) };
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).arg("--modal").spawn();
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // `try_borrow_mut` : SetWindowPos ou ReleaseCapture renvoient des messages pendant
    // qu'on traite déjà le précédent ; ceux-là vont au traitement par défaut.
    let handled = OVERLAY.with(|o| match o.try_borrow_mut() {
        Ok(mut o) => o.as_mut().filter(|o| o.hwnd == hwnd).and_then(|o| o.message(msg, lparam)),
        Err(_) => None,
    });
    handled.unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam))
}

unsafe extern "system" fn on_event(_: HWINEVENTHOOK, event: u32, hwnd: HWND, id_object: i32, id_child: i32, _: u32, _: u32) {
    if id_object != OBJID_WINDOW || id_child != 0 {
        return;
    }
    OVERLAY.with(|o| {
        let Ok(mut o) = o.try_borrow_mut() else { return };
        let Some(o) = o.as_mut().filter(|o| o.claude == hwnd) else { return };
        match event {
            EVENT_SYSTEM_MOVESIZESTART => {
                o.is_moving = true;
                if o.is_shown {
                    ShowWindow(o.hwnd, SW_HIDE);
                    o.is_shown = false;
                }
            }
            EVENT_SYSTEM_MOVESIZEEND => {
                o.is_moving = false;
                o.place();
                ShowWindow(o.hwnd, SW_SHOWNOACTIVATE);
                o.is_shown = true;
            }
            _ => o.place(),
        }
    });
}

/// Le bouton : crée la fenêtre et tourne jusqu'à la fermeture de Claude.
pub fn run_button(offset: (i32, i32)) {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide("GitPushButton");
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: std::ptr::null_mut(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_HAND),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&wc);
        let title = wide(BUTTON_TITLE);
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            -4000,
            -4000,
            BUTTON_W as i32,
            BUTTON_H as i32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return;
        }
        render(hwnd, 1.0, false);
        OVERLAY.with(|o| {
            *o.borrow_mut() = Some(Overlay {
                hwnd,
                claude: std::ptr::null_mut(),
                hooks: [std::ptr::null_mut(); 2],
                is_moving: false,
                offset,
                scale: 1.0,
                is_hover: false,
                is_pressed: false,
                is_shown: false,
                drag: None,
                started: Instant::now(),
                last_seen: None,
                last_search: Instant::now() - Duration::from_secs(60),
            })
        });
        SetTimer(hwnd, 1, 100, None);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
