//! Compose: descubrir ficheros, extraer servicios y resaltar YAML/.env.
use crate::model::Container;
use crate::theme::*;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const COMPOSE_NAMES: [&str; 4] = [
    "compose.yaml",
    "compose.yml",
    "docker-compose.yaml",
    "docker-compose.yml",
];

#[derive(Clone, Debug, Default)]
pub struct Project {
    pub name: String,
    pub working_dir: String,
    pub files: Vec<String>,
    /// (servicio, nombres de contenedor, ¿en marcha?)
    pub services: Vec<ServiceInfo>,
    pub running: usize,
    pub total: usize,
    /// Detectado en disco (cwd o añadido a mano) sin contenedores asociados.
    pub on_disk_only: bool,
}

#[derive(Clone, Debug)]
pub struct ServiceInfo {
    pub name: String,
    pub containers: Vec<String>,
    pub running: usize,
}

impl Project {
    pub fn key(&self) -> String {
        format!("{}|{}", self.name, self.working_dir)
    }
    pub fn state_label(&self) -> &'static str {
        if self.total == 0 {
            "sin crear"
        } else if self.running == 0 {
            "detenido"
        } else if self.running < self.total {
            "parcial"
        } else {
            "en marcha"
        }
    }
}

/// Primer fichero compose de un directorio.
pub fn find_in_dir(dir: &Path) -> Option<PathBuf> {
    COMPOSE_NAMES
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_file())
}

/// Nombre de proyecto como lo calcula Compose por defecto: directorio en minúsculas
/// y solo `[a-z0-9_-]`.
pub fn default_project_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| "proyecto".into())
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect()
}

/// `name:` de primer nivel del compose (Compose lo prefiere al nombre de la carpeta).
pub fn yaml_name(yaml: &str) -> Option<String> {
    yaml.lines().find_map(|l| {
        let v = l.strip_prefix("name:")?;
        let v = v.split(" #").next()?.trim().trim_matches(['"', '\'']);
        (!v.is_empty()).then(|| v.to_string())
    })
}

/// Proyectos a partir de los contenedores + ficheros conocidos en disco.
pub fn build_projects(containers: &[Container], known_files: &[PathBuf]) -> Vec<Project> {
    let mut map: BTreeMap<String, Project> = BTreeMap::new();
    for c in containers {
        let Some(cr) = &c.compose else { continue };
        let p = map
            .entry(format!("{}|{}", cr.project, cr.working_dir))
            .or_insert_with(|| Project {
                name: cr.project.clone(),
                working_dir: cr.working_dir.clone(),
                files: cr.files.clone(),
                ..Default::default()
            });
        p.total += 1;
        let up = c.state == crate::model::State::Running;
        if up {
            p.running += 1;
        }
        match p.services.iter_mut().find(|s| s.name == cr.service) {
            Some(s) => {
                s.containers.push(c.name.clone());
                s.running += up as usize;
            }
            None => p.services.push(ServiceInfo {
                name: cr.service.clone(),
                containers: vec![c.name.clone()],
                running: up as usize,
            }),
        }
    }
    for f in known_files {
        let dir = f
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default();
        let already = map
            .values()
            .any(|p| p.files.iter().any(|x| Path::new(x) == f.as_path()));
        if already {
            continue;
        }
        let text = std::fs::read_to_string(f).unwrap_or_default();
        let name = yaml_name(&text)
            .unwrap_or_else(|| default_project_name(f.parent().unwrap_or(Path::new(""))));
        let mut p = Project {
            name,
            working_dir: dir,
            files: vec![f.to_string_lossy().into_owned()],
            on_disk_only: true,
            ..Default::default()
        };
        p.services = service_names(&text)
            .into_iter()
            .map(|n| ServiceInfo {
                name: n,
                containers: vec![],
                running: 0,
            })
            .collect();
        map.insert(p.key(), p);
    }
    map.into_values().collect()
}

/// Servicios declarados bajo `services:` (análisis por indentación, sin validar).
pub fn service_names(yaml: &str) -> Vec<String> {
    let mut out = vec![];
    let mut in_services = false;
    let mut child_indent: Option<usize> = None;
    for line in yaml.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() || trimmed.trim_start().starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent == 0 {
            in_services = trimmed.starts_with("services:");
            child_indent = None;
            continue;
        }
        if !in_services {
            continue;
        }
        let ci = *child_indent.get_or_insert(indent);
        if indent == ci
            && let Some((k, _)) = trimmed.trim().split_once(':')
        {
            out.push(k.trim().trim_matches(['"', '\'']).to_string());
        }
    }
    out
}

/// Línea (1-based) donde empieza el servicio `name` bajo `services:`.
pub fn service_line(yaml: &str, name: &str) -> Option<usize> {
    let mut in_services = false;
    let mut child_indent: Option<usize> = None;
    for (i, line) in yaml.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent == 0 {
            in_services = t.starts_with("services:");
            child_indent = None;
            continue;
        }
        if !in_services {
            continue;
        }
        let ci = *child_indent.get_or_insert(indent);
        if indent == ci
            && t.split_once(':')
                .is_some_and(|(k, _)| k.trim().trim_matches(['"', '\'']) == name)
        {
            return Some(i + 1);
        }
    }
    None
}

/// Rango de líneas (1-based, inclusivo) del bloque del servicio.
pub fn service_block(yaml: &str, name: &str) -> Option<(usize, usize)> {
    let start = service_line(yaml, name)?;
    let lines: Vec<&str> = yaml.lines().collect();
    let base = lines[start - 1].len() - lines[start - 1].trim_start().len();
    let mut end = start;
    for (i, l) in lines.iter().enumerate().skip(start) {
        if l.trim().is_empty() {
            continue;
        }
        let ind = l.len() - l.trim_start().len();
        if ind <= base && !l.trim_start().starts_with('#') {
            break;
        }
        end = i + 1;
    }
    Some((start, end))
}

// ───────────────────── Resaltado de sintaxis ─────────────────────

fn s(text: impl Into<String>, st: Style) -> Span<'static> {
    Span::styled(text.into(), st)
}

/// Colorea el valor de una línea YAML/env.
fn value_spans(v: &str) -> Vec<Span<'static>> {
    let mut out = vec![];
    let (body, comment) = split_comment(v);
    // Interpolaciones `${VAR}` destacadas dentro del valor.
    let base = value_style(body.trim());
    let mut rest = body;
    while let Some(i) = rest.find("${") {
        if let Some(j) = rest[i..].find('}') {
            if i > 0 {
                out.push(s(&rest[..i], base));
            }
            out.push(s(
                &rest[i..i + j + 1],
                Style::new().fg(AEGEAN).add_modifier(Modifier::BOLD),
            ));
            rest = &rest[i + j + 1..];
            continue;
        }
        break;
    }
    if !rest.is_empty() {
        out.push(s(rest, base));
    }
    if !comment.is_empty() {
        out.push(s(comment, faint().add_modifier(Modifier::ITALIC)));
    }
    out
}

fn value_style(v: &str) -> Style {
    let lower = v.to_ascii_lowercase();
    if v.starts_with('"') || v.starts_with('\'') {
        Style::new().fg(OK)
    } else if matches!(
        lower.as_str(),
        "true" | "false" | "yes" | "no" | "null" | "~" | "on" | "off"
    ) || (!v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.'))
    {
        Style::new().fg(GOLD)
    } else if v.starts_with('&') || v.starts_with('*') || v.starts_with('!') {
        Style::new().fg(BRAND_TEXT)
    } else {
        Style::new().fg(INK)
    }
}

/// Separa `valor # comentario` (el `#` debe ir tras un espacio y fuera de comillas).
fn split_comment(v: &str) -> (&str, &str) {
    let mut q: Option<char> = None;
    let mut prev = ' ';
    for (i, c) in v.char_indices() {
        match (q, c) {
            (None, '"' | '\'') => q = Some(c),
            (Some(x), c) if c == x => q = None,
            (None, '#') if prev.is_whitespace() => return (&v[..i], &v[i..]),
            _ => {}
        }
        prev = c;
    }
    (v, "")
}

/// Una línea de YAML → spans coloreados.
pub fn yaml_line(line: &str) -> Vec<Span<'static>> {
    let indent = line.len() - line.trim_start().len();
    let body = &line[indent..];
    let mut out = vec![s(&line[..indent], Style::new())];
    if body.is_empty() {
        return out;
    }
    if body.starts_with('#') {
        out.push(s(body, faint().add_modifier(Modifier::ITALIC)));
        return out;
    }
    if body == "---" || body == "..." {
        out.push(s(body, faint()));
        return out;
    }
    let mut rest = body;
    while let Some(r) = rest.strip_prefix("- ") {
        out.push(s("- ", Style::new().fg(INK_MUTED)));
        rest = r.trim_start();
    }
    // `clave: valor` (la clave no puede empezar por comilla de valor ni contener «://»).
    if let Some(pos) = find_key_colon(rest) {
        let key = &rest[..pos];
        let top = indent == 0 && out.len() == 1;
        let kst = if top {
            Style::new().fg(GOLD).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(AEGEAN)
        };
        out.push(s(key, kst));
        out.push(s(":", Style::new().fg(INK_MUTED)));
        out.extend(value_spans(&rest[pos + 1..]));
    } else {
        out.extend(value_spans(rest));
    }
    out
}

fn find_key_colon(t: &str) -> Option<usize> {
    let mut q: Option<char> = None;
    for (i, c) in t.char_indices() {
        match (q, c) {
            (None, '"' | '\'') if i == 0 => q = Some(c),
            (Some(x), c) if c == x => q = None,
            (None, ':') => {
                let after = t[i + 1..].chars().next();
                if after.is_none() || after == Some(' ') {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Línea de un `.env`: `CLAVE=valor`.
pub fn env_line(line: &str) -> Vec<Span<'static>> {
    let t = line.trim_start();
    if t.is_empty() {
        return vec![s(line, Style::new())];
    }
    if t.starts_with('#') {
        return vec![s(line, faint().add_modifier(Modifier::ITALIC))];
    }
    match line.split_once('=') {
        Some((k, v)) => {
            let mut out = vec![
                s(k, Style::new().fg(AEGEAN)),
                s("=", Style::new().fg(INK_MUTED)),
            ];
            out.extend(value_spans(v));
            out
        }
        None => vec![s(line, Style::new().fg(INK))],
    }
}

/// ¿Es un `.env`?
pub fn is_env_file(path: &str) -> bool {
    Path::new(path)
        .file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with(".env"))
}

/// Ficheros relacionados con un proyecto: los de configuración y el `.env` contiguo.
pub fn project_files(p: &Project) -> Vec<String> {
    let mut v = p.files.clone();
    let env = Path::new(&p.working_dir).join(".env");
    if env.is_file() {
        let e = env.to_string_lossy().into_owned();
        if !v.contains(&e) {
            v.push(e);
        }
    }
    v
}

/// Argumentos base de `docker compose` para un proyecto.
pub fn compose_args(p: &Project) -> Vec<String> {
    let mut a: Vec<String> = vec!["compose".into(), "-p".into(), p.name.clone()];
    if !p.working_dir.is_empty() {
        a.push("--project-directory".into());
        a.push(p.working_dir.clone());
    }
    for f in p.files.iter().filter(|f| !is_env_file(f)) {
        a.push("-f".into());
        a.push(f.clone());
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    const Y: &str = "name: demo\nservices:\n  web:\n    image: nginx # servidor\n    ports:\n      - \"8080:80\"\n\n  db:\n    image: postgres:18\n    environment:\n      POSTGRES_PASSWORD: ${DB_PASS}\nvolumes:\n  datos:\n";

    #[test]
    fn servicios_y_bloques() {
        assert_eq!(service_names(Y), ["web", "db"]);
        assert_eq!(service_line(Y, "db"), Some(8));
        assert_eq!(service_block(Y, "web"), Some((3, 6)));
        assert_eq!(service_block(Y, "db"), Some((8, 11)));
        assert_eq!(service_line(Y, "nada"), None);
    }

    fn texto(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn el_resaltado_conserva_el_texto() {
        for l in Y.lines().chain([
            "  - 8080:80",
            "k: 'a # b' # c",
            "  # nota",
            "x: &anc 1",
            "url: http://h:80/p",
        ]) {
            assert_eq!(texto(&yaml_line(l)), l, "línea: {l:?}");
        }
        assert_eq!(texto(&env_line("A=1 # x")), "A=1 # x");
    }

    #[test]
    fn nombre_del_yaml() {
        assert_eq!(yaml_name(Y).as_deref(), Some("demo"));
        assert_eq!(
            yaml_name("name: \"a b\" # x\nservices: {}").as_deref(),
            Some("a b")
        );
        assert_eq!(yaml_name("services:\n  name: no\n"), None);
    }

    #[test]
    fn nombre_de_proyecto_por_defecto() {
        assert_eq!(default_project_name(Path::new("/x/Mi App.v2")), "miappv2");
    }
}
