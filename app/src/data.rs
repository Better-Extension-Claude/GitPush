use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub remote: String,
    pub branch: String,
    pub selection: Vec<String>,
    pub last_by: Option<String>,
    pub last_at: Option<String>,
    pub last_commit: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Position du bouton par rapport au coin haut-gauche de Claude, en pixels.
    pub offset: Option<(i32, i32)>,
    pub projects: HashMap<String, Project>,
}

fn config_path() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    Path::new(&base).join("GitPush").join("config.json")
}

impl Config {
    pub fn load() -> Config {
        fs::read_to_string(config_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = config_path();
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, text);
        }
    }
}

#[derive(Clone)]
pub struct Conversation {
    pub title: String,
    pub cwd: String,
    #[allow(dead_code)]
    pub modified: SystemTime,
}

impl Conversation {
    pub fn label(&self) -> String {
        let folder = Path::new(&self.cwd).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        format!("{}  —  {}", self.title, folder)
    }
}

/// Conversations, celle affichée dans Claude en premier : d'abord les sessions de l'app
/// desktop (triées par `lastFocusedAt`), puis les .jsonl de Claude Code en complément.
pub fn recent_conversations(count: usize) -> Vec<Conversation> {
    let mut list = desktop_sessions(count);
    for c in jsonl_conversations(count) {
        if list.len() >= count {
            break;
        }
        if !list.iter().any(|d| d.cwd.eq_ignore_ascii_case(&c.cwd)) {
            list.push(c);
        }
    }
    list
}

/// L'app desktop garde une fiche par session :
/// %LOCALAPPDATA%\Claude-Data\<profil>\claude-code-sessions\<compte>\<org>\local_<id>.json,
/// avec `lastFocusedAt` mis à jour quand la conversation est affichée.
fn desktop_sessions(count: usize) -> Vec<Conversation> {
    let mut roots = Vec::new();
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        if let Ok(profiles) = fs::read_dir(Path::new(&local).join("Claude-Data")) {
            roots.extend(profiles.flatten().map(|p| p.path().join("claude-code-sessions")));
        }
    }
    if let Ok(roaming) = std::env::var("APPDATA") {
        roots.push(Path::new(&roaming).join("Claude").join("claude-code-sessions"));
    }
    let field = |name: &str| Regex::new(&format!(r#""{name}":("(?:[^"\\]|\\.)*"|[0-9]+|true|false)"#)).unwrap();
    let (cwd_rx, title_rx, focus_rx, activity_rx, archived_rx) =
        (field("cwd"), field("title"), field("lastFocusedAt"), field("lastActivityAt"), field("isArchived"));
    let mut found: Vec<(u64, Conversation)> = Vec::new();
    for root in roots {
        for account in fs::read_dir(&root).into_iter().flatten().flatten() {
            for org in fs::read_dir(account.path()).into_iter().flatten().flatten() {
                for e in fs::read_dir(org.path()).into_iter().flatten().flatten() {
                    let path = e.path();
                    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    if !name.starts_with("local_") || !name.ends_with(".json") {
                        continue;
                    }
                    let head = chunk(&path, 0, 16 * 1024);
                    let get = |rx: &Regex| rx.captures(&head).and_then(|c| c.get(1)).map(|m| m.as_str().to_string());
                    let text = |rx: &Regex| get(rx).and_then(|raw| serde_json::from_str::<String>(&raw).ok());
                    if get(&archived_rx).as_deref() == Some("true") {
                        continue;
                    }
                    let Some(cwd) = text(&cwd_rx) else { continue };
                    if !Path::new(&cwd).is_dir() {
                        continue;
                    }
                    let focus = get(&focus_rx).or_else(|| get(&activity_rx)).and_then(|n| n.parse().ok()).unwrap_or(0);
                    let title = text(&title_rx).filter(|t| !t.is_empty()).unwrap_or_else(|| folder_name(&cwd));
                    let modified = e.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                    found.push((focus, Conversation { title, cwd, modified }));
                }
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().take(count).map(|(_, c)| c).collect()
}

fn folder_name(cwd: &str) -> String {
    Path::new(cwd).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()
}

/// Les conversations Claude Code sont des fichiers .jsonl sous ~/.claude/projects ;
/// la plus récemment écrite est celle où l'on vient de discuter.
fn jsonl_conversations(count: usize) -> Vec<Conversation> {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let root = Path::new(&home).join(".claude").join("projects");
    let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
    if let Ok(dirs) = fs::read_dir(&root) {
        for dir in dirs.flatten() {
            if let Ok(entries) = fs::read_dir(dir.path()) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.extension().map(|x| x == "jsonl").unwrap_or(false) {
                        if let Ok(m) = e.metadata().and_then(|m| m.modified()) {
                            files.push((m, p));
                        }
                    }
                }
            }
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    let cwd_rx = Regex::new(r#""cwd":"((?:[^"\\]|\\.)*)""#).unwrap();
    let title_rx = Regex::new(r#""customTitle":"((?:[^"\\]|\\.)*)""#).unwrap();
    let mut list = Vec::new();
    for (modified, path) in files.into_iter().take(count * 3) {
        let head = chunk(&path, 0, 128 * 1024);
        let tail = chunk_tail(&path, 512 * 1024);
        let cwd = last_match(&cwd_rx, &tail).or_else(|| last_match(&cwd_rx, &head));
        let Some(cwd) = cwd else { continue };
        if !Path::new(&cwd).is_dir() {
            continue;
        }
        let title = last_match(&title_rx, &tail)
            .or_else(|| last_match(&title_rx, &head))
            .unwrap_or_else(|| Path::new(&cwd).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default());
        list.push(Conversation { title, cwd, modified });
        if list.len() >= count {
            break;
        }
    }
    list
}

fn chunk(path: &Path, start: u64, size: usize) -> String {
    let Ok(mut f) = fs::File::open(path) else { return String::new() };
    let _ = f.seek(SeekFrom::Start(start));
    let mut buf = Vec::with_capacity(size);
    let _ = f.take(size as u64).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

fn chunk_tail(path: &Path, size: u64) -> String {
    let len = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    chunk(path, len.saturating_sub(size), size as usize)
}

fn last_match(rx: &Regex, text: &str) -> Option<String> {
    let raw = rx.captures_iter(text).last()?.get(1)?.as_str().to_string();
    serde_json::from_str::<String>(&format!("\"{raw}\"")).ok()
}

pub struct Run {
    pub ok: bool,
    pub out: String,
    pub err: String,
}

impl Run {
    pub fn last(&self) -> String {
        let text = if self.err.trim().is_empty() { &self.out } else { &self.err };
        let lines: Vec<&str> = text.trim().lines().collect();
        lines[lines.len().saturating_sub(3)..].join(" ")
    }
}

pub fn run(exe: &str, cwd: &str, timeout: Duration, args: &[&str]) -> Run {
    let child = Command::new(exe)
        .args(args)
        .current_dir(if cwd.is_empty() { "." } else { cwd })
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return Run { ok: false, out: String::new(), err: format!("{exe} introuvable ({e})") },
    };
    let mut so = child.stdout.take().unwrap();
    let mut se = child.stderr.take().unwrap();
    let to = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = so.read_to_end(&mut s);
        s
    });
    let te = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = se.read_to_end(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(30)),
            Err(_) => break None,
        }
    };
    let out = String::from_utf8_lossy(&to.join().unwrap_or_default()).into_owned();
    let err = String::from_utf8_lossy(&te.join().unwrap_or_default()).into_owned();
    match status {
        Some(s) => Run { ok: s.success(), out, err },
        None => Run { ok: false, out, err: format!("{exe} : délai dépassé") },
    }
}

pub fn git(cwd: &str, args: &[&str]) -> Run {
    let mut all = vec!["-c", "core.quotePath=false"];
    all.extend_from_slice(args);
    run("git", cwd, Duration::from_secs(120), &all)
}

pub fn lines(text: &str) -> Vec<String> {
    text.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

// ---- Sélection : chemins relatifs avec « / » ; un dossier coché couvre tout son contenu.

#[derive(Clone, Copy, PartialEq)]
pub enum Check {
    Off,
    On,
    Partial,
}

#[derive(Clone)]
pub struct Entry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
}

pub fn is_under(p: &str, dir: &str) -> bool {
    p.len() > dir.len() + 1 && p.starts_with(dir) && p.as_bytes()[dir.len()] == b'/'
}

pub fn parent(p: &str) -> &str {
    p.rfind('/').map(|i| &p[..i]).unwrap_or("")
}

pub fn check(sel: &[String], path: &str) -> Check {
    if sel.iter().any(|s| s == path || is_under(path, s)) {
        Check::On
    } else if sel.iter().any(|s| is_under(s, path)) {
        Check::Partial
    } else {
        Check::Off
    }
}

pub fn full(cwd: &str, rel: &str) -> PathBuf {
    if rel.is_empty() {
        PathBuf::from(cwd)
    } else {
        Path::new(cwd).join(rel.replace('/', "\\"))
    }
}

pub fn list_dir(cwd: &str, dir: &str) -> Vec<Entry> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    if let Ok(rd) = fs::read_dir(full(cwd, dir)) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name == ".git" {
                continue;
            }
            let path = if dir.is_empty() { name.clone() } else { format!("{dir}/{name}") };
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir { &mut dirs } else { &mut files }.push(Entry { path, name, is_dir });
        }
    }
    dirs.sort_by_key(|e| e.name.to_lowercase());
    files.sort_by_key(|e| e.name.to_lowercase());
    dirs.extend(files);
    dirs
}

pub fn ignored(cwd: &str) -> HashSet<String> {
    let r = git(cwd, &["ls-files", "--others", "--ignored", "--exclude-standard", "--directory"]);
    if !r.ok {
        return HashSet::new();
    }
    lines(&r.out).into_iter().map(|s| s.trim_end_matches('/').to_string()).collect()
}

pub fn toggle(cwd: &str, sel: &[String], path: &str, ign: &HashSet<String>) -> Vec<String> {
    if check(sel, path) != Check::On {
        let mut next: Vec<String> = sel.iter().filter(|s| !is_under(s, path)).cloned().collect();
        next.push(path.to_string());
        return normalize(cwd, next, ign);
    }
    let ancestor = sel.iter().find(|s| *s == path || is_under(path, s)).cloned().unwrap();
    let mut next: Vec<String> = sel.iter().filter(|s| **s != ancestor).cloned().collect();
    // Décocher un élément d'un dossier coché : on garde ses frères et sœurs.
    let mut dir = path.to_string();
    while dir != ancestor {
        let up = parent(&dir).to_string();
        next.extend(list_dir(cwd, &up).into_iter().map(|e| e.path).filter(|p| *p != dir));
        dir = up;
    }
    normalize(cwd, next, ign)
}

/// Un dossier dont tout le contenu (hors fichiers ignorés par git) est coché devient le dossier lui-même.
pub fn normalize(cwd: &str, sel: Vec<String>, ign: &HashSet<String>) -> Vec<String> {
    let mut set: Vec<String> = sel;
    let mut dirs: Vec<String> = Vec::new();
    for s in &set {
        let mut d = parent(s).to_string();
        while !d.is_empty() {
            if !dirs.contains(&d) {
                dirs.push(d.clone());
            }
            d = parent(&d).to_string();
        }
    }
    dirs.sort_by_key(|d| std::cmp::Reverse(d.matches('/').count()));
    for dir in dirs {
        let covered = |set: &Vec<String>, p: &str| set.iter().any(|s| s == p || is_under(p, s));
        if covered(&set, &dir) {
            continue;
        }
        let children: Vec<Entry> = list_dir(cwd, &dir).into_iter().filter(|c| !ign.contains(&c.path)).collect();
        if !children.is_empty() && children.iter().all(|c| covered(&set, &c.path)) {
            set.retain(|s| !is_under(s, &dir));
            set.push(dir);
        }
    }
    set.sort();
    set.dedup();
    set
}

// ---- Git

/// Config du projet ; si le dernier commit poussé n'est pas le nôtre (Claude ou un autre
/// outil a poussé), on pré-coche ce que git suit.
pub fn load_project(cfg: &mut Config, cwd: &str) -> Project {
    let mut p = cfg.projects.get(cwd).cloned().unwrap_or_default();
    if p.branch.is_empty() {
        p.branch = "main".into();
    }
    if !Path::new(cwd).join(".git").exists() {
        return p;
    }
    if p.remote.is_empty() {
        let r = git(cwd, &["remote", "get-url", "origin"]);
        if r.ok {
            p.remote = r.out.trim().into();
        }
        let b = git(cwd, &["branch", "--show-current"]);
        if b.ok && !b.out.trim().is_empty() {
            p.branch = b.out.trim().into();
        }
    }
    let head = git(cwd, &["rev-parse", "HEAD"]);
    let up = git(cwd, &["rev-parse", "@{u}"]);
    let head_id = head.out.trim().to_string();
    if head.ok && up.ok && head_id == up.out.trim() && p.last_commit.as_deref() != Some(head_id.as_str()) {
        let files = lines(&git(cwd, &["ls-files"]).out);
        let body = git(cwd, &["log", "-1", "--format=%B"]).out;
        p.selection = normalize(cwd, files, &ignored(cwd));
        p.last_by = Some(if body.to_lowercase().contains("claude") { "Claude".into() } else { "un autre outil".into() });
        p.last_at = Some(git(cwd, &["log", "-1", "--format=%cI"]).out.trim().into());
        p.last_commit = Some(head_id);
        let b = git(cwd, &["branch", "--show-current"]);
        if b.ok && !b.out.trim().is_empty() {
            p.branch = b.out.trim().into();
        }
        cfg.projects.insert(cwd.into(), p.clone());
        cfg.save();
    }
    p
}

fn ensure_identity(cwd: &str) {
    if !git(cwd, &["config", "user.email"]).out.trim().is_empty() {
        return;
    }
    let me = run("gh", cwd, Duration::from_secs(30), &["api", "user", "-q", ".id,.login"]);
    let parts = lines(&me.out);
    if me.ok && parts.len() == 2 {
        git(cwd, &["config", "user.name", &parts[1]]);
        git(cwd, &["config", "user.email", &format!("{}+{}@users.noreply.github.com", parts[0], parts[1])]);
    }
}

pub struct PushResult {
    pub ok: bool,
    pub message: String,
    pub can_force: bool,
    pub commit: Option<String>,
}

pub fn push(cwd: &str, remote: &str, branch: &str, selection: &[String], message: &str, force: bool) -> PushResult {
    let fail = |m: String, can_force: bool| PushResult { ok: false, message: m, can_force, commit: None };
    let sel: Vec<&String> = selection.iter().filter(|s| full(cwd, s).exists()).collect();
    if remote.trim().is_empty() {
        return fail("Aucun dépôt choisi.".into(), false);
    }
    if sel.is_empty() {
        return fail("Aucun fichier coché.".into(), false);
    }
    let branch = if branch.trim().is_empty() { "main" } else { branch.trim() };

    if !Path::new(cwd).join(".git").exists() {
        let r = git(cwd, &["init", "-q", "-b", branch]);
        if !r.ok {
            return fail(format!("Échec (git init) : {}", r.last()), false);
        }
    }
    let current = git(cwd, &["remote", "get-url", "origin"]);
    if !current.ok {
        git(cwd, &["remote", "add", "origin", remote]);
    } else if current.out.trim() != remote {
        git(cwd, &["remote", "set-url", "origin", remote]);
    }

    // On se place sur la branche sans toucher aux fichiers, puis on part de l'état distant :
    // le push est une simple avance et le dépôt reçoit exactement les éléments cochés.
    git(cwd, &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")]);
    let has_remote = git(cwd, &["fetch", "-q", "origin", branch]).ok;
    if has_remote {
        git(cwd, &["reset", "-q", &format!("origin/{branch}")]);
    }
    git(cwd, &["rm", "-r", "-q", "--cached", "--ignore-unmatch", "--", "."]);
    let list = std::env::temp_dir().join(format!("gitpush-{}.txt", std::process::id()));
    let _ = fs::write(&list, sel.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n") + "\n");
    let added = git(cwd, &["add", "-A", &format!("--pathspec-from-file={}", list.display())]);
    let _ = fs::remove_file(&list);
    if !added.ok {
        return fail(format!("Échec (git add) : {}", added.last()), false);
    }
    let has_changes = !git(cwd, &["diff", "--cached", "--quiet"]).ok;
    let has_head = git(cwd, &["rev-parse", "-q", "--verify", "HEAD"]).ok;
    let head = || Some(git(cwd, &["rev-parse", "HEAD"]).out.trim().to_string());
    if !has_changes && has_remote {
        return PushResult { ok: true, message: format!("Déjà à jour sur {branch}."), can_force: false, commit: head() };
    }
    if has_changes || !has_head {
        ensure_identity(cwd);
        let msg = if message.trim().is_empty() { "Mise à jour depuis GitPush".to_string() } else { message.trim().to_string() };
        let c = git(cwd, &["commit", "-q", "--allow-empty", "-m", &msg]);
        if !c.ok {
            return fail(format!("Échec (git commit) : {}", c.last()), false);
        }
    }
    let target = format!("HEAD:refs/heads/{branch}");
    let mut args = vec!["push", "-u"];
    if force {
        args.push("--force");
    }
    args.extend_from_slice(&["origin", &target]);
    let r = run("git", cwd, Duration::from_secs(300), &args);
    if !r.ok {
        return fail(format!("Échec (git push) : {}", r.last()), true);
    }
    let n = sel.len();
    PushResult {
        ok: true,
        message: format!("Poussé sur {branch} ({n} élément{}).", if n > 1 { "s" } else { "" }),
        can_force: false,
        commit: head(),
    }
}

pub fn my_repos() -> Option<Vec<String>> {
    let r = run("gh", "", Duration::from_secs(60), &["repo", "list", "--limit", "100", "--json", "url", "-q", ".[].url"]);
    r.ok.then(|| lines(&r.out))
}

pub fn remote_branches(remote: &str) -> Vec<String> {
    let r = run("git", "", Duration::from_secs(60), &["ls-remote", "--heads", remote]);
    lines(&r.out).iter().filter_map(|l| l.split("refs/heads/").nth(1).map(String::from)).collect()
}

pub fn create_repo(name: &str, public: bool) -> Result<String, String> {
    let r = run("gh", "", Duration::from_secs(60), &["repo", "create", name, if public { "--public" } else { "--private" }]);
    if r.ok {
        Ok(format!("{}.git", lines(&r.out).last().cloned().unwrap_or_default()))
    } else {
        Err(r.last())
    }
}
