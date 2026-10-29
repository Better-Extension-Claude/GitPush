use crate::data::{self, Check, Config, Conversation, Entry, Project, PushResult};
use crate::win::{self, SharedRef};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, FontFamily, FontId, Key, Layout, Margin, Rect, RichText, Rounding, Sense,
    Shape, Stroke, Ui,
};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

pub const DEFAULT_OFFSET: (i32, i32) = (300, 44);

const BG: Color32 = Color32::from_rgb(0x26, 0x26, 0x24);
const FIELD: Color32 = Color32::from_rgb(0x1f, 0x1e, 0x1d);
const HOVER: Color32 = Color32::from_rgb(0x34, 0x33, 0x30);
const BORDER: Color32 = Color32::from_rgb(0x45, 0x44, 0x3f);
const TEXT: Color32 = Color32::from_rgb(0xec, 0xeb, 0xe6);
const SUBTLE: Color32 = Color32::from_rgb(0x9a, 0x98, 0x93);
const ACCENT: Color32 = Color32::from_rgb(0xd9, 0x77, 0x57);
const ACCENT_HOVER: Color32 = Color32::from_rgb(0xe3, 0x8b, 0x6e);
const OK: Color32 = Color32::from_rgb(0x8c, 0xc0, 0x76);
const ERR: Color32 = Color32::from_rgb(0xe5, 0x6b, 0x5d);

fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

pub struct App {
    cfg: Config,
    shared: SharedRef,
    drag: Option<((i32, i32), (i32, i32))>,
    is_pressed: bool,
    modal: Option<Modal>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, cfg: Config, shared: SharedRef) -> App {
        setup_style(&cc.egui_ctx);
        App { cfg, shared, drag: None, is_pressed: false, modal: None }
    }

    fn open_modal(&mut self, ctx: &egui::Context) {
        if self.modal.is_some() {
            ctx.send_viewport_cmd_to(modal_id(), egui::ViewportCommand::Focus);
            return;
        }
        self.modal = Some(Modal::new(ctx));
    }
}

fn modal_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("gitpush-modal")
}

fn setup_style(ctx: &egui::Context) {
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

    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = BG;
    v.extreme_bg_color = FIELD;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT.linear_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.rounding = Rounding::same(7.0);
    }
    v.widgets.inactive.weak_bg_fill = HOVER;
    v.widgets.inactive.bg_fill = FIELD;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x3d, 0x3c, 0x38);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, SUBTLE);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(0x48, 0x47, 0x42);
    v.window_stroke = Stroke::new(1.0_f32, BORDER);
    v.window_rounding = Rounding::same(10.0);
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = vec2(8.0, 8.0);
        s.spacing.button_padding = vec2(12.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.5));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.5));
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(12.5));
    });
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(250));

        // Le bouton flottant.
        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
            let rect = ui.max_rect();
            let resp = ui.interact(rect, ui.id().with("gitpush-button"), Sense::hover());
            let fill = if resp.hovered() || self.drag.is_some() { ACCENT_HOVER } else { ACCENT };
            ui.painter().rect_filled(rect.shrink(1.0), Rounding::same(rect.height() / 2.0), fill);
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, "↑  GitPush", bold(13.5), Color32::WHITE);

            // Clics lus directement dans les événements : egui ignore l'appui qui arrive
            // dans la même image que l'activation de la fenêtre, d'où des clics perdus.
            let mut open = false;
            for e in ctx.input(|i| i.events.clone()) {
                let egui::Event::PointerButton { pos, button, pressed, .. } = e else { continue };
                match (button, pressed) {
                    (egui::PointerButton::Primary, true) => self.is_pressed = rect.contains(pos),
                    (egui::PointerButton::Primary, false) => {
                        open = self.is_pressed && rect.contains(pos);
                        self.is_pressed = false;
                    }
                    // Clic droit maintenu : déplacer le bouton.
                    (egui::PointerButton::Secondary, true) if rect.contains(pos) => {
                        let offset = self.shared.lock().unwrap().offset;
                        self.drag = Some((win::cursor(), offset));
                    }
                    (egui::PointerButton::Secondary, false) if self.drag.is_some() => {
                        self.drag = None;
                        self.cfg.offset = Some(self.shared.lock().unwrap().offset);
                        self.cfg.save();
                    }
                    _ => {}
                }
            }
            if let Some((start, offset)) = self.drag {
                let now = win::cursor();
                win::move_button(&self.shared, (offset.0 + now.0 - start.0, offset.1 + now.1 - start.1));
                ctx.request_repaint();
            }
            if open {
                crate::debug("bouton : ouverture");
                self.open_modal(ctx);
            }
        });

        // La fenêtre GitPush : une fenêtre Windows normale, centrée sur Claude à l'ouverture.
        let App { modal, cfg, shared, .. } = self;
        let mut is_closed = false;
        if let Some(m) = modal.as_mut() {
            let (rect, scale) = {
                let g = shared.lock().unwrap();
                (g.rect, g.scale.max(1.0))
            };
            let size = [800.0_f32, 720.0];
            let mut builder = egui::ViewportBuilder::default()
                .with_title(win::MODAL_TITLE)
                .with_inner_size(size)
                .with_min_inner_size([520.0, 420.0])
                .with_active(true);
            if rect.w > 0 {
                let x = (rect.x as f32 + (rect.w as f32 - size[0] * scale) / 2.0) / scale;
                let y = (rect.y as f32 + (rect.h as f32 - size[1] * scale) / 2.0) / scale;
                builder = builder.with_position([x.max(0.0), y.max(0.0)]);
            }
            ctx.show_viewport_immediate(modal_id(), builder, |ctx, _class| {
                is_closed = m.show(ctx, cfg, shared);
            });
        }
        if is_closed {
            *modal = None;
        }
    }
}

// ---------------------------------------------------------------- Modal

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Info,
    Ok,
    Err,
}

enum Msg {
    Loaded { cwd: String, project: Project, ignored: HashSet<String>, selection: Vec<String> },
    Repos(Option<Vec<String>>),
    Branches(String, Vec<String>),
    Pushed { cwd: String, result: PushResult, selection: Vec<String>, date: String },
    Created(Result<String, String>),
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
        };
        m.select(0);
        m
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

    fn select(&mut self, index: usize) {
        let Some(c) = self.convs.get(index) else {
            self.set_status("Aucune conversation Claude trouvée.", Kind::Err);
            return;
        };
        self.conv = index;
        self.cwd = c.cwd.clone();
        self.cache.clear();
        self.expanded.clear();
        self.sel.clear();
        self.branches.clear();
        self.is_loading = true;
        self.set_status("", Kind::Info);
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
                }
                Msg::Loaded { .. } => {}
                Msg::Repos(repos) => {
                    self.is_loading_repos = false;
                    if repos.is_none() {
                        self.set_status("gh indisponible ou non connecté (lance « gh auth login »).", Kind::Err);
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
            }
        }
    }

    /// Dessine le modal ; renvoie vrai quand il faut le fermer.
    fn show(&mut self, ctx: &egui::Context, cfg: &mut Config, shared: &SharedRef) -> bool {
        self.receive(cfg);
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape) || i.viewport().close_requested());
        if self.close_at.map(|t| Instant::now() >= t).unwrap_or(false) {
            close = true;
        }
        if self.close_at.is_some() || self.is_busy || self.is_loading {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG).inner_margin(Margin::same(20.0)))
            .show(ctx, |ui| {
                if self.contents(ui, cfg, shared) {
                    close = true;
                }
            });
        close
    }

    fn contents(&mut self, ui: &mut Ui, cfg: &mut Config, shared: &SharedRef) -> bool {
        let mut close = false;

        // En-tête
        ui.horizontal(|ui| {
            ui.label(RichText::new("↑ GitPush").font(bold(22.0)).color(ACCENT));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new("✕").size(18.0)).frame(false)).on_hover_text("Fermer (Échap)").clicked() {
                    close = true;
                }
                if ui.add(egui::Button::new(RichText::new("Quitter GitPush").small().color(SUBTLE)).frame(false)).clicked() {
                    std::process::exit(0);
                }
                if ui.add(egui::Button::new(RichText::new("Replacer le bouton").small().color(SUBTLE)).frame(false)).clicked() {
                    cfg.offset = None;
                    cfg.save();
                    shared.lock().unwrap().offset = DEFAULT_OFFSET;
                }
            });
        });
        ui.add_space(4.0);

        let field_w = ui.available_width() - 110.0;
        egui::Grid::new("form").num_columns(2).spacing([14.0, 10.0]).min_col_width(96.0).show(ui, |ui| {
            ui.label(RichText::new("Conversation").color(SUBTLE));
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
                self.select(i);
            }
            ui.end_row();

            ui.label(RichText::new("Dépôt").color(SUBTLE));
            ui.horizontal(|ui| {
                let mut remote = self.project.remote.clone();
                let edit = ui.add(egui::TextEdit::singleline(&mut remote).hint_text("https://github.com/moi/projet.git").desired_width(field_w - 250.0));
                if edit.changed() {
                    self.project.remote = remote.clone();
                }
                if edit.lost_focus() {
                    self.set_remote(&remote);
                }
                let mut chosen = None;
                let combo = egui::ComboBox::from_id_salt("repos").width(120.0).selected_text("Mes dépôts").show_ui(ui, |ui| {
                    match &self.repos {
                        None => {
                            ui.label(RichText::new("Chargement…").color(SUBTLE));
                        }
                        Some(list) if list.is_empty() => {
                            ui.label(RichText::new("Aucun dépôt").color(SUBTLE));
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
                if combo.response.clicked() && self.repos.is_none() && !self.is_loading_repos {
                    self.is_loading_repos = true;
                    self.spawn(|| Msg::Repos(data::my_repos()));
                }
                if let Some(url) = chosen {
                    self.set_remote(&url);
                }
                let label = if self.is_new_repo_open { "Annuler" } else { "＋ Nouveau" };
                if ui.button(label).clicked() {
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

            ui.label(RichText::new("Branche").color(SUBTLE));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.project.branch).hint_text("main").desired_width(field_w - 250.0));
                let mut chosen = None;
                egui::ComboBox::from_id_salt("branches").width(120.0).selected_text("Existantes").show_ui(ui, |ui| {
                    if self.branches.is_empty() {
                        ui.label(RichText::new("Aucune branche distante").color(SUBTLE));
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
            });
            ui.end_row();
        });

        ui.add_space(6.0);
        let mut actions = Vec::new();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Fichiers").font(bold(15.0)));
            ui.label(RichText::new(format!("{} coché{}", self.sel.len(), if self.sel.len() > 1 { "s" } else { "" })).color(SUBTLE));
            ui.add_space(8.0);
            let link = |ui: &mut Ui, t: &str| ui.add(egui::Button::new(RichText::new(t).small().color(ACCENT)).frame(false)).clicked();
            if link(ui, "Tout cocher") {
                self.sel = data::list_dir(&self.cwd, "").into_iter().filter(|e| !self.ignored.contains(&e.path)).map(|e| e.path).collect();
            }
            if link(ui, "Tout décocher") {
                self.sel.clear();
            }
            if link(ui, "↺ Dernier push") {
                self.sel = data::normalize(&self.cwd, self.project.selection.clone(), &self.ignored);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let info = match (&self.project.last_by, &self.project.last_at) {
                    (Some(by), Some(at)) => format!("pré-coché : dernier push de {by}, {}", short_date(at)),
                    (Some(by), None) => format!("pré-coché : dernier push de {by}"),
                    _ => "aucun push enregistré".to_string(),
                };
                ui.label(RichText::new(info).small().color(SUBTLE));
            });
        });

        let tree_h = (ui.available_height() - 96.0).max(80.0);
        egui::Frame::none().fill(FIELD).rounding(Rounding::same(10.0)).inner_margin(Margin::same(6.0)).show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(tree_h).min_scrolled_height(tree_h).auto_shrink([false, false]).show(ui, |ui| {
                if self.is_loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Lecture du projet…").color(SUBTLE));
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
                let button = egui::Button::new(RichText::new(label).font(bold(14.5)).color(Color32::WHITE)).fill(ACCENT).min_size(vec2(170.0, 34.0));
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
                Kind::Info => ("…", SUBTLE),
                Kind::Ok => ("✓", OK),
                Kind::Err => ("✗", ERR),
            };
            ui.label(RichText::new(format!("{icon} {}", self.status)).color(color));
        }
        close
    }

    fn rows(&mut self, ui: &mut Ui, dir: &str, depth: usize, out: &mut Vec<Action>, budget: &mut usize) {
        let cwd = self.cwd.clone();
        let entries = self.cache.entry(dir.to_string()).or_insert_with(|| data::list_dir(&cwd, dir)).clone();
        if entries.is_empty() && depth == 0 {
            ui.label(RichText::new("Dossier vide.").color(SUBTLE));
        }
        for e in entries {
            if *budget == 0 {
                ui.label(RichText::new("… liste tronquée").color(SUBTLE));
                return;
            }
            *budget -= 1;
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
            let painter = ui.painter();
            if resp.hovered() {
                painter.rect_filled(rect, Rounding::same(6.0), HOVER);
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
                painter.add(Shape::convex_polygon(pts, SUBTLE, Stroke::NONE));
            }
            let state = data::check(&self.sel, &e.path);
            paint_check(painter, Rect::from_min_size(pos2(x0 + 22.0, cy - 8.0), vec2(16.0, 16.0)), state);
            let is_ignored = self.ignored.contains(&e.path);
            let color = if is_ignored || state == Check::Off { SUBTLE } else { TEXT };
            let font = if e.is_dir { bold(14.0) } else { FontId::proportional(14.0) };
            let name = if e.is_dir { format!("{}/", e.name) } else { e.name.clone() };
            painter.text(pos2(x0 + 48.0, cy), Align2::LEFT_CENTER, name, font, color);
            if is_ignored {
                painter.text(rect.right_center() - vec2(8.0, 0.0), Align2::RIGHT_CENTER, "ignoré par git", FontId::proportional(12.0), SUBTLE);
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
            painter.rect_stroke(r, round, Stroke::new(1.5_f32, SUBTLE));
        }
        Check::On => {
            painter.rect_filled(r, round, ACCENT);
            let (l, cy) = (r.left(), r.center().y);
            painter.add(Shape::line(
                vec![pos2(l + 3.5, cy), pos2(l + 6.8, cy + 3.3), pos2(l + 12.5, cy - 3.5)],
                Stroke::new(2.0_f32, Color32::WHITE),
            ));
        }
        Check::Partial => {
            painter.rect_stroke(r, round, Stroke::new(1.5_f32, ACCENT));
            painter.rect_filled(r.shrink(4.0), Rounding::same(2.0), ACCENT);
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
