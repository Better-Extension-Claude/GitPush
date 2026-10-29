#![windows_subsystem = "windows"]

mod data;
mod ui;
mod win;

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

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
    // La fenêtre GitPush, lancée par le bouton dans son propre processus.
    if std::env::args().any(|a| a == "--modal") {
        if !win::single_instance("Local\\GitPush-fenetre") {
            win::focus_modal();
            return;
        }
        ui::run_modal();
        return;
    }
    if !win::single_instance("Local\\GitPush-overlay") {
        return;
    }
    win::run_button(data::Config::load().offset.unwrap_or(win::DEFAULT_OFFSET));
}
