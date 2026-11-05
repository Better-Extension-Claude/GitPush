use crate::data::{self, Check, Config, Conversation, Entry, GitHub, Login, Project, PushResult};
use crate::win;
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, FontFamily, FontId, Key, Layout, Margin, Rect, RichText, Rounding, Sense,
    Shape, Stroke, Ui,
};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};


/// Couleurs de la fenêtre : le thème réglé dans BetterClaude (Réglages → Apparence), qui suit
/// Claude par défaut. Les rôles intermédiaires (survol fort, appui) en sont dérivés.
#[derive(Clone, Copy, PartialEq)]
pub struct Palette {
    pub bg: Color32,
    pub field: Color32,
    pub hover: Color32,
    pub hover_strong: Color32,
    pub active: Color32,
    pub border: Color32,
    pub text: Color32,
    pub subtle: Color32,
    pub accent: Color32,
    pub ok: Color32,
    pub err: Color32,
}

fn c(rgb: claudeext::theme::Rgb) -> Color32 {
    Color32::from_rgb(rgb.0, rgb.1, rgb.2)
}

impl Palette {
    fn from_sdk(p: &claudeext::theme::Palette) -> Palette {
        let mix = |a: claudeext::theme::Rgb, t: f32| c(a.mix(p.texte, t));
        Palette {
            bg: c(p.fond),
            field: c(p.champ),
            hover: c(p.survol),
            hover_strong: mix(p.survol, 0.06),
            active: mix(p.survol, 0.12),
            border: c(p.bordure),
            text: c(p.texte),
            subtle: c(p.texte_secondaire),
            accent: c(p.accent),
            ok: c(p.succes),
            err: c(p.erreur),
        }
    }
}

/// Thème en cours : (sombre ?, palette).
static THEME: std::sync::RwLock<Option<(bool, Palette)>> = std::sync::RwLock::new(None);

/// Thème à suivre : GITPUSH_THEME=light|dark (captures de test), sinon celui de BetterClaude.
fn read_theme() -> (bool, Palette) {
    use claudeext::theme;
    match std::env::var("GITPUSH_THEME").as_deref() {
        Ok("light") => (false, Palette::from_sdk(&theme::Palette::clair())),
        Ok("dark") => (true, Palette::from_sdk(&theme::Palette::sombre())),
        _ => (theme::is_dark(), Palette::from_sdk(&theme::current())),
    }
}

fn theme() -> (bool, Palette) {
    if let Some(t) = *THEME.read().unwrap_or_else(|e| e.into_inner()) {
        return t;
    }
    let t = read_theme();
    *THEME.write().unwrap_or_else(|e| e.into_inner()) = Some(t);
    t
}

pub fn pal() -> Palette {
    theme().1
}

/// Suit le thème de BetterClaude : relu toutes les secondes, couleurs changées à la volée.
pub fn follow_theme(ctx: &egui::Context) {
    let id = egui::Id::new("theme-checked");
    let now = ctx.input(|i| i.time);
    let last = ctx.data(|d| d.get_temp::<f64>(id));
    if last.is_none_or(|t| now - t >= 1.0) {
        ctx.data_mut(|d| d.insert_temp(id, now));
        let fresh = read_theme();
        let old = THEME.write().unwrap_or_else(|e| e.into_inner()).replace(fresh);
        if old != Some(fresh) || last.is_none() {
            apply_visuals(ctx);
        }
    }
    ctx.request_repaint_after(Duration::from_secs(1));
}
// Listes déroulantes « Mes dépôts » / « Existantes » et boutons « Nouveau ».
const LIST_W: f32 = 200.0;
const LIST_H: f32 = 360.0;
/// Hauteur d’une ligne de liste déroulante, espacement compris.
const ROW_H: f32 = 38.0;
const NEW_W: f32 = 92.0;

fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// La fenêtre GitPush : une fenêtre Windows normale, centrée sur Claude à l'ouverture.
pub fn run_modal() {
    let cfg = Config::load();
    let size = [800.0_f32, 720.0];
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(win::MODAL_TITLE)
        .with_inner_size(size)
        .with_min_inner_size([520.0, 420.0])
        // Sans la barre de Windows : on la déplace par son en-tête et on la ferme par sa croix.
        .with_decorations(false)
        .with_active(true);
    if let Some((rect, scale)) = win::claude_rect() {
        let x = (rect.x as f32 + (rect.w as f32 - size[0] * scale) / 2.0) / scale;
        let y = (rect.y as f32 + (rect.h as f32 - size[1] * scale) / 2.0) / scale;
        // Pas de x.max(0) : l'écran de Claude peut être à gauche (coordonnées négatives).
        viewport = viewport.with_position([x, y]);
    }
    // GITPUSH_POS=x,y : ouvre la fenêtre là, sans lui donner le focus (captures en arrière-plan).
    if let Some((x, y)) = std::env::var("GITPUSH_POS").ok().and_then(|p| {
        let (x, y) = p.split_once(',')?;
        Some((x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?))
    }) {
        viewport = viewport.with_position([x, y]).with_active(false);
    }
    // GITPUSH_INACTIVE : ouverte à sa place normale, sans prendre le focus (tests).
    if std::env::var_os("GITPUSH_INACTIVE").is_some() {
        viewport = viewport.with_active(false);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    let _ = eframe::run_native(
        win::MODAL_TITLE,
        options,
        Box::new(move |cc| {
            setup_style(&cc.egui_ctx);
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(RawWindowHandle::Win32(h)) = cc.window_handle().map(|h| h.as_raw()) {
                win::round_corners(h.hwnd.get());
                // Position définitive en pixels physiques, sur l'écran de Claude.
                if std::env::var_os("GITPUSH_POS").is_none() {
                    win::center_on_claude(h.hwnd.get(), size);
                }
            }
            Ok(Box::new(ModalApp { modal: Modal::new(&cc.egui_ctx), cfg }))
        }),
    );
}

/// Croix de fermeture : fond rouge et croix blanche au survol, comme celle de Windows.
fn close_button(ui: &mut Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(36.0, 28.0), Sense::click());
    let hovered = resp.hovered();
    if hovered {
        let red = if resp.is_pointer_button_down_on() { Color32::from_rgb(0x94, 0x1e, 0x13) } else { Color32::from_rgb(0xc4, 0x2b, 0x1c) };
        ui.painter().rect_filled(rect, Rounding::same(6.0), red);
    }
    let (c, d) = (rect.center(), 5.5);
    let stroke = Stroke::new(1.6_f32, if hovered { Color32::WHITE } else { pal().text });
    ui.painter().line_segment([c + vec2(-d, -d), c + vec2(d, d)], stroke);
    ui.painter().line_segment([c + vec2(-d, d), c + vec2(d, -d)], stroke);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Croix seule (sans fond ni contour) pour retirer une ligne : discrète, plus claire au survol.
fn remove_cross(ui: &mut Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());
    let (c, d) = (rect.center(), 4.5);
    let stroke = Stroke::new(1.5_f32, if resp.hovered() { pal().text } else { pal().subtle });
    ui.painter().line_segment([c + vec2(-d, -d), c + vec2(d, d)], stroke);
    ui.painter().line_segment([c + vec2(-d, d), c + vec2(d, -d)], stroke);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Liste déroulante aux lignes hautes, à la taille de son contenu (« Dossier », « Mes dépôts », « Existantes »).
/// egui garde la taille du premier affichage d'une liste : quand son nombre de lignes change (« Chargement… »
/// puis les dépôts), on la recrée sous un autre identifiant, rouverte si elle l'était.
fn tall_list<R>(id: &str, text: &str, width: f32, rows: usize, reopen: bool, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<Option<R>> {
    let popup_of = |ui: &Ui, n: usize| ui.make_persistent_id(egui::Id::new(format!("{id}#{n}"))).with("popup");
    let last_key = egui::Id::new(id).with("rows");
    let last = ui.data(|d| d.get_temp::<usize>(last_key));
    ui.data_mut(|d| d.insert_temp(last_key, rows));
    let was_open = last.filter(|n| *n != rows).is_some_and(|n| ui.memory(|m| m.is_popup_open(popup_of(ui, n))));
    // GITPUSH_OPEN_LIST=<id> garde la liste ouverte : sert à la capturer sans toucher à la souris.
    if reopen || was_open || std::env::var("GITPUSH_OPEN_LIST").is_ok_and(|v| v == id) {
        let popup = popup_of(ui, rows);
        ui.memory_mut(|m| m.open_popup(popup));
    }
    egui::ComboBox::from_id_salt(format!("{id}#{rows}")).width(width).height(LIST_H).selected_text(text).show_ui(ui, |ui| {
        // À la taille de son contenu, jusqu'à LIST_H (puis défilement).
        ui.set_min_height((8.0 + rows.max(1) as f32 * ROW_H).min(LIST_H - 20.0));
        ui.spacing_mut().button_padding = vec2(12.0, 10.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.add_space(4.0);
        let r = add(ui);
        ui.add_space(4.0);
        r
    })
}

/// Image egui en BMP 32 bits (de haut en bas).
fn bmp(image: &egui::ColorImage) -> Vec<u8> {
    let [w, h] = image.size;
    let mut out = Vec::with_capacity(54 + w * h * 4);
    let size = (54 + w * h * 4) as u32;
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(-(h as i32)).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for p in &image.pixels {
        out.extend_from_slice(&[p.b(), p.g(), p.r(), 255]);
    }
    out
}

struct ModalApp {
    cfg: Config,
    modal: Modal,
}

impl eframe::App for ModalApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        follow_theme(ctx);
        self.modal.poll_refresh();
        if self.modal.show(ctx, &mut self.cfg) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // GITPUSH_SHOT=<fichier.bmp> : capture la fenêtre au bout de 3 s puis la ferme.
        if let Ok(path) = std::env::var("GITPUSH_SHOT") {
            let shot = ctx.input(|i| {
                i.raw.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = shot {
                let _ = std::fs::write(&path, bmp(&image));
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if ctx.input(|i| i.time) > 3.0 && !ctx.data(|d| d.get_temp::<bool>(egui::Id::new("shot")).unwrap_or(false)) {
                ctx.data_mut(|d| d.insert_temp(egui::Id::new("shot"), true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
            }
            ctx.request_repaint();
        }
    }
}

pub fn setup_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    let mut load = |key: &str, file: &str| -> bool {
        match std::fs::read(format!("C:\\Windows\\Fonts\\{file}")) {
            Ok(bytes) => {
                fonts.font_data.insert(key.into(), egui::FontData::from_owned(bytes));
                true
            }
            Err(_) => false,
        }
    };
    let has_regular = load("segoe", "segoeui.ttf");
    let has_bold = load("segoe-bold", "segoeuib.ttf");
    let has_symbols = load("segoe-symbol", "seguisym.ttf");
    let prop = fonts.families.get_mut(&FontFamily::Proportional).unwrap();
    if has_regular {
        prop.insert(0, "segoe".into());
    }
    if has_symbols {
        prop.push("segoe-symbol".into());
    }
    let mut bold_family = if has_bold { vec!["segoe-bold".to_string()] } else { Vec::new() };
    bold_family.extend(fonts.families[&FontFamily::Proportional].clone());
    bold_family.extend(fallback);
    fonts.families.insert(FontFamily::Name("bold".into()), bold_family);
    ctx.set_fonts(fonts);
    apply_visuals(ctx);
    // Les deux thèmes d'egui (il suit le réglage « applications » de Windows) : sinon, en mode
    // clair, boutons et textes reprennent la taille par défaut.
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = vec2(8.0, 8.0);
        s.spacing.button_padding = vec2(12.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.5));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.5));
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(12.5));
    });
}

/// Couleurs des widgets egui, d'après la palette du thème en cours.
fn apply_visuals(ctx: &egui::Context) {
    let p = pal();
    let mut v = if theme().0 { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = p.bg;
    v.window_fill = p.bg;
    v.extreme_bg_color = p.field;
    v.faint_bg_color = p.hover;
    v.override_text_color = Some(p.text);
    v.selection.bg_fill = p.accent.linear_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0_f32, p.accent);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.rounding = Rounding::same(7.0);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.inactive.weak_bg_fill = p.hover;
    v.widgets.inactive.bg_fill = p.field;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.hovered.weak_bg_fill = p.hover_strong;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.subtle);
    v.widgets.active.weak_bg_fill = p.active;
    v.widgets.open.weak_bg_fill = p.hover_strong;
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.window_rounding = Rounding::same(10.0);
    v.menu_rounding = Rounding::same(8.0);
    // egui suit le thème de Windows : sous Windows en mode clair, il prendrait ses propres
    // couleurs claires (boutons gris « Windows »). Nos couleurs valent pour ses deux thèmes.
    ctx.set_visuals_of(egui::Theme::Dark, v.clone());
    ctx.set_visuals_of(egui::Theme::Light, v);
}

// ---------------------------------------------------------------- Modal

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Info,
    Ok,
    Err,
}

enum Msg {
    /// Conversations relues après un clic sur le bouton, titre affiché dans Claude, seconde lecture.
    Conversations(Vec<Conversation>, Option<String>, bool),
    Loaded { cwd: String, project: Project, ignored: HashSet<String>, selection: Vec<String> },
    Repos(Option<Vec<String>>),
    Branches(String, Vec<String>),
    Pushed { cwd: String, result: PushResult, selection: Vec<String>, date: String },
    Created(Result<String, String>),
    Account(GitHub),
    Login(Login),
    AccountChanged(Result<(), String>),
    NoGit,
    RemoteFiles { remote: String, auto: bool, result: Option<(Vec<String>, usize, usize)> },
    Nothing,
}

struct Modal {
    ctx: egui::Context,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    convs: Vec<Conversation>,
    conv: usize,
    cwd: String,
    project: Project,
    sel: Vec<String>,
    ignored: HashSet<String>,
    expanded: HashSet<String>,
    cache: HashMap<String, Vec<Entry>>,
    is_loading: bool,
    repos: Option<Vec<String>>,
    is_loading_repos: bool,
    branches: Vec<String>,
    is_new_repo_open: bool,
    new_name: String,
    is_new_public: bool,
    message: String,
    status: String,
    kind: Kind,
    is_busy: bool,
    can_force: bool,
    close_at: Option<Instant>,
    account: GitHub,
    is_logging_in: bool,
    is_git_missing: bool,
    /// Sélection prise dans le dépôt GitHub : (fichiers présents ici, fichiers en ligne).
    from_remote: Option<(usize, usize)>,
    /// Rouvre la liste des dossiers après un retrait (egui la ferme à chaque clic).
    reopen_folders: bool,
    /// Signal envoyé par un clic sur le bouton alors que la fenêtre est déjà ouverte.
    signal: isize,
    /// Seconde lecture de la conversation affichée : Claude l’enregistre environ 1 s après le changement.
    recheck_at: Option<Instant>,
    /// Conversation choisie à la main dans la liste : la seconde lecture ne la remplace pas.
    picked_by_hand: bool,
}

enum Action {
    Expand(String),
    Toggle(String),
}

impl Modal {
    fn new(ctx: &egui::Context) -> Modal {
        let (tx, rx) = channel();
        let t = Instant::now();
        let convs = data::recent_conversations(25);
        crate::debug(format!("modal : {} conversations lues en {:?}, affichée : {}", convs.len(), t.elapsed(), convs.first().map(|c| c.label()).unwrap_or_default()));
        for c in &convs {
            crate::debug(format!("  {} | clé {} | dossiers {:?} | poussé {}", c.title, c.key, c.folders, c.cwd));
        }
        let mut m = Modal {
            ctx: ctx.clone(),
            tx,
            rx,
            convs,
            conv: 0,
            cwd: String::new(),
            project: Project::default(),
            sel: Vec::new(),
            ignored: HashSet::new(),
            expanded: HashSet::new(),
            cache: HashMap::new(),
            is_loading: false,
            repos: None,
            is_loading_repos: false,
            branches: Vec::new(),
            is_new_repo_open: false,
            new_name: String::new(),
            is_new_public: false,
            message: String::new(),
            status: String::new(),
            kind: Kind::Info,
            is_busy: false,
            can_force: false,
            close_at: None,
            account: GitHub::Checking,
            is_logging_in: false,
            is_git_missing: false,
            from_remote: None,
            reopen_folders: false,
            signal: win::refresh_signal(),
            recheck_at: Some(Instant::now() + Duration::from_millis(1500)),
            picked_by_hand: false,
        };
        let shown = displayed_index(&m.convs, win::displayed_title().as_deref()).unwrap_or(0);
        m.select(shown);
        m.check_account();
        m.spawn(|| if data::tool_available("git") { Msg::Nothing } else { Msg::NoGit });
        m
    }

    fn check_account(&self) {
        self.spawn(|| Msg::Account(data::github_account()));
    }

    /// Compte GitHub de l'app, en haut à droite (mise en page de droite à gauche).
    fn account_badge(&mut self, ui: &mut Ui) {
        if self.is_git_missing {
            let text = RichText::new("Installer Git").small().color(Color32::WHITE);
            if ui.add(egui::Button::new(text).fill(pal().accent)).clicked() {
                data::winget_install("Git.Git");
                self.set_status("Installation de Git lancée dans une console. Rouvrez GitPush une fois terminée.", Kind::Info);
            }
            ui.add_space(8.0);
        }
        if self.is_logging_in {
            ui.label(RichText::new("Connexion GitHub en cours…").small().color(pal().subtle));
            return;
        }
        match self.account.clone() {
            GitHub::Checking => {
                ui.label(RichText::new("GitHub…").small().color(pal().subtle));
            }
            GitHub::Connected { login, accounts } => {
                let text = RichText::new(format!("●  GitHub : {login}  ▾")).small().color(pal().ok);
                ui.menu_button(text, |ui| {
                    ui.label(RichText::new("Compte utilisé pour les push, indépendant de Claude").small().color(pal().subtle));
                    ui.separator();
                    for account in &accounts {
                        if account == &login {
                            ui.label(RichText::new(format!("✓  {account}")).color(pal().ok));
                        } else if ui.button(format!("Utiliser {account}")).clicked() {
                            let account = account.clone();
                            self.spawn(move || Msg::AccountChanged(data::github_switch(&account)));
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    if ui.button("Ajouter un compte GitHub…").clicked() {
                        self.sign_in();
                        ui.close_menu();
                    }
                    if ui.button(format!("Déconnecter {login}")).clicked() {
                        self.spawn(move || Msg::AccountChanged(data::github_logout(&login)));
                        ui.close_menu();
                    }
                });
            }
            GitHub::SignedOut => {
                let text = RichText::new("Connecter GitHub").small().color(Color32::WHITE);
                if ui.add(egui::Button::new(text).fill(pal().accent)).clicked() {
                    self.sign_in();
                }
            }
            GitHub::NoCli => {
                let text = RichText::new("Installer GitHub CLI").small().color(Color32::WHITE);
                if ui.add(egui::Button::new(text).fill(pal().accent)).on_hover_text("GitPush utilise l’outil GitHub CLI pour se connecter à GitHub.").clicked() {
                    data::winget_install("GitHub.cli");
                    self.set_status("Installation lancée dans une console. Rouvrez GitPush une fois terminée.", Kind::Info);
                }
            }
        }
    }

    /// Connexion d'un compte : page GitHub dans le navigateur, code affiché ici.
    fn sign_in(&mut self) {
        self.is_logging_in = true;
        self.set_status("Ouverture de la page GitHub…", Kind::Info);
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            data::github_login(|step| {
                let _ = tx.send(Msg::Login(step));
                ctx.request_repaint();
            });
        });
    }

    fn spawn(&self, work: impl FnOnce() -> Msg + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(work());
            ctx.request_repaint();
        });
    }

    fn set_status(&mut self, text: impl Into<String>, kind: Kind) {
        self.status = text.into();
        self.kind = kind;
    }

    /// Conversation affichée dans Claude, relue au clic sur le bouton (et une seconde fois 1,5 s
    /// après, le temps que Claude l'enregistre). Rien n'est relu le reste du temps.
    fn poll_refresh(&mut self) {
        let now = Instant::now();
        if win::refresh_requested(self.signal) {
            self.picked_by_hand = false;
            self.refresh_conversation(false);
            self.recheck_at = Some(now + Duration::from_millis(1500));
        } else if self.recheck_at.is_some_and(|t| now >= t) {
            self.recheck_at = None;
            if !self.picked_by_hand {
                self.refresh_conversation(true);
            }
        }
        if let Some(t) = self.recheck_at {
            self.ctx.request_repaint_after(t.saturating_duration_since(now));
        }
    }

    /// Au clic : la conversation affichée est lue tout de suite dans la fenêtre de Claude (son titre)
    /// et choisie dans la liste déjà chargée ; la liste est ensuite relue en arrière-plan.
    fn refresh_conversation(&mut self, second: bool) {
        let title = win::displayed_title();
        if !second {
            if let Some(i) = displayed_index(&self.convs, title.as_deref()).filter(|i| *i != self.conv) {
                if !self.is_busy {
                    self.select(i);
                }
            }
        }
        self.spawn(move || Msg::Conversations(data::recent_conversations(25), title, second));
    }

    fn apply_conversations(&mut self, convs: Vec<Conversation>, title: Option<String>) {
        if convs.is_empty() || self.is_busy {
            return;
        }
        let index = displayed_index(&convs, title.as_deref()).unwrap_or(0);
        let current = self.convs.get(self.conv).map(|c| c.key.clone());
        crate::debug(format!("actualisation : affichée {} (titre lu : {title:?})", convs[index].label()));
        if current.as_deref() == Some(convs[index].key.as_str()) {
            // Même conversation : on garde ce qui est à l'écran (dossier, cases cochées).
            let cwd = self.cwd.clone();
            self.convs = convs;
            self.conv = index;
            self.convs[index].cwd = cwd;
        } else {
            self.convs = convs;
            self.select(index);
        }
    }
    fn select(&mut self, index: usize) {
        let Some(c) = self.convs.get(index) else {
            self.set_status("Aucune conversation Claude trouvée.", Kind::Err);
            return;
        };
        self.conv = index;
        let cwd = c.cwd.clone();
        self.open_folder(cwd);
    }

    /// Dossier choisi pour la conversation (liste ou « Autre dossier… »), retenu dans la config.
    fn choose_folder(&mut self, folder: String, cfg: &mut Config) {
        let Some(c) = self.convs.get_mut(self.conv) else { return };
        if !c.folders.iter().any(|f| f.eq_ignore_ascii_case(&folder)) {
            c.folders.push(folder.clone());
        }
        c.cwd = folder.clone();
        cfg.folders.insert(c.key.clone(), folder.clone());
        if let Some(h) = cfg.hidden.get_mut(&c.key) {
            h.retain(|x| !x.eq_ignore_ascii_case(&folder));
        }
        cfg.save();
        self.open_folder(folder);
    }

    /// Retire un dossier de la liste de la conversation, pour de bon (retenu dans la config).
    fn remove_folder(&mut self, folder: String, cfg: &mut Config) {
        let Some(c) = self.convs.get_mut(self.conv) else { return };
        c.folders.retain(|f| !f.eq_ignore_ascii_case(&folder));
        let hidden = cfg.hidden.entry(c.key.clone()).or_default();
        if !hidden.iter().any(|h| h.eq_ignore_ascii_case(&folder)) {
            hidden.push(folder.clone());
        }
        let was_current = c.cwd.eq_ignore_ascii_case(&folder);
        if cfg.folders.get(&c.key).is_some_and(|f| f.eq_ignore_ascii_case(&folder)) {
            cfg.folders.remove(&c.key);
        }
        let next = c.folders.first().cloned();
        cfg.save();
        if let (true, Some(next)) = (was_current, next) {
            self.convs[self.conv].cwd = next.clone();
            self.open_folder(next);
        }
    }

    fn open_folder(&mut self, cwd: String) {
        self.cwd = cwd;
        self.from_remote = None;
        self.cache.clear();
        self.expanded.clear();
        self.sel.clear();
        self.branches.clear();
        self.is_loading = true;
        let no_folder = self.convs.get(self.conv).map(|c| c.folders.is_empty()).unwrap_or(false)
            && (self.cwd.contains("scratch-workspaces") || self.cwd.contains("side-session-notes"));
        if no_folder {
            self.set_status("Cette conversation n’a pas de dossier de projet : choisissez-le avec « Autre dossier… ».", Kind::Info);
        } else {
            self.set_status("", Kind::Info);
        }
        let cwd = self.cwd.clone();
        self.spawn(move || {
            let mut cfg = Config::load();
            let project = data::load_project(&mut cfg, &cwd);
            let ignored = data::ignored(&cwd);
            let selection = data::normalize(&cwd, project.selection.clone(), &ignored);
            Msg::Loaded { cwd, project, ignored, selection }
        });
    }

    fn load_branches(&self) {
        let remote = self.project.remote.clone();
        if remote.trim().is_empty() {
            return;
        }
        self.spawn(move || Msg::Branches(remote.clone(), data::remote_branches(&remote)));
    }

    fn set_remote(&mut self, url: &str) {
        let url = url.trim().trim_end_matches('/');
        if url.is_empty() {
            return;
        }
        self.project.remote = if url.ends_with(".git") { url.to_string() } else { format!("{url}.git") };
        self.branches.clear();
        self.load_branches();
        self.load_remote_files(true);
    }

    /// Pré-coche les fichiers déjà présents dans le dépôt GitHub. `auto` : seulement si le
    /// projet n'a encore rien de coché (pas de push enregistré).
    fn load_remote_files(&self, auto: bool) {
        let (cwd, remote, branch) = (self.cwd.clone(), self.project.remote.clone(), self.project.branch.clone());
        if remote.trim().is_empty() || (auto && (!self.sel.is_empty() || self.project.last_by.is_some())) {
            return;
        }
        self.spawn(move || Msg::RemoteFiles { result: data::remote_selection(&cwd, &remote, &branch), remote, auto });
    }

    fn push(&mut self, force: bool) {
        if self.is_busy {
            return;
        }
        self.is_busy = true;
        self.can_force = false;
        self.set_status("Push en cours…", Kind::Info);
        let (cwd, remote, branch) = (self.cwd.clone(), self.project.remote.clone(), self.project.branch.clone());
        let (selection, message) = (self.sel.clone(), self.message.clone());
        self.spawn(move || {
            let result = data::push(&cwd, &remote, &branch, &selection, &message, force);
            let date = data::git(&cwd, &["log", "-1", "--format=%cI"]).out.trim().to_string();
            Msg::Pushed { cwd, result, selection, date }
        });
    }

    fn receive(&mut self, cfg: &mut Config) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Loaded { cwd, project, ignored, selection } if cwd == self.cwd => {
                    self.project = project;
                    self.ignored = ignored;
                    self.sel = selection;
                    self.is_loading = false;
                    self.load_branches();
                    self.load_remote_files(true);
                }
                Msg::RemoteFiles { remote, auto, result } if remote == self.project.remote => match result {
                    // Automatique : seulement si rien n'est coché (aucun push enregistré).
                    Some((files, here, total)) if !auto || self.sel.is_empty() => {
                        self.sel = files;
                        self.from_remote = Some((here, total));
                    }
                    Some(_) => {}
                    None if !auto => self.set_status("Impossible de lire les fichiers du dépôt sur GitHub (branche absente ?).", Kind::Err),
                    None => {}
                },
                Msg::RemoteFiles { .. } => {}
                Msg::Loaded { .. } => {}
                Msg::Repos(repos) => {
                    self.is_loading_repos = false;
                    if repos.is_none() {
                        self.set_status("Impossible de lister vos dépôts : connectez GitHub (en haut à droite).", Kind::Err);
                    }
                    self.repos = repos;
                }
                Msg::Branches(remote, list) => {
                    if remote == self.project.remote {
                        self.branches = list;
                    }
                }
                Msg::Pushed { cwd, result, selection, date } => {
                    self.is_busy = false;
                    self.can_force = result.can_force;
                    self.set_status(result.message.clone(), if result.ok { Kind::Ok } else { Kind::Err });
                    if result.ok {
                        let mut p = self.project.clone();
                        p.selection = selection;
                        p.last_by = Some("vous".into());
                        p.last_at = Some(date);
                        p.last_commit = result.commit;
                        cfg.projects.insert(cwd, p.clone());
                        cfg.save();
                        self.project = p;
                        self.from_remote = None;
                        self.message.clear();
                        self.close_at = Some(Instant::now() + Duration::from_millis(1600));
                        self.load_branches();
                    }
                }
                Msg::Created(Ok(url)) => {
                    self.is_busy = false;
                    self.is_new_repo_open = false;
                    self.project.branch = "main".into();
                    self.set_remote(&url);
                    self.repos = None;
                    self.set_status(format!("Dépôt créé : {}", url.trim_end_matches(".git")), Kind::Ok);
                }
                Msg::Created(Err(e)) => {
                    self.is_busy = false;
                    self.set_status(format!("Échec de la création : {e}"), Kind::Err);
                }
                Msg::Account(account) => {
                    if let GitHub::Connected { login, .. } = &account {
                        let changed = matches!(&self.account, GitHub::Connected { login: old, .. } if old != login);
                        if changed {
                            self.set_status(format!("Les push passent maintenant par le compte GitHub {login}."), Kind::Ok);
                            self.repos = None;
                        }
                    }
                    self.account = account;
                }
                Msg::Login(Login::Code(code)) => {
                    self.set_status(
                        format!("Code {code} (copié) : collez-le dans la page GitHub ouverte. Pour un autre compte, changez de compte sur cette page."),
                        Kind::Info,
                    );
                }
                Msg::Login(Login::Done(result)) => {
                    self.is_logging_in = false;
                    match result {
                        Ok(()) => self.set_status("Compte GitHub connecté.", Kind::Ok),
                        Err(e) => self.set_status(format!("Connexion GitHub annulée ou échouée : {e}"), Kind::Err),
                    }
                    self.check_account();
                }
                Msg::AccountChanged(Err(e)) => self.set_status(e, Kind::Err),
                Msg::AccountChanged(Ok(())) => self.check_account(),
                Msg::Conversations(convs, title, second) => {
                    if !(second && self.picked_by_hand) {
                        self.apply_conversations(convs, title);
                    }
                }
                Msg::NoGit => {
                    self.is_git_missing = true;
                    self.set_status("Git n'est pas installé : GitPush en a besoin pour pousser.", Kind::Err);
                }
                Msg::Nothing => {}
            }
        }
    }

    /// Dessine le modal ; renvoie vrai quand il faut le fermer.
    fn show(&mut self, ctx: &egui::Context, cfg: &mut Config) -> bool {
        self.receive(cfg);
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape) || i.viewport().close_requested());
        if self.close_at.map(|t| Instant::now() >= t).unwrap_or(false) {
            close = true;
        }
        if self.close_at.is_some() || self.is_busy || self.is_loading {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(pal().bg).stroke(Stroke::new(1.0_f32, pal().border)).inner_margin(Margin::same(20.0)))
            .show(ctx, |ui| {
                if self.contents(ui, cfg) {
                    close = true;
                }
            });
        close
    }

    fn contents(&mut self, ui: &mut Ui, cfg: &mut Config) -> bool {
        let mut close = false;

        // En-tête, qui sert aussi de poignée pour déplacer la fenêtre (elle n'a pas de barre de
        // titre). Créée avant les boutons de l'en-tête, qui passent donc devant elle.
        let top = ui.max_rect();
        let handle = Rect::from_min_size(top.min - vec2(20.0, 20.0), vec2(top.width() + 40.0, 58.0));
        let drag = ui.interact(handle, ui.id().with("deplacer"), Sense::click_and_drag());
        if drag.drag_started_by(egui::PointerButton::Primary) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("↑ GitPush").font(bold(22.0)).color(pal().accent));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if close_button(ui).on_hover_text("Fermer (Échap)").clicked() {
                    close = true;
                }
                ui.add_space(12.0);
                self.account_badge(ui);
            });
        });
        ui.add_space(4.0);

        let field_w = ui.available_width() - 110.0;
        egui::Grid::new("form").num_columns(2).spacing([14.0, 10.0]).min_col_width(96.0).show(ui, |ui| {
            ui.label(RichText::new("Conversation").color(pal().subtle));
            let current = self.convs.get(self.conv).map(|c| c.label()).unwrap_or_default();
            let mut pick = None;
            egui::ComboBox::from_id_salt("conv").width(field_w).selected_text(current).show_ui(ui, |ui| {
                for (i, c) in self.convs.iter().enumerate() {
                    if ui.selectable_label(i == self.conv, c.label()).clicked() {
                        pick = Some(i);
                    }
                }
            });
            if let Some(i) = pick.filter(|i| *i != self.conv) {
                self.picked_by_hand = true;
                self.select(i);
            }
            ui.end_row();

            // Conversation avec plusieurs dossiers, ou sans dossier : on choisit lequel pousser.
            ui.label(RichText::new("Dossier").color(pal().subtle));
            ui.horizontal(|ui| {
                let folders = self.convs.get(self.conv).map(|c| c.folders.clone()).unwrap_or_default();
                let (mut chosen, mut removed) = (None, None);
                let list_w = field_w - 140.0;
                tall_list("folder", &self.cwd.clone(), list_w, folders.len(), std::mem::take(&mut self.reopen_folders), ui, |ui| {
                    let can_remove = folders.len() > 1;
                    for f in &folders {
                        ui.horizontal(|ui| {
                            // Ligne pleine largeur, texte à gauche ; la croix à droite retire le dossier.
                            let w = list_w - 16.0 - if can_remove { 44.0 } else { 0.0 };
                            let (rect, row) = ui.allocate_exact_size(vec2(w, 36.0), Sense::click());
                            let is_current = f.eq_ignore_ascii_case(&self.cwd);
                            // Ligne choisie : fond de sélection du thème ; survol : fond de survol ; texte du thème.
                            let fill = if is_current { Some(ui.visuals().selection.bg_fill) } else { row.hovered().then(|| pal().hover) };
                            if let Some(fill) = fill {
                                ui.painter().rect_filled(rect, Rounding::same(6.0), fill);
                            }
                            let color = pal().text;
                            ui.painter().text(rect.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, f, FontId::proportional(14.0), color);
                            let row = row.on_hover_cursor(egui::CursorIcon::PointingHand);
                            if row.clicked() {
                                chosen = Some(f.clone());
                            }
                            if can_remove && remove_cross(ui).on_hover_text("Retirer ce dossier de la liste").clicked() {
                                removed = Some(f.clone());
                            }
                        });
                    }
                });
                if let Some(f) = removed {
                    self.remove_folder(f, cfg);
                    self.reopen_folders = true;
                    self.ctx.request_repaint();
                }
                if ui.button("Autre dossier…").clicked() {
                    chosen = win::pick_folder("Dossier du projet à pousser", win::MODAL_TITLE);
                }
                if let Some(f) = chosen.filter(|f| !f.eq_ignore_ascii_case(&self.cwd)) {
                    self.choose_folder(f, cfg);
                }
            });
            ui.end_row();

            ui.label(RichText::new("Dépôt").color(pal().subtle));
            ui.horizontal(|ui| {
                let mut remote = self.project.remote.clone();
                let edit = ui.add(egui::TextEdit::singleline(&mut remote).hint_text("https://github.com/moi/projet.git").desired_width(field_w - 320.0));
                if edit.changed() {
                    self.project.remote = remote.clone();
                }
                if edit.lost_focus() {
                    self.set_remote(&remote);
                }
                let mut chosen = None;
                let combo = tall_list("repos", "Mes dépôts", LIST_W, self.repos.as_ref().map_or(1, |r| r.len()), false, ui, |ui| {
                    match &self.repos {
                        None => {
                            ui.label(RichText::new("Chargement…").color(pal().subtle));
                        }
                        Some(list) if list.is_empty() => {
                            ui.label(RichText::new("Aucun dépôt").color(pal().subtle));
                        }
                        Some(list) => {
                            for url in list {
                                if ui.selectable_label(false, url.trim_start_matches("https://github.com/")).clicked() {
                                    chosen = Some(url.clone());
                                }
                            }
                        }
                    }
                });
                // Chargée dès que la liste est ouverte (clic, ou ouverte pour une capture de test).
                if (combo.response.clicked() || combo.inner.is_some()) && self.repos.is_none() && !self.is_loading_repos {
                    self.is_loading_repos = true;
                    self.spawn(|| Msg::Repos(data::my_repos()));
                }
                if let Some(url) = chosen {
                    self.set_remote(&url);
                }
                let label = if self.is_new_repo_open { "Annuler" } else { "Nouveau" };
                if ui.add_sized([NEW_W, 30.0], egui::Button::new(label)).clicked() {
                    self.is_new_repo_open = !self.is_new_repo_open;
                }
            });
            ui.end_row();

            if self.is_new_repo_open {
                ui.label("");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.new_name).hint_text("nom-du-depot").desired_width(field_w - 300.0));
                    ui.radio_value(&mut self.is_new_public, false, "Privé");
                    ui.radio_value(&mut self.is_new_public, true, "Public");
                    let create = ui.add_enabled(!self.is_busy && !self.new_name.trim().is_empty(), egui::Button::new("Créer sur GitHub"));
                    if create.clicked() {
                        self.is_busy = true;
                        self.set_status("Création du dépôt…", Kind::Info);
                        let (name, public) = (self.new_name.trim().to_string(), self.is_new_public);
                        self.spawn(move || Msg::Created(data::create_repo(&name, public)));
                    }
                });
                ui.end_row();
            }

            ui.label(RichText::new("Branche").color(pal().subtle));
            ui.horizontal(|ui| {
                let hint = if self.project.branch.is_empty() { "nom de la nouvelle branche" } else { "main" };
                let edit = ui.add(egui::TextEdit::singleline(&mut self.project.branch).hint_text(hint).desired_width(field_w - 320.0));
                let mut chosen = None;
                tall_list("branches", "Existantes", LIST_W, self.branches.len(), false, ui, |ui| {
                    if self.branches.is_empty() {
                        ui.label(RichText::new("Aucune branche distante").color(pal().subtle));
                    }
                    for b in &self.branches {
                        if ui.selectable_label(*b == self.project.branch, b).clicked() {
                            chosen = Some(b.clone());
                        }
                    }
                });
                if let Some(b) = chosen {
                    self.project.branch = b;
                }
                // Nouvelle branche : on vide le champ pour taper son nom ; le push la crée.
                if ui.add_sized([NEW_W, 30.0], egui::Button::new("Nouveau")).on_hover_text("Créée sur GitHub au prochain push").clicked() {
                    self.project.branch.clear();
                    edit.request_focus();
                }
            });
            ui.end_row();
        });

        ui.add_space(6.0);
        let mut actions = Vec::new();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Fichiers").font(bold(15.0)));
            ui.label(RichText::new(format!("{} coché{}", self.sel.len(), if self.sel.len() > 1 { "s" } else { "" })).color(pal().subtle));
            ui.add_space(8.0);
            let link = |ui: &mut Ui, t: &str| ui.add(egui::Button::new(RichText::new(t).small().color(pal().accent)).frame(false)).clicked();
            if link(ui, "Tout cocher") {
                self.sel = data::list_dir(&self.cwd, "").into_iter().filter(|e| !self.ignored.contains(&e.path)).map(|e| e.path).collect();
            }
            if link(ui, "Tout décocher") {
                self.sel.clear();
            }
            if self.project.last_by.is_some() && link(ui, "↺ Dernier push") {
                self.sel = data::normalize(&self.cwd, self.project.selection.clone(), &self.ignored);
                self.from_remote = None;
            }
            if !self.project.remote.trim().is_empty() && link(ui, "↺ Fichiers du dépôt") {
                self.load_remote_files(false);
            }
        });
        // Sur sa propre ligne : à côté des liens, le texte se chevauchait.
        let info = match (&self.from_remote, &self.project.last_by, &self.project.last_at) {
            (Some((here, total)), _, _) => format!("Pré-coché : fichiers déjà sur GitHub ({here} sur {total} présents ici)"),
            (None, Some(by), Some(at)) => format!("Pré-coché : dernier push de {by}, {}", short_date(at)),
            (None, Some(by), None) => format!("Pré-coché : dernier push de {by}"),
            _ => "Aucun push enregistré".to_string(),
        };
        ui.add_space(-4.0);
        ui.label(RichText::new(info).small().color(pal().subtle));

        let tree_h = (ui.available_height() - 96.0).max(80.0);
        egui::Frame::none().fill(pal().field).rounding(Rounding::same(10.0)).inner_margin(Margin::same(6.0)).show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(tree_h).min_scrolled_height(tree_h).auto_shrink([false, false]).show(ui, |ui| {
                if self.is_loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Lecture du projet…").color(pal().subtle));
                    });
                } else {
                    let mut budget = 3000;
                    self.rows(ui, "", 0, &mut actions, &mut budget);
                }
            });
        });
        for a in actions {
            match a {
                Action::Expand(p) => {
                    if !self.expanded.remove(&p) {
                        self.expanded.insert(p);
                    }
                }
                Action::Toggle(p) => self.sel = data::toggle(&self.cwd, &self.sel, &p, &self.ignored),
            }
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let msg = ui.add(egui::TextEdit::singleline(&mut self.message).hint_text("Message du commit (optionnel)").desired_width(ui.available_width() - 250.0));
            if msg.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                self.push(false);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let branch = if self.project.branch.trim().is_empty() { "main" } else { self.project.branch.trim() };
                let label = if self.is_busy { "Push en cours…".to_string() } else { format!("↑  Pousser sur {branch}") };
                let button = egui::Button::new(RichText::new(label).font(bold(14.5)).color(Color32::WHITE)).fill(pal().accent).min_size(vec2(170.0, 34.0));
                if ui.add_enabled(!self.is_busy && !self.is_loading, button).clicked() {
                    self.push(false);
                }
                if self.can_force && ui.button("Forcer").on_hover_text("Écrase la branche distante").clicked() {
                    self.push(true);
                }
            });
        });
        if !self.status.is_empty() {
            let (icon, color) = match self.kind {
                Kind::Info => ("…", pal().subtle),
                Kind::Ok => ("✓", pal().ok),
                Kind::Err => ("✗", pal().err),
            };
            ui.label(RichText::new(format!("{icon} {}", self.status)).color(color));
        }
        close
    }

    fn rows(&mut self, ui: &mut Ui, dir: &str, depth: usize, out: &mut Vec<Action>, budget: &mut usize) {
        let cwd = self.cwd.clone();
        let entries = self.cache.entry(dir.to_string()).or_insert_with(|| data::list_dir(&cwd, dir)).clone();
        if entries.is_empty() && depth == 0 {
            ui.label(RichText::new("Dossier vide.").color(pal().subtle));
        }
        for e in entries {
            if *budget == 0 {
                ui.label(RichText::new("… liste tronquée").color(pal().subtle));
                return;
            }
            *budget -= 1;
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
            let painter = ui.painter();
            if resp.hovered() {
                painter.rect_filled(rect, Rounding::same(6.0), pal().hover);
            }
            let x0 = rect.left() + 6.0 + depth as f32 * 18.0;
            let cy = rect.center().y;
            let is_open = self.expanded.contains(&e.path);
            if e.is_dir {
                let pts = if is_open {
                    vec![pos2(x0 + 3.0, cy - 3.0), pos2(x0 + 13.0, cy - 3.0), pos2(x0 + 8.0, cy + 3.0)]
                } else {
                    vec![pos2(x0 + 5.0, cy - 5.0), pos2(x0 + 11.0, cy), pos2(x0 + 5.0, cy + 5.0)]
                };
                painter.add(Shape::convex_polygon(pts, pal().subtle, Stroke::NONE));
            }
            let state = data::check(&self.sel, &e.path);
            paint_check(painter, Rect::from_min_size(pos2(x0 + 22.0, cy - 8.0), vec2(16.0, 16.0)), state);
            let is_ignored = self.ignored.contains(&e.path);
            let color = if is_ignored || state == Check::Off { pal().subtle } else { pal().text };
            let font = if e.is_dir { bold(14.0) } else { FontId::proportional(14.0) };
            let name = if e.is_dir { format!("{}/", e.name) } else { e.name.clone() };
            painter.text(pos2(x0 + 48.0, cy), Align2::LEFT_CENTER, name, font, color);
            if is_ignored {
                painter.text(rect.right_center() - vec2(8.0, 0.0), Align2::RIGHT_CENTER, "ignoré par git", FontId::proportional(12.0), pal().subtle);
            }
            if resp.clicked() {
                let x = resp.interact_pointer_pos().map(|p| p.x).unwrap_or(0.0);
                if e.is_dir && x < x0 + 20.0 {
                    out.push(Action::Expand(e.path.clone()));
                } else {
                    out.push(Action::Toggle(e.path.clone()));
                }
            }
            if resp.double_clicked() && e.is_dir {
                out.push(Action::Toggle(e.path.clone()));
                out.push(Action::Expand(e.path.clone()));
            }
            if e.is_dir && is_open {
                self.rows(ui, &e.path, depth + 1, out, budget);
            }
        }
    }
}

fn paint_check(painter: &egui::Painter, r: Rect, state: Check) {
    let round = Rounding::same(4.0);
    match state {
        Check::Off => {
            painter.rect_stroke(r, round, Stroke::new(1.5_f32, pal().subtle));
        }
        Check::On => {
            painter.rect_filled(r, round, pal().accent);
            let (l, cy) = (r.left(), r.center().y);
            painter.add(Shape::line(
                vec![pos2(l + 3.5, cy), pos2(l + 6.8, cy + 3.3), pos2(l + 12.5, cy - 3.5)],
                Stroke::new(2.0_f32, Color32::WHITE),
            ));
        }
        Check::Partial => {
            painter.rect_stroke(r, round, Stroke::new(1.5_f32, pal().accent));
            painter.rect_filled(r.shrink(4.0), Rounding::same(2.0), pal().accent);
        }
    }
}

fn short_date(iso: &str) -> String {
    // 2026-10-29T18:25:49+01:00 -> 29/10 18:25
    if iso.len() >= 16 {
        format!("{}/{} {}", &iso[8..10], &iso[5..7], &iso[11..16])
    } else {
        iso.to_string()
    }
}

/// Conversation dont le titre est celui affiché dans Claude (la plus récente s'il y en a plusieurs).
fn displayed_index(convs: &[Conversation], title: Option<&str>) -> Option<usize> {
    let title = title?;
    convs.iter().position(|c| c.title == title)
}
