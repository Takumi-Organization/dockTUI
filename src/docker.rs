//! Capa de acceso a Docker: ejecuta el CLI `docker` (respeta contextos,
//! `DOCKER_HOST` y los permisos del usuario) y lo convierte en datos.
use crate::model::*;
use crate::util::strip_ansi;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

fn command(args: &[String], cwd: Option<&PathBuf>) -> Command {
    let mut c = Command::new("docker");
    c.args(args)
        .env("DOCKER_CLI_HINTS", "false")
        .env("NO_COLOR", "1")
        .env("COMPOSE_ANSI", "never")
        .env("COMPOSE_PROGRESS", "plain")
        .env("BUILDKIT_PROGRESS", "plain")
        .stdin(Stdio::null());
    if let Some(d) = cwd.filter(|d| d.is_dir()) {
        c.current_dir(d);
    }
    c
}

/// Ejecuta `docker <args>` y devuelve stdout; en error, el stderr limpio.
pub fn run(args: &[&str]) -> Result<String, String> {
    let v: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    run_in(&v, None)
}

pub fn run_in(args: &[String], cwd: Option<&PathBuf>) -> Result<String, String> {
    match command(args, cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
        Ok(o) => {
            let e = strip_ansi(&String::from_utf8_lossy(&o.stderr));
            let e = e.trim();
            Err(if e.is_empty() {
                format!("docker terminó con {}", o.status)
            } else {
                e.to_string()
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err("No encuentro el comando `docker` en el PATH".into())
        }
        Err(e) => Err(e.to_string()),
    }
}

fn json(args: &[&str]) -> Result<Value, String> {
    let t = run(args)?;
    serde_json::from_str(&t).map_err(|e| format!("JSON inválido de docker: {e}"))
}

/// `ids` → `docker <kind> inspect ids…` (lista vacía si no hay nada).
fn inspect_all(list_cmd: &[&str], inspect_cmd: &[&str]) -> Result<Value, String> {
    let ids = run(list_cmd)?;
    let mut ids: Vec<&str> = ids
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Ok(Value::Array(vec![]));
    }
    let mut args: Vec<&str> = inspect_cmd.to_vec();
    args.extend(ids);
    json(&args)
}

/// Lee contenedores, imágenes, volúmenes y redes en paralelo.
pub fn fetch_snapshot() -> Result<Snapshot, String> {
    thread::scope(|sc| {
        let info = sc.spawn(|| {
            let v = json(&["info", "--format", "{{json .}}"])?;
            // El contexto no cambia mientras la app está abierta: se pregunta una vez.
            static CONTEXT: OnceLock<String> = OnceLock::new();
            let ctx = CONTEXT
                .get_or_init(|| {
                    run(&["context", "show"])
                        .unwrap_or_default()
                        .trim()
                        .to_string()
                })
                .clone();
            Ok::<_, String>(parse_info(&v, ctx))
        });
        let cont =
            sc.spawn(|| inspect_all(&["ps", "-aq", "--no-trunc"], &["container", "inspect"]));
        let img =
            sc.spawn(|| inspect_all(&["image", "ls", "-q", "--no-trunc"], &["image", "inspect"]));
        let vol = sc.spawn(|| inspect_all(&["volume", "ls", "-q"], &["volume", "inspect"]));
        let net = sc.spawn(|| {
            inspect_all(
                &["network", "ls", "-q", "--no-trunc"],
                &["network", "inspect"],
            )
        });
        let join = |h: thread::ScopedJoinHandle<'_, Result<Value, String>>| {
            h.join()
                .unwrap_or_else(|_| Err("hilo de lectura caído".into()))
        };
        let info = info
            .join()
            .unwrap_or_else(|_| Err("hilo de lectura caído".into()))?;
        let containers = parse_containers(&join(cont)?);
        let images = parse_images(&join(img)?, &containers);
        let volumes = parse_volumes(&join(vol)?, &containers);
        let networks = parse_networks(&join(net)?);
        Ok(Snapshot {
            info,
            containers,
            images,
            volumes,
            networks,
        })
    })
}

/// Tamaños de volúmenes: caro (recorre el disco), por eso va aparte y espaciado.
pub fn fetch_volume_sizes() -> HashMap<String, u64> {
    json(&["system", "df", "-v", "--format", "json"])
        .map(|v| parse_volume_sizes(&v))
        .unwrap_or_default()
}

pub fn fetch_history(image_id: &str) -> Result<Vec<Layer>, String> {
    run(&["history", "--no-trunc", "--format", "json", image_id]).map(|t| parse_history(&t))
}

/// Proceso hijo con salida en streaming. Se mata al soltarlo.
pub struct Streamer {
    child: Arc<Mutex<Child>>,
}

impl Streamer {
    pub fn kill(&self) {
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
        }
    }
}

impl Drop for Streamer {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Lanza `docker <args>` y llama a `on_line` por cada línea de stdout/stderr
/// (ya sin ANSI) y a `on_end` con el código de salida al terminar.
pub fn stream(
    args: Vec<String>,
    cwd: Option<PathBuf>,
    on_line: impl Fn(String) + Send + Sync + 'static,
    on_end: impl FnOnce(Option<i32>) + Send + 'static,
) -> Result<Streamer, String> {
    let mut child = command(&args, cwd.as_ref())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "No encuentro el comando `docker` en el PATH".to_string()
            } else {
                e.to_string()
            }
        })?;
    let out = child.stdout.take();
    let err = child.stderr.take();
    let child = Arc::new(Mutex::new(child));
    let on_line = Arc::new(on_line);

    let reader = |src: Option<Box<dyn Read + Send>>, cb: Arc<dyn Fn(String) + Send + Sync>| {
        thread::spawn(move || {
            let Some(src) = src else { return };
            let mut r = BufReader::new(src);
            let mut buf = Vec::new();
            while let Ok(n) = r.read_until(b'\n', &mut buf) {
                if n == 0 {
                    break;
                }
                let text = String::from_utf8_lossy(&buf).into_owned();
                for part in text.split(['\n', '\r']) {
                    let clean = strip_ansi(part);
                    if !clean.trim().is_empty() {
                        cb(clean);
                    }
                }
                buf.clear();
            }
        })
    };
    let h1 = reader(
        out.map(|x| Box::new(x) as Box<dyn Read + Send>),
        on_line.clone(),
    );
    let h2 = reader(err.map(|x| Box::new(x) as Box<dyn Read + Send>), on_line);

    let waiter = child.clone();
    thread::spawn(move || {
        let _ = h1.join();
        let _ = h2.join();
        let code = loop {
            match waiter.lock().map(|mut c| c.try_wait()) {
                Ok(Ok(Some(st))) => break st.code(),
                Ok(Ok(None)) => {}
                _ => break None,
            }
            thread::sleep(Duration::from_millis(40));
        };
        on_end(code);
    });
    Ok(Streamer { child })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn stream_recoge_lineas_y_codigo() {
        // `docker` puede no existir en CI: en ese caso el error es el esperado.
        let (tx, rx) = mpsc::channel();
        let tx2 = tx.clone();
        let r = stream(
            vec![
                "version".into(),
                "--format".into(),
                "{{.Client.Version}}".into(),
            ],
            None,
            move |l| {
                let _ = tx.send(Ok(l));
            },
            move |c| {
                let _ = tx2.send(Err(c));
            },
        );
        if r.is_err() {
            return;
        }
        let mut lines = 0;
        loop {
            match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
                Ok(_) => lines += 1,
                Err(code) => {
                    assert_eq!(code, Some(0));
                    break;
                }
            }
        }
        assert_eq!(lines, 1);
    }
}
