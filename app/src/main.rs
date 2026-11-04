#![windows_subsystem = "windows"]

mod data;
mod ui;
mod win;

use std::path::{Path, PathBuf};

/// Identifiant de l'extension dans BetterClaude (dossier de données, manifeste).
pub const ID: &str = "gitpush";

/// Dossier de données de GitPush : `%LOCALAPPDATA%\BetterExt\data\gitpush` (réglages, journaux).
pub fn data_dir() -> PathBuf {
    let dir = claudeext::extension_data_dir(ID);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Journal de débogage (`debug.log` du dossier de données), actif avec GITPUSH_DEBUG=1.
pub fn debug(msg: impl AsRef<str>) {
    use std::io::Write;
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Option<Instant>> = OnceLock::new();
    let Some(start) = START.get_or_init(|| std::env::var_os("GITPUSH_DEBUG").map(|_| Instant::now())) else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(data_dir().join("debug.log")) {
        let _ = writeln!(f, "{:>8.3}s  {}", start.elapsed().as_secs_f64(), msg.as_ref());
    }
}

/// Modes de GitPush.exe :
/// - `--button` : le bouton sur Claude (lancé par BetterClaude au début de chaque session) ;
/// - `--modal` : la fenêtre GitPush, ouverte par le bouton ;
/// - autrement (lancé à la main) : GitPush s'installe par BetterClaude, jamais seul.
fn main() {
    data::refresh_path();
    let args: Vec<String> = std::env::args().collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    if has("--modal") {
        if !win::single_instance("Local\\GitPush-fenetre") {
            win::focus_modal();
            return;
        }
        ui::run_modal();
        return;
    }
    // Copie installée par BetterClaude lancée à la main : c'est le bouton.
    if has("--button") || is_installed_copy() {
        // Relance d'un bouton coincé : attendre que l'ancien processus ait quitté.
        if let Some(pid) = value("--wait-pid").and_then(|p| p.parse().ok()) {
            win::wait_for_process(pid);
        }
        if !win::single_instance("Local\\GitPush-overlay") {
            return;
        }
        win::run_button(data::Config::load().offset.unwrap_or(win::DEFAULT_OFFSET));
        return;
    }
    // Une ancienne fenêtre GitPush (avant 1.1) lance la version téléchargée avec
    // `--update-to` : on attend qu'elle se ferme, puis on passe la main à BetterClaude.
    if let Some(pid) = value("--wait-pid").and_then(|p| p.parse().ok()) {
        win::wait_for_process(pid);
    }
    hand_off();
}

/// L'exe est-il celui que BetterClaude a installé (dans son dossier des extensions) ?
fn is_installed_copy() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let root = claudeext::extensions_dir();
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(&exe).starts_with(canon(&root))
}

/// Lancé à la main : installation par BetterClaude (comme « Ajouter » dans sa fenêtre),
/// avec l'extension.json livré à côté de l'exe. Sans BetterClaude, on dit où l'installer.
fn hand_off() {
    const TITLE: &str = "GitPush";
    let manifest = std::env::current_exe().ok().and_then(|e| Some(e.parent()?.join("extension.json")));
    let better = std::env::var("LOCALAPPDATA")
        .map(|d| Path::new(&d).join("Programs").join("BetterClaude").join("BetterClaude.exe"))
        .ok()
        .filter(|p| p.exists());
    let (Some(better), Some(manifest)) = (better, manifest.filter(|m| m.exists())) else {
        win::message(
            TITLE,
            "GitPush s'installe avec BetterClaude : ouvrez BetterClaude, onglet Découverte, puis installez GitPush.",
        );
        return;
    };
    let ok = std::process::Command::new(better)
        .arg("--add")
        .arg(&manifest)
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        win::info(TITLE, "GitPush est installé dans BetterClaude. Le bouton apparaît sur Claude.");
    } else {
        win::message(TITLE, "L'installation dans BetterClaude n'a pas abouti. Installez GitPush depuis BetterClaude, onglet Découverte.");
    }
}
