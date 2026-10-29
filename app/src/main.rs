#![windows_subsystem = "windows"]

mod data;
mod ui;
mod win;

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

const DETACHED_PROCESS: u32 = 0x0000_0008;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// Journal de débogage (%LOCALAPPDATA%\GitPush\debug.log), actif avec GITPUSH_DEBUG=1.
pub fn debug(msg: impl AsRef<str>) {
    use std::io::Write;
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Option<Instant>> = OnceLock::new();
    let Some(start) = START.get_or_init(|| std::env::var_os("GITPUSH_DEBUG").map(|_| Instant::now())) else { return };
    let path = std::path::Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("GitPush").join("debug.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{:>8.3}s  {}", start.elapsed().as_secs_f64(), msg.as_ref());
    }
}

fn main() {
    // Lancé par le hook de Claude : on se relance détaché pour que Claude n'attende pas.
    if std::env::args().any(|a| a == "--launch") {
        if let Ok(exe) = std::env::current_exe() {
            let _ = Command::new(exe)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
                .spawn();
        }
        return;
    }
    if !win::single_instance() {
        return;
    }

    let cfg = data::Config::load();
    let shared = Arc::new(Mutex::new(win::Shared { offset: cfg.offset.unwrap_or(ui::DEFAULT_OFFSET), ..Default::default() }));
    win::track(shared.clone());

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(win::BUTTON_TITLE)
            .with_inner_size([118.0, 30.0])
            .with_position([-4000.0, -4000.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_taskbar(false),
        ..Default::default()
    };
    let _ = eframe::run_native(
        win::BUTTON_TITLE,
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, cfg, shared)))),
    );
}
