//! Vista de detalle: resumen, logs en vivo, rendimiento, código (Compose/.env), capas e inspect.
use super::*;
use crate::compose;
use crate::model::*;
use ratatui::style::Color;
use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(d) = &app.detail else { return };
    let (tab, key, sub_i) = (d.tab, d.key.clone(), d.sub);
    let subs = app.detail_subtabs(d);
    let sub = subs.get(sub_i).copied().unwrap_or(SubTab::Summary);

    let (title, badge) = title_of(app, tab, &key);
    let block = panel(Line::from(vec![
        Span::styled(" ‹ ", faint()),
        Span::styled(title, bold()),
        Span::styled(" › ", faint()),
        badge,
    ]))
    .border_style(Style::new().fg(INK_FAINT));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let [tabs_row, sep, content] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(inner);

    // Pestañas
    app.hits.subtabs.clear();
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    let mut x = tabs_row.x + 1;
    for (i, s) in subs.iter().enumerate() {
        let label = format!(" {} {} ", i + 1, s.title(tab));
        let w = text_width(&label) as u16;
        app.hits.subtabs.push((Rect::new(x, tabs_row.y, w, 1), i));
        spans.push(Span::styled(
            label,
            if i == sub_i { inverse() } else { muted() },
        ));
        x += w;
    }
    f.render_widget(Paragraph::new(Line::from(spans)), tabs_row);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(sep.width as usize),
            Style::new().fg(LINE),
        ))),
        sep,
    );

    if content.height == 0 {
        return;
    }
    match sub {
        SubTab::Stats => draw_stats(f, app, content),
        SubTab::File => draw_file(f, app, content),
        SubTab::Logs => draw_logs(f, app, content),
        _ => {
            let lines = text_lines(app, tab, &key, sub);
            draw_scroll(f, app, content, lines, true);
        }
    }
}

fn title_of(app: &App, tab: Tab, key: &str) -> (String, Span<'static>) {
    match tab {
        Tab::Containers => app
            .snap
            .containers
            .iter()
            .find(|c| c.id == key)
            .map(|c| {
                let (l, col) = state_look(c, app.tick, app.anim);
                (
                    c.name.clone(),
                    Span::styled(l.clone(), state_style(&l, col)),
                )
            })
            .unwrap_or_default(),
        Tab::Images => app
            .snap
            .images
            .iter()
            .find(|i| i.id == key)
            .map(|i| (i.title(), Span::styled(human_bytes(i.size), muted())))
            .unwrap_or_default(),
        Tab::Volumes => (
            key.chars().take(40).collect(),
            Span::styled("volumen", muted()),
        ),
        Tab::Networks => (key.to_string(), Span::styled("red", muted())),
        Tab::Compose => app
            .projects
            .iter()
            .find(|p| p.key() == key)
            .map(|p| {
                (
                    p.name.clone(),
                    Span::styled(
                        format!("{} {}/{}", p.state_label(), p.running, p.total),
                        muted(),
                    ),
                )
            })
            .unwrap_or_default(),
    }
}

/// Pinta líneas con desplazamiento y barra; guarda alto y largo para el teclado.
fn draw_scroll(
    f: &mut Frame,
    app: &mut App,
    area: Rect,
    lines: Vec<Line<'static>>,
    from_top: bool,
) {
    let h = area.height as usize;
    app.view_len.set(lines.len());
    app.view_h.set(h);
    let max = lines.len().saturating_sub(h);
    let scroll = app.detail.as_ref().map(|d| d.scroll).unwrap_or(0).min(max);
    let _ = from_top;
    let text_area = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );
    f.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), text_area);
    scrollbar(f, area, max + h, scroll);
}

fn scrollbar(f: &mut Frame, area: Rect, total: usize, pos: usize) {
    if total > area.height as usize {
        let mut sb = ScrollbarState::new(total.saturating_sub(area.height as usize)).position(pos);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_symbol("┃")
                .track_symbol(Some("│"))
                .begin_symbol(None)
                .end_symbol(None)
                .style(Style::new().fg(LINE)),
            area,
            &mut sb,
        );
    }
}

fn text_lines(app: &App, tab: Tab, key: &str, sub: SubTab) -> Vec<Line<'static>> {
    let msg = |t: &str| vec![Line::from(Span::styled(t.to_string(), faint()))];
    match (tab, sub) {
        (Tab::Containers, SubTab::Summary) => app
            .snap
            .containers
            .iter()
            .find(|c| c.id == key)
            .map(|c| summary::container(app, c, true))
            .unwrap_or_default(),
        (Tab::Images, SubTab::Summary) => app
            .snap
            .images
            .iter()
            .find(|i| i.id == key)
            .map(|i| summary::image(app, i, true))
            .unwrap_or_default(),
        (Tab::Volumes, SubTab::Summary) => app
            .snap
            .volumes
            .iter()
            .find(|v| v.name == key)
            .map(|v| summary::volume(app, v, true))
            .unwrap_or_default(),
        (Tab::Networks, SubTab::Summary) => app
            .snap
            .networks
            .iter()
            .find(|n| n.name == key)
            .map(summary::network)
            .unwrap_or_default(),
        (_, SubTab::Inspect) => {
            let raw = match tab {
                Tab::Containers => app
                    .snap
                    .containers
                    .iter()
                    .find(|c| c.id == key)
                    .map(|c| &c.raw),
                Tab::Images => app.snap.images.iter().find(|i| i.id == key).map(|i| &i.raw),
                Tab::Volumes => app
                    .snap
                    .volumes
                    .iter()
                    .find(|v| v.name == key)
                    .map(|v| &v.raw),
                Tab::Networks => app
                    .snap
                    .networks
                    .iter()
                    .find(|n| n.name == key)
                    .map(|n| &n.raw),
                Tab::Compose => None,
            };
            raw.and_then(|v| serde_json::to_string_pretty(v).ok())
                .map(|t| t.lines().map(json_line).collect())
                .unwrap_or_default()
        }
        (Tab::Images, SubTab::Layers) => layers(app, key),
        (Tab::Images, SubTab::UsedBy) => app
            .snap
            .images
            .iter()
            .find(|i| i.id == key)
            .map(|i| {
                used_by(
                    app,
                    &i.used_by
                        .iter()
                        .map(|n| (n.clone(), String::new()))
                        .collect::<Vec<_>>(),
                    "Ningún contenedor usa esta imagen",
                )
            })
            .unwrap_or_default(),
        (Tab::Volumes, SubTab::UsedBy) => {
            let list: Vec<(String, String)> = app
                .snap
                .containers
                .iter()
                .flat_map(|c| {
                    c.mounts
                        .iter()
                        .filter(|m| m.kind == "volume" && m.name == key)
                        .map(move |m| {
                            (
                                c.name.clone(),
                                format!(
                                    "→ {}{}",
                                    m.dest,
                                    if m.rw { "" } else { " (solo lectura)" }
                                ),
                            )
                        })
                })
                .collect();
            used_by(app, &list, "Ningún contenedor tiene montado este volumen")
        }
        (Tab::Networks, SubTab::UsedBy) => app
            .snap
            .networks
            .iter()
            .find(|n| n.name == key)
            .map(|n| used_by(app, &n.members, "No hay contenedores conectados"))
            .unwrap_or_default(),
        (Tab::Compose, SubTab::Services) => services(app, key),
        _ => msg("—"),
    }
}

fn used_by(app: &App, list: &[(String, String)], empty: &str) -> Vec<Line<'static>> {
    if list.is_empty() {
        return vec![Line::from(Span::styled(format!("○ {empty}"), muted()))];
    }
    list.iter()
        .map(|(name, extra)| {
            let c = app.snap.containers.iter().find(|c| &c.name == name);
            let (label, color) = c
                .map(|c| state_look(c, app.tick, app.anim))
                .unwrap_or(("●".into(), OK));
            Line::from(vec![
                Span::styled(format!(" {:<14}", label), state_style(&label, color)),
                Span::styled(format!("{:<32}", name), Style::new().fg(INK)),
                Span::styled(extra.clone(), muted()),
            ])
        })
        .collect()
}

fn services(app: &App, key: &str) -> Vec<Line<'static>> {
    let Some(p) = app.projects.iter().find(|p| p.key() == key) else {
        return vec![];
    };
    let text = p
        .files
        .first()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .unwrap_or_default();
    let mut v = vec![
        section(&format!("{} servicios", p.services.len())),
        Line::raw(""),
    ];
    for s in &p.services {
        let (icon, color) = if s.running > 0 {
            ("●", OK)
        } else {
            ("○", INK_MUTED)
        };
        let line_no = compose::service_line(&text, &s.name)
            .map(|l| format!("línea {l}"))
            .unwrap_or_default();
        v.push(Line::from(vec![
            Span::styled(format!(" {icon} "), Style::new().fg(color)),
            Span::styled(format!("{:<22}", s.name), bold()),
            Span::styled(
                format!(
                    "{}/{} en marcha   ",
                    s.running,
                    s.containers.len().max(s.running)
                ),
                muted(),
            ),
            Span::styled(line_no, faint()),
        ]));
        for c in &s.containers {
            if let Some(cn) = app.snap.containers.iter().find(|x| &x.name == c) {
                let (l, col) = state_look(cn, app.tick, app.anim);
                let ports = cn
                    .ports
                    .iter()
                    .filter(|p| p.host.is_some())
                    .map(Port::show)
                    .collect::<Vec<_>>()
                    .join(" ");
                v.push(Line::from(vec![
                    Span::raw("      "),
                    Span::styled(format!("{:<14}", l), Style::new().fg(col)),
                    Span::styled(format!("{:<28}", cn.name), dim()),
                    Span::styled(ports, accent()),
                ]));
            }
        }
    }
    v
}

fn layers(app: &App, key: &str) -> Vec<Line<'static>> {
    match app.history.get(key) {
        None => vec![Line::from(Span::styled(
            format!("{} leyendo capas…", spinner(app.tick, app.anim)),
            muted(),
        ))],
        Some(Err(e)) => vec![Line::from(Span::styled(format!("✕ {e}"), inverse()))],
        Some(Ok(ls)) => {
            let max = ls.iter().map(|l| l.size).max().unwrap_or(1).max(1);
            let total: u64 = ls.iter().map(|l| l.size).sum();
            let mut v = vec![
                section(&format!("{} capas · {}", ls.len(), human_bytes(total))),
                Line::from(Span::styled(
                    "la más reciente arriba · la barra compara el tamaño de cada capa",
                    faint(),
                )),
                Line::raw(""),
            ];
            for l in ls {
                let cmd = clean_layer_cmd(&l.created_by);
                let mut sp = bar(
                    l.size as f32 / max as f32,
                    10,
                    if l.size == 0 { INK_FAINT } else { AEGEAN },
                );
                sp.insert(0, Span::raw(" "));
                sp.push(Span::styled(
                    format!(" {:>8}  ", human_bytes(l.size)),
                    dim(),
                ));
                sp.push(Span::styled(cmd, Style::new().fg(INK)));
                v.push(Line::from(sp));
            }
            v
        }
    }
}

fn clean_layer_cmd(c: &str) -> String {
    let c = c
        .trim()
        .trim_start_matches("/bin/sh -c ")
        .trim_start_matches("#(nop) ")
        .trim();
    let c = c.split_whitespace().collect::<Vec<_>>().join(" ");
    c.chars().take(240).collect()
}

/// JSON ya formateado → spans (clave, cadenas, números…).
fn json_line(line: &str) -> Line<'static> {
    let indent = line.len() - line.trim_start().len();
    let body = line.trim_start();
    let mut spans = vec![Span::raw(line[..indent].to_string())];
    if let Some(rest) = body.strip_prefix('"')
        && let Some(end) = rest.find("\": ")
    {
        spans.push(Span::styled(format!("\"{}\"", &rest[..end]), accent()));
        spans.push(Span::styled(": ", muted()));
        spans.push(json_value(&rest[end + 3..]));
        return Line::from(spans);
    }
    spans.push(json_value(body));
    Line::from(spans)
}

fn json_value(v: &str) -> Span<'static> {
    let t = v.trim_end_matches(',');
    let color: Color = if t.starts_with('"') {
        OK
    } else if matches!(t, "true" | "false" | "null") || t.parse::<f64>().is_ok() {
        GOLD
    } else {
        INK_MUTED
    };
    Span::styled(v.to_string(), Style::new().fg(color))
}

// ───────────────────────── logs ─────────────────────────

/// Nivel de un mensaje: primero claves de logs estructurados (`"level":"error"`, `level=warn`)
/// y, si no hay, las primeras palabras del mensaje. No busca «error» en cualquier parte:
/// un SQL que menciona una tabla `Errors` no es un error.
fn level_color(msg: &str) -> Style {
    let low = msg.to_lowercase();
    let word = |s: &str| -> Option<Style> {
        let w: String = s.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        match w.as_str() {
            "error" | "err" | "fatal" | "panic" | "critical" | "crit" | "severe" => Some(inverse()),
            "warn" | "warning" => Some(bold()),
            "debug" | "trace" | "dbg" => Some(faint()),
            "info" | "information" | "notice" => Some(dim()),
            _ => None,
        }
    };
    for key in [
        "\"loglevel\":\"",
        "\"level\":\"",
        "\"severity\":\"",
        "\"lvl\":\"",
        "level=\"",
        "level=",
        "lvl=",
    ] {
        if let Some(p) = low.find(key)
            && let Some(c) = word(&low[p + key.len()..])
        {
            return c;
        }
    }
    let head: String = low.chars().take(48).collect();
    for tok in head
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|t| !t.is_empty())
        .take(4)
    {
        if let Some(c) = word(tok) {
            return c;
        }
    }
    if low.starts_with("traceback") || low.contains("panic:") || low.contains("unhandled exception")
    {
        return inverse();
    }
    dim()
}

fn service_color(name: &str) -> Color {
    // Sin color: seis intensidades de gris de la escala Takumi (más una por servicio).
    const P: [Color; 6] = [
        INK,
        INK_DIM,
        INK_MUTED,
        crate::takumi::GRISES[3],
        INK_DIM,
        INK,
    ];
    P[name
        .bytes()
        .fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize))
        % P.len()]
}

fn log_line(raw: &str, filter: &str) -> Line<'static> {
    // Compose: «servicio-1  | 2026-…Z mensaje»; contenedor: «2026-…Z mensaje».
    let (prefix, rest) = match raw.split_once(" | ") {
        Some((p, r))
            if p.len() < 40 && !p.contains(' ')
                || raw
                    .split_once(" | ")
                    .is_some_and(|(p, _)| p.trim().len() < 40 && !p.trim().contains(' ')) =>
        {
            (Some(p.trim()), r)
        }
        _ => (None, raw),
    };
    let (ts, msg) = match rest.split_once(' ') {
        Some((t, m)) if t.len() >= 20 && t.as_bytes().get(10) == Some(&b'T') => {
            (Some(t[11..19].to_string()), m)
        }
        _ => (None, rest),
    };
    let mut spans = vec![];
    if let Some(p) = prefix {
        spans.push(Span::styled(
            format!("{:<14}", clip(p, 14)),
            Style::new().fg(service_color(p)),
        ));
    }
    if let Some(t) = ts {
        spans.push(Span::styled(format!("{t}  "), faint()));
    }
    let lvl = level_color(msg);
    if !filter.is_empty() {
        let low = msg.to_lowercase();
        let f = filter.to_lowercase();
        let mut at = 0;
        while let Some(p) = low[at..].find(&f) {
            let s = at + p;
            if !msg.is_char_boundary(s) || !msg.is_char_boundary(s + f.len()) {
                break;
            }
            spans.push(Span::styled(msg[at..s].to_string(), lvl));
            spans.push(Span::styled(msg[s..s + f.len()].to_string(), inverse()));
            at = s + f.len();
        }
        spans.push(Span::styled(msg[at..].to_string(), lvl));
    } else {
        spans.push(Span::styled(msg.to_string(), lvl));
    }
    Line::from(spans)
}

fn draw_logs(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(d) = &app.detail else { return };
    let filter = d.filter.value.clone();
    let filtering = d.filtering;
    let follow = d.follow;
    let [bar_row, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);

    let (n, ended) = app
        .logs
        .as_ref()
        .map(|l| (l.lines.len(), l.ended))
        .unwrap_or((0, false));
    let mut top: Vec<Span> = vec![Span::raw(" ")];
    top.push(if follow {
        Span::styled(
            format!(
                "{} siguiendo",
                if ended {
                    "■"
                } else {
                    spinner(app.tick, app.anim)
                }
            ),
            Style::new().fg(if ended { INK_MUTED } else { OK }),
        )
    } else {
        Span::styled("❚❚ en pausa (f sigue)", bold())
    });
    top.push(Span::styled(format!("   {n} líneas"), muted()));
    if ended {
        top.push(Span::styled("   · el contenedor dejó de emitir", faint()));
    }
    if filtering || !filter.is_empty() {
        top.push(Span::styled("   filtro: ", muted()));
        let w = 30;
        if filtering {
            top.extend(app.detail.as_ref().unwrap().filter.spans(w, true, ""));
        } else {
            top.push(Span::styled(filter.clone(), Style::new().fg(GOLD)));
        }
    }
    f.render_widget(Paragraph::new(Line::from(top)), bar_row);

    let lines: Vec<Line> = match &app.logs {
        None => vec![],
        Some(l) => {
            let f = filter.to_lowercase();
            l.lines
                .iter()
                .filter(|x| f.is_empty() || x.to_lowercase().contains(&f))
                .map(|x| log_line(x, &filter))
                .collect()
        }
    };
    let h = body.height as usize;
    app.view_len.set(lines.len());
    app.view_h.set(h);
    let max = lines.len().saturating_sub(h);
    let scroll = if follow {
        max
    } else {
        app.detail.as_ref().map(|d| d.scroll).unwrap_or(0).min(max)
    };
    if lines.is_empty() {
        let msg = if app.logs.as_ref().is_some_and(|l| l.ended) {
            "Sin salida de logs".to_string()
        } else {
            format!("{} esperando logs…", spinner(app.tick, app.anim))
        };
        f.render_widget(
            Paragraph::new(Span::styled(format!(" {msg}"), faint())),
            body,
        );
        return;
    }
    let text_area = Rect::new(
        body.x + 1,
        body.y,
        body.width.saturating_sub(2),
        body.height,
    );
    f.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), text_area);
    scrollbar(f, body, max + h, scroll);
}

// ───────────────────────── rendimiento ─────────────────────────

fn draw_stats(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(d) = &app.detail else { return };
    let Some(c) = app.snap.containers.iter().find(|c| c.id == d.key) else {
        return;
    };
    app.view_len.set(0);
    app.view_h.set(area.height as usize);
    if c.state != State::Running {
        let r = centered(area, area.width, 3);
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled("○ El contenedor no está en marcha", bold())),
                Line::from(Span::styled(
                    "Pulsa s para iniciarlo y ver CPU, memoria y red en vivo",
                    muted(),
                )),
            ])
            .alignment(ratatui::layout::Alignment::Center),
            r,
        );
        return;
    }
    let Some(s) = app.stats.get(&c.short) else {
        f.render_widget(
            Paragraph::new(Span::styled(
                format!(
                    " {} esperando la primera muestra de docker stats…",
                    spinner(app.tick, app.anim)
                ),
                muted(),
            )),
            area,
        );
        return;
    };
    let w = area.width.saturating_sub(4) as usize;
    let chart_h = ((area.height as usize).saturating_sub(8) / 2).clamp(3, 8);
    let cpu: Vec<f32> = s.cpu.iter().copied().collect();
    let mem: Vec<f32> = s.mem.iter().copied().collect();
    let cpu_max = cpu.iter().cloned().fold(0.0f32, f32::max).max(10.0) * 1.15;
    let l = &s.last;
    let mut lines: Vec<Line> = vec![];

    let head = |name: &str, big: String, extra: String, col: Color| {
        Line::from(vec![
            Span::styled(format!(" {name}  "), muted()),
            Span::styled(big, Style::new().fg(col).add_modifier(Modifier::BOLD)),
            Span::styled(format!("   {extra}"), muted()),
        ])
    };
    lines.push(head(
        "CPU",
        format!("{:.1}%", l.cpu),
        format!(
            "de {} núcleos · escala hasta {:.0}% · {} muestras",
            app.snap.info.cpus.max(1),
            cpu_max / 1.15,
            cpu.len()
        ),
        load_color(l.cpu),
    ));
    let mut g = vec![Span::raw(" ")];
    g.extend(bar(
        (l.cpu / (100.0 * app.snap.info.cpus.max(1) as f32)).min(1.0),
        w,
        load_color(l.cpu),
    ));
    lines.push(Line::from(g));
    for row in area_chart(&cpu, cpu_max, w, chart_h, load_color(l.cpu)) {
        let mut v = vec![Span::raw(" ")];
        v.extend(row.spans);
        lines.push(Line::from(v));
    }
    lines.push(Line::raw(""));
    lines.push(head(
        "MEM",
        human_bytes(l.mem_used),
        format!("de {} · {:.1}%", human_bytes(l.mem_limit), l.mem_pct),
        load_color(l.mem_pct),
    ));
    let mut g = vec![Span::raw(" ")];
    g.extend(bar(l.mem_pct / 100.0, w, load_color(l.mem_pct)));
    lines.push(Line::from(g));
    for row in area_chart(
        &mem,
        mem.iter().cloned().fold(0.0f32, f32::max).max(5.0) * 1.2,
        w,
        chart_h,
        AEGEAN,
    ) {
        let mut v = vec![Span::raw(" ")];
        v.extend(row.spans);
        lines.push(Line::from(v));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled(" RED ", muted()),
        Span::styled(format!("{}   ", l.net), Style::new().fg(INK)),
        Span::styled(" DISCO ", muted()),
        Span::styled(format!("{}   ", l.block), Style::new().fg(INK)),
        Span::styled(" PROCESOS ", muted()),
        Span::styled(l.pids.clone(), Style::new().fg(INK)),
    ]));
    f.render_widget(Paragraph::new(lines), area);
}

// ───────────────────────── código ─────────────────────────

fn draw_file(f: &mut Frame, app: &mut App, area: Rect) {
    let files = app.detail_files();
    let Some(path) = app.detail_file_path() else {
        f.render_widget(
            Paragraph::new(Span::styled(" No hay ficheros asociados", faint())),
            area,
        );
        return;
    };
    let [bar_row, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
    let idx = app
        .detail
        .as_ref()
        .map(|d| d.file_idx)
        .unwrap_or(0)
        .min(files.len().saturating_sub(1));

    // Cabecera: ruta, validación y navegación entre ficheros.
    let entry = app.files.get(&path);
    let mut top: Vec<Span> = vec![Span::raw(" ")];
    top.push(Span::styled(
        summary::home_short(&path),
        Style::new().fg(AEGEAN).add_modifier(Modifier::BOLD),
    ));
    if files.len() > 1 {
        top.push(Span::styled(
            format!("  ({}/{}  [ ] cambia)", idx + 1, files.len()),
            faint(),
        ));
    }
    if !compose::is_env_file(&path) {
        match entry.and_then(|e| e.check.as_ref()) {
            None => top.push(Span::styled(
                format!("   {} validando…", spinner(app.tick, app.anim)),
                muted(),
            )),
            Some(Ok(())) => top.push(Span::styled("   ✓ Compose válido", Style::new().fg(OK))),
            Some(Err(e)) => {
                // Docker antepone avisos (`level=warning`); el error real es la primera línea que no lo es.
                let msg = e
                    .lines()
                    .find(|l| !l.contains("level=warning"))
                    .or(e.lines().next())
                    .unwrap_or("");
                top.push(Span::styled(format!("   ✕ {}", clip(msg, 90)), inverse()));
            }
        }
    }
    f.render_widget(Paragraph::new(Line::from(top)), bar_row);

    let Some(entry) = entry else {
        f.render_widget(
            Paragraph::new(Span::styled(
                format!(" {} leyendo…", spinner(app.tick, app.anim)),
                muted(),
            )),
            body,
        );
        return;
    };
    let text = match &entry.text {
        Ok(t) => t,
        Err(e) => {
            f.render_widget(
                Paragraph::new(Span::styled(
                    format!(" ✕ No puedo leer el fichero: {e}"),
                    inverse(),
                )),
                body,
            );
            return;
        }
    };
    let is_env = compose::is_env_file(&path);
    let svc = app.detail_service();
    let block = if is_env {
        None
    } else {
        svc.as_deref().and_then(|s| compose::service_block(text, s))
    };
    let all: Vec<&str> = text.lines().collect();
    let digits = all.len().max(1).to_string().len();
    let lines: Vec<Line> = all
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let n = i + 1;
            let in_block = block.is_some_and(|(a, b)| n >= a && n <= b);
            let gutter = if in_block {
                Span::styled(format!("▌{:>w$} ", n, w = digits), Style::new().fg(GOLD))
            } else {
                Span::styled(format!(" {:>w$} ", n, w = digits), faint())
            };
            let mut spans = vec![gutter];
            spans.extend(if is_env {
                compose::env_line(l)
            } else {
                compose::yaml_line(l)
            });
            let line = Line::from(spans);
            if in_block {
                line.style(Style::new().bg(RAISED))
            } else {
                line
            }
        })
        .collect();

    let h = body.height as usize;
    app.view_len.set(lines.len());
    app.view_h.set(h);
    let max = lines.len().saturating_sub(h);
    let jump = app.detail.as_ref().map(|d| d.jump).unwrap_or(false);
    let scroll = match (jump, block) {
        (true, Some((a, _))) => a.saturating_sub(3).min(max),
        _ => app.detail.as_ref().map(|d| d.scroll).unwrap_or(0).min(max),
    };
    if let Some(d) = &mut app.detail {
        d.scroll = scroll; // el desplazamiento mostrado es el real, también tras el salto
    }
    if lines.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(" (fichero vacío)", faint())),
            body,
        );
        return;
    }
    f.render_widget(
        Paragraph::new(lines).scroll((scroll as u16, 0)),
        Rect::new(body.x, body.y, body.width.saturating_sub(1), body.height),
    );
    scrollbar(f, body, max + h, scroll);
}
