//! Piezas visuales reutilizables: spinners, olas, barras, sparklines y campo de texto.
use crate::theme::*;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

const BRAILLE: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const EIGHTHS: [char; 9] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];
const LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Fotograma del spinner; con movimiento reducido queda fijo.
pub fn spinner(tick: u64, anim: bool) -> &'static str {
    if anim {
        BRAILLE[(tick as usize) % BRAILLE.len()]
    } else {
        "…"
    }
}

/// Barra de progreso con medios bloques; la parte vacía es un riel tintado.
pub fn bar(frac: f32, width: usize, color: Color) -> Vec<Span<'static>> {
    let frac = frac.clamp(0.0, 1.0);
    let cells = frac * width as f32;
    let full = cells.floor() as usize;
    let part = ((cells - full as f32) * 8.0).round() as usize;
    let mut out = vec![];
    if full > 0 {
        out.push(Span::styled(
            "█".repeat(full.min(width)),
            Style::new().fg(color).bg(HIGH),
        ));
    }
    let mut used = full.min(width);
    if used < width {
        out.push(Span::styled(
            EIGHTHS[part.min(8)].to_string(),
            Style::new().fg(color).bg(HIGH),
        ));
        used += 1;
    }
    if used < width {
        out.push(Span::styled(
            " ".repeat(width - used),
            Style::new().bg(HIGH),
        ));
    }
    out
}

/// Barra con un destello que recorre la parte llena (descargas, builds).
pub fn shimmer_bar(
    frac: f32,
    width: usize,
    tick: u64,
    color: Color,
    anim: bool,
) -> Vec<Span<'static>> {
    let filled = (frac.clamp(0.0, 1.0) * width as f32).round() as usize;
    let head = if anim {
        (tick as usize * 2) % (width + 8)
    } else {
        usize::MAX
    };
    let mut out = Vec::with_capacity(width);
    for i in 0..width {
        if i < filled {
            let d = (i as i64 - head as i64 + 4).unsigned_abs() as f32;
            let glow = (1.0 - d / 4.0).clamp(0.0, 1.0) * 0.55;
            out.push(Span::styled(
                "█",
                Style::new().fg(mix(color, INK, glow)).bg(HIGH),
            ));
        } else {
            out.push(Span::styled(" ", Style::new().bg(HIGH)));
        }
    }
    out
}

/// Barra indeterminada: un bloque que rebota con borde suavizado.
pub fn indeterminate_bar(width: usize, tick: u64, color: Color, anim: bool) -> Vec<Span<'static>> {
    let seg = (width / 4).max(3);
    let span = width.saturating_sub(seg).max(1);
    let t = if anim {
        (tick as usize / 2) % (span * 2)
    } else {
        span / 2
    };
    let pos = if t <= span { t } else { span * 2 - t };
    (0..width)
        .map(|i| {
            if i >= pos && i < pos + seg {
                let edge = (i - pos).min(pos + seg - 1 - i) as f32;
                let k = (edge / 2.0).min(1.0);
                Span::styled(
                    "█",
                    Style::new().fg(mix(HIGH, color, 0.45 + 0.55 * k)).bg(HIGH),
                )
            } else {
                Span::styled(" ", Style::new().bg(HIGH))
            }
        })
        .collect()
}

/// Sparkline de las últimas `width` muestras (alineada a la derecha).
pub fn spark(values: &[f32], max: f32, width: usize, color: Color) -> Vec<Span<'static>> {
    let take = values.len().min(width);
    let slice = &values[values.len() - take..];
    let max = max.max(f32::EPSILON);
    let mut s = String::new();
    for v in slice {
        let l = ((v / max).clamp(0.0, 1.0) * 7.0).round() as usize;
        s.push(LEVELS[l]);
    }
    let pad = width - take;
    vec![
        Span::styled("·".repeat(pad), faint()),
        Span::styled(s, Style::new().fg(color)),
    ]
}

/// Gráfico de área de `height` filas: cada columna es una muestra (alineadas a la derecha).
pub fn area_chart(
    values: &[f32],
    max: f32,
    width: usize,
    height: usize,
    color: Color,
) -> Vec<Line<'static>> {
    let take = values.len().min(width);
    let slice = &values[values.len() - take..];
    let max = max.max(f32::EPSILON);
    let total = height * 8;
    let heights: Vec<usize> = slice
        .iter()
        .map(|v| (((v / max).clamp(0.0, 1.0)) * total as f32).round() as usize)
        .map(|h| h.max(1))
        .collect();
    (0..height)
        .map(|row| {
            // `row` 0 es la fila superior.
            let base = (height - 1 - row) * 8;
            // El tramo sin muestras todavía lleva una línea de base tenue en la fila inferior.
            let empty = width - take;
            let mut spans = vec![if row == height - 1 {
                Span::styled("·".repeat(empty), Style::new().fg(LINE))
            } else {
                Span::raw(" ".repeat(empty))
            }];
            for (i, h) in heights.iter().enumerate() {
                let fill = h.saturating_sub(base).min(8);
                let ch = if fill == 0 { ' ' } else { LEVELS[fill - 1] };
                // Más intensidad cuanto más alta es la columna.
                let c = mix(HIGH, color, 0.35 + 0.65 * (*h as f32 / total as f32));
                let _ = i;
                spans.push(Span::styled(ch.to_string(), Style::new().fg(c)));
            }
            Line::from(spans)
        })
        .collect()
}

/// Ola animada de una fila. `amp` 0..1 (más alta cuando hay tareas en curso).
pub fn wave_line(width: usize, tick: u64, amp: f32, anim: bool) -> Line<'static> {
    let t = if anim { tick as f32 } else { 0.0 };
    let spans = (0..width)
        .map(|x| {
            let x = x as f32;
            let v = ((x * 0.21 + t * 0.11).sin() + 0.6 * (x * 0.47 - t * 0.17).sin()) / 1.6;
            let level = (((v * 0.5 + 0.5) * amp.clamp(0.0, 1.0)) * 7.0).round() as usize;
            let level = level.min(7);
            Span::styled(
                LEVELS[level].to_string(),
                Style::new().fg(mix(LINE, AEGEAN, level as f32 / 7.0)),
            )
        })
        .collect::<Vec<_>>();
    Line::from(spans)
}

/// Campo de texto de una línea con cursor.
#[derive(Clone, Debug, Default)]
pub struct TextField {
    pub value: String,
    pub cursor: usize, // índice de carácter
}

impl TextField {
    pub fn new(v: &str) -> Self {
        TextField {
            value: v.to_string(),
            cursor: v.chars().count(),
        }
    }
    fn byte(&self, ch: usize) -> usize {
        self.value
            .char_indices()
            .nth(ch)
            .map(|(i, _)| i)
            .unwrap_or(self.value.len())
    }
    pub fn insert(&mut self, s: &str) {
        let at = self.byte(self.cursor);
        let clean: String = s.chars().filter(|c| !c.is_control()).collect();
        self.value.insert_str(at, &clean);
        self.cursor += clean.chars().count();
    }
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let a = self.byte(self.cursor - 1);
            let b = self.byte(self.cursor);
            self.value.replace_range(a..b, "");
            self.cursor -= 1;
        }
    }
    pub fn delete(&mut self) {
        if self.cursor < self.value.chars().count() {
            let a = self.byte(self.cursor);
            let b = self.byte(self.cursor + 1);
            self.value.replace_range(a..b, "");
        }
    }
    fn kill_word(&mut self) {
        let chars: Vec<char> = self.value.chars().collect();
        let mut i = self.cursor;
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        let (a, b) = (self.byte(i), self.byte(self.cursor));
        self.value.replace_range(a..b, "");
        self.cursor = i;
    }
    /// Procesa una tecla de edición; `true` si la consumió.
    pub fn key(&mut self, k: KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Char('u') if ctrl => {
                let b = self.byte(self.cursor);
                self.value.replace_range(..b, "");
                self.cursor = 0;
            }
            KeyCode::Char('w') if ctrl => self.kill_word(),
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = self.value.chars().count(),
            KeyCode::Char(c) if !ctrl && !k.modifiers.contains(KeyModifiers::ALT) => {
                self.insert(&c.to_string())
            }
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.value.chars().count()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.value.chars().count(),
            _ => return false,
        }
        true
    }

    /// Pinta el campo en `width` columnas; hace scroll horizontal para mantener el cursor visible.
    pub fn spans(&self, width: usize, focused: bool, placeholder: &str) -> Vec<Span<'static>> {
        if self.value.is_empty() && !focused {
            return vec![Span::styled(crate::util::pad(placeholder, width), faint())];
        }
        let chars: Vec<char> = self.value.chars().collect();
        let w = width.max(2);
        let start = (self.cursor + 1).saturating_sub(w);
        let end = (start + w - 1).min(chars.len());
        let before: String = chars[start..self.cursor.min(chars.len())].iter().collect();
        let at: String = chars
            .get(self.cursor)
            .map(|c| c.to_string())
            .unwrap_or(" ".into());
        let after: String = if self.cursor < end {
            chars[self.cursor + 1..end.max(self.cursor + 1)]
                .iter()
                .collect()
        } else {
            String::new()
        };
        let mut out = vec![Span::styled(before, Style::new().fg(INK))];
        if focused {
            out.push(Span::styled(at, Style::new().fg(PAPER).bg(INK)));
        } else {
            out.push(Span::styled(at, Style::new().fg(INK)));
        }
        out.push(Span::styled(after, Style::new().fg(INK)));
        out
    }
}

/// Atajo + etiqueta para la barra inferior: ` e  editar`.
pub fn hint(key: &str, label: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(format!(" {key} "), key_cap()),
        Span::styled(format!(" {label}  "), muted()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn txt(v: &[Span]) -> String {
        v.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn las_barras_miden_lo_pedido() {
        for f in [0.0, 0.33, 0.5, 1.0, 1.7] {
            assert_eq!(crate::util::text_width(&txt(&bar(f, 12, OK))), 12);
            assert_eq!(
                crate::util::text_width(&txt(&shimmer_bar(f, 12, 3, OK, true))),
                12
            );
        }
        assert_eq!(
            crate::util::text_width(&txt(&indeterminate_bar(20, 9, OK, true))),
            20
        );
        assert_eq!(
            crate::util::text_width(&txt(&spark(&[1.0, 5.0, 9.0], 10.0, 8, OK))),
            8
        );
        assert_eq!(wave_line(30, 5, 0.5, true).width(), 30);
    }

    #[test]
    fn campo_de_texto_edita_con_unicode() {
        let mut f = TextField::new("añb");
        f.cursor = 2;
        f.backspace();
        assert_eq!(f.value, "ab");
        f.insert("ñ");
        assert_eq!((f.value.as_str(), f.cursor), ("añb", 2));
        // Ctrl+W borra la palabra anterior al cursor (aquí «añ»), no lo que sigue.
        f.key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!((f.value.as_str(), f.cursor), ("b", 0));
    }
}
