//! Utilidades puras: tamaños, fechas, texto. Sin dependencias de UI.
use std::time::{SystemTime, UNIX_EPOCH};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Tamaño legible en unidades decimales, como hace Docker (`117MB`, `2.8kB`).
pub fn human_bytes(b: u64) -> String {
    const U: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{b}B")
    } else if v >= 100.0 {
        format!("{v:.0}{}", U[i])
    } else if v >= 10.0 {
        format!("{v:.1}{}", U[i])
    } else {
        format!("{v:.2}{}", U[i])
    }
}

/// Interpreta `117.4MB`, `2.824kB`, `1.2GiB`, `0B`. `None` si no es un tamaño.
pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let (num, unit) = s.split_at(split);
    let n: f64 = num.parse().ok()?;
    let mult: f64 = match unit.trim().to_ascii_lowercase().as_str() {
        "b" => 1.0,
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        "kib" => 1024.0,
        "mib" => 1024.0 * 1024.0,
        "gib" => 1024.0 * 1024.0 * 1024.0,
        "tib" => 1024f64.powi(4),
        _ => return None,
    };
    Some((n * mult) as u64)
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `2026-10-30T13:21:36.811207125Z` (o con `+01:00`) → segundos Unix.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[10] != b'T' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, mi, se) = (n(11..13)?, n(14..16)?, n(17..19)?);
    if y < 1970 {
        return None; // «0001-01-01T00:00:00Z» = nunca
    }
    // Días desde 1970 (algoritmo de Howard Hinnant).
    let (yy, mm) = if mo <= 2 {
        (y - 1, mo + 9)
    } else {
        (y, mo - 3)
    };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * mm + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let mut t = days * 86400 + h * 3600 + mi * 60 + se;
    // Desfase final: «Z», «+01:00» o «-05:00».
    let rest = s.get(19..)?;
    let rest = rest.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    if let Some(sign) = rest.chars().next().filter(|c| *c == '+' || *c == '-') {
        let oh: i64 = rest.get(1..3)?.parse().ok()?;
        let om: i64 = rest.get(4..6).and_then(|x| x.parse().ok()).unwrap_or(0);
        let off = oh * 3600 + om * 60;
        t -= if sign == '+' { off } else { -off };
    }
    Some(t)
}

/// «hace 3 d», «hace 5 min».
pub fn ago(secs: i64) -> String {
    let s = secs.max(0);
    match s {
        0..=59 => "hace un momento".into(),
        60..=3599 => format!("hace {} min", s / 60),
        3600..=86399 => format!("hace {} h", s / 3600),
        86400..=2_591_999 => format!("hace {} d", s / 86400),
        2_592_000..=31_535_999 => format!("hace {} meses", s / 2_592_000),
        31_536_000..=63_071_999 => "hace 1 año".into(),
        _ => format!("hace {} años", s / 31_536_000),
    }
}

/// «2d 4h», «3h 12m», «45s».
pub fn uptime(secs: i64) -> String {
    let s = secs.max(0);
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else if s < 86400 {
        format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
    } else {
        format!("{}d {}h", s / 86400, (s % 86400) / 3600)
    }
}

pub fn text_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Recorta a `w` columnas añadiendo «…». Cuenta columnas, no bytes.
pub fn truncate(s: &str, w: usize) -> String {
    if text_width(s) <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + cw > w.saturating_sub(1) {
            break;
        }
        out.push(c);
        used += cw;
    }
    out.push('…');
    out
}

/// Rellena con espacios hasta `w` columnas (o recorta).
pub fn pad(s: &str, w: usize) -> String {
    let t = truncate(s, w);
    let extra = w.saturating_sub(text_width(&t));
    format!("{t}{}", " ".repeat(extra))
}

/// Quita secuencias ANSI (CSI y OSC) y caracteres de control salvo tabulador.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            match it.peek() {
                Some('[') => {
                    it.next();
                    for n in it.by_ref() {
                        if ('@'..='~').contains(&n) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    it.next();
                    for n in it.by_ref() {
                        if n == '\x07' {
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else if c == '\t' {
            out.push_str("    ");
        } else if !c.is_control() {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tamanos() {
        assert_eq!(human_bytes(0), "0B");
        assert_eq!(human_bytes(117_400_000), "117MB");
        assert_eq!(human_bytes(2_824), "2.82kB");
        assert_eq!(parse_size("117.4MB"), Some(117_400_000));
        assert_eq!(parse_size("2.824kB"), Some(2824));
        assert_eq!(parse_size("1GiB"), Some(1_073_741_824));
        assert_eq!(parse_size("N/A"), None);
    }

    #[test]
    fn fechas() {
        assert_eq!(parse_rfc3339("1970-01-02T00:00:00Z"), Some(86400));
        assert_eq!(
            parse_rfc3339("2026-10-30T13:21:36.811207125Z"),
            Some(1_793_366_496)
        );
        assert_eq!(
            parse_rfc3339("2026-10-30T14:21:36+01:00"),
            Some(1_793_366_496)
        );
        assert_eq!(parse_rfc3339("0001-01-01T00:00:00Z"), None);
        assert_eq!(uptime(3 * 3600 + 12 * 60), "3h 12m");
        assert_eq!(ago(40_000_000), "hace 1 año");
        assert_eq!(ago(3 * 86400), "hace 3 d");
    }

    #[test]
    fn texto() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(pad("ab", 4), "ab  ");
        assert_eq!(strip_ansi("\x1b[31mhola\x1b[0m\x1b[K"), "hola");
        assert_eq!(truncate("日本語です", 5), "日本…");
    }
}
