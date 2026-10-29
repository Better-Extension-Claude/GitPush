//! Installateur intégré : GitPush.exe lancé hors de son dossier d'installation (téléchargé,
//! par exemple) s'ouvre sur cet écran. Il se copie dans le dossier choisi, ajoute le hook
//! SessionStart dans les réglages de Claude Code et propose d'installer Git et GitHub CLI.

use crate::data::{self, Config};
use crate::win;
use eframe::egui::{self, Color32, Margin, RichText};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

const BG: Color32 = Color32::from_rgb(0x26, 0x26, 0x24);
const SUBTLE: Color32 = Color32::from_rgb(0x9a, 0x98, 0x93);
const ACCENT: Color32 = Color32::from_rgb(0xd9, 0x77, 0x57);
const OK: Color32 = Color32::from_rgb(0x8c, 0xc0, 0x76);
const ERR: Color32 = Color32::from_rgb(0xe5, 0x6b, 0x5d);

/// Réglages de Claude Code : ~/.claude/settings.json (ou $CLAUDE_CONFIG_DIR).
pub fn settings_path() -> PathBuf {
    let dir = std::env::var("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(&std::env::var("USERPROFILE").unwrap_or_default()).join(".claude"));
    dir.join("settings.json")
}

fn hook_command(exe: &Path) -> String {
    format!("\"{}\" --launch", exe.to_string_lossy().replace('\\', "/"))
}

fn is_gitpush_hook(hook: &Value) -> bool {
    hook.get("command").and_then(Value::as_str).map(|c| c.to_lowercase().contains("gitpush.exe")).unwrap_or(false)
}

/// Commande du hook GitPush déjà présent dans les réglages de Claude, s'il y en a un.
pub fn current_hook() -> Option<String> {
    let text = std::fs::read_to_string(settings_path()).ok()?;
    let settings: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let groups = settings.get("hooks")?.get("SessionStart")?.as_array()?;
    groups
        .iter()
        .filter_map(|g| g.get("hooks")?.as_array())
        .flatten()
        .find(|h| is_gitpush_hook(h))
        .and_then(|h| h.get("command")?.as_str().map(String::from))
}

/// Remplace le hook GitPush (lancer `exe` à chaque session) ou le retire (`None`), sans
/// toucher au reste des réglages. Une copie de l'ancien fichier est gardée à côté.
pub fn set_hook(exe: Option<&Path>) -> Result<(), String> {
    let path = settings_path();
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| "{}".into());
    // Un éditeur Windows peut avoir ajouté une marque BOM, que serde_json refuse.
    let text = text.trim_start_matches('\u{feff}');
    let mut settings: Value = serde_json::from_str(if text.trim().is_empty() { "{}" } else { text })
        .map_err(|e| format!("{} n'est pas un JSON valide ({e}) : rien n'a été modifié.", path.display()))?;
    let Some(root) = settings.as_object_mut() else {
        return Err(format!("{} n'est pas un objet JSON : rien n'a été modifié.", path.display()));
    };

    let hooks = root.entry("hooks").or_insert_with(|| json!({}));
    let Some(hooks) = hooks.as_object_mut() else { return Err("« hooks » n'est pas un objet.".into()) };
    let start = hooks.entry("SessionStart").or_insert_with(|| json!([]));
    let Some(groups) = start.as_array_mut() else { return Err("« SessionStart » n'est pas une liste.".into()) };
    for group in groups.iter_mut() {
        if let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) {
            list.retain(|h| !is_gitpush_hook(h));
        }
    }
    groups.retain(|g| g.get("hooks").and_then(Value::as_array).map(|l| !l.is_empty()).unwrap_or(true));
    if let Some(exe) = exe {
        groups.push(json!({ "hooks": [ { "type": "command", "command": hook_command(exe), "timeout": 10 } ] }));
    }
    if groups.is_empty() {
        hooks.remove("SessionStart");
    }
    if hooks.is_empty() {
        root.remove("hooks");
    }

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if path.exists() {
        let _ = std::fs::copy(&path, path.with_extension("json.gitpush.bak"));
    }
    let out = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(&path, out + "\n").map_err(|e| format!("Écriture de {} impossible : {e}", path.display()))
}

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_lowercase();
    norm(a) == norm(b)
}

/// Vrai si ce GitPush.exe est la copie installée (ou l'ancien emplacement de build.sh).
pub fn is_installed_copy() -> bool {
    let Ok(me) = std::env::current_exe() else { return false };
    let legacy = Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("GitPush").join("GitPush.exe");
    Config::load().install_path.map(|p| same_path(Path::new(&p), &me)).unwrap_or(false) || same_path(&legacy, &me)
}

fn default_folder() -> String {
    if let Some(p) = Config::load().install_path {
        if let Some(dir) = Path::new(&p).parent() {
            return dir.to_string_lossy().into();
        }
    }
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default())
        .join("Programs")
        .join("GitPush")
        .to_string_lossy()
        .into()
}

/// Ferme le bouton en cours (il verrouille son .exe), puis copie ce .exe dans `folder`.
pub fn install(folder: &str, with_hook: bool) -> Result<PathBuf, String> {
    let folder = PathBuf::from(folder.trim());
    if folder.as_os_str().is_empty() {
        return Err("Choisis un dossier.".into());
    }
    std::fs::create_dir_all(&folder).map_err(|e| format!("Dossier impossible à créer : {e}"))?;
    let target = folder.join("GitPush.exe");
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    if !same_path(&me, &target) {
        win::quit_button();
        let mut copied = Err(String::new());
        for _ in 0..30 {
            copied = std::fs::copy(&me, &target).map(|_| ()).map_err(|e| e.to_string());
            if copied.is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        copied.map_err(|e| format!("Copie impossible vers {} : {e}", target.display()))?;
    }
    let mut cfg = Config::load();
    let old = cfg.install_path.replace(target.to_string_lossy().into());
    cfg.save();
    // Ancienne copie ailleurs : on la retire pour ne pas en garder deux.
    if let Some(old) = old.map(PathBuf::from).filter(|o| !same_path(o, &target) && !same_path(o, &me)) {
        let _ = std::fs::remove_file(&old);
    }
    set_hook(if with_hook { Some(target.as_path()) } else { None })?;
    let _ = std::process::Command::new(&target).arg("--launch").spawn();
    Ok(target)
}

fn uninstall() -> Result<(), String> {
    set_hook(None)?;
    win::quit_button();
    let mut cfg = Config::load();
    if let Some(p) = cfg.install_path.take() {
        let p = PathBuf::from(p);
        let me = std::env::current_exe().unwrap_or_default();
        if !same_path(&p, &me) {
            for _ in 0..30 {
                if std::fs::remove_file(&p).is_ok() || !p.exists() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            if let Some(dir) = p.parent() {
                let _ = std::fs::remove_dir(dir);
            }
        }
    }
    cfg.save();
    Ok(())
}

enum Msg {
    Tools { git: bool, gh: bool },
    Done(Result<String, String>),
}

struct Installer {
    folder: String,
    with_hook: bool,
    installed: Option<String>,
    hook: Option<String>,
    git: Option<bool>,
    gh: Option<bool>,
    status: Option<Result<String, String>>,
    is_busy: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl Installer {
    fn new(ctx: &egui::Context) -> Installer {
        let (tx, rx) = channel();
        let installer = Installer {
            folder: default_folder(),
            with_hook: true,
            installed: Config::load().install_path.filter(|p| Path::new(p).exists()),
            hook: current_hook(),
            git: None,
            gh: None,
            status: None,
            is_busy: false,
            tx,
            rx,
        };
        installer.check_tools(ctx);
        installer
    }

    fn check_tools(&self, ctx: &egui::Context) {
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Tools { git: data::tool_available("git"), gh: data::tool_available("gh") });
            ctx.request_repaint();
        });
    }

    fn run(&mut self, ctx: &egui::Context, work: impl FnOnce() -> Result<String, String> + Send + 'static) {
        self.is_busy = true;
        self.status = None;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Done(work()));
            ctx.request_repaint();
        });
    }

    fn tool_row(&self, ui: &mut egui::Ui, name: &str, found: Option<bool>, winget_id: &str, why: &str) {
        ui.horizontal(|ui| {
            match found {
                None => ui.label(RichText::new(format!("…  {name}")).color(SUBTLE)),
                Some(true) => ui.label(RichText::new(format!("✓  {name}")).color(OK)),
                Some(false) => ui.label(RichText::new(format!("✗  {name} manquant")).color(ERR)),
            };
            ui.label(RichText::new(why).small().color(SUBTLE));
            if found == Some(false) && ui.button("Installer").clicked() {
                data::winget_install(winget_id);
            }
        });
    }
}

impl eframe::App for Installer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Tools { git, gh } => {
                    self.git = Some(git);
                    self.gh = Some(gh);
                }
                Msg::Done(result) => {
                    self.is_busy = false;
                    self.installed = Config::load().install_path.filter(|p| Path::new(p).exists());
                    self.hook = current_hook();
                    self.status = Some(result);
                }
            }
        }
        egui::CentralPanel::default().frame(egui::Frame::none().fill(BG).inner_margin(Margin::same(22.0))).show(ctx, |ui| {
            ui.label(RichText::new("↑ GitPush").size(24.0).strong().color(ACCENT));
            ui.label(RichText::new("Un bouton sur Claude pour pousser le projet d'une conversation sur GitHub.").color(SUBTLE));
            ui.add_space(14.0);

            if let Some(p) = &self.installed {
                ui.label(RichText::new(format!("Installé dans {p}")).color(OK));
                ui.add_space(6.0);
            }
            ui.label("Dossier d'installation");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.folder).desired_width(ui.available_width() - 110.0));
                if ui.button("Parcourir…").clicked() {
                    if let Some(dir) = win::pick_folder("Dossier où installer GitPush") {
                        self.folder = dir;
                    }
                }
            });
            ui.add_space(8.0);
            ui.checkbox(&mut self.with_hook, "Démarrer avec Claude");
            ui.label(
                RichText::new(format!("Ajoute un hook SessionStart dans {}", settings_path().display())).small().color(SUBTLE),
            );
            if let Some(h) = &self.hook {
                ui.label(RichText::new(format!("Hook actuel : {h}")).small().color(SUBTLE));
            }

            ui.add_space(14.0);
            ui.label("Prérequis");
            self.tool_row(ui, "Git", self.git, "Git.Git", "pour pousser");
            self.tool_row(ui, "GitHub CLI", self.gh, "GitHub.cli", "pour se connecter à GitHub");
            if self.git == Some(false) || self.gh == Some(false) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("L'installation s'ouvre dans une console.").small().color(SUBTLE));
                    if ui.small_button("Revérifier").clicked() {
                        self.git = None;
                        self.gh = None;
                        self.check_tools(ctx);
                    }
                });
            }

            ui.add_space(18.0);
            ui.horizontal(|ui| {
                let label = if self.installed.is_some() { "Mettre à jour" } else { "Installer" };
                let button = egui::Button::new(RichText::new(label).strong().color(Color32::WHITE)).fill(ACCENT);
                if ui.add_enabled(!self.is_busy, button).clicked() {
                    let (folder, with_hook) = (self.folder.clone(), self.with_hook);
                    self.run(ctx, move || {
                        install(&folder, with_hook).map(|p| {
                            format!("Installé : {}. Le bouton GitPush apparaît sur Claude ; tu peux fermer cette fenêtre.", p.display())
                        })
                    });
                }
                if self.installed.is_some() && ui.add_enabled(!self.is_busy, egui::Button::new("Désinstaller")).clicked() {
                    self.run(ctx, || uninstall().map(|_| "GitPush est désinstallé et retiré des réglages de Claude.".into()));
                }
                if self.is_busy {
                    ui.spinner();
                }
            });
            match &self.status {
                Some(Ok(text)) => {
                    ui.add_space(8.0);
                    ui.label(RichText::new(text).color(OK));
                }
                Some(Err(text)) => {
                    ui.add_space(8.0);
                    ui.label(RichText::new(text).color(ERR));
                }
                None => {}
            }
        });
    }
}

pub fn run_installer() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Installer GitPush")
            .with_inner_size([620.0, 470.0])
            .with_resizable(false),
        ..Default::default()
    };
    let _ = eframe::run_native(
        "Installer GitPush",
        options,
        Box::new(|cc| {
            crate::ui::setup_style(&cc.egui_ctx);
            Ok(Box::new(Installer::new(&cc.egui_ctx)))
        }),
    );
}
