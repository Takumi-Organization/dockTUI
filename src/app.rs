//! Estado de la aplicación y reducción de mensajes. Sin dibujo ni E/S de terminal.
use crate::compose::{self, Project};
use crate::docker::{self, Streamer};
use crate::model::*;
use crate::widgets::TextField;
use ratatui::layout::Rect;
use ratatui::widgets::TableState;
use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant, SystemTime};

pub const MAX_LOG_LINES: usize = 5000;
pub const HISTORY: usize = 240;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Tab {
    Containers,
    Images,
    Volumes,
    Networks,
    Compose,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::Containers,
        Tab::Images,
        Tab::Volumes,
        Tab::Networks,
        Tab::Compose,
    ];
    pub fn idx(self) -> usize {
        self as usize
    }
    pub fn title(self) -> &'static str {
        match self {
            Tab::Containers => "Contenedores",
            Tab::Images => "Imágenes",
            Tab::Volumes => "Volúmenes",
            Tab::Networks => "Redes",
            Tab::Compose => "Compose",
        }
    }
    pub fn sorts(self) -> &'static [&'static str] {
        match self {
            Tab::Containers => &["estado", "nombre", "reciente", "cpu"],
            Tab::Images => &["nombre", "tamaño", "reciente", "uso"],
            Tab::Volumes => &["nombre", "tamaño", "uso"],
            Tab::Networks => &["nombre", "driver"],
            Tab::Compose => &["nombre", "estado"],
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SubTab {
    Summary,
    Logs,
    Stats,
    File,
    Inspect,
    Layers,
    UsedBy,
    Services,
}

impl SubTab {
    pub fn title(self, tab: Tab) -> &'static str {
        match self {
            SubTab::Summary => "Resumen",
            SubTab::Logs => "Logs",
            SubTab::Stats => "Rendimiento",
            SubTab::File => "Código",
            SubTab::Inspect => "Inspect",
            SubTab::Layers => "Capas",
            SubTab::UsedBy if tab == Tab::Networks => "Conectados",
            SubTab::UsedBy => "Usado por",
            SubTab::Services => "Servicios",
        }
    }
}

pub fn subtabs_for(tab: Tab, has_compose: bool) -> Vec<SubTab> {
    match tab {
        Tab::Containers => {
            let mut v = vec![SubTab::Summary, SubTab::Logs, SubTab::Stats];
            if has_compose {
                v.push(SubTab::File);
            }
            v.push(SubTab::Inspect);
            v
        }
        Tab::Images => vec![
            SubTab::Summary,
            SubTab::Layers,
            SubTab::UsedBy,
            SubTab::Inspect,
        ],
        Tab::Volumes | Tab::Networks => vec![SubTab::Summary, SubTab::UsedBy, SubTab::Inspect],
        Tab::Compose => vec![SubTab::File, SubTab::Services, SubTab::Logs],
    }
}

#[derive(Clone, Debug)]
pub enum Conn {
    Connecting,
    Up,
    Down(String),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastKind {
    Ok,
    Err,
    Info,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub born: Instant,
}

#[derive(Clone, Debug)]
pub enum JobState {
    Running,
    Ok,
    Failed(Option<i32>),
}

pub struct Job {
    pub id: u64,
    pub title: String,
    pub lines: Vec<String>,
    pub state: JobState,
    pub started: Instant,
    pub ended: Option<Instant>,
    pub progress: Option<(usize, usize)>,
    pub quiet: bool,
    pub targets: Vec<String>,
    pub streamer: Option<Streamer>,
}

/// Todo lo necesario para lanzar un comando docker como tarea.
#[derive(Clone, Debug, Default)]
pub struct JobSpec {
    pub title: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub quiet: bool,
    pub targets: Vec<String>,
    pub pending: String,
}

pub struct Pending {
    pub label: String,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum LogKey {
    Container(String),
    Project(String),
}

pub struct LogBuf {
    pub gen_id: u64,
    pub key: LogKey,
    pub lines: VecDeque<String>,
    pub ended: bool,
    /// Se mantiene solo para que `Drop` mate el proceso al cambiar de pestaña.
    pub _streamer: Option<Streamer>,
}

#[derive(Default)]
pub struct StatHist {
    pub cpu: VecDeque<f32>,
    pub mem: VecDeque<f32>,
    pub last: StatSample,
    pub seen: Option<Instant>,
}

pub struct FileEntry {
    pub text: Result<String, String>,
    pub mtime: Option<SystemTime>,
    pub polled: Instant,
    pub check: Option<Result<(), String>>,
}

pub enum Msg {
    Snapshot(Result<Snapshot, String>),
    VolSizes(HashMap<String, u64>),
    Stat(StatSample),
    StatsEnded,
    Log { gen_id: u64, line: String },
    LogEnd { gen_id: u64 },
    JobLine { id: u64, line: String },
    JobEnd { id: u64, code: Option<i32> },
    History(String, Result<Vec<Layer>, String>),
    ComposeCheck(String, Result<(), String>),
}

pub enum RefreshCmd {
    Now,
    Sizes,
}

pub struct Detail {
    pub tab: Tab,
    pub key: String,
    pub sub: usize,
    pub scroll: usize,
    pub follow: bool,
    pub filter: TextField,
    pub filtering: bool,
    pub file_idx: usize,
    /// Al abrir el código desde un contenedor, salta a su servicio hasta que se desplace.
    pub jump: bool,
}

pub struct External {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub title: String,
    pub pause_after: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputKind {
    Pull,
    NewVolume,
    NewNetwork,
    AddCompose,
    Rename(String),
    Tag(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormKind {
    NewContainer,
    Build,
}

pub struct FormField {
    pub label: &'static str,
    pub hint: &'static str,
    pub field: TextField,
}

pub enum Modal {
    Confirm {
        title: String,
        lines: Vec<String>,
        danger: bool,
        spec: JobSpec,
    },
    Input {
        title: String,
        prompt: String,
        hint: String,
        field: TextField,
        kind: InputKind,
    },
    Form {
        title: String,
        kind: FormKind,
        fields: Vec<FormField>,
        focus: usize,
        error: Option<String>,
    },
    Help,
    JobView {
        id: u64,
        scroll: usize,
    },
}

#[derive(Default)]
pub struct Hits {
    pub tabs: Vec<(Rect, Tab)>,
    pub subtabs: Vec<(Rect, usize)>,
    pub list: Rect,
    pub list_first_row: u16,
}

pub struct App {
    pub tx: Sender<Msg>,
    pub refresh_tx: Sender<RefreshCmd>,
    pub conn: Conn,
    pub snap: Snapshot,
    pub vol_sizes: HashMap<String, u64>,
    pub projects: Vec<Project>,
    pub registered: Vec<PathBuf>,
    pub cwd_file: Option<PathBuf>,

    pub tab: Tab,
    pub tables: [TableState; 5],
    pub cursor: [usize; 5],
    pub sel_key: [String; 5],
    pub filters: [TextField; 5],
    pub filtering: bool,
    pub sort: [usize; 5],
    pub marked: HashSet<String>,
    pub detail: Option<Detail>,
    pub modal: Option<Modal>,

    pub jobs: Vec<Job>,
    pub next_job: u64,
    pub pending: HashMap<String, Pending>,
    pub toasts: Vec<Toast>,
    pub flash: HashMap<String, Instant>,
    pub prev_state: HashMap<String, State>,

    pub stats: HashMap<String, StatHist>,
    pub stats_stream: Option<Streamer>,
    pub stats_retry: Option<Instant>,
    pub logs: Option<LogBuf>,
    pub next_gen: u64,
    pub history: HashMap<String, Result<Vec<Layer>, String>>,
    pub history_asked: HashSet<String>,
    pub files: HashMap<String, FileEntry>,
    pub check_asked: HashSet<(String, Option<SystemTime>)>,

    pub tick: u64,
    pub started: Instant,
    pub got_first: bool,
    pub splash: bool,
    pub anim: bool,
    pub quit: bool,
    pub external: Option<External>,
    pub hits: Hits,
    pub view_len: Cell<usize>,
    pub view_h: Cell<usize>,
    pub last_snapshot: Option<Instant>,
    pub last_click: Option<(Instant, u16, u16)>,
}

impl App {
    pub fn new(
        tx: Sender<Msg>,
        refresh_tx: Sender<RefreshCmd>,
        anim: bool,
        start: Option<PathBuf>,
    ) -> App {
        let cwd_file = start
            .and_then(|p| {
                if p.is_dir() {
                    compose::find_in_dir(&p)
                } else if p.is_file() {
                    Some(p)
                } else {
                    None
                }
            })
            .map(|p| p.canonicalize().unwrap_or(p));
        let mut app = App {
            tx,
            refresh_tx,
            conn: Conn::Connecting,
            snap: Snapshot::default(),
            vol_sizes: HashMap::new(),
            projects: vec![],
            registered: crate::config::load_registered(),
            cwd_file,
            tab: Tab::Containers,
            tables: Default::default(),
            cursor: [0; 5],
            sel_key: Default::default(),
            filters: Default::default(),
            filtering: false,
            sort: [0; 5],
            marked: HashSet::new(),
            detail: None,
            modal: None,
            jobs: vec![],
            next_job: 1,
            pending: HashMap::new(),
            toasts: vec![],
            flash: HashMap::new(),
            prev_state: HashMap::new(),
            stats: HashMap::new(),
            stats_stream: None,
            stats_retry: None,
            logs: None,
            next_gen: 1,
            history: HashMap::new(),
            history_asked: HashSet::new(),
            files: HashMap::new(),
            check_asked: HashSet::new(),
            tick: 0,
            started: Instant::now(),
            got_first: false,
            splash: true,
            anim,
            quit: false,
            external: None,
            hits: Hits::default(),
            view_len: Cell::new(0),
            view_h: Cell::new(0),
            last_snapshot: None,
            last_click: None,
        };
        app.rebuild_projects();
        app
    }

    // ───────────── listas visibles ─────────────

    pub fn len(&self, tab: Tab) -> usize {
        match tab {
            Tab::Containers => self.snap.containers.len(),
            Tab::Images => self.snap.images.len(),
            Tab::Volumes => self.snap.volumes.len(),
            Tab::Networks => self.snap.networks.len(),
            Tab::Compose => self.projects.len(),
        }
    }

    pub fn key_of(&self, tab: Tab, i: usize) -> String {
        match tab {
            Tab::Containers => self.snap.containers[i].id.clone(),
            Tab::Images => self.snap.images[i].id.clone(),
            Tab::Volumes => self.snap.volumes[i].name.clone(),
            Tab::Networks => self.snap.networks[i].name.clone(),
            Tab::Compose => self.projects[i].key(),
        }
    }

    pub fn mark_key(&self, tab: Tab, key: &str) -> String {
        format!("{}:{key}", tab.idx())
    }

    pub fn is_marked(&self, tab: Tab, key: &str) -> bool {
        self.marked.contains(&self.mark_key(tab, key))
    }

    fn haystack(&self, tab: Tab, i: usize) -> String {
        let h = match tab {
            Tab::Containers => {
                let c = &self.snap.containers[i];
                let p = c.compose.as_ref().map(|c| c.project.as_str()).unwrap_or("");
                let ports: Vec<_> = c.ports.iter().map(Port::show).collect();
                format!(
                    "{} {} {:?} {} {}",
                    c.name,
                    c.image,
                    c.state,
                    p,
                    ports.join(" ")
                )
            }
            Tab::Images => {
                let im = &self.snap.images[i];
                format!("{} {}", im.tags.join(" "), im.short)
            }
            Tab::Volumes => {
                let v = &self.snap.volumes[i];
                format!(
                    "{} {}",
                    v.name,
                    v.project().map(String::as_str).unwrap_or("")
                )
            }
            Tab::Networks => {
                let n = &self.snap.networks[i];
                format!("{} {} {}", n.name, n.driver, n.subnets.join(" "))
            }
            Tab::Compose => {
                let p = &self.projects[i];
                format!("{} {}", p.name, p.files.join(" "))
            }
        };
        h.to_lowercase()
    }

    pub fn state_rank(s: State) -> u8 {
        match s {
            State::Running => 0,
            State::Restarting => 1,
            State::Paused => 2,
            State::Created => 3,
            State::Exited => 4,
            State::Dead | State::Removing => 5,
        }
    }

    pub fn visible(&self, tab: Tab) -> Vec<usize> {
        let f = self.filters[tab.idx()].value.to_lowercase();
        let mut v: Vec<usize> = (0..self.len(tab))
            .filter(|&i| f.is_empty() || self.haystack(tab, i).contains(&f))
            .collect();
        let mode = self.sort[tab.idx()];
        match tab {
            Tab::Containers => {
                let c = &self.snap.containers;
                v.sort_by(|&a, &b| match mode {
                    0 => (Self::state_rank(c[a].state), &c[a].name)
                        .cmp(&(Self::state_rank(c[b].state), &c[b].name)),
                    1 => c[a].name.cmp(&c[b].name),
                    2 => c[b].created.cmp(&c[a].created),
                    _ => {
                        let cpu = |i: usize| {
                            self.stats
                                .get(&c[i].short)
                                .map(|s| s.last.cpu)
                                .unwrap_or(-1.0)
                        };
                        cpu(b).total_cmp(&cpu(a))
                    }
                });
            }
            Tab::Images => {
                let im = &self.snap.images;
                v.sort_by(|&a, &b| match mode {
                    0 => im[a].title().cmp(&im[b].title()),
                    1 => im[b].size.cmp(&im[a].size),
                    2 => im[b].created.cmp(&im[a].created),
                    _ => im[b]
                        .used_by
                        .len()
                        .cmp(&im[a].used_by.len())
                        .then(im[a].title().cmp(&im[b].title())),
                });
            }
            Tab::Volumes => {
                let vo = &self.snap.volumes;
                let size = |i: usize| self.vol_sizes.get(&vo[i].name).copied().unwrap_or(0);
                v.sort_by(|&a, &b| match mode {
                    0 => vo[a].name.cmp(&vo[b].name),
                    1 => size(b).cmp(&size(a)),
                    _ => vo[b]
                        .used_by
                        .len()
                        .cmp(&vo[a].used_by.len())
                        .then(vo[a].name.cmp(&vo[b].name)),
                });
            }
            Tab::Networks => {
                let n = &self.snap.networks;
                v.sort_by(|&a, &b| match mode {
                    0 => n[a].name.cmp(&n[b].name),
                    _ => n[a]
                        .driver
                        .cmp(&n[b].driver)
                        .then(n[a].name.cmp(&n[b].name)),
                });
            }
            Tab::Compose => {
                let p = &self.projects;
                v.sort_by(|&a, &b| match mode {
                    0 => p[a].name.cmp(&p[b].name),
                    _ => p[b]
                        .running
                        .cmp(&p[a].running)
                        .then(p[a].name.cmp(&p[b].name)),
                });
            }
        }
        v
    }

    /// Índice (en la lista subyacente) del elemento bajo el cursor.
    pub fn current(&self) -> Option<usize> {
        self.visible(self.tab)
            .get(self.cursor[self.tab.idx()])
            .copied()
    }

    pub fn current_container(&self) -> Option<&Container> {
        (self.tab == Tab::Containers).then(|| self.current().map(|i| &self.snap.containers[i]))?
    }
    pub fn current_image(&self) -> Option<&Image> {
        (self.tab == Tab::Images).then(|| self.current().map(|i| &self.snap.images[i]))?
    }
    pub fn current_project(&self) -> Option<&Project> {
        (self.tab == Tab::Compose).then(|| self.current().map(|i| &self.projects[i]))?
    }

    /// Elementos sobre los que actúa una acción: los marcados o, si no hay, el del cursor.
    pub fn targets(&self) -> Vec<usize> {
        let vis = self.visible(self.tab);
        let marked: Vec<usize> = vis
            .iter()
            .copied()
            .filter(|&i| self.is_marked(self.tab, &self.key_of(self.tab, i)))
            .collect();
        if !marked.is_empty() {
            return marked;
        }
        self.current().into_iter().collect()
    }

    pub fn sync_cursor(&mut self, tab: Tab) {
        let vis = self.visible(tab);
        let t = tab.idx();
        if let Some(p) = vis
            .iter()
            .position(|&i| self.key_of(tab, i) == self.sel_key[t])
        {
            self.cursor[t] = p;
        } else {
            self.cursor[t] = self.cursor[t].min(vis.len().saturating_sub(1));
            self.sel_key[t] = vis
                .get(self.cursor[t])
                .map(|&i| self.key_of(tab, i))
                .unwrap_or_default();
        }
    }

    pub fn move_cursor(&mut self, delta: i64) {
        let vis = self.visible(self.tab);
        if vis.is_empty() {
            return;
        }
        let t = self.tab.idx();
        let n = vis.len() as i64;
        self.cursor[t] = (self.cursor[t] as i64 + delta).clamp(0, n - 1) as usize;
        self.sel_key[t] = self.key_of(self.tab, vis[self.cursor[t]]);
    }

    pub fn set_cursor(&mut self, pos: usize) {
        let vis = self.visible(self.tab);
        if vis.is_empty() {
            return;
        }
        let t = self.tab.idx();
        self.cursor[t] = pos.min(vis.len() - 1);
        self.sel_key[t] = self.key_of(self.tab, vis[self.cursor[t]]);
    }

    pub fn switch_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.filtering = false;
        self.sync_cursor(tab);
    }

    // ───────────── datos ─────────────

    pub fn known_files(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self.registered.clone();
        if let Some(f) = &self.cwd_file
            && !v.contains(f)
        {
            v.push(f.clone());
        }
        v
    }

    pub fn rebuild_projects(&mut self) {
        self.projects = compose::build_projects(&self.snap.containers, &self.known_files());
    }

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toasts.push(Toast {
            text: text.into(),
            kind,
            born: Instant::now(),
        });
        if self.toasts.len() > 6 {
            self.toasts.remove(0);
        }
    }

    pub fn busy(&self) -> bool {
        !self.pending.is_empty()
            || self
                .jobs
                .iter()
                .any(|j| matches!(j.state, JobState::Running))
    }

    pub fn request_refresh(&self) {
        let _ = self.refresh_tx.send(RefreshCmd::Now);
    }

    pub fn handle(&mut self, m: Msg) {
        match m {
            Msg::Snapshot(r) => self.apply_snapshot(r),
            Msg::VolSizes(s) => self.vol_sizes = s,
            Msg::Stat(s) => {
                let h = self.stats.entry(s.short.clone()).or_default();
                h.cpu.push_back(s.cpu);
                h.mem.push_back(s.mem_pct);
                while h.cpu.len() > HISTORY {
                    h.cpu.pop_front();
                    h.mem.pop_front();
                }
                h.last = s;
                h.seen = Some(Instant::now());
            }
            Msg::StatsEnded => {
                self.stats_stream = None;
                self.stats_retry = Some(Instant::now() + Duration::from_secs(3));
            }
            Msg::Log { gen_id, line } => {
                if let Some(l) = self.logs.as_mut().filter(|l| l.gen_id == gen_id) {
                    l.lines.push_back(line);
                    while l.lines.len() > MAX_LOG_LINES {
                        l.lines.pop_front();
                    }
                }
            }
            Msg::LogEnd { gen_id } => {
                if let Some(l) = self.logs.as_mut().filter(|l| l.gen_id == gen_id) {
                    l.ended = true;
                }
            }
            Msg::JobLine { id, line } => {
                if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                    update_progress(j, &line);
                    j.lines.push(line);
                    if j.lines.len() > 4000 {
                        j.lines.drain(..1000);
                    }
                }
            }
            Msg::JobEnd { id, code } => self.finish_job(id, code),
            Msg::History(id, r) => {
                self.history.insert(id, r);
            }
            Msg::ComposeCheck(path, r) => {
                if let Some(f) = self.files.get_mut(&path) {
                    f.check = Some(r);
                }
            }
        }
    }

    fn apply_snapshot(&mut self, r: Result<Snapshot, String>) {
        self.got_first = true;
        self.last_snapshot = Some(Instant::now());
        match r {
            Ok(s) => {
                let was_down = !matches!(self.conn, Conn::Up);
                let first = self.snap.info.version.is_empty();
                for c in &s.containers {
                    if let Some(old) = self.prev_state.get(&c.id)
                        && *old != c.state
                    {
                        self.flash.insert(c.id.clone(), Instant::now());
                    }
                }
                self.prev_state = s
                    .containers
                    .iter()
                    .map(|c| (c.id.clone(), c.state))
                    .collect();
                let live: HashSet<&str> = s.containers.iter().map(|c| c.short.as_str()).collect();
                self.stats.retain(|k, _| live.contains(k.as_str()));
                self.snap = s;
                self.conn = Conn::Up;
                // Una acción pendiente que ya no tiene tarea viva se da por resuelta.
                let running: HashSet<&String> = self
                    .jobs
                    .iter()
                    .filter(|j| matches!(j.state, JobState::Running))
                    .flat_map(|j| j.targets.iter())
                    .collect();
                self.pending.retain(|k, _| running.contains(k));
                self.rebuild_projects();
                for t in Tab::ALL {
                    self.sync_cursor(t);
                }
                // Si el elemento del detalle desapareció, volver a la lista.
                if let Some(d) = &self.detail
                    && !self.exists(d.tab, &d.key)
                {
                    self.detail = None;
                    self.logs = None;
                }
                if was_down && !first {
                    let v = self.snap.info.version.clone();
                    self.toast(ToastKind::Ok, format!("✓ Conectado a Docker {v}"));
                }
            }
            Err(e) => {
                self.conn = Conn::Down(e);
            }
        }
    }

    pub fn exists(&self, tab: Tab, key: &str) -> bool {
        (0..self.len(tab)).any(|i| self.key_of(tab, i) == key)
    }

    pub fn container_stat(&self, c: &Container) -> Option<&StatHist> {
        if c.state != State::Running {
            return None;
        }
        self.stats.get(&c.short)
    }

    /// CPU y memoria agregadas de los contenedores en marcha.
    pub fn totals(&self) -> (f32, u64, usize) {
        let mut cpu = 0.0;
        let mut mem = 0;
        let mut n = 0;
        for c in self
            .snap
            .containers
            .iter()
            .filter(|c| c.state == State::Running)
        {
            n += 1;
            if let Some(s) = self.stats.get(&c.short) {
                cpu += s.last.cpu;
                mem += s.last.mem_used;
            }
        }
        (cpu, mem, n)
    }

    // ───────────── bucle ─────────────

    /// Avanza animaciones y mantiene las dependencias declarativas del detalle.
    pub fn on_tick(&mut self) {
        self.tick += 1;
        let now = Instant::now();
        self.toasts.retain(|t| {
            now.duration_since(t.born)
                < if t.kind == ToastKind::Err {
                    Duration::from_secs(9)
                } else {
                    Duration::from_secs(4)
                }
        });
        self.flash
            .retain(|_, t| now.duration_since(*t) < Duration::from_millis(1400));
        self.jobs.retain(|j| {
            !(matches!(j.state, JobState::Ok)
                && j.ended
                    .is_some_and(|e| now.duration_since(e) > Duration::from_secs(6)))
        });
        if self.splash && self.got_first && self.started.elapsed() > Duration::from_millis(1700) {
            self.splash = false;
        }
        if matches!(self.conn, Conn::Up) {
            self.ensure_stats();
        }
        self.ensure_detail_data();
    }

    fn ensure_stats(&mut self) {
        if self.stats_stream.is_some() {
            return;
        }
        if self.stats_retry.is_some_and(|t| Instant::now() < t) {
            return;
        }
        if !self
            .snap
            .containers
            .iter()
            .any(|c| c.state == State::Running)
        {
            return;
        }
        let tx = self.tx.clone();
        let tx2 = self.tx.clone();
        let r = docker::stream(
            vec!["stats".into(), "--format".into(), "{{json .}}".into()],
            None,
            move |l| {
                if l.starts_with('{')
                    && let Some(s) = parse_stat(&l)
                {
                    let _ = tx.send(Msg::Stat(s));
                }
            },
            move |_| {
                let _ = tx2.send(Msg::StatsEnded);
            },
        );
        match r {
            Ok(s) => self.stats_stream = Some(s),
            Err(_) => self.stats_retry = Some(Instant::now() + Duration::from_secs(10)),
        }
    }

    /// Logs, capas, ficheros: lo que pide la pestaña abierta del detalle.
    fn ensure_detail_data(&mut self) {
        let Some(d) = &self.detail else {
            if self.logs.is_some() {
                self.logs = None;
            }
            return;
        };
        let tabs = self.detail_subtabs(d);
        let sub = tabs.get(d.sub).copied().unwrap_or(SubTab::Summary);
        let (tab, key) = (d.tab, d.key.clone());

        // Logs
        let want = match (sub, tab) {
            (SubTab::Logs, Tab::Containers) => Some(LogKey::Container(key.clone())),
            (SubTab::Logs, Tab::Compose) => Some(LogKey::Project(key.clone())),
            _ => None,
        };
        match want {
            Some(k) => {
                if self.logs.as_ref().map(|l| &l.key) != Some(&k) {
                    self.start_logs(k);
                }
            }
            None => self.logs = None,
        }

        // Capas de una imagen
        if sub == SubTab::Layers && !self.history_asked.contains(&key) {
            self.history_asked.insert(key.clone());
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let r = docker::fetch_history(&key);
                let _ = tx.send(Msg::History(key, r));
            });
        }

        // Fichero de código
        if sub == SubTab::File
            && let Some(path) = self.detail_file_path()
        {
            self.ensure_file(&path);
        }
    }

    fn start_logs(&mut self, key: LogKey) {
        let gen_id = self.next_gen;
        self.next_gen += 1;
        let (args, cwd) = match &key {
            LogKey::Container(id) => (
                vec![
                    "logs".into(),
                    "-f".into(),
                    "--tail".into(),
                    "400".into(),
                    "--timestamps".into(),
                    id.clone(),
                ],
                None,
            ),
            LogKey::Project(pk) => {
                let Some(p) = self.projects.iter().find(|p| &p.key() == pk) else {
                    return;
                };
                let mut a = compose::compose_args(p);
                a.extend(
                    ["logs", "-f", "--tail", "300", "--no-color", "--timestamps"].map(String::from),
                );
                (
                    a,
                    (!p.working_dir.is_empty()).then(|| PathBuf::from(&p.working_dir)),
                )
            }
        };
        let tx = self.tx.clone();
        let tx2 = self.tx.clone();
        let streamer = docker::stream(
            args,
            cwd,
            move |line| {
                let _ = tx.send(Msg::Log { gen_id, line });
            },
            move |_| {
                let _ = tx2.send(Msg::LogEnd { gen_id });
            },
        );
        self.logs = Some(LogBuf {
            gen_id,
            key,
            lines: VecDeque::new(),
            ended: false,
            _streamer: streamer.ok(),
        });
        if let Some(d) = &mut self.detail {
            d.follow = true;
        }
    }

    pub fn detail_subtabs(&self, d: &Detail) -> Vec<SubTab> {
        let has_compose = d.tab == Tab::Containers
            && self
                .snap
                .containers
                .iter()
                .find(|c| c.id == d.key)
                .is_some_and(|c| c.compose.as_ref().is_some_and(|r| !r.files.is_empty()));
        subtabs_for(d.tab, has_compose)
    }

    /// Ficheros de código asociados al elemento del detalle.
    pub fn detail_files(&self) -> Vec<String> {
        let Some(d) = &self.detail else { return vec![] };
        match d.tab {
            Tab::Containers => self
                .snap
                .containers
                .iter()
                .find(|c| c.id == d.key)
                .and_then(|c| c.compose.as_ref())
                .map(|r| {
                    let p = Project {
                        name: r.project.clone(),
                        working_dir: r.working_dir.clone(),
                        files: r.files.clone(),
                        ..Default::default()
                    };
                    compose::project_files(&p)
                })
                .unwrap_or_default(),
            Tab::Compose => self
                .projects
                .iter()
                .find(|p| p.key() == d.key)
                .map(compose::project_files)
                .unwrap_or_default(),
            _ => vec![],
        }
    }

    pub fn detail_file_path(&self) -> Option<String> {
        let files = self.detail_files();
        let idx = self
            .detail
            .as_ref()?
            .file_idx
            .min(files.len().saturating_sub(1));
        files.get(idx).cloned()
    }

    /// Servicio al que debe saltar el visor de código (si el detalle es de un contenedor).
    pub fn detail_service(&self) -> Option<String> {
        let d = self.detail.as_ref()?;
        if d.tab != Tab::Containers {
            return None;
        }
        self.snap
            .containers
            .iter()
            .find(|c| c.id == d.key)?
            .compose
            .as_ref()
            .map(|r| r.service.clone())
    }

    pub fn ensure_file(&mut self, path: &str) {
        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let stale = match self.files.get(path) {
            None => true,
            Some(f) => f.polled.elapsed() > Duration::from_millis(800) && f.mtime != mtime,
        };
        if stale {
            let text = std::fs::read_to_string(path).map_err(|e| e.to_string());
            self.files.insert(
                path.to_string(),
                FileEntry {
                    text,
                    mtime,
                    polled: Instant::now(),
                    check: None,
                },
            );
        } else if let Some(f) = self.files.get_mut(path)
            && f.polled.elapsed() > Duration::from_millis(800)
        {
            f.polled = Instant::now();
        }
        // Validación con `docker compose config -q` (solo ficheros compose, una vez por versión).
        if !compose::is_env_file(path) && self.check_asked.insert((path.to_string(), mtime)) {
            let tx = self.tx.clone();
            let p = path.to_string();
            std::thread::spawn(move || {
                let dir = std::path::Path::new(&p).parent().map(PathBuf::from);
                let args: Vec<String> = ["compose", "-f", &p, "config", "-q"]
                    .map(String::from)
                    .to_vec();
                let r = docker::run_in(&args, dir.as_ref()).map(|_| ());
                let _ = tx.send(Msg::ComposeCheck(p, r));
            });
        }
    }
}

/// Estima el avance de un `pull` (capas) o `build` (`[3/8]`) a partir de la salida.
fn update_progress(j: &mut Job, line: &str) {
    let (mut done, mut total) = j.progress.unwrap_or((0, 0));
    if line.contains("Pulling fs layer") {
        total += 1;
    } else if line.contains("Pull complete") || line.contains("Already exists") {
        done += 1;
        total = total.max(done);
    } else if let Some(open) = line.find('[') {
        // «#7 [builder 3/8] RUN …»
        let tail = &line[open + 1..];
        if let Some(close) = tail.find(']') {
            let inner = &tail[..close];
            if let Some((a, b)) = inner.rsplit(' ').next().and_then(|x| x.split_once('/'))
                && let (Ok(a), Ok(b)) = (a.parse::<usize>(), b.parse::<usize>())
                && b > 0
                && a <= b
            {
                total = b;
                done = done.max(a.saturating_sub(1));
            }
        }
    }
    if total > 0 {
        j.progress = Some((done.min(total), total));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job() -> Job {
        Job {
            id: 1,
            title: "t".into(),
            lines: vec![],
            state: JobState::Running,
            started: Instant::now(),
            ended: None,
            progress: None,
            quiet: false,
            targets: vec![],
            streamer: None,
        }
    }

    #[test]
    fn progreso_de_pull_y_build() {
        let mut j = job();
        for l in [
            "a1: Pulling fs layer",
            "b2: Pulling fs layer",
            "a1: Pull complete",
        ] {
            update_progress(&mut j, l);
        }
        assert_eq!(j.progress, Some((1, 2)));
        let mut j = job();
        update_progress(&mut j, "#7 [builder 3/8] RUN make");
        assert_eq!(j.progress, Some((2, 8)));
    }
}
