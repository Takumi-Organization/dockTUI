//! Acciones sobre Docker: lanzar tareas, abrir diálogos y procesar sus resultados.
use crate::app::*;
use crate::compose;
use crate::docker;
use crate::model::*;
use crate::widgets::TextField;
use std::path::PathBuf;
use std::time::Instant;

/// Divide como un shell sencillo: espacios, comillas simples y dobles.
pub fn shell_split(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut has = false;
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                has = true;
            }
            (None, c) if c.is_whitespace() => {
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

fn val(fields: &[FormField], label: &str) -> String {
    fields
        .iter()
        .find(|f| f.label == label)
        .map(|f| f.field.value.trim().to_string())
        .unwrap_or_default()
}

/// Construye el comando a partir de un formulario; `Err` explica qué falta.
pub fn form_command(kind: &FormKind, fields: &[FormField]) -> Result<Vec<String>, String> {
    let mut a: Vec<String> = vec![];
    match kind {
        FormKind::NewContainer => {
            let image = val(fields, "Imagen");
            if image.is_empty() {
                return Err("Falta la imagen".into());
            }
            a.extend(["run", "-d"].map(String::from));
            let name = val(fields, "Nombre");
            if !name.is_empty() {
                a.extend(["--name".into(), name]);
            }
            for p in csv(&val(fields, "Puertos")) {
                a.extend(["-p".into(), p]);
            }
            for v in csv(&val(fields, "Volúmenes")) {
                a.extend(["-v".into(), v]);
            }
            for e in csv(&val(fields, "Variables")) {
                if !e.contains('=') {
                    return Err(format!("Variable sin «=»: {e}"));
                }
                a.extend(["-e".into(), e]);
            }
            let net = val(fields, "Red");
            if !net.is_empty() {
                a.extend(["--network".into(), net]);
            }
            let restart = val(fields, "Reinicio");
            if !restart.is_empty() && restart != "no" {
                if !["always", "unless-stopped", "on-failure"].contains(&restart.as_str()) {
                    return Err("Reinicio: no | always | unless-stopped | on-failure".into());
                }
                a.extend(["--restart".into(), restart]);
            }
            a.extend(shell_split(&val(fields, "Opciones extra")));
            a.push(image);
            a.extend(shell_split(&val(fields, "Comando")));
        }
        FormKind::Build => {
            let tag = val(fields, "Etiqueta");
            let ctx = val(fields, "Carpeta");
            if ctx.is_empty() {
                return Err("Falta la carpeta de contexto".into());
            }
            a.push("build".into());
            if !tag.is_empty() {
                a.extend(["-t".into(), tag]);
            }
            let df = val(fields, "Dockerfile");
            if !df.is_empty() {
                a.extend(["-f".into(), df]);
            }
            a.push(ctx);
        }
    }
    Ok(a)
}

pub fn new_container_form(image: &str) -> Modal {
    let f = |label, hint, v: &str| FormField {
        label,
        hint,
        field: TextField::new(v),
    };
    Modal::Form {
        title: "Nuevo contenedor".into(),
        kind: FormKind::NewContainer,
        fields: vec![
            f("Imagen", "nginx:alpine", image),
            f("Nombre", "mi-web (opcional)", ""),
            f("Puertos", "8080:80, 8443:443", ""),
            f("Volúmenes", "datos:/data, ./web:/usr/share/nginx/html", ""),
            f("Variables", "CLAVE=valor, OTRA=1", ""),
            f("Red", "nombre de red (opcional)", ""),
            f("Reinicio", "no | always | unless-stopped | on-failure", ""),
            f("Comando", "sobrescribe el CMD de la imagen", ""),
            f("Opciones extra", "--rm --cpus 1 -m 512m …", ""),
        ],
        focus: if image.is_empty() { 0 } else { 1 },
        error: None,
    }
}

pub fn build_form() -> Modal {
    let f = |label, hint, v: &str| FormField {
        label,
        hint,
        field: TextField::new(v),
    };
    Modal::Form {
        title: "Construir imagen".into(),
        kind: FormKind::Build,
        fields: vec![
            f("Carpeta", "contexto de construcción", "."),
            f("Etiqueta", "miapp:latest", ""),
            f("Dockerfile", "ruta (vacío = Dockerfile del contexto)", ""),
        ],
        focus: 1,
        error: None,
    }
}

impl App {
    // ───────────── tareas ─────────────

    pub fn spawn_job(&mut self, spec: JobSpec) {
        let id = self.next_job;
        self.next_job += 1;
        for t in &spec.targets {
            self.pending.insert(
                t.clone(),
                Pending {
                    label: spec.pending.clone(),
                },
            );
        }
        let tx = self.tx.clone();
        let tx2 = self.tx.clone();
        let streamer = docker::stream(
            spec.args.clone(),
            spec.cwd.clone(),
            move |line| {
                let _ = tx.send(Msg::JobLine { id, line });
            },
            move |code| {
                let _ = tx2.send(Msg::JobEnd { id, code });
            },
        );
        let mut job = Job {
            id,
            title: spec.title.clone(),
            lines: vec![format!("$ docker {}", spec.args.join(" "))],
            state: JobState::Running,
            started: Instant::now(),
            ended: None,
            progress: None,
            quiet: spec.quiet,
            targets: spec.targets.clone(),
            streamer: None,
        };
        match streamer {
            Ok(s) => job.streamer = Some(s),
            Err(e) => {
                job.state = JobState::Failed(None);
                job.ended = Some(Instant::now());
                job.lines.push(e.clone());
                for t in &spec.targets {
                    self.pending.remove(t);
                }
                self.toast(ToastKind::Err, format!("✕ {}: {e}", spec.title));
            }
        }
        self.jobs.push(job);
    }

    pub fn finish_job(&mut self, id: u64, code: Option<i32>) {
        let Some(pos) = self.jobs.iter().position(|j| j.id == id) else {
            return;
        };
        let (title, quiet, targets, last) = {
            let j = &mut self.jobs[pos];
            if matches!(j.state, JobState::Running) {
                j.state = if code == Some(0) {
                    JobState::Ok
                } else {
                    JobState::Failed(code)
                };
            }
            j.ended = Some(Instant::now());
            j.streamer = None;
            if let Some((_, t)) = j.progress
                && code == Some(0)
            {
                j.progress = Some((t, t));
            }
            let last = j
                .lines
                .iter()
                .rev()
                .find(|l| !l.trim().is_empty() && !l.starts_with("$ "))
                .cloned()
                .unwrap_or_default();
            (j.title.clone(), j.quiet, j.targets.clone(), last)
        };
        for t in &targets {
            self.pending.remove(t);
        }
        if code == Some(0) {
            self.toast(ToastKind::Ok, format!("✓ {title}"));
            if quiet {
                self.jobs.remove(pos);
            }
        } else {
            let why = if last.is_empty() {
                format!("código {code:?}")
            } else {
                last
            };
            self.toast(ToastKind::Err, format!("✕ {title}: {why}"));
        }
        self.request_refresh();
        let _ = self.refresh_tx.send(RefreshCmd::Sizes);
    }

    pub fn cancel_job(&mut self, id: u64) {
        if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id)
            && let Some(s) = &j.streamer
        {
            s.kill();
        }
    }

    pub fn dismiss_finished_jobs(&mut self) {
        self.jobs.retain(|j| matches!(j.state, JobState::Running));
    }

    // ───────────── constructores de acciones ─────────────

    fn names(&self, tab: Tab, idx: &[usize]) -> String {
        let mut n: Vec<String> = idx
            .iter()
            .map(|&i| match tab {
                Tab::Containers => self.snap.containers[i].name.clone(),
                Tab::Images => self.snap.images[i].title(),
                Tab::Volumes => self.snap.volumes[i].name.chars().take(24).collect(),
                Tab::Networks => self.snap.networks[i].name.clone(),
                Tab::Compose => self.projects[i].name.clone(),
            })
            .collect();
        if n.len() > 3 {
            let rest = n.len() - 2;
            n.truncate(2);
            n.push(format!("y {rest} más"));
        }
        n.join(", ")
    }

    /// start, stop, restart, pause, unpause, kill sobre los contenedores objetivo.
    pub fn container_verb(&mut self, verb: &str) {
        let idx = self.targets();
        if self.tab != Tab::Containers || idx.is_empty() {
            return;
        }
        let ids: Vec<String> = idx
            .iter()
            .map(|&i| self.snap.containers[i].id.clone())
            .collect();
        let (label, past) = match verb {
            "start" => ("iniciando", "Iniciado"),
            "stop" => ("deteniendo", "Detenido"),
            "restart" => ("reiniciando", "Reiniciado"),
            "pause" => ("pausando", "Pausado"),
            "unpause" => ("reanudando", "Reanudado"),
            "kill" => ("matando", "Matado"),
            _ => ("trabajando", "Hecho"),
        };
        let mut args = vec!["container".to_string(), verb.to_string()];
        args.extend(ids.iter().cloned());
        self.spawn_job(JobSpec {
            title: format!("{past} {}", self.names(Tab::Containers, &idx)),
            args,
            quiet: true,
            targets: ids,
            pending: label.into(),
            ..Default::default()
        });
        self.marked.clear();
    }

    /// Pausa o reanuda según el estado del contenedor bajo el cursor.
    pub fn toggle_pause(&mut self) {
        let Some(c) = self.current_container() else {
            return;
        };
        let v = if c.state == State::Paused {
            "unpause"
        } else {
            "pause"
        };
        self.container_verb(v);
    }

    /// Inicia si está parado y detiene si está en marcha.
    pub fn toggle_start(&mut self) {
        let Some(c) = self.current_container() else {
            return;
        };
        let v = if matches!(c.state, State::Running | State::Restarting | State::Paused) {
            "stop"
        } else {
            "start"
        };
        self.container_verb(v);
    }

    /// Diálogo de confirmación para eliminar lo seleccionado.
    pub fn ask_remove(&mut self) {
        let idx = self.targets();
        if idx.is_empty() {
            return;
        }
        let tab = self.tab;
        if tab == Tab::Compose {
            self.ask_compose("down");
            return;
        }
        let (noun, mut args, ids): (&str, Vec<String>, Vec<String>) = match tab {
            Tab::Containers => (
                "contenedor",
                vec!["container".into(), "rm".into(), "-f".into(), "-v".into()],
                idx.iter()
                    .map(|&i| self.snap.containers[i].id.clone())
                    .collect(),
            ),
            Tab::Images => (
                "imagen",
                vec!["image".into(), "rm".into(), "-f".into()],
                idx.iter()
                    .map(|&i| self.snap.images[i].id.clone())
                    .collect(),
            ),
            Tab::Volumes => (
                "volumen",
                vec!["volume".into(), "rm".into(), "-f".into()],
                idx.iter()
                    .map(|&i| self.snap.volumes[i].name.clone())
                    .collect(),
            ),
            _ => (
                "red",
                vec!["network".into(), "rm".into()],
                idx.iter()
                    .map(|&i| self.snap.networks[i].name.clone())
                    .collect(),
            ),
        };
        if tab == Tab::Networks && idx.iter().any(|&i| self.snap.networks[i].builtin()) {
            self.toast(
                ToastKind::Err,
                "✕ Las redes bridge, host y none son del sistema",
            );
            return;
        }
        args.extend(ids.iter().cloned());
        let mut lines = vec![format!(
            "Se eliminará {}: {}",
            if idx.len() == 1 {
                format!("el {noun}")
            } else {
                format!("{} {noun}s", idx.len())
            },
            self.names(tab, &idx)
        )];
        match tab {
            Tab::Containers => {
                if idx
                    .iter()
                    .any(|&i| self.snap.containers[i].state == State::Running)
                {
                    lines.push("Hay contenedores en marcha: se detendrán a la fuerza.".into());
                }
                lines.push("Sus volúmenes anónimos también se borran.".into());
            }
            Tab::Images => {
                let used: usize = idx.iter().map(|&i| self.snap.images[i].used_by.len()).sum();
                if used > 0 {
                    lines.push(format!("{used} contenedor(es) usan estas imágenes."));
                }
            }
            Tab::Volumes => {
                lines.push("Los datos del volumen se pierden. No se puede deshacer.".into());
                let used: usize = idx
                    .iter()
                    .map(|&i| self.snap.volumes[i].used_by.len())
                    .sum();
                if used > 0 {
                    lines.push(format!("▲ {used} contenedor(es) lo tienen montado."));
                }
            }
            _ => {}
        }
        self.modal = Some(Modal::Confirm {
            title: format!("Eliminar {noun}"),
            lines,
            danger: true,
            spec: JobSpec {
                title: format!("Eliminado {}", self.names(tab, &idx)),
                args,
                quiet: true,
                targets: ids,
                pending: "eliminando".into(),
                ..Default::default()
            },
        });
    }

    pub fn ask_prune(&mut self) {
        let (title, line, args): (&str, &str, Vec<&str>) = match self.tab {
            Tab::Containers => (
                "Limpiar contenedores",
                "Se eliminarán todos los contenedores detenidos.",
                vec!["container", "prune", "-f"],
            ),
            Tab::Images => (
                "Limpiar imágenes",
                "Se eliminarán las imágenes sin etiqueta ni contenedor (dangling).",
                vec!["image", "prune", "-f"],
            ),
            Tab::Volumes => (
                "Limpiar volúmenes",
                "Se eliminarán los volúmenes que ningún contenedor usa. Sus datos se pierden.",
                vec!["volume", "prune", "-f"],
            ),
            Tab::Networks => (
                "Limpiar redes",
                "Se eliminarán las redes sin contenedores.",
                vec!["network", "prune", "-f"],
            ),
            Tab::Compose => return,
        };
        self.modal = Some(Modal::Confirm {
            title: title.into(),
            lines: vec![line.into()],
            danger: true,
            spec: JobSpec {
                title: title.into(),
                args: args.into_iter().map(String::from).collect(),
                quiet: true,
                ..Default::default()
            },
        });
    }

    /// Acciones de Compose sobre el proyecto bajo el cursor (o el del detalle).
    pub fn ask_compose(&mut self, verb: &str) {
        let Some(p) = self.focus_project() else {
            self.toast(
                ToastKind::Info,
                "Este contenedor no pertenece a un proyecto Compose",
            );
            return;
        };
        let mut args = compose::compose_args(&p);
        let (title, extra, pending): (&str, &[&str], &str) = match verb {
            "up" => ("up -d", &["up", "-d"], "levantando"),
            "build" => ("up -d --build", &["up", "-d", "--build"], "construyendo"),
            "stop" => ("stop", &["stop"], "deteniendo"),
            "restart" => ("restart", &["restart"], "reiniciando"),
            "pull" => ("pull", &["pull"], "descargando"),
            _ => ("down", &["down"], "bajando"),
        };
        args.extend(extra.iter().map(|s| s.to_string()));
        let spec = JobSpec {
            title: format!("compose {title} · {}", p.name),
            args,
            cwd: (!p.working_dir.is_empty()).then(|| PathBuf::from(&p.working_dir)),
            quiet: false,
            targets: vec![p.key()],
            pending: pending.into(),
        };
        if verb == "down" {
            self.modal = Some(Modal::Confirm {
                title: "Bajar proyecto".into(),
                lines: vec![
                    format!(
                        "`compose down` para «{}»: detiene y elimina sus contenedores y redes.",
                        p.name
                    ),
                    "Los volúmenes con nombre se conservan.".into(),
                ],
                danger: true,
                spec,
            });
        } else {
            self.spawn_job(spec);
        }
    }

    /// Proyecto Compose relevante: el del cursor o el del contenedor en el detalle.
    pub fn focus_project(&self) -> Option<compose::Project> {
        match (&self.detail, self.tab) {
            (Some(d), _) if d.tab == Tab::Containers => {
                let c = self.snap.containers.iter().find(|c| c.id == d.key)?;
                let r = c.compose.as_ref()?;
                self.projects
                    .iter()
                    .find(|p| p.name == r.project && p.working_dir == r.working_dir)
                    .cloned()
            }
            (Some(d), _) if d.tab == Tab::Compose => {
                self.projects.iter().find(|p| p.key() == d.key).cloned()
            }
            (None, Tab::Compose) => self.current_project().cloned(),
            (None, Tab::Containers) => {
                let r = self.current_container()?.compose.as_ref()?;
                self.projects
                    .iter()
                    .find(|p| p.name == r.project && p.working_dir == r.working_dir)
                    .cloned()
            }
            _ => None,
        }
    }

    // ───────────── diálogos ─────────────

    pub fn open_input(&mut self, kind: InputKind) {
        let (title, prompt, hint, initial): (&str, &str, &str, String) = match &kind {
            InputKind::Pull => (
                "Descargar imagen",
                "Imagen",
                "ubuntu:24.04 · postgres · ghcr.io/org/app:1.0",
                String::new(),
            ),
            InputKind::NewVolume => ("Nuevo volumen", "Nombre", "datos-postgres", String::new()),
            InputKind::NewNetwork => ("Nueva red", "Nombre", "backend", String::new()),
            InputKind::AddCompose => (
                "Añadir Compose",
                "Ruta",
                "carpeta o fichero compose.yaml",
                String::new(),
            ),
            InputKind::Rename(old) => ("Renombrar contenedor", "Nuevo nombre", "", old.clone()),
            InputKind::Tag(_) => ("Etiquetar imagen", "Etiqueta", "miapp:1.0", String::new()),
        };
        self.modal = Some(Modal::Input {
            title: title.into(),
            prompt: prompt.into(),
            hint: hint.into(),
            field: TextField::new(&initial),
            kind,
        });
    }

    pub fn submit_input(&mut self, kind: InputKind, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        match kind {
            InputKind::Pull => self.spawn_job(JobSpec {
                title: format!("Descarga de {text}"),
                args: vec!["pull".into(), text],
                ..Default::default()
            }),
            InputKind::NewVolume => self.spawn_job(JobSpec {
                title: format!("Volumen {text} creado"),
                args: vec!["volume".into(), "create".into(), text],
                quiet: true,
                ..Default::default()
            }),
            InputKind::NewNetwork => self.spawn_job(JobSpec {
                title: format!("Red {text} creada"),
                args: vec!["network".into(), "create".into(), text],
                quiet: true,
                ..Default::default()
            }),
            InputKind::Rename(old) => self.spawn_job(JobSpec {
                title: format!("Renombrado a {text}"),
                args: vec!["rename".into(), old.clone(), text],
                quiet: true,
                targets: vec![old],
                pending: "renombrando".into(),
                ..Default::default()
            }),
            InputKind::Tag(id) => self.spawn_job(JobSpec {
                title: format!("Etiqueta {text} añadida"),
                args: vec!["tag".into(), id, text],
                quiet: true,
                ..Default::default()
            }),
            InputKind::AddCompose => self.add_compose(&text),
        }
    }

    fn add_compose(&mut self, text: &str) {
        let expanded = if let Some(rest) = text.strip_prefix("~/") {
            std::env::var_os("HOME")
                .map(|h| PathBuf::from(h).join(rest))
                .unwrap_or_else(|| PathBuf::from(text))
        } else {
            PathBuf::from(text)
        };
        let file = if expanded.is_dir() {
            compose::find_in_dir(&expanded)
        } else {
            Some(expanded.clone()).filter(|p| p.is_file())
        };
        match file.map(|f| f.canonicalize().unwrap_or(f)) {
            Some(f) => {
                if !self.registered.contains(&f) {
                    self.registered.push(f.clone());
                    if let Err(e) = crate::config::save_registered(&self.registered) {
                        self.toast(ToastKind::Err, format!("✕ No pude guardar la lista: {e}"));
                    }
                }
                self.rebuild_projects();
                let key = self
                    .projects
                    .iter()
                    .find(|p| p.files.iter().any(|x| std::path::Path::new(x) == f))
                    .map(|p| p.key());
                self.switch_tab(Tab::Compose);
                if let Some(k) = key {
                    self.sel_key[Tab::Compose.idx()] = k;
                    self.sync_cursor(Tab::Compose);
                }
                self.toast(ToastKind::Ok, format!("✓ Añadido {}", f.display()));
            }
            None => self.toast(
                ToastKind::Err,
                format!("✕ No hay un compose.yaml en «{text}»"),
            ),
        }
    }

    /// Olvida un proyecto añadido a mano (no toca el disco ni Docker).
    pub fn forget_project(&mut self) {
        let Some(p) = self.current_project().cloned() else {
            return;
        };
        let files: Vec<PathBuf> = p.files.iter().map(PathBuf::from).collect();
        let before = self.registered.len();
        self.registered.retain(|f| !files.contains(f));
        if self.registered.len() != before {
            let _ = crate::config::save_registered(&self.registered);
            self.rebuild_projects();
            self.sync_cursor(Tab::Compose);
            self.toast(ToastKind::Info, format!("Proyecto «{}» olvidado", p.name));
        } else {
            self.toast(
                ToastKind::Info,
                "Solo se pueden olvidar proyectos añadidos con A",
            );
        }
    }

    pub fn submit_form(&mut self) {
        let Some(Modal::Form {
            kind,
            fields,
            error,
            title,
            ..
        }) = &mut self.modal
        else {
            return;
        };
        match form_command(kind, fields) {
            Ok(args) => {
                let kind = kind.clone();
                let name = val(fields, "Nombre");
                let tag = val(fields, "Etiqueta");
                let image = val(fields, "Imagen");
                let _ = title;
                self.modal = None;
                let (t, quiet) = match kind {
                    FormKind::NewContainer => (
                        format!(
                            "Contenedor {} creado",
                            if name.is_empty() { image } else { name }
                        ),
                        false,
                    ),
                    FormKind::Build => (
                        format!(
                            "Construcción de {}",
                            if tag.is_empty() { "imagen".into() } else { tag }
                        ),
                        false,
                    ),
                };
                self.spawn_job(JobSpec {
                    title: t,
                    args,
                    quiet,
                    ..Default::default()
                });
            }
            Err(e) => *error = Some(e),
        }
    }

    // ───────────── externos ─────────────

    pub fn exec_shell(&mut self) {
        let Some(c) = self.current_container().cloned().or_else(|| {
            let d = self.detail.as_ref()?;
            self.snap.containers.iter().find(|c| c.id == d.key).cloned()
        }) else {
            return;
        };
        if c.state != State::Running {
            self.toast(ToastKind::Err, format!("✕ {} no está en marcha", c.name));
            return;
        }
        self.external = Some(External {
            program: "docker".into(),
            args: vec![
                "exec".into(),
                "-it".into(),
                c.id.clone(),
                "sh".into(),
                "-c".into(),
                "command -v bash >/dev/null 2>&1 && exec bash || exec sh".into(),
            ],
            cwd: None,
            title: format!(
                "Shell en {}  ·  escribe «exit» para volver a docktui",
                c.name
            ),
            pause_after: false,
        });
    }

    pub fn edit_current_file(&mut self) {
        let Some(path) = self.detail_file_path() else {
            return;
        };
        let line = self.detail_service().and_then(|svc| {
            let text = self.files.get(&path)?.text.as_ref().ok()?.clone();
            compose::service_line(&text, &svc)
        });
        self.edit_file(&path, line);
    }

    pub fn edit_file(&mut self, path: &str, line: Option<usize>) {
        let editor = ["VISUAL", "EDITOR"]
            .iter()
            .filter_map(|v| std::env::var(v).ok())
            .find(|e| !e.trim().is_empty())
            .or_else(|| {
                ["nvim", "vim", "nano", "vi"]
                    .iter()
                    .find(|b| in_path(b))
                    .map(|b| b.to_string())
            });
        let Some(editor) = editor else {
            self.toast(ToastKind::Err, "✕ No hay editor: define $EDITOR");
            return;
        };
        let mut parts = shell_split(&editor);
        let program = parts.remove(0);
        let base = std::path::Path::new(&program)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut args = parts;
        match (base.as_str(), line) {
            ("code" | "codium" | "code-oss", l) => {
                args.push("--wait".into());
                match l {
                    Some(l) => args.extend(["-g".into(), format!("{path}:{l}")]),
                    None => args.push(path.into()),
                }
            }
            ("hx" | "helix" | "zed" | "subl" | "lite-xl", Some(l)) => {
                args.push(format!("{path}:{l}"))
            }
            ("kate", Some(l)) => args.extend(["-l".into(), l.to_string(), path.into()]),
            // vim, nvim, nano, micro, emacs y la mayoría aceptan `+línea fichero`.
            (_, Some(l)) => args.extend([format!("+{l}"), path.into()]),
            (_, None) => args.push(path.into()),
        }
        self.external = Some(External {
            program,
            args,
            cwd: std::path::Path::new(path).parent().map(PathBuf::from),
            title: format!("Editando {path}"),
            pause_after: false,
        });
    }

    pub fn open_browser(&mut self) {
        let Some(c) = self.current_container().cloned().or_else(|| {
            let d = self.detail.as_ref()?;
            self.snap.containers.iter().find(|c| c.id == d.key).cloned()
        }) else {
            return;
        };
        let Some(port) = c.ports.iter().find_map(|p| p.host) else {
            self.toast(ToastKind::Info, "Este contenedor no publica puertos");
            return;
        };
        let url = format!("http://localhost:{port}");
        let r = std::process::Command::new("xdg-open")
            .arg(&url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match r {
            Ok(_) => self.toast(ToastKind::Ok, format!("✓ Abriendo {url}")),
            Err(e) => self.toast(ToastKind::Err, format!("✕ xdg-open: {e}")),
        }
    }
}

pub fn in_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divide_como_un_shell() {
        assert_eq!(
            shell_split(r#"sh -c "echo hola mundo" 'x y'"#),
            ["sh", "-c", "echo hola mundo", "x y"]
        );
        assert_eq!(shell_split("  a   b "), ["a", "b"]);
        assert_eq!(shell_split(r#""" a"#), ["", "a"]);
    }

    #[test]
    fn el_formulario_genera_docker_run() {
        let Modal::Form {
            kind, mut fields, ..
        } = new_container_form("nginx:alpine")
        else {
            panic!()
        };
        assert_eq!(
            form_command(&kind, &fields).unwrap(),
            ["run", "-d", "nginx:alpine"]
        );
        let set = |f: &mut Vec<FormField>, l: &str, v: &str| {
            f.iter_mut().find(|x| x.label == l).unwrap().field = TextField::new(v)
        };
        set(&mut fields, "Nombre", "web");
        set(&mut fields, "Puertos", "8080:80, 8443:443");
        set(&mut fields, "Variables", "A=1");
        set(&mut fields, "Reinicio", "always");
        set(&mut fields, "Comando", "nginx -g 'daemon off;'");
        let a = form_command(&kind, &fields).unwrap();
        assert_eq!(
            a,
            [
                "run",
                "-d",
                "--name",
                "web",
                "-p",
                "8080:80",
                "-p",
                "8443:443",
                "-e",
                "A=1",
                "--restart",
                "always",
                "nginx:alpine",
                "nginx",
                "-g",
                "daemon off;"
            ]
        );
        set(&mut fields, "Variables", "SINIGUAL");
        assert!(form_command(&kind, &fields).is_err());
        set(&mut fields, "Imagen", "");
        assert!(form_command(&kind, &fields).is_err());
    }
}
