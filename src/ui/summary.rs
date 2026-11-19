//! Contenido textual reutilizado por la vista previa y las pestañas del detalle.
use super::*;
use crate::compose::Project;
use crate::model::*;
use ratatui::style::Color;
use std::path::Path;

fn sp(t: impl Into<String>, c: Color) -> Span<'static> {
    Span::styled(t.into(), Style::new().fg(c))
}

/// Sustituye el valor de variables con pinta de secreto.
pub fn mask_env(kv: &str) -> String {
    let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
    let up = k.to_uppercase();
    if ["PASS", "SECRET", "TOKEN", "KEY", "CREDENTIAL", "PRIVATE"]
        .iter()
        .any(|w| up.contains(w))
        && !v.is_empty()
    {
        format!("{k}=••••••••")
    } else {
        kv.to_string()
    }
}

pub fn home_short(p: &str) -> String {
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() && p.starts_with(&h) => format!("~{}", &p[h.len()..]),
        _ => p.to_string(),
    }
}

pub fn container(app: &App, c: &Container, full: bool) -> Vec<Line<'static>> {
    let now = now_secs();
    let (label, color) = state_look(c, app.tick, app.anim);
    let mut state = vec![sp(label, color)];
    if let (State::Running, Some(s)) = (c.state, c.started) {
        state.push(sp(
            format!("  ·  activo desde hace {}", uptime(now - s)),
            INK_MUTED,
        ));
    } else if let Some(fin) = c.finished {
        state.push(sp(format!("  ·  {}", ago(now - fin)), INK_MUTED));
    }
    if let Some(p) = app.pending.get(&c.id) {
        state = vec![sp(
            format!("{} {}…", spinner(app.tick, app.anim), p.label),
            GOLD,
        )];
    }
    let mut l = vec![
        kv("Estado", state),
        kv_text("Imagen", c.image.clone()),
        kv_text("ID", c.short.clone()),
    ];
    if !c.cmd.is_empty() {
        l.push(kv_text("Comando", c.cmd.clone()));
    }
    if !c.ports.is_empty() {
        let mut v = vec![];
        for p in &c.ports {
            v.push(sp(
                format!("{}   ", p.show()),
                if p.host.is_some() { AEGEAN } else { INK_MUTED },
            ));
        }
        l.push(kv("Puertos", v));
    } else {
        l.push(kv("Puertos", vec![sp("ninguno publicado", INK_FAINT)]));
    }
    if !c.networks.is_empty() {
        let v = c
            .networks
            .iter()
            .map(|(n, ip)| sp(format!("{n} {ip}   "), INK))
            .collect();
        l.push(kv("Redes", v));
    }
    if c.mounts.is_empty() {
        l.push(kv("Montajes", vec![sp("ninguno", INK_FAINT)]));
    }
    for (i, m) in c.mounts.iter().enumerate() {
        let tag = match m.kind.as_str() {
            "volume" => ("volumen", OK),
            "bind" => ("carpeta", AEGEAN),
            _ => ("tmpfs", INK_MUTED),
        };
        l.push(kv(
            if i == 0 { "Montajes" } else { "" },
            vec![
                sp(format!("{} ", tag.0), tag.1),
                sp(format!("{} → {}", clip(&m.name, 34), m.dest), INK),
                sp(if m.rw { "" } else { "  (solo lectura)" }, INK_MUTED),
            ],
        ));
    }
    if let Some(r) = &c.compose {
        l.push(kv(
            "Compose",
            vec![
                sp(format!("{} / {}", r.project, r.service), GOLD),
                sp(
                    if r.files.is_empty() {
                        String::new()
                    } else {
                        "   c → ver código".into()
                    },
                    INK_FAINT,
                ),
            ],
        ));
    }
    if !c.restart.is_empty() && c.restart != "no" {
        l.push(kv_text("Reinicio", c.restart.clone()));
    }
    if full {
        l.push(kv_text("Creado", ago(now - c.created)));
        if !c.env.is_empty() {
            l.push(Line::raw(""));
            l.push(section(&format!("Variables de entorno ({})", c.env.len())));
            l.push(Line::from(sp(
                "los valores que parecen secretos se ocultan",
                INK_FAINT,
            )));
            for e in &c.env {
                let m = mask_env(e);
                let (k, v) = m.split_once('=').unwrap_or((&m, ""));
                l.push(Line::from(vec![
                    sp(format!("  {k}"), AEGEAN),
                    sp("=", INK_MUTED),
                    sp(v.to_string(), INK),
                ]));
            }
        }
        if !c.labels.is_empty() {
            l.push(Line::raw(""));
            l.push(section(&format!("Etiquetas ({})", c.labels.len())));
            for (k, v) in &c.labels {
                l.push(Line::from(vec![
                    sp(format!("  {k}"), INK_MUTED),
                    sp(" = ", INK_FAINT),
                    sp(clip(v, 80), INK_DIM),
                ]));
            }
        }
    }
    l
}

pub fn image(app: &App, im: &Image, full: bool) -> Vec<Line<'static>> {
    let now = now_secs();
    let mut l = vec![];
    if im.tags.is_empty() {
        l.push(kv("Etiquetas", vec![sp("▲ sin etiqueta (dangling)", WARN)]));
    } else {
        for (i, t) in im.tags.iter().enumerate() {
            l.push(kv(
                if i == 0 { "Etiquetas" } else { "" },
                vec![sp(t.clone(), INK)],
            ));
        }
    }
    l.push(kv_text("ID", im.short.clone()));
    l.push(kv_text(
        "Tamaño",
        format!("{}  ({} bytes)", human_bytes(im.size), im.size),
    ));
    l.push(kv_text("Creada", ago(now - im.created)));
    l.push(kv_text("Plataforma", format!("{}/{}", im.os, im.arch)));
    if im.used_by.is_empty() {
        l.push(kv("En uso", vec![sp("○ ningún contenedor", INK_MUTED)]));
    } else {
        l.push(kv(
            "En uso",
            vec![sp(format!("● {}", im.used_by.join(", ")), OK)],
        ));
    }
    let strs = |p: &str| -> Vec<String> {
        im.raw
            .pointer(p)
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    let ep = strs("/Config/Entrypoint");
    let cmd = strs("/Config/Cmd");
    if !ep.is_empty() {
        l.push(kv_text("Entrypoint", ep.join(" ")));
    }
    if !cmd.is_empty() {
        l.push(kv_text("Cmd", cmd.join(" ")));
    }
    if let Some(p) = im
        .raw
        .pointer("/Config/ExposedPorts")
        .and_then(|v| v.as_object())
    {
        l.push(kv_text(
            "Puertos",
            p.keys().cloned().collect::<Vec<_>>().join("  "),
        ));
    }
    if let Some(u) = im
        .raw
        .pointer("/Config/User")
        .and_then(|v| v.as_str())
        .filter(|u| !u.is_empty())
    {
        l.push(kv_text("Usuario", u));
    }
    if let Some(w) = im
        .raw
        .pointer("/Config/WorkingDir")
        .and_then(|v| v.as_str())
        .filter(|u| !u.is_empty())
    {
        l.push(kv_text("Directorio", w));
    }
    if full {
        let env = strs("/Config/Env");
        if !env.is_empty() {
            l.push(Line::raw(""));
            l.push(section(&format!("Variables de entorno ({})", env.len())));
            for e in &env {
                l.push(Line::from(sp(format!("  {}", summary_mask(e)), INK_DIM)));
            }
        }
    }
    let _ = app;
    l
}

fn summary_mask(e: &str) -> String {
    mask_env(e)
}

pub fn volume(app: &App, v: &Volume, full: bool) -> Vec<Line<'static>> {
    let mut l = vec![
        kv_text("Nombre", v.name.clone()),
        kv_text("Driver", v.driver.clone()),
    ];
    match app.vol_sizes.get(&v.name) {
        Some(s) => l.push(kv_text("Tamaño", human_bytes(*s))),
        None => l.push(kv(
            "Tamaño",
            vec![sp(
                format!("{} calculando…", spinner(app.tick, app.anim)),
                INK_FAINT,
            )],
        )),
    }
    l.push(kv_text("Ruta", v.mountpoint.clone()));
    if v.created > 0 {
        l.push(kv_text("Creado", ago(now_secs() - v.created)));
    }
    if v.anonymous() {
        l.push(kv(
            "Tipo",
            vec![sp(
                "anónimo (creado por un contenedor sin nombre)",
                INK_MUTED,
            )],
        ));
    }
    if let Some(p) = v.project() {
        l.push(kv("Compose", vec![sp(p.clone(), GOLD)]));
    }
    if v.used_by.is_empty() {
        l.push(kv("En uso", vec![sp("○ ningún contenedor", INK_MUTED)]));
    } else {
        l.push(kv(
            "En uso",
            vec![sp(format!("● {}", v.used_by.join(", ")), OK)],
        ));
    }
    if full && !v.labels.is_empty() {
        l.push(Line::raw(""));
        l.push(section("Etiquetas"));
        for (k, val) in &v.labels {
            l.push(Line::from(vec![
                sp(format!("  {k}"), INK_MUTED),
                sp(" = ", INK_FAINT),
                sp(val.clone(), INK_DIM),
            ]));
        }
    }
    l
}

pub fn network(n: &Network) -> Vec<Line<'static>> {
    let mut l = vec![
        kv_text("Nombre", n.name.clone()),
        kv_text("ID", n.id.clone()),
        kv_text("Driver", n.driver.clone()),
        kv_text("Ámbito", n.scope.clone()),
    ];
    if n.created > 0 {
        l.push(kv_text("Creada", ago(now_secs() - n.created)));
    }
    if !n.subnets.is_empty() {
        l.push(kv_text("Subred", n.subnets.join("  ")));
    }
    if !n.gateway.is_empty() {
        l.push(kv_text("Puerta enlace", n.gateway.clone()));
    }
    if n.internal {
        l.push(kv("Aislada", vec![sp("sí, sin salida a internet", WARN)]));
    }
    if n.builtin() {
        l.push(kv(
            "Tipo",
            vec![sp("del sistema (no se puede borrar)", INK_MUTED)],
        ));
    }
    if n.members.is_empty() {
        l.push(kv("Conectados", vec![sp("○ ninguno", INK_MUTED)]));
    } else {
        l.push(kv(
            "Conectados",
            vec![sp(
                format!(
                    "● {}",
                    n.members
                        .iter()
                        .map(|m| m.0.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                OK,
            )],
        ));
    }
    l
}

pub fn project(p: &Project) -> Vec<Line<'static>> {
    let (icon, color) = match p.state_label() {
        "en marcha" => ("●", OK),
        "parcial" => ("▲", WARN),
        "detenido" => ("○", INK_MUTED),
        _ => ("◌", INK_MUTED),
    };
    let mut l = vec![
        kv(
            "Estado",
            vec![sp(
                format!("{icon} {} ({}/{})", p.state_label(), p.running, p.total),
                color,
            )],
        ),
        kv_text("Carpeta", home_short(&p.working_dir)),
    ];
    for (i, f) in p.files.iter().enumerate() {
        let name = Path::new(f)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        l.push(kv(
            if i == 0 { "Ficheros" } else { "" },
            vec![sp(name, AEGEAN)],
        ));
    }
    l.push(Line::raw(""));
    l.push(section(&format!("Servicios ({})", p.services.len())));
    for s in &p.services {
        let (i, c) = if s.running > 0 {
            ("●", OK)
        } else {
            ("○", INK_MUTED)
        };
        l.push(Line::from(vec![
            sp(format!(" {i} "), c),
            sp(s.name.clone(), INK),
            sp(format!("   {}", s.containers.join(", ")), INK_FAINT),
        ]));
    }
    if p.on_disk_only {
        l.push(Line::raw(""));
        l.push(Line::from(sp(
            "Todavía no hay contenedores: u para levantarlo",
            INK_FAINT,
        )));
    }
    l
}
