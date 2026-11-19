//! Diálogos: confirmación, entrada de texto, formularios, ayuda y salida de tareas.
use super::*;
use crate::actions::form_command;

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let tick = app.tick;
    let anim = app.anim;
    let Some(m) = &app.modal else { return };
    match m {
        Modal::Confirm {
            title,
            lines,
            danger,
            spec,
        } => {
            let color = if *danger { ERROR } else { INK };
            let w = 64.min(area.width.saturating_sub(4));
            let mut body: Vec<Line> = vec![Line::raw("")];
            for l in lines {
                for chunk in wrap(l, w.saturating_sub(4) as usize) {
                    body.push(Line::from(Span::styled(
                        format!("  {chunk}"),
                        Style::new().fg(INK),
                    )));
                }
            }
            body.push(Line::raw(""));
            body.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    " y ",
                    Style::new()
                        .fg(PAPER)
                        .bg(color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    if *danger {
                        " Sí, continuar   "
                    } else {
                        " Aceptar   "
                    },
                    Style::new().fg(color),
                ),
                Span::styled(" n ", key_cap()),
                Span::styled(" Cancelar", muted()),
            ]));
            body.push(Line::from(Span::styled(
                format!(
                    "  $ docker {}",
                    clip(&spec.args.join(" "), w.saturating_sub(12))
                ),
                faint(),
            )));
            let h = body.len() as u16 + 2;
            let r = centered(area, w, h);
            clear(f, r);
            let block = panel(Line::from(Span::styled(
                format!(
                    " {} ",
                    if *danger {
                        format!("▲ {title}")
                    } else {
                        title.clone()
                    }
                ),
                Style::new().fg(color).add_modifier(Modifier::BOLD),
            )))
            .border_style(Style::new().fg(color));
            let inner = block.inner(r);
            f.render_widget(block, r);
            f.render_widget(Paragraph::new(body), inner);
        }
        Modal::Input {
            title,
            prompt,
            hint,
            field,
            ..
        } => {
            let w = 66.min(area.width.saturating_sub(4));
            let r = centered(area, w, 7);
            clear(f, r);
            let block = panel(Line::from(Span::styled(format!(" {title} "), bold())))
                .border_style(Style::new().fg(INK_DIM));
            let inner = block.inner(r);
            f.render_widget(block, r);
            let fw = inner
                .width
                .saturating_sub(prompt.chars().count() as u16 + 5) as usize;
            let mut l = vec![
                Span::raw(" "),
                Span::styled(format!("{prompt} "), muted()),
                Span::styled("▏", Style::new().fg(GOLD)),
            ];
            let mut fs = field.spans(fw, true, "");
            tint_field(&mut fs);
            l.extend(fs);
            let lines = vec![
                Line::raw(""),
                Line::from(l),
                Line::from(Span::styled(format!("  {hint}"), faint())),
                Line::raw(""),
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled(" ⏎ ", key_cap()),
                    Span::styled(" aceptar   ", muted()),
                    Span::styled(" Esc ", key_cap()),
                    Span::styled(" cancelar", muted()),
                ]),
            ];
            f.render_widget(Paragraph::new(lines), inner);
        }
        Modal::Form {
            title,
            kind,
            fields,
            focus,
            error,
        } => {
            let w = 84.min(area.width.saturating_sub(4));
            let h = (fields.len() as u16 + 9).min(area.height);
            let r = centered(area, w, h);
            clear(f, r);
            let block = panel(Line::from(Span::styled(format!(" {title} "), bold())))
                .border_style(Style::new().fg(INK_DIM));
            let inner = block.inner(r);
            f.render_widget(block, r);
            let label_w = fields
                .iter()
                .map(|x| x.label.chars().count())
                .max()
                .unwrap_or(8)
                + 2;
            let fw = (inner.width as usize).saturating_sub(label_w + 4);
            let mut lines: Vec<Line> = vec![Line::raw("")];
            for (i, fld) in fields.iter().enumerate() {
                let focused = i == *focus;
                let mut l = vec![
                    Span::styled(if focused { "▌" } else { " " }, Style::new().fg(GOLD)),
                    Span::styled(
                        format!("{:<w$}", fld.label, w = label_w),
                        if focused { bold() } else { muted() },
                    ),
                ];
                if focused {
                    let mut fs = fld.field.spans(fw, true, "");
                    tint_field(&mut fs);
                    let used: usize = fs.iter().map(|s| text_width(&s.content)).sum();
                    l.extend(fs);
                    l.push(Span::styled(
                        " ".repeat(fw.saturating_sub(used)),
                        Style::new().bg(RAISED),
                    ));
                } else if fld.field.value.is_empty() {
                    l.push(Span::styled(pad(fld.hint, fw), faint()));
                } else {
                    l.push(Span::styled(
                        pad(&fld.field.value, fw),
                        Style::new().fg(INK),
                    ));
                }
                lines.push(Line::from(l));
            }
            lines.push(Line::raw(""));
            // Vista previa del comando que se ejecutará.
            let preview = match form_command(kind, fields) {
                Ok(a) => Line::from(vec![
                    Span::styled(" $ docker ", Style::new().fg(GOLD)),
                    Span::styled(clip(&a.join(" "), inner.width.saturating_sub(12)), dim()),
                ]),
                Err(e) => Line::from(Span::styled(format!(" ▲ {e}"), Style::new().fg(WARN))),
            };
            lines.push(preview);
            if let Some(e) = error {
                lines.push(Line::from(Span::styled(
                    format!(" ✕ {e}"),
                    Style::new().fg(ERROR),
                )));
            } else {
                lines.push(Line::raw(""));
            }
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(" ⏎ ", key_cap()),
                Span::styled(" ejecutar   ", muted()),
                Span::styled(" Tab ", key_cap()),
                Span::styled(" siguiente campo   ", muted()),
                Span::styled(" Esc ", key_cap()),
                Span::styled(" cancelar", muted()),
            ]));
            f.render_widget(Paragraph::new(lines), inner);
        }
        Modal::Help => help(f, area),
        Modal::JobView { id, scroll } => job_view(f, app, area, *id, *scroll),
    }
    let _ = (tick, anim);
}

/// Fondo del campo salvo en el cursor (segundo span), que va invertido.
fn tint_field(spans: &mut [Span<'static>]) {
    for (i, s) in spans.iter_mut().enumerate() {
        if i != 1 {
            s.style = s.style.bg(RAISED);
        }
    }
}

fn wrap(s: &str, w: usize) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    for word in s.split_whitespace() {
        if !cur.is_empty() && text_width(&cur) + 1 + text_width(word) > w {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn help(f: &mut Frame, area: Rect) {
    let rows: Vec<(&str, Vec<(&str, &str)>)> = vec![
        (
            "Navegación",
            vec![
                ("1-5 / Tab", "cambiar de sección"),
                ("↑↓ / j k", "mover"),
                ("g / G", "primero / último"),
                ("⏎ / →", "más detalles"),
                ("/", "filtrar"),
                ("o", "cambiar orden"),
                ("␣ / a", "marcar / marcar todo"),
                ("t / T", "ver tareas / limpiar"),
                ("F5", "refrescar ya"),
                ("q", "salir"),
            ],
        ),
        (
            "Contenedores",
            vec![
                ("s", "iniciar / detener"),
                ("r", "reiniciar"),
                ("p", "pausar / reanudar"),
                ("K", "matar (SIGKILL)"),
                ("e", "shell dentro"),
                ("l", "logs en vivo"),
                ("c", "código del Compose"),
                ("i", "inspect"),
                ("m", "renombrar"),
                ("b", "abrir en navegador"),
                ("n", "nuevo"),
                ("d", "eliminar"),
                ("P", "limpiar detenidos"),
            ],
        ),
        (
            "Imágenes",
            vec![
                ("p", "descargar (pull)"),
                ("n / r", "ejecutar una imagen"),
                ("b", "construir (build)"),
                ("t", "etiquetar"),
                ("d", "eliminar"),
                ("P", "limpiar dangling"),
            ],
        ),
        (
            "Volúmenes y redes",
            vec![("n", "crear"), ("d", "eliminar"), ("P", "limpiar sin uso")],
        ),
        (
            "Compose",
            vec![
                ("u / U", "up -d / con build"),
                ("x", "stop"),
                ("d", "down (o olvidar)"),
                ("r", "restart"),
                ("p", "pull"),
                ("e", "editar fichero"),
                ("A", "añadir compose.yaml"),
            ],
        ),
        (
            "En el detalle",
            vec![
                ("← →", "cambiar de pestaña"),
                ("f", "seguir logs"),
                ("/", "filtrar logs"),
                ("[ ]", "otro fichero (.env…)"),
                ("e", "editar el código"),
                ("Esc", "volver"),
            ],
        ),
    ];
    let col_w = 40u16;
    let cols = if area.width >= 2 * col_w + 6 { 2 } else { 1 };
    let mut lines_cols: Vec<Vec<Line>> = vec![vec![]; cols];
    let mut heights = vec![0usize; cols];
    for (title, items) in rows {
        let c = (0..cols).min_by_key(|&c| heights[c]).unwrap_or(0);
        lines_cols[c].push(section(title));
        for (k, d) in items {
            lines_cols[c].push(Line::from(vec![
                Span::styled(format!(" {:<10}", k), Style::new().fg(AEGEAN)),
                Span::styled(d.to_string(), dim()),
            ]));
        }
        lines_cols[c].push(Line::raw(""));
        heights[c] = lines_cols[c].len();
    }
    let h = (*heights.iter().max().unwrap_or(&10) as u16 + 5).min(area.height);
    let w = (col_w * cols as u16 + 4).min(area.width);
    let r = centered(area, w, h);
    clear(f, r);
    let block = panel(Line::from(Span::styled(" Atajos de dockTUI ", bold())))
        .border_style(Style::new().fg(INK_DIM));
    let inner = block.inner(r);
    f.render_widget(block, r);
    let [body, foot] = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).areas(inner);
    let areas = Layout::horizontal(vec![Constraint::Ratio(1, cols as u32); cols]).split(body);
    for (i, l) in lines_cols.into_iter().enumerate() {
        f.render_widget(
            Paragraph::new(l),
            Rect::new(
                areas[i].x + 1,
                areas[i].y,
                areas[i].width.saturating_sub(1),
                areas[i].height,
            ),
        );
    }
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                " Acepta ratón: clic en pestañas y filas, doble clic abre, rueda desplaza.",
                faint(),
            )),
            Line::from(Span::styled(
                " Sin animaciones: DOCKTUI_NO_ANIM=1 o --no-anim",
                faint(),
            )),
        ]),
        foot,
    );
}

fn job_view(f: &mut Frame, app: &App, area: Rect, id: u64, scroll: usize) {
    let Some(j) = app.jobs.iter().find(|j| j.id == id) else {
        return;
    };
    let r = centered(
        area,
        area.width.saturating_sub(6).min(130),
        area.height.saturating_sub(4),
    );
    clear(f, r);
    let (icon, color, state) = match &j.state {
        JobState::Running => (
            spinner(app.tick, app.anim).to_string(),
            GOLD,
            "en curso".to_string(),
        ),
        JobState::Ok => ("✓".into(), OK, "terminada".into()),
        JobState::Failed(c) => (
            "✕".into(),
            ERROR,
            format!(
                "falló ({})",
                c.map(|c| c.to_string()).unwrap_or("sin código".into())
            ),
        ),
    };
    let block = panel(Line::from(vec![
        Span::styled(format!(" {icon} "), Style::new().fg(color)),
        Span::styled(format!("{} ", j.title), bold()),
        Span::styled(format!("· {state} "), Style::new().fg(color)),
    ]))
    .border_style(Style::new().fg(color));
    let inner = block.inner(r);
    f.render_widget(block, r);
    let [bar_row, body, foot] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(inner);
    let bw = (bar_row.width as usize).saturating_sub(4);
    let spans = match (&j.state, j.progress) {
        (JobState::Running, Some((d, t))) => {
            shimmer_bar(d as f32 / t.max(1) as f32, bw, app.tick, color, app.anim)
        }
        (JobState::Running, None) => indeterminate_bar(bw, app.tick, color, app.anim),
        (JobState::Ok, _) => bar(1.0, bw, OK),
        (JobState::Failed(_), p) => bar(
            p.map(|(d, t)| d as f32 / t.max(1) as f32).unwrap_or(0.0),
            bw,
            ERROR,
        ),
    };
    let mut l = vec![Span::raw("  ")];
    l.extend(spans);
    f.render_widget(Paragraph::new(vec![Line::raw(""), Line::from(l)]), bar_row);

    let h = body.height as usize;
    app.view_h.set(h);
    let max = j.lines.len().saturating_sub(h);
    let sc = scroll.min(max);
    let lines: Vec<Line> = j
        .lines
        .iter()
        .map(|l| {
            let c = if l.starts_with("$ ") {
                GOLD
            } else {
                level_of(l)
            };
            Line::from(Span::styled(format!(" {l}"), Style::new().fg(c)))
        })
        .collect();
    f.render_widget(Paragraph::new(lines).scroll((sc as u16, 0)), body);
    let hint = match j.state {
        JobState::Running => vec![
            Span::styled(" x ", key_cap()),
            Span::styled(" cancelar   ", muted()),
        ],
        _ => vec![
            Span::styled(" d ", key_cap()),
            Span::styled(" descartar   ", muted()),
        ],
    };
    let mut foot_spans = vec![Span::raw(" ")];
    foot_spans.extend(hint);
    foot_spans.extend([
        Span::styled(" ↑↓ ", key_cap()),
        Span::styled(" desplazar   ", muted()),
        Span::styled(" Esc ", key_cap()),
        Span::styled(" cerrar", muted()),
    ]);
    f.render_widget(Paragraph::new(Line::from(foot_spans)), foot);
}

fn level_of(l: &str) -> ratatui::style::Color {
    let low = l.to_lowercase();
    if low.contains("error") || low.contains("denied") || low.contains("failed") {
        ERROR
    } else if low.contains("warn") {
        WARN
    } else if low.contains("complete") || low.contains("done") || low.contains("success") {
        OK
    } else {
        INK_DIM
    }
}
