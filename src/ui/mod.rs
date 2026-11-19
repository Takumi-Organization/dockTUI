//! Dibujo: marco general (cabecera, cuerpo, tareas, pie) y utilidades comunes.
mod detail;
mod lists;
mod modal;
mod splash;
mod summary;
#[cfg(test)]
mod tests;

use crate::app::*;
use crate::model::State;
use crate::theme::*;
use crate::util::*;
use crate::widgets::*;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    f.render_widget(Block::new().style(base()), area);
    if area.width < 40 || area.height < 10 {
        let msg = Paragraph::new(vec![
            Line::from(Span::styled("dockTUI", bold())),
            Line::from(Span::styled("Ventana pequeña: amplía el terminal", muted())),
            Line::from(Span::styled("(mínimo 40×10)", faint())),
        ]);
        f.render_widget(msg, area);
        return;
    }
    if app.splash {
        splash::draw(f, app, area);
        return;
    }

    let jobs_h = jobs_height(app, area.height);
    let [header, body, jobs, footer] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(5),
        Constraint::Length(jobs_h),
        Constraint::Length(2),
    ])
    .areas(area);

    draw_header(f, app, header);
    match (&app.conn, app.detail.is_some()) {
        (Conn::Down(_), _) | (Conn::Connecting, _) if app.snap.info.version.is_empty() => {
            splash::draw_down(f, app, body)
        }
        (_, true) => detail::draw(f, app, body),
        _ => lists::draw(f, app, body),
    }
    if jobs_h > 0 {
        draw_jobs(f, app, jobs);
    }
    draw_footer(f, app, footer);

    if app.modal.is_some() {
        dim_background(f);
        modal::draw(f, app, area);
    }
}

/// Atenúa lo que hay detrás de un diálogo.
fn dim_background(f: &mut Frame) {
    for cell in f.buffer_mut().content.iter_mut() {
        let fg = mix(cell.fg, PAPER, 0.62);
        cell.set_fg(fg);
        let bg = mix(cell.bg, PAPER, 0.4);
        cell.set_bg(bg);
    }
}

pub fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

pub fn panel(title: Line<'static>) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(LINE))
        .title(title)
        .style(base())
}

/// Línea «etiqueta  valor» con la etiqueta en un ancho fijo.
pub fn kv(label: &str, value: Vec<Span<'static>>) -> Line<'static> {
    let mut v = vec![Span::styled(format!("{:<13}", label), muted())];
    v.extend(value);
    Line::from(v)
}

pub fn kv_text(label: &str, value: impl Into<String>) -> Line<'static> {
    kv(
        label,
        vec![Span::styled(value.into(), Style::new().fg(INK))],
    )
}

pub fn section(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        title.to_uppercase(),
        Style::new().fg(GOLD).add_modifier(Modifier::BOLD),
    ))
}

/// Icono + texto + color de un estado de contenedor (nunca solo color).
pub fn state_look(
    c: &crate::model::Container,
    tick: u64,
    anim: bool,
) -> (String, ratatui::style::Color) {
    match c.state {
        State::Running => match c.health.as_str() {
            "unhealthy" => ("▲ no sano".into(), ERROR),
            "starting" => (format!("{} arrancando", spinner(tick, anim)), WARN),
            "healthy" => ("● sano".into(), OK),
            _ => ("● activo".into(), OK),
        },
        State::Paused => ("❚❚ pausado".into(), WARN),
        State::Restarting => (format!("{} reiniciando", spinner(tick, anim)), WARN),
        State::Created => ("◌ creado".into(), INK_MUTED),
        State::Exited if c.exit_code != 0 => (format!("✕ salió ({})", c.exit_code), ERROR),
        State::Exited => ("○ detenido".into(), INK_MUTED),
        State::Dead => ("✕ muerto".into(), ERROR),
        State::Removing => (format!("{} eliminando", spinner(tick, anim)), WARN),
    }
}

// ───────────────────────── cabecera ─────────────────────────

fn draw_header(f: &mut Frame, app: &mut App, area: Rect) {
    let [row0, row1] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);
    let mut spans: Vec<Span> = vec![
        Span::styled(" ● ", Style::new().fg(BRAND)),
        Span::styled("dockTUI ", bold()),
    ];
    let busy = app.busy();
    if busy {
        spans.push(Span::styled(
            format!("{} ", spinner(app.tick, app.anim)),
            Style::new().fg(GOLD),
        ));
    } else {
        spans.push(Span::raw("  "));
    }
    let mut x = row0.x
        + spans
            .iter()
            .map(|s| text_width(&s.content) as u16)
            .sum::<u16>();
    app.hits.tabs.clear();
    for (i, t) in Tab::ALL.iter().enumerate() {
        let count = app.len(*t);
        let label = format!(" {} {} {} ", i + 1, t.title(), count);
        let w = text_width(&label) as u16;
        let active = *t == app.tab;
        let style = if active {
            inverse()
        } else {
            Style::new().fg(INK_MUTED)
        };
        app.hits.tabs.push((Rect::new(x, row0.y, w, 1), *t));
        // El contador va más tenue que el nombre.
        let head = format!(" {} {} ", i + 1, t.title());
        spans.push(Span::styled(head, style));
        spans.push(Span::styled(
            format!("{count} "),
            if active { inverse() } else { faint() },
        ));
        x += w;
    }

    // Derecha: estado de la conexión y totales; se elige la variante más completa que quepa.
    let (cpu, mem, running) = app.totals();
    let lw: usize = spans.iter().map(|s| text_width(&s.content)).sum();
    let room = (row0.width as usize).saturating_sub(lw + 2);
    let variants: Vec<Vec<Span>> = match &app.conn {
        Conn::Up => {
            let docker = Span::styled(
                format!("● Docker {} ", app.snap.info.version),
                Style::new().fg(OK),
            );
            let ctx = Span::styled(format!("[{}] ", app.snap.info.context), faint());
            let load = [
                Span::styled(format!("CPU {cpu:.1}% "), Style::new().fg(load_color(cpu))),
                Span::styled(format!("RAM {} ", human_bytes(mem)), muted()),
                Span::styled(format!("▶ {running} "), muted()),
            ];
            let mut full: Vec<Span> = load.to_vec();
            full.push(docker.clone());
            if !app.snap.info.context.is_empty() {
                full.push(ctx.clone());
            }
            let mut mid = vec![docker.clone()];
            if !app.snap.info.context.is_empty() {
                mid.push(ctx);
            }
            vec![
                full,
                mid,
                vec![docker],
                vec![Span::styled("● ", Style::new().fg(OK))],
            ]
        }
        Conn::Down(_) => vec![
            vec![Span::styled("✕ sin conexión ", inverse())],
            vec![Span::styled("✕ ", inverse())],
        ],
        Conn::Connecting => vec![vec![Span::styled(
            format!("{} conectando ", spinner(app.tick, app.anim)),
            muted(),
        )]],
    };
    let right = variants
        .into_iter()
        .find(|v| v.iter().map(|s| text_width(&s.content)).sum::<usize>() <= room)
        .unwrap_or_default();
    let rw: usize = right.iter().map(|s| text_width(&s.content)).sum();
    let gap = (row0.width as usize).saturating_sub(lw + rw);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.extend(right);
    f.render_widget(Paragraph::new(Line::from(spans)), row0);

    let amp = if busy { 0.85 } else { 0.38 };
    f.render_widget(
        Paragraph::new(wave_line(row1.width as usize, app.tick, amp, app.anim)),
        row1,
    );
}

// ───────────────────────── tareas ─────────────────────────

fn visible_jobs(app: &App) -> Vec<&Job> {
    let mut v: Vec<&Job> = app.jobs.iter().filter(|j| !j.quiet).collect();
    let skip = v.len().saturating_sub(4);
    v.drain(..skip);
    v
}

fn jobs_height(app: &App, total: u16) -> u16 {
    let n = visible_jobs(app).len() as u16;
    if n == 0 || total < 24 { 0 } else { n * 2 + 2 }
}

fn draw_jobs(f: &mut Frame, app: &App, area: Rect) {
    let jobs = visible_jobs(app);
    let running = jobs
        .iter()
        .filter(|j| matches!(j.state, JobState::Running))
        .count();
    let title = Line::from(vec![
        Span::styled(" Tareas ", bold()),
        Span::styled(
            format!("{running} en curso · t abre la salida · T limpia "),
            faint(),
        ),
    ]);
    let block = panel(title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let w = inner.width as usize;
    let mut lines: Vec<Line> = vec![];
    for j in jobs {
        let (icon, color) = match j.state {
            JobState::Running => (spinner(app.tick, app.anim).to_string(), GOLD),
            JobState::Ok => ("✓".into(), OK),
            JobState::Failed(_) => ("✕".into(), ERROR),
        };
        let secs = j
            .ended
            .unwrap_or_else(std::time::Instant::now)
            .duration_since(j.started)
            .as_secs();
        let tail = format!(" {secs}s");
        let bar_w = (w / 3).clamp(10, 36);
        let mut l1 = vec![
            Span::styled(format!("{icon} "), Style::new().fg(color)),
            Span::styled(
                pad(&j.title, w.saturating_sub(bar_w + tail.len() + 5)),
                bold(),
            ),
            Span::raw(" "),
        ];
        let bar_spans = match (&j.state, j.progress) {
            (JobState::Running, Some((d, t))) => {
                shimmer_bar(d as f32 / t.max(1) as f32, bar_w, app.tick, color, app.anim)
            }
            (JobState::Running, None) => indeterminate_bar(bar_w, app.tick, color, app.anim),
            (JobState::Ok, _) => bar(1.0, bar_w, OK),
            (JobState::Failed(_), p) => bar(
                p.map(|(d, t)| d as f32 / t.max(1) as f32).unwrap_or(0.0),
                bar_w,
                ERROR,
            ),
        };
        l1.extend(bar_spans);
        l1.push(Span::styled(tail, muted()));
        lines.push(Line::from(l1));
        let last = j
            .lines
            .iter()
            .rev()
            .find(|l| !l.trim().is_empty())
            .cloned()
            .unwrap_or_default();
        let prog = j
            .progress
            .map(|(d, t)| format!("{d}/{t} · "))
            .unwrap_or_default();
        lines.push(Line::from(Span::styled(
            format!(
                "  {}",
                truncate(&format!("{prog}{last}"), w.saturating_sub(2))
            ),
            faint(),
        )));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

// ───────────────────────── pie ─────────────────────────

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let [status, hints] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);
    let w = area.width as usize;

    // Línea de estado: aviso más reciente o contexto de la lista.
    let line = if let Some(t) = app.toasts.last() {
        // Éxito: negrita; error: inversión (con su texto); información: gris suave.
        let tstyle = match t.kind {
            ToastKind::Ok => bold(),
            ToastKind::Err => inverse(),
            ToastKind::Info => dim(),
        };
        // Entrada deslizante: el texto «escribe» sus primeros 250 ms.
        let age = t.born.elapsed().as_millis() as usize;
        let shown: String = if app.anim && age < 250 {
            t.text
                .chars()
                .take(t.text.chars().count() * age / 250 + 1)
                .collect()
        } else {
            t.text.clone()
        };
        Line::from(vec![
            Span::raw(" "),
            Span::styled(truncate(&shown, w.saturating_sub(2)), tstyle),
        ])
    } else {
        let tab = app.tab;
        let mut s: Vec<Span> = vec![Span::raw(" ")];
        let filter = &app.filters[tab.idx()];
        if app.filtering {
            s.push(Span::styled("/ ", Style::new().fg(GOLD)));
            s.extend(filter.spans(w.saturating_sub(20).max(8), true, ""));
            s.push(Span::styled("   ⏎ aceptar · Esc borrar", faint()));
        } else if !filter.value.is_empty() {
            s.push(Span::styled(
                format!("Filtro «{}» · Esc lo quita", filter.value),
                Style::new().fg(GOLD),
            ));
        } else if !app.marked.is_empty() {
            s.push(Span::styled(
                format!(
                    "{} marcados · las acciones se aplican a todos · Esc desmarca",
                    app.marked.len()
                ),
                Style::new().fg(GOLD),
            ));
        } else if app.detail.is_none() {
            s.push(Span::styled(
                format!("Orden: {}", tab.sorts()[app.sort[tab.idx()]]),
                faint(),
            ));
        }
        Line::from(s)
    };
    f.render_widget(Paragraph::new(line), status);

    // Línea de atajos según el contexto.
    let keys = hint_set(app);
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    let mut used = 1;
    let tail = hint("?", "ayuda");
    let tail_w: usize = tail.iter().map(|s| text_width(&s.content)).sum();
    for (k, l) in keys {
        let h = hint(k, l);
        let hw: usize = h.iter().map(|s| text_width(&s.content)).sum();
        if used + hw + tail_w > w {
            break;
        }
        used += hw;
        spans.extend(h);
    }
    spans.extend(tail);
    f.render_widget(Paragraph::new(Line::from(spans)), hints);
}

fn hint_set(app: &App) -> Vec<(&'static str, &'static str)> {
    if !matches!(app.conn, Conn::Up) && app.snap.info.version.is_empty() {
        return vec![("F5", "reintentar"), ("q", "salir")];
    }
    if let Some(d) = &app.detail {
        let subs = app.detail_subtabs(d);
        let sub = subs.get(d.sub).copied().unwrap_or(SubTab::Summary);
        let mut v = vec![("Esc", "volver"), ("←→", "pestañas"), ("↑↓", "desplazar")];
        match sub {
            SubTab::Logs => v.extend([("f", "seguir"), ("/", "filtrar"), ("c", "limpiar")]),
            SubTab::File => v.extend([("e", "editar"), ("[ ]", "otro fichero")]),
            _ => {}
        }
        match d.tab {
            Tab::Containers => {
                v.extend([("s", "iniciar/parar"), ("r", "reiniciar")]);
                if sub != SubTab::File {
                    v.push(("e", "shell"));
                }
                v.push(("d", "eliminar"));
            }
            Tab::Compose => v.extend([("u", "up"), ("d", "down"), ("r", "restart")]),
            _ => v.push(("d", "eliminar")),
        }
        return v;
    }
    let mut v = vec![("⏎", "detalles")];
    match app.tab {
        Tab::Containers => v.extend([
            ("s", "iniciar/parar"),
            ("r", "reiniciar"),
            ("e", "shell"),
            ("l", "logs"),
            ("c", "código"),
            ("d", "eliminar"),
            ("n", "nuevo"),
            ("b", "navegador"),
        ]),
        Tab::Images => v.extend([
            ("p", "descargar"),
            ("n", "ejecutar"),
            ("b", "construir"),
            ("t", "etiquetar"),
            ("d", "eliminar"),
            ("P", "limpiar"),
        ]),
        Tab::Volumes => v.extend([("n", "nuevo"), ("d", "eliminar"), ("P", "limpiar")]),
        Tab::Networks => v.extend([("n", "nueva"), ("d", "eliminar"), ("P", "limpiar")]),
        Tab::Compose => v.extend([
            ("u", "up"),
            ("x", "stop"),
            ("d", "down"),
            ("c", "código"),
            ("e", "editar"),
            ("A", "añadir"),
        ]),
    }
    v.extend([
        ("/", "filtrar"),
        ("␣", "marcar"),
        ("o", "orden"),
        ("q", "salir"),
    ]);
    v
}

/// Recorte seguro para mostrar texto en `w` columnas.
pub fn clip(s: &str, w: u16) -> String {
    truncate(s, w as usize)
}

pub fn clear(f: &mut Frame, r: Rect) {
    f.render_widget(Clear, r);
    f.render_widget(Block::new().style(base()), r);
}
