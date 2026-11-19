//! Persistencia mínima: lista de ficheros Compose añadidos a mano.
use std::path::PathBuf;

fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("docktui"))
}

fn list_path() -> Option<PathBuf> {
    dir().map(|d| d.join("compose.list"))
}

pub fn load_registered() -> Vec<PathBuf> {
    let Some(p) = list_path() else { return vec![] };
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .map(|l| PathBuf::from(l.trim()))
        .filter(|p| p.is_file())
        .collect()
}

pub fn save_registered(list: &[PathBuf]) -> std::io::Result<()> {
    let Some(p) = list_path() else { return Ok(()) };
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let text: String = list.iter().map(|p| format!("{}\n", p.display())).collect();
    std::fs::write(p, text)
}
