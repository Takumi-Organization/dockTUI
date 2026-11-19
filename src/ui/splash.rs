//! Pantalla de arranque animada y pantalla de «Docker no responde».
use super::*;
use ratatui::layout::Alignment;
use ratatui::style::Color;

/// Píxel art de la ballena (36×16). Cada carácter es un píxel; se pinta con medios bloques.
fn whale_pixel(x: i32, y: i32, reveal: u64, eye_open: bool) -> Option<Color> {
    let body = INK_MUTED;
    let belly = mix(INK_MUTED, INK, 0.45);
    // Cuerpo: rangos de x por fila.
    let (a, b) = match y {
        9 => (4, 34),
        10 => (2, 36),
        11 => (1, 37),
        12 => (3, 37),
        13 => (6, 35),
        14 => (11, 31),
        15 => (17, 26),
        _ => (1, 0),
    };
    // Contenedores: (x0, x1, y0, y1, color, aparece en tick)
    let boxes: [(i32, i32, i32, i32, Color, u64); 6] = [
        (7, 12, 5, 8, INK_DIM, 8),
        (14, 19, 5, 8, INK, 12),
        (21, 26, 5, 8, INK_DIM, 16),
        (28, 33, 5, 8, INK, 20),
        (14, 19, 1, 4, BRAND, 26),
        (21, 26, 1, 4, INK, 30),
    ];
    for (x0, x1, y0, y1, c, at) in boxes {
        if reveal >= at && x >= x0 && x <= x1 && y >= y0 && y <= y1 {
            // Línea de junta entre contenedores: un píxel más oscuro en el borde inferior.
            return Some(if y == y1 { mix(c, PAPER, 0.35) } else { c });
        }
    }
    if reveal >= 2 && y >= 9 && x >= a && x <= b {
        if x == 31 && y == 11 {
            return Some(if eye_open {
                PAPER
            } else {
                mix(body, PAPER, 0.5)
            });
        }
        // Aleta pectoral un poco más oscura.
        if (19..=24).contains(&x) && y >= 13 {
            return Some(mix(INK_MUTED, PAPER, 0.3));
        }
        return Some(if y >= 13 { belly } else { body });
    }
    // Cola.
    if reveal >= 3 && ((x == 0 && (7..=10).contains(&y)) || (x == 1 && (8..=10).contains(&y))) {
        return Some(body);
    }
    None
}

fn whale_lines(reveal: u64, eye_open: bool) -> Vec<Line<'static>> {
    (0..8)
        .map(|row| {
            let spans: Vec<Span> = (0..38)
                .map(|x| {
                    let top = whale_pixel(x, row * 2, reveal, eye_open);
                    let bot = whale_pixel(x, row * 2 + 1, reveal, eye_open);
                    match (top, bot) {
                        (None, None) => Span::raw(" "),
                        (Some(t), None) => Span::styled("▀", Style::new().fg(t)),
                        (None, Some(b)) => Span::styled("▄", Style::new().fg(b)),
                        (Some(t), Some(b)) if t == b => Span::styled("█", Style::new().fg(t)),
                        (Some(t), Some(b)) => Span::styled("▀", Style::new().fg(t).bg(b)),
                    }
                })
                .collect();
            Line::from(spans)
        })
        .collect()
}

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let t = app.tick;
    let elapsed = app.started.elapsed().as_millis() as f32 / 1700.0;
    // Con movimiento reducido se muestra directamente la imagen final.
    let reveal = if app.anim { t } else { 99 };
    let art = whale_lines(reveal, !(app.anim && (t % 50) > 47));

    let h = 8 + 1 + 1 + 2 + 2 + 2;
    let r = centered(area, area.width.min(60), h.min(area.height));
    let mut lines: Vec<Line> = art;
    lines.push(wave_line(38, t, 0.9, app.anim));
    lines.push(Line::raw(""));

    // Título que se escribe letra a letra.
    let title = "d o c k T U I";
    let shown = if app.anim {
        ((t as usize).saturating_sub(10) / 2).min(title.chars().count())
    } else {
        title.chars().count()
    };
    let typed: String = title.chars().take(shown).collect();
    lines.push(Line::from(vec![
        Span::styled("● ", Style::new().fg(BRAND)),
        Span::styled(typed, bold()),
        Span::styled(
            if app.anim && shown < title.chars().count() {
                "▏"
            } else {
                ""
            },
            Style::new().fg(GOLD),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        "Docker Desktop, pero en tu terminal",
        muted(),
    )));
    lines.push(Line::raw(""));

    let steps = [
        "Buscando el daemon de Docker",
        "Leyendo contenedores",
        "Leyendo imágenes y volúmenes",
        "Preparando la interfaz",
    ];
    let waiting = !app.got_first && elapsed > 1.0;
    let step = if waiting {
        "Esperando al daemon"
    } else {
        steps[((elapsed * 4.0) as usize).min(3)]
    };
    let bw = 34;
    let spans = if waiting {
        indeterminate_bar(bw, t, AEGEAN, app.anim)
    } else {
        shimmer_bar(elapsed.min(1.0), bw, t, AEGEAN, app.anim)
    };
    lines.push(Line::from(spans));
    lines.push(Line::from(vec![
        Span::styled(format!("{} ", spinner(t, app.anim)), Style::new().fg(GOLD)),
        Span::styled(step.to_string(), muted()),
    ]));
    if app.got_first && matches!(app.conn, Conn::Up) {
        lines.push(Line::from(Span::styled(
            "cualquier tecla para entrar",
            faint(),
        )));
    }
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), r);
}

/// El motor no responde: explicación, causa probable y reintento animado.
pub fn draw_down(f: &mut Frame, app: &App, area: Rect) {
    let err = match &app.conn {
        Conn::Down(e) => e.clone(),
        _ => String::new(),
    };
    let lower = err.to_lowercase();
    let (cause, fix): (&str, Vec<&str>) = if lower.contains("permission denied") {
        (
            "No tienes permiso para usar el socket de Docker",
            vec!["sudo usermod -aG docker $USER   (y vuelve a iniciar sesión)"],
        )
    } else if lower.contains("no encuentro el comando") {
        (
            "Docker no está instalado o no está en el PATH",
            vec!["sudo pacman -S docker docker-compose"],
        )
    } else if [
        "cannot connect",
        "failed to connect",
        "is the docker daemon running",
        "connection refused",
        "no such file",
    ]
    .iter()
    .any(|k| lower.contains(k))
    {
        (
            "El daemon de Docker no está en marcha",
            vec!["sudo systemctl start docker"],
        )
    } else {
        (
            "No consigo hablar con Docker",
            vec!["docker info   (para ver el error completo)"],
        )
    };
    let t = app.tick;
    let mut lines = whale_lines(99, false);
    lines.push(wave_line(38, t, 0.15, app.anim));
    let z = ["z", "z Z", "z Z z", "Z z", "z"][((t / 10) % 5) as usize];
    lines.insert(
        0,
        Line::from(Span::styled(format!("{z:>40}"), Style::new().fg(INK_MUTED))),
    );
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(format!("✕ {cause}"), inverse())));
    lines.push(Line::raw(""));
    for f in fix {
        lines.push(Line::from(vec![
            Span::styled("$ ", Style::new().fg(GOLD)),
            Span::styled(f.to_string(), Style::new().fg(INK)),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled(format!("{} ", spinner(t, app.anim)), Style::new().fg(GOLD)),
        Span::styled(
            "Reintentando cada pocos segundos; se conectará solo",
            muted(),
        ),
    ]));
    if !err.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            clip(err.lines().next().unwrap_or(""), 70),
            faint(),
        )));
    }
    let h = lines.len() as u16;
    let r = centered(area, area.width.min(76), h.min(area.height));
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), r);
}
