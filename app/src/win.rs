//! Bouton flottant : une fenêtre Win32 native, seule sur son fil, posée juste au-dessus de
//! Claude dans l'ordre des fenêtres. Elle n'est pas « possédée » par Claude : ce lien relie les
//! files d'entrée des deux processus, et Claude peut se figer si le bouton est tué ou occupé.
//! Sans moteur graphique ni rendu à attendre, elle se replace dès que Windows signale que
//! Claude a bougé ou repasse devant. La fenêtre GitPush elle-même est un autre processus (`--modal`).

use crate::data::Config;
use claude_band::window as band;
use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};
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
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows, FindWindowW,
    GetClassNameW, GetCursorPos, GetMessageW, GetWindow, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
    IsWindowVisible, LoadCursorW, RegisterClassW, SetForegroundWindow, SetTimer, SetWindowPos, ShowWindow,
    TranslateMessage, UpdateLayeredWindow, ASFW_ANY, GW_OWNER, IDC_HAND, MA_NOACTIVATE, MSG, SWP_NOACTIVATE, SWP_NOSIZE,
    SWP_NOZORDER, SW_RESTORE, ULW_ALPHA, WM_CAPTURECHANGED, WM_CLOSE, WM_DESTROY,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_RBUTTONDOWN,
    WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

pub const BUTTON_TITLE: &str = "GitPush-bouton";
pub const MODAL_TITLE: &str = "GitPush";
pub const DEFAULT_OFFSET: (i32, i32) = (300, 44);

const BUTTON_W: f32 = 118.0;
const BUTTON_H: f32 = 30.0;
const WM_MOUSELEAVE: u32 = 0x02A3;

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

/// Signal « on vient de cliquer sur le bouton » envoyé à la fenêtre GitPush déjà ouverte :
/// elle relit alors la conversation affichée dans Claude.
const REFRESH_EVENT: &str = "Local\\GitPush-actualiser";

/// Côté fenêtre : crée le signal (à garder ouvert tant qu'elle vit).
pub fn refresh_signal() -> isize {
    use windows_sys::Win32::System::Threading::CreateEventW;
    let name = wide(REFRESH_EVENT);
    unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) as isize }
}

/// Côté fenêtre : vrai si un clic sur le bouton a eu lieu depuis le dernier appel.
pub fn refresh_requested(signal: isize) -> bool {
    use windows_sys::Win32::System::Threading::WaitForSingleObject;
    signal != 0 && unsafe { WaitForSingleObject(signal as _, 0) } == 0
}

fn request_refresh() {
    use windows_sys::Win32::System::Threading::{OpenEventW, SetEvent, EVENT_MODIFY_STATE};
    let name = wide(REFRESH_EVENT);
    unsafe {
        let h = OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr());
        if !h.is_null() {
            SetEvent(h);
            windows_sys::Win32::Foundation::CloseHandle(h);
        }
    }
}

/// Ramène au premier plan une fenêtre GitPush déjà ouverte.
pub fn focus_modal() {
    let title = wide(MODAL_TITLE);
    unsafe {
        let m = FindWindowW(std::ptr::null(), title.as_ptr());
        if !m.is_null() {
            // Réduite dans la barre des tâches : on la rouvre.
            if IsIconic(m) != 0 {
                ShowWindow(m, SW_RESTORE);
            }
            // Ramenée au centre de Claude, là où l'on vient de cliquer (elle a pu rester sur
            // un autre écran ou derrière une autre fenêtre) : taille actuelle, en points.
            let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect(m, &mut r);
            let scale = (GetDpiForWindow(m) as f32 / 96.0).max(1.0);
            center_on_claude(m as isize, [(r.right - r.left) as f32 / scale, (r.bottom - r.top) as f32 / scale]);
            request_refresh();
            SetForegroundWindow(m);
        }
    }
}

/// Attend (30 s au plus) la fin d'un processus.
pub fn wait_for_process(pid: u32) {
    use windows_sys::Win32::System::Threading::WaitForSingleObject;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    unsafe {
        let h = OpenProcess(SYNCHRONIZE, 0, pid);
        if !h.is_null() {
            WaitForSingleObject(h, 30_000);
            CloseHandle(h);
        }
    }
}

/// Boîte de dialogue Windows « choisir un dossier » ; bloque jusqu'au choix.
pub fn pick_folder(title: &str, owner: &str) -> Option<String> {
    // Le sélecteur de dossier de l'explorateur Windows, ouvert par-dessus la fenêtre GitPush.
    let owner_title = wide(owner);
    let owner = unsafe { FindWindowW(std::ptr::null(), owner_title.as_ptr()) };
    let mut dialog = rfd::FileDialog::new().set_title(title);
    if let Some(parent) = std::num::NonZeroIsize::new(owner as isize).map(Owner) {
        dialog = dialog.set_parent(&parent);
    }
    dialog.pick_folder().map(|p| p.to_string_lossy().into_owned())
}

/// Boîte de message Windows (erreur).
pub fn message(title: &str, text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};
    let (title, text) = (wide(title), wide(text));
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONWARNING) };
}

/// Centre la fenêtre GitPush (taille logique `size`) sur Claude, en pixels physiques, sans la
/// laisser déborder de l'écran de Claude. L'écran peut être à gauche de l'écran principal
/// (coordonnées négatives) : on ne ramène jamais rien à 0.
pub fn center_on_claude(hwnd: isize, size: [f32; 2]) {
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    let claude = find_claude();
    if claude.is_null() {
        return;
    }
    let (r, scale) = (bounds(claude), scale_of(claude));
    let (w, h) = ((size[0] * scale).round() as i32, (size[1] * scale).round() as i32);
    let mut x = r.x + (r.w - w) / 2;
    let mut y = r.y + (r.h - h) / 2;
    unsafe {
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(MonitorFromWindow(claude, MONITOR_DEFAULTTONEAREST), &mut mi) != 0 {
            let work = mi.rcWork;
            x = x.min(work.right - w).max(work.left);
            y = y.min(work.bottom - h).max(work.top);
        }
        SetWindowPos(hwnd as HWND, std::ptr::null_mut(), x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

/// Boîte de message Windows (information).
pub fn info(title: &str, text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};
    let (title, text) = (wide(title), wide(text));
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONINFORMATION) };
}

/// Coins arrondis de Windows 11 pour une fenêtre sans barre de titre.
pub fn round_corners(hwnd: isize) {
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
    let pref = DWMWCP_ROUND;
    unsafe {
        DwmSetWindowAttribute(
            hwnd as HWND,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &pref as *const _ as *const c_void,
            std::mem::size_of_val(&pref) as u32,
        );
    }
}

struct Owner(std::num::NonZeroIsize);

impl raw_window_handle::HasWindowHandle for Owner {
    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let raw = raw_window_handle::Win32WindowHandle::new(self.0);
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw.into()) })
    }
}

impl raw_window_handle::HasDisplayHandle for Owner {
    fn display_handle(&self) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Ok(raw_window_handle::DisplayHandle::windows())
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

/// Dessine la pastille « ↑ GitPush » (anticrénelée, coins transparents) dans la fenêtre, à la
/// couleur d'accent du thème de BetterClaude (un peu plus claire au survol).
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
        let accent = claudeext::theme::current().accent;
        let a = if hover { accent.mix(claudeext::theme::Rgb(255, 255, 255), 0.12) } else { accent };
        let fill: u32 = (a.0 as u32) << 16 | (a.1 as u32) << 8 | a.2 as u32;
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
    /// Suivi de Claude et de l'ordre des fenêtres (claude-band).
    watch: band::ClaudeWatch,
    /// Depuis quand le bouton est coincé sous Claude sans rien qui le recouvre.
    stuck_since: Option<Instant>,
    /// Encore coincé juste après une relance : incident noté, on attend.
    gave_up: bool,
    /// Le clic droit en cours a déplacé le bouton (sinon : retour à la place par défaut).
    drag_moved: bool,
    /// Couleur d'accent du dernier dessin : on redessine quand le thème change.
    accent: claudeext::theme::Rgb,
    /// Claude est déplacé ou redimensionné à la souris : bouton caché jusqu'au lâcher.
    is_moving: bool,
    offset: (i32, i32),
    scale: f32,
    is_hover: bool,
    is_pressed: bool,
    is_shown: bool,
    drag: Option<((i32, i32), (i32, i32))>,
    /// Dernière position donnée au bouton : on ne le replace que si elle change.
    pos: Option<(i32, i32)>,
    started: Instant,
    last_seen: Option<Instant>,
    last_search: Instant,
}

/// Le bouton, pour la remise en ordre que claude-band appelle depuis son propre fil.
static BUTTON: AtomicIsize = AtomicIsize::new(0);

/// Ordre du bouton (règle commune des extensions, `claude_band::window::order`) : « toujours
/// au-dessus » tant que rien ne recouvre sa place sur Claude, sinon juste au-dessus de Claude.
fn order_button(claude: HWND) {
    let button = BUTTON.load(Ordering::Relaxed) as HWND;
    let before = band::is_topmost(button);
    band::order(button, claude);
    let after = band::is_topmost(button);
    if before != after || band::is_stuck(button, claude) {
        crate::debug(format!(
            "bouton : toujours au-dessus {before} → {after}, coincé={}, Claude dessus={}",
            band::is_stuck(button, claude),
            band::is_above(claude, button)
        ));
    }
}

/// Note un incident dans `incidents.log` du dossier de données.
fn incident(msg: &str) {
    use std::io::Write;
    let dir = crate::data_dir();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("incidents.log")) {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "{secs}  {msg}");
    }
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
                crate::debug("bouton : caché (Claude introuvable)");
                band::hide(self.hwnd);
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
        // Thème changé dans BetterClaude : la pastille prend le nouvel accent.
        let accent = claudeext::theme::current().accent;
        if accent != self.accent {
            self.accent = accent;
            render(self.hwnd, self.scale, self.is_hover);
        }
        // Claude réduit : le bouton se cache (« toujours au-dessus », il flotterait seul).
        if unsafe { IsIconic(self.claude) } != 0 {
            if self.is_shown {
                band::hide(self.hwnd);
                self.is_shown = false;
            }
            return;
        }
        self.place();
        if !self.is_shown && !self.is_moving {
            crate::debug("bouton : affiché");
            band::show(self.hwnd, self.claude);
            self.is_shown = true;
            order_button(self.claude);
        }
        // Coincé sous Claude alors que rien ne le recouvre (Windows ignore ses changements
        // d'ordre) : au bout de 3 s, on se relance ; une fenêtre neuve n'a pas le problème.
        if band::is_stuck(self.hwnd, self.claude) {
            let since = *self.stuck_since.get_or_insert_with(Instant::now);
            if since.elapsed() > Duration::from_secs(3) {
                // Une relance par minute au plus : si la fenêtre neuve est coincée aussi, se
                // relancer en boucle ne servirait à rien.
                if relaunched_recently() {
                    if !self.gave_up {
                        self.gave_up = true;
                        incident("bouton toujours coincé après une relance : pas de nouvelle relance avant 1 min");
                    }
                } else {
                    incident("bouton coincé sous Claude depuis 3 s : relance");
                    relaunch();
                }
            }
        } else {
            self.stuck_since = None;
            self.gave_up = false;
        }
    }

    /// Nouvelle fenêtre Claude : on s'y accroche et on écoute ses déplacements.
    /// claude-band écoute Claude (déplacements, glisser à la souris, premier plan) et toutes
    /// les fenêtres qui pourraient recouvrir le bouton ; il remet l'ordre (`order_button`)
    /// et nous signale les mouvements de Claude par WM_CLAUDE_MOVED.
    fn attach(&mut self, claude: HWND) {
        crate::debug(format!("bouton : accroché à {:?}", claude));
        self.is_moving = false;
        self.claude = claude;
        self.watch.attach(claude, self.hwnd);
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
        // Position seulement si elle change ; l'ordre des fenêtres, à part (`order`).
        if self.pos != Some((x, y)) {
            crate::debug(format!("bouton : position ({x}, {y})"));
            self.pos = Some((x, y));
            unsafe { SetWindowPos(self.hwnd, std::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER) };
        }
        order_button(self.claude);
    }

    fn reset(&mut self) {
        self.offset = DEFAULT_OFFSET;
        save_offset(None);
        self.place();
    }

    fn set_hover(&mut self, hover: bool) {
        if hover != self.is_hover {
            self.is_hover = hover;
            render(self.hwnd, self.scale, hover);
        }
    }

    fn message(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        let inside = || {
            let (x, y) = ((lparam & 0xffff) as i16 as f32, ((lparam >> 16) & 0xffff) as i16 as f32);
            x >= 0.0 && y >= 0.0 && x < BUTTON_W * self.scale && y < BUTTON_H * self.scale
        };
        match msg {
            WM_TIMER => self.tick(),
            // Claude bouge (0), est saisi à la souris (1), lâché (2), ou passe devant (3).
            band::WM_CLAUDE_MOVED => match wparam {
                1 => {
                    crate::debug("bouton : Claude saisi");
                    self.is_moving = true;
                    if self.is_shown {
                        band::hide(self.hwnd);
                        self.is_shown = false;
                    }
                }
                2 => {
                    crate::debug("bouton : Claude lâché");
                    self.is_moving = false;
                    self.place();
                    if unsafe { IsIconic(self.claude) } == 0 {
                        band::show(self.hwnd, self.claude);
                        self.is_shown = true;
                        order_button(self.claude);
                    }
                }
                _ => self.place(),
            },
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
                    if (now.0 - start.0).abs() + (now.1 - start.1).abs() > 3 {
                        self.drag_moved = true;
                    }
                    if self.drag_moved {
                        self.offset = (offset.0 + now.0 - start.0, offset.1 + now.1 - start.1);
                        self.place();
                    }
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
                self.drag_moved = false;
                unsafe { SetCapture(self.hwnd) };
            }
            WM_RBUTTONUP => {
                if self.drag.take().is_some() {
                    unsafe { ReleaseCapture() };
                    if self.drag_moved {
                        save_offset(Some(self.offset));
                    } else {
                        // Clic droit sans bouger : retour à la place par défaut.
                        self.reset();
                    }
                }
            }
            WM_CAPTURECHANGED => {
                if self.drag.take().is_some() && self.drag_moved {
                    save_offset(Some(self.offset));
                }
            }
            // Fermeture demandée par une autre application : ignorée (le bouton vit avec Claude).
            WM_CLOSE => {}
            // Fenêtre détruite quand même : on quitte, sinon le processus garderait le verrou
            // d'instance unique sans fenêtre, et le bouton ne reviendrait plus.
            WM_DESTROY => std::process::exit(0),
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
        Ok(mut o) => o.as_mut().filter(|o| o.hwnd == hwnd).and_then(|o| o.message(msg, wparam, lparam)),
        Err(_) => None,
    });
    handled.unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam))
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
            // « Toujours au-dessus » dès la création : Windows refuse ensuite de le passer devant
            // Claude actif (règle claude-band : nos fenêtres ne quittent jamais cette couche).
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
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
        BUTTON.store(hwnd as isize, Ordering::Relaxed);
        band::set_order(order_button);
        OVERLAY.with(|o| {
            *o.borrow_mut() = Some(Overlay {
                hwnd,
                claude: std::ptr::null_mut(),
                watch: band::ClaudeWatch::default(),
                stuck_since: None,
                gave_up: false,
                drag_moved: false,
                accent: claudeext::theme::current().accent,
                is_moving: false,
                offset,
                scale: 1.0,
                is_hover: false,
                is_pressed: false,
                is_shown: false,
                drag: None,
                pos: None,
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

/// Relance le bouton dans un nouveau processus, qui attend la fin de celui-ci (verrou
/// d'instance unique), puis quitte.
fn relaunch() {
    if let Ok(exe) = std::env::current_exe() {
        let pid = std::process::id().to_string();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0).to_string();
        let _ = std::process::Command::new(exe).args(["--button", "--wait-pid", &pid, "--relaunched-at", &now]).spawn();
    }
    std::process::exit(0);
}

/// Heure (secondes Unix) de la dernière relance, passée par `--relaunched-at`.
fn relaunched_recently() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let at = args.iter().position(|a| a == "--relaunched-at").and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<u64>().ok());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    at.is_some_and(|t| now.saturating_sub(t) < 60)
}

/// Titre de la conversation affichée dans Claude, lu dans sa fenêtre par UI Automation
/// (la page s'appelle « <titre> - Claude Code »). Immédiat, contrairement aux fiches de
/// session que Claude n'enregistre qu'environ 1 s après un changement de conversation.
pub fn displayed_title() -> Option<String> {
    use windows::core::VARIANT;
    use windows::Win32::Foundation::HWND as WHWND;
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation, TreeScope_Descendants, UIA_ControlTypePropertyId, UIA_DocumentControlTypeId};
    let t = std::time::Instant::now();
    let claude = find_claude();
    if claude.is_null() {
        crate::debug("titre affiché : fenêtre de Claude introuvable");
        return None;
    }
    let read = || -> Result<String, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).map_err(|e| format!("UIA {e}"))?;
            let root = uia.ElementFromHandle(WHWND(claude as _)).map_err(|e| format!("fenêtre {e}"))?;
            let cond = uia
                .CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(UIA_DocumentControlTypeId.0))
                .map_err(|e| format!("condition {e}"))?;
            // Plusieurs pages (une vide sert à Claude) : celle qui a un nom « <titre> - Claude… ».
            let pages = root.FindAll(TreeScope_Descendants, &cond).map_err(|e| format!("pages {e}"))?;
            let mut names = Vec::new();
            for i in 0..pages.Length().unwrap_or(0) {
                let Ok(p) = pages.GetElement(i) else { continue };
                let name = p.CurrentName().map(|s| s.to_string()).unwrap_or_default();
                if name.contains(" - ") {
                    return Ok(name);
                }
                names.push(name);
            }
            Err(format!("aucune page nommée parmi {names:?}"))
        }
    };
    let name = read();
    crate::debug(format!("titre affiché : {name:?} en {:?}", t.elapsed()));
    let name = name.ok()?;
    let title = name.rsplit_once(" - ").map(|(t, _)| t).unwrap_or(&name).trim().to_string();
    (!title.is_empty()).then_some(title)
}
