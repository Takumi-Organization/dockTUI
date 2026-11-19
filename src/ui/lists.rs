//! Pantalla principal: tabla del recurso activo y vista previa a la derecha.
use super::*;
use crate::model::*;
use ratatui::layout::Alignment;
use ratatui::style::Color;
use ratatui::widgets::{Cell, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table};

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let wide = area.width >= 112;
    let (list_area, prev_area) = if wide {
        let [a, b] = Layout::horizontal([Constraint::Percentage(63), Constraint::Percentage(37)])
            .areas(area);
        (a, Some(b))
    } else {
        (area, None)
    };
    draw_list(f, app, list_area);
    if let Some(p) = prev_area {
        draw_preview(f, app, p);
    }
}

fn mark_cell(app: &App, tab: Tab, key: &str) -> Cell<'static> {
    if app.is_marked(tab, key) {
        Cell::from(Span::styled(
            "✓",
            Style::new().fg(GOLD).add_modifier(Modifier::BOLD),
        ))
    } else {
        Cell::from(" ")
    }
}

fn head(cols: &[&str]) -> Row<'static> {
    Row::new(
        cols.iter()
            .map(|c| {
                Cell::from(Span::styled(
                    c.to_uppercase(),
                    Style::new().fg(INK_MUTED).add_modifier(Modifier::BOLD),
                ))
            })
            .collect::<Vec<_>>(),
    )
    .height(1)
}

fn flash_style(app: &App, id: &str) -> Style {
    match app.flash.get(id) {
        Some(t) if app.anim => {
            let k = 1.0 - (t.elapsed().as_millis() as f32 / 1400.0).clamp(0.0, 1.0);
            Style::new().bg(mix(PAPER, AEGEAN, 0.28 * k))
        }
        _ => Style::new(),
    }
}

fn draw_list(f: &mut Frame, app: &mut App, area: Rect) {
    let tab = app.tab;
    let vis = app.visible(tab);
    let total = app.len(tab);
    let filter = app.filters[tab.idx()].value.clone();
    let title = Line::from(vec![
        Span::styled(format!(" {} ", tab.title()), bold()),
        Span::styled(
            if filter.is_empty() {
                format!("{total} ")
            } else {
                format!("{}/{} ", vis.len(), total)
            },
            Style::new().fg(GOLD),
        ),
    ]);
    let mut block = panel(title).border_style(Style::new().fg(INK_FAINT));
    block = block.title_bottom(
        Line::from(Span::styled(
            format!(" orden: {} ", tab.sorts()[app.sort[tab.idx()]]),
            faint(),
        ))
        .right_aligned(),
    );
    let inner = block.inner(area);
    f.render_widget(block, area);

    if vis.is_empty() {
        draw_empty(f, app, inner, &filter);
        app.hits.list = Rect::default();
        return;
    }

    let w = inner.width;
    let max_size = match tab {
        Tab::Images => app.snap.images.iter().map(|i| i.size).max().unwrap_or(1),
        Tab::Volumes => app.vol_sizes.values().copied().max().unwrap_or(1),
        _ => 1,
    }
    .max(1);

    let (header, widths, rows): (Row, Vec<Constraint>, Vec<Row>) = match tab {
        Tab::Containers => containers_table(app, &vis, w),
        Tab::Images => images_table(app, &vis, w, max_size),
        Tab::Volumes => volumes_table(app, &vis, w, max_size),
        Tab::Networks => networks_table(app, &vis, w),
        Tab::Compose => compose_table(app, &vis, w),
    };

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(1)
        .row_highlight_style(Style::new().bg(HIGH).add_modifier(Modifier::BOLD))
        .highlight_symbol(Span::styled("▌", Style::new().fg(GOLD)));
    let t = tab.idx();
    app.tables[t].select(Some(app.cursor[t]));
    f.render_stateful_widget(table, inner, &mut app.tables[t]);
    app.hits.list = inner;
    app.hits.list_first_row = inner.y + 1;

    if vis.len() > inner.height.saturating_sub(1) as usize {
        let mut sb = ScrollbarState::new(vis.len()).position(app.cursor[t]);
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

fn draw_empty(f: &mut Frame, app: &App, area: Rect, filter: &str) {
    let tab = app.tab;
    let (head, hint) = if !filter.is_empty() {
        (
            format!("Sin resultados para «{filter}»"),
            "Esc quita el filtro".to_string(),
        )
    } else {
        match tab {
            Tab::Containers => (
                "No hay contenedores".into(),
                "n crea uno nuevo  ·  2 muestra las imágenes disponibles".into(),
            ),
            Tab::Images => (
                "No hay imágenes".into(),
                "p descarga una (por ejemplo nginx:alpine)  ·  b construye desde un Dockerfile"
                    .into(),
            ),
            Tab::Volumes => ("No hay volúmenes".into(), "n crea uno".into()),
            Tab::Networks => ("No hay redes".into(), "n crea una".into()),
            Tab::Compose => (
                "Sin proyectos Compose".into(),
                "A añade un compose.yaml  ·  o abre docktui desde la carpeta del proyecto".into(),
            ),
        }
    };
    let h = 4;
    let r = centered(area, area.width, h);
    let frames = ["·  ", "·· ", "···", " ··", "  ·", "   "];
    let dots = if app.anim {
        frames[(app.tick as usize / 4) % frames.len()]
    } else {
        "   "
    };
    let lines = vec![
        Line::from(Span::styled(format!("◌ {head}"), bold())),
        Line::raw(""),
        Line::from(Span::styled(hint, muted())),
        Line::from(Span::styled(dots, faint())),
    ];
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), r);
}

// ───────────────────────── tablas ─────────────────────────

fn containers_table(
    app: &App,
    vis: &[usize],
    w: u16,
) -> (Row<'static>, Vec<Constraint>, Vec<Row<'static>>) {
    let show_image = w >= 78;
    let show_ports = w >= 100;
    let show_cpu = w >= 64;
    let show_mem = w >= 88;
    let mut cols = vec!["", "Estado", "Nombre"];
    let name_w = vis
        .iter()
        .map(|&i| text_width(&app.snap.containers[i].name))
        .max()
        .unwrap_or(10)
        .clamp(10, 34) as u16;
    let mut widths = vec![
        Constraint::Length(1),
        Constraint::Length(15),
        Constraint::Length(name_w),
    ];
    if show_image {
        cols.push("Imagen");
        widths.push(Constraint::Fill(1));
    } else {
        widths[2] = Constraint::Fill(1);
    }
    if show_ports {
        cols.push("Puertos");
        widths.push(Constraint::Length(18));
    }
    if show_cpu {
        cols.push("CPU");
        widths.push(Constraint::Length(14));
    }
    if show_mem {
        cols.push("Mem");
        widths.push(Constraint::Length(8));
    }
    cols.push("Activo");
    widths.push(Constraint::Length(10));
    let now = now_secs();
    let rows = vis
        .iter()
        .map(|&i| {
            let c = &app.snap.containers[i];
            let (label, color) = match app.pending.get(&c.id) {
                Some(p) => (format!("{} {}", spinner(app.tick, app.anim), p.label), GOLD),
                None => state_look(c, app.tick, app.anim),
            };
            let mut cells = vec![
                mark_cell(app, Tab::Containers, &c.id),
                Cell::from(Span::styled(label, Style::new().fg(color))),
                Cell::from(Span::styled(c.name.clone(), Style::new().fg(INK))),
            ];
            if show_image {
                cells.push(Cell::from(Span::styled(c.image.clone(), muted())));
            }
            if show_ports {
                let p = c
                    .ports
                    .iter()
                    .filter(|p| p.host.is_some())
                    .map(|p| p.show())
                    .collect::<Vec<_>>()
                    .join(" ");
                cells.push(Cell::from(if p.is_empty() {
                    Span::styled(
                        c.ports.first().map(|p| p.show()).unwrap_or_default(),
                        faint(),
                    )
                } else {
                    Span::styled(p, accent())
                }));
            }
            let stat = app.container_stat(c);
            if show_cpu {
                cells.push(match stat {
                    Some(s) => {
                        let mut sp = bar((s.last.cpu / 100.0).min(1.0), 6, load_color(s.last.cpu));
                        sp.push(Span::styled(format!(" {:>5.1}%", s.last.cpu), dim()));
                        Cell::from(Line::from(sp))
                    }
                    None => Cell::from(Span::styled(
                        if c.state == State::Running {
                            format!("{} …", spinner(app.tick, app.anim))
                        } else {
                            "—".into()
                        },
                        faint(),
                    )),
                });
            }
            if show_mem {
                cells.push(Cell::from(Span::styled(
                    stat.map(|s| human_bytes(s.last.mem_used))
                        .unwrap_or("—".into()),
                    dim(),
                )));
            }
            let up = match (c.state, c.started, c.finished) {
                (State::Running, Some(s), _) => uptime(now - s),
                (_, _, Some(fin)) => ago(now - fin).replace("hace ", ""),
                _ => "—".into(),
            };
            cells.push(Cell::from(Span::styled(up, muted())));
            Row::new(cells).style(flash_style(app, &c.id))
        })
        .collect();
    (head(&cols), widths, rows)
}

fn images_table(
    app: &App,
    vis: &[usize],
    w: u16,
    max: u64,
) -> (Row<'static>, Vec<Constraint>, Vec<Row<'static>>) {
    let show_id = w >= 84;
    let show_age = w >= 66;
    let mut cols = vec!["", "Imagen"];
    let mut widths = vec![Constraint::Length(1), Constraint::Fill(1)];
    if show_id {
        cols.push("ID");
        widths.push(Constraint::Length(12));
    }
    cols.push("Tamaño");
    widths.push(Constraint::Length(19));
    if show_age {
        cols.push("Creada");
        widths.push(Constraint::Length(11));
    }
    cols.push("Uso");
    widths.push(Constraint::Length(12));
    let now = now_secs();
    let rows = vis
        .iter()
        .map(|&i| {
            let im = &app.snap.images[i];
            let mut name = vec![Span::styled(
                im.title(),
                Style::new().fg(if im.dangling() { WARN } else { INK }),
            )];
            if im.tags.len() > 1 {
                name.push(Span::styled(format!("  +{}", im.tags.len() - 1), faint()));
            }
            let pending = app.pending.get(&im.id);
            let mut cells = vec![
                mark_cell(app, Tab::Images, &im.id),
                Cell::from(Line::from(name)),
            ];
            if show_id {
                cells.push(Cell::from(Span::styled(im.short.clone(), faint())));
            }
            let mut sz = bar(im.size as f32 / max as f32, 8, AEGEAN);
            sz.push(Span::styled(format!(" {:>8}", human_bytes(im.size)), dim()));
            cells.push(Cell::from(Line::from(sz)));
            if show_age {
                cells.push(Cell::from(Span::styled(
                    ago(now - im.created).replace("hace ", ""),
                    muted(),
                )));
            }
            cells.push(Cell::from(match pending {
                Some(p) => Span::styled(
                    format!("{} {}", spinner(app.tick, app.anim), p.label),
                    Style::new().fg(GOLD),
                ),
                None if im.used_by.is_empty() => Span::styled("○ sin uso", muted()),
                None => Span::styled(
                    format!("● {} en uso", im.used_by.len()),
                    Style::new().fg(OK),
                ),
            }));
            Row::new(cells)
        })
        .collect();
    (head(&cols), widths, rows)
}

fn volumes_table(
    app: &App,
    vis: &[usize],
    w: u16,
    max: u64,
) -> (Row<'static>, Vec<Constraint>, Vec<Row<'static>>) {
    let show_proj = w >= 86;
    let mut cols = vec!["", "Volumen", "Driver", "Tamaño"];
    let mut widths = vec![
        Constraint::Length(1),
        Constraint::Fill(2),
        Constraint::Length(7),
        Constraint::Length(19),
    ];
    if show_proj {
        cols.push("Proyecto");
        widths.push(Constraint::Fill(1));
    }
    cols.push("Uso");
    widths.push(Constraint::Fill(1));
    let rows = vis
        .iter()
        .map(|&i| {
            let v = &app.snap.volumes[i];
            let name = if v.anonymous() {
                vec![
                    Span::styled("anónimo ", muted()),
                    Span::styled(v.name.chars().take(12).collect::<String>(), faint()),
                ]
            } else {
                vec![Span::styled(v.name.clone(), Style::new().fg(INK))]
            };
            let size = match app.vol_sizes.get(&v.name) {
                Some(s) => {
                    let mut sp = bar(*s as f32 / max as f32, 8, GOLD);
                    sp.push(Span::styled(format!(" {:>8}", human_bytes(*s)), dim()));
                    Line::from(sp)
                }
                None => Line::from(Span::styled(
                    format!("{} …", spinner(app.tick, app.anim)),
                    faint(),
                )),
            };
            let mut cells = vec![
                mark_cell(app, Tab::Volumes, &v.name),
                Cell::from(Line::from(name)),
                Cell::from(Span::styled(v.driver.clone(), muted())),
                Cell::from(size),
            ];
            if show_proj {
                cells.push(Cell::from(Span::styled(
                    v.project().cloned().unwrap_or_default(),
                    Style::new().fg(GOLD),
                )));
            }
            cells.push(Cell::from(match app.pending.get(&v.name) {
                Some(p) => Span::styled(
                    format!("{} {}", spinner(app.tick, app.anim), p.label),
                    Style::new().fg(GOLD),
                ),
                None if v.used_by.is_empty() => Span::styled("○ sin uso", muted()),
                None => Span::styled(format!("● {}", v.used_by.join(", ")), Style::new().fg(OK)),
            }));
            Row::new(cells)
        })
        .collect();
    (head(&cols), widths, rows)
}

fn networks_table(
    app: &App,
    vis: &[usize],
    w: u16,
) -> (Row<'static>, Vec<Constraint>, Vec<Row<'static>>) {
    let show_subnet = w >= 70;
    let mut cols = vec!["", "Red", "Driver", "Ámbito"];
    let mut widths = vec![
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(9),
        Constraint::Length(7),
    ];
    if show_subnet {
        cols.push("Subred");
        widths.push(Constraint::Length(18));
    }
    cols.push("Conectados");
    widths.push(Constraint::Fill(1));
    let rows = vis
        .iter()
        .map(|&i| {
            let n = &app.snap.networks[i];
            let mut name = vec![Span::styled(n.name.clone(), Style::new().fg(INK))];
            if n.builtin() {
                name.push(Span::styled("  sistema", faint()));
            }
            let mut cells = vec![
                mark_cell(app, Tab::Networks, &n.name),
                Cell::from(Line::from(name)),
                Cell::from(Span::styled(n.driver.clone(), muted())),
                Cell::from(Span::styled(n.scope.clone(), muted())),
            ];
            if show_subnet {
                cells.push(Cell::from(Span::styled(
                    n.subnets.first().cloned().unwrap_or_default(),
                    accent(),
                )));
            }
            cells.push(Cell::from(match app.pending.get(&n.name) {
                Some(p) => Span::styled(
                    format!("{} {}", spinner(app.tick, app.anim), p.label),
                    Style::new().fg(GOLD),
                ),
                None if n.members.is_empty() => Span::styled("○ ninguno", muted()),
                None => Span::styled(
                    format!(
                        "● {}",
                        n.members
                            .iter()
                            .map(|m| m.0.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    Style::new().fg(OK),
                ),
            }));
            Row::new(cells)
        })
        .collect();
    (head(&cols), widths, rows)
}

fn compose_table(
    app: &App,
    vis: &[usize],
    w: u16,
) -> (Row<'static>, Vec<Constraint>, Vec<Row<'static>>) {
    let show_dir = w >= 76;
    let mut cols = vec!["", "Proyecto", "Estado"];
    let mut widths = vec![
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(20),
    ];
    cols.push("Servicios");
    widths.push(Constraint::Length(10));
    if show_dir {
        cols.push("Carpeta");
        widths.push(Constraint::Fill(2));
    }
    let rows = vis
        .iter()
        .map(|&i| {
            let p = &app.projects[i];
            let (icon, color): (&str, Color) = match p.state_label() {
                "en marcha" => ("●", OK),
                "parcial" => ("▲", WARN),
                "detenido" => ("○", INK_MUTED),
                _ => ("◌", INK_MUTED),
            };
            let state = match app.pending.get(&p.key()) {
                Some(pd) => Span::styled(
                    format!("{} {}", spinner(app.tick, app.anim), pd.label),
                    Style::new().fg(GOLD),
                ),
                None => Span::styled(
                    format!("{icon} {} {}/{}", p.state_label(), p.running, p.total),
                    Style::new().fg(color),
                ),
            };
            let mut cells = vec![
                mark_cell(app, Tab::Compose, &p.key()),
                Cell::from(Span::styled(p.name.clone(), Style::new().fg(INK))),
                Cell::from(state),
                Cell::from(Span::styled(p.services.len().to_string(), muted())),
            ];
            if show_dir {
                cells.push(Cell::from(Span::styled(
                    summary::home_short(&p.working_dir),
                    muted(),
                )));
            }
            Row::new(cells)
        })
        .collect();
    (head(&cols), widths, rows)
}

// ───────────────────────── vista previa ─────────────────────────

fn draw_preview(f: &mut Frame, app: &mut App, area: Rect) {
    let block = panel(Line::from(Span::styled(" Vista previa ", bold())));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let Some(i) = app.current() else {
        f.render_widget(
            Paragraph::new(Span::styled("Nada seleccionado", faint())),
            inner,
        );
        return;
    };
    let mut lines: Vec<Line> = vec![];
    match app.tab {
        Tab::Containers => {
            let c = &app.snap.containers[i];
            lines.push(Line::from(Span::styled(clip(&c.name, inner.width), bold())));
            lines.push(Line::raw(""));
            if let Some(s) = app.container_stat(c) {
                let gw = (inner.width as usize).saturating_sub(18).clamp(8, 40);
                let cpu: Vec<f32> = s.cpu.iter().copied().collect();
                let mem: Vec<f32> = s.mem.iter().copied().collect();
                let mut l1 = vec![Span::styled("CPU ", muted())];
                l1.extend(spark(
                    &cpu,
                    cpu.iter().cloned().fold(10.0, f32::max),
                    gw,
                    load_color(s.last.cpu),
                ));
                l1.push(Span::styled(format!(" {:.1}%", s.last.cpu), dim()));
                let mut l2 = vec![Span::styled("MEM ", muted())];
                l2.extend(spark(&mem, 100.0, gw, load_color(s.last.mem_pct)));
                l2.push(Span::styled(
                    format!(" {}", human_bytes(s.last.mem_used)),
                    dim(),
                ));
                lines.push(Line::from(l1));
                lines.push(Line::from(l2));
                lines.push(Line::raw(""));
            }
            lines.extend(summary::container(app, c, false));
        }
        Tab::Images => lines.extend(summary::image(app, &app.snap.images[i], false)),
        Tab::Volumes => lines.extend(summary::volume(app, &app.snap.volumes[i], false)),
        Tab::Networks => lines.extend(summary::network(&app.snap.networks[i])),
        Tab::Compose => lines.extend(summary::project(&app.projects[i])),
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled(" ⏎ ", key_cap()),
        Span::styled(" más detalles", muted()),
    ]));
    f.render_widget(Paragraph::new(lines), inner);
}
