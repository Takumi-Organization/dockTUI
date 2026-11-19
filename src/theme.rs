//! Paleta Ryoku Kami · Grafito (ver vault: «Estilo visual Ryoku Kami»).
//! El estado nunca va solo en color: cada color lleva icono y texto.
use ratatui::style::{Color, Modifier, Style};

pub const PAPER: Color = Color::Rgb(0x21, 0x1D, 0x1A);
pub const RAISED: Color = Color::Rgb(0x2A, 0x25, 0x21);
pub const HIGH: Color = Color::Rgb(0x35, 0x2F, 0x2A);
pub const INK: Color = Color::Rgb(0xEC, 0xE4, 0xD8);
pub const INK_DIM: Color = Color::Rgb(0xC9, 0xC0, 0xB3);
pub const INK_MUTED: Color = Color::Rgb(0xA6, 0x9D, 0x90);
pub const INK_FAINT: Color = Color::Rgb(0x8F, 0x86, 0x7A);
pub const LINE: Color = Color::Rgb(0x4B, 0x44, 0x3D);
pub const AEGEAN: Color = Color::Rgb(0x8F, 0xBD, 0xD1); // secundario: información
pub const GOLD: Color = Color::Rgb(0xD9, 0xA4, 0x41);
pub const BRAND: Color = Color::Rgb(0xE2, 0x34, 0x2A);
pub const BRAND_TEXT: Color = Color::Rgb(0xF4, 0x66, 0x5A);
pub const ERROR: Color = Color::Rgb(0xF2, 0x7A, 0x70);
pub const WARN: Color = Color::Rgb(0xF0, 0xB2, 0x5E);
pub const OK: Color = Color::Rgb(0x7F, 0xD0, 0xAE);

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
pub fn key_cap() -> Style {
    Style::new()
        .fg(PAPER)
        .bg(INK_DIM)
        .add_modifier(Modifier::BOLD)
}

/// Mezcla lineal entre dos colores RGB (t en 0..=1).
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) = (a, b) else {
        return a;
    };
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t.clamp(0.0, 1.0)).round() as u8;
    Color::Rgb(f(ar, br), f(ag, bg), f(ab, bb))
}

/// Color de carga: verde → ámbar → rojo según el porcentaje.
pub fn load_color(pct: f32) -> Color {
    if pct < 50.0 {
        OK
    } else if pct < 80.0 {
        WARN
    } else {
        ERROR
    }
}
