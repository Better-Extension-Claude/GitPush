#![windows_subsystem = "windows"]

mod data;
mod ui;
mod win;

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

const DETACHED_PROCESS: u32 = 0x0000_0008;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

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
