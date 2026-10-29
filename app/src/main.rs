#![windows_subsystem = "windows"]

mod data;
mod install;
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

/// Modes de GitPush.exe :
/// - `--launch` (hook de Claude) : relance le bouton détaché, puis rend la main ;
/// - `--button` : le bouton sur Claude ;
/// - `--modal` : la fenêtre GitPush, ouverte par le bouton ;
/// - sans argument : le bouton si c'est la copie installée, sinon l'installateur
///   (`--install` force l'installateur).
fn main() {
    let has = |flag: &str| std::env::args().any(|a| a == flag);
    if has("--launch") {
        if let Ok(exe) = std::env::current_exe() {
            let _ = Command::new(exe)
                .arg("--button")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
                .spawn();
        }
        return;
    }
    if has("--modal") {
        if !win::single_instance("Local\\GitPush-fenetre") {
            win::focus_modal();
            return;
        }
        ui::run_modal();
        return;
    }
    // Installation sans fenêtre : `--install-to <dossier>` (hook compris).
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--install-to") {
        let result = install::install(args.get(i + 1).map(String::as_str).unwrap_or(""), true);
        debug(format!("installation silencieuse : {:?}", result.as_ref().map(|p| p.display().to_string())));
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    if has("--install") || (!has("--button") && !install::is_installed_copy()) {
        if win::single_instance("Local\\GitPush-installateur") {
            install::run_installer();
        }
        return;
    }
    if !win::single_instance("Local\\GitPush-overlay") {
        return;
    }
    win::run_button(data::Config::load().offset.unwrap_or(win::DEFAULT_OFFSET));
}
