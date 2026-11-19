//! Estilo Takumi (ver takumi-design, `src/takumi.rs`): monocromo estricto, sin color de acento.
//! El estado nunca va solo en color: cada estado lleva icono y texto; lo importante se resalta
//! por inversión (fondo tinta, texto fondo) o por peso, nunca con rojo/verde/ámbar.
use crate::takumi::{GRISES, oscuro as tk};
use ratatui::style::{Color, Modifier, Style};

pub const PAPER: Color = tk::FONDO;
pub const RAISED: Color = tk::ELEVADO;
pub const HIGH: Color = tk::LINEA; // pista de barras y gráficos
pub const INK: Color = tk::TINTA;
pub const INK_DIM: Color = tk::TINTA_SUAVE;
pub const INK_MUTED: Color = tk::TINTA_APAGADA;
pub const INK_FAINT: Color = GRISES[3]; // 94: texto terciario y bordes de panel
pub const LINE: Color = tk::LINEA_ALTA;
// Antiguos colores de acento y de alerta: ahora son tinta en distinta intensidad.
pub const AEGEAN: Color = tk::TINTA_SUAVE; // secundario: información
pub const GOLD: Color = tk::TINTA; // destacado
pub const BRAND: Color = tk::TINTA;
pub const BRAND_TEXT: Color = tk::TINTA;
pub const ERROR: Color = tk::TINTA;
pub const WARN: Color = tk::TINTA_SUAVE;
pub const OK: Color = tk::TINTA_SUAVE;

pub fn base() -> Style {
    Style::new().fg(INK).bg(PAPER)
}
pub fn dim() -> Style {
    Style::new().fg(INK_DIM)
}
pub fn muted() -> Style {
    Style::new().fg(INK_MUTED)
}
pub fn faint() -> Style {
    Style::new().fg(INK_FAINT)
}
pub fn bold() -> Style {
    Style::new().fg(INK).add_modifier(Modifier::BOLD)
}
pub fn accent() -> Style {
    Style::new().fg(AEGEAN)
}
/// Inversión: seleccionado, pestaña activa y errores (fondo tinta, texto fondo).
pub fn inverse() -> Style {
    Style::new()
        .fg(tk::INVERSO_TINTA)
        .bg(tk::INVERSO_FONDO)
        .add_modifier(Modifier::BOLD)
}
/// Estilo de un estado: los de fallo o aviso (`✕`, `▲`) van invertidos; el resto, en gris.
pub fn state_style(label: &str, color: Color) -> Style {
    if label.starts_with('✕') || label.starts_with('▲') {
        inverse()
    } else {
        Style::new().fg(color)
    }
}
pub fn key_cap() -> Style {
    inverse()
}

/// Mezcla lineal entre dos colores RGB (t en 0..=1).
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) = (a, b) else {
        return a;
    };
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t.clamp(0.0, 1.0)).round() as u8;
    Color::Rgb(f(ar, br), f(ag, bg), f(ab, bb))
}

/// Color de carga: solo intensidad de gris (más carga, más claro).
pub fn load_color(pct: f32) -> Color {
    if pct < 50.0 {
        INK_MUTED
    } else if pct < 80.0 {
        INK_DIM
    } else {
        INK
    }
}
