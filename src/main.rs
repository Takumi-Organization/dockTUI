//! dockTUI: gestor de Docker para el terminal (sustituto de Docker Desktop).
mod actions;
mod app;
mod compose;
mod config;
mod docker;
mod input;
mod model;
mod theme;
mod ui;
mod util;
mod widgets;

use app::{App, Msg, RefreshCmd};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyEventKind,
};
use ratatui::crossterm::execute;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

const HELP: &str = "dockTUI — gestor de Docker para el terminal

USO
  docktui [CARPETA|FICHERO]   abre la interfaz; si hay un compose.yaml lo muestra en Compose
  docktui --help | --version

OPCIONES
  --no-anim     sin animaciones (también DOCKTUI_NO_ANIM=1)

Dentro de la interfaz, pulsa ? para ver todos los atajos.
";

fn setup() -> DefaultTerminal {
    let t = ratatui::init();
    let _ = execute!(io::stdout(), EnableMouseCapture, EnableBracketedPaste);
    t
}

fn teardown() {
    let _ = execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();
}

/// Hilos de lectura. El estado se vuelve a leer cuando `docker events` avisa de un cambio
/// (o al pedirlo la app); si el flujo de eventos cae, se sondea cada 2,5 s.
fn spawn_refresher(tx: Sender<Msg>) -> Sender<RefreshCmd> {
    let (ctl_tx, ctl_rx): (Sender<RefreshCmd>, Receiver<RefreshCmd>) = mpsc::channel();
    let (snap_tx, snap_rx) = mpsc::channel::<()>();
    let events_alive = Arc::new(AtomicBool::new(false));

    let tx_snap = tx.clone();
    let alive = events_alive.clone();
    std::thread::spawn(move || {
        loop {
            let r = docker::fetch_snapshot();
            let ok = r.is_ok();
            if tx_snap.send(Msg::Snapshot(r)).is_err() {
                return;
            }
            let wait = match (ok, alive.load(Ordering::Relaxed)) {
                (false, _) => Duration::from_millis(2000),
                (true, true) => Duration::from_secs(20),
                (true, false) => Duration::from_millis(2500),
            };
            if snap_rx.recv_timeout(wait).is_ok() {
                // Agrupa ráfagas (un `compose up` emite decenas de eventos).
                std::thread::sleep(Duration::from_millis(200));
            }
            while snap_rx.try_recv().is_ok() {}
        }
    });

    // Flujo de eventos del motor: cualquier cambio relevante provoca una lectura.
    let trigger = snap_tx.clone();
    std::thread::spawn(move || {
        loop {
            let (done_tx, done_rx) = mpsc::channel::<()>();
            let t = trigger.clone();
            let args = ["events", "--format", "{{json .}}"]
                .map(String::from)
                .to_vec();
            let stream = docker::stream(
                args,
                None,
                move |line| {
                    // Las comprobaciones de salud y los exec no cambian lo que se ve.
                    if line.starts_with('{') && !line.contains("\"Action\":\"exec_") {
                        let _ = t.send(());
                    }
                },
                move |_| {
                    let _ = done_tx.send(());
                },
            );
            if stream.is_ok() {
                events_alive.store(true, Ordering::Relaxed);
                let _ = done_rx.recv();
                events_alive.store(false, Ordering::Relaxed);
            }
            drop(stream);
            std::thread::sleep(Duration::from_secs(3));
        }
    });

    let size_tx = {
        let (size_tx, size_rx) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            loop {
                let sizes = docker::fetch_volume_sizes();
                if !sizes.is_empty() && tx.send(Msg::VolSizes(sizes)).is_err() {
                    return;
                }
                let _ = size_rx.recv_timeout(Duration::from_secs(60));
                while size_rx.try_recv().is_ok() {}
            }
        });
        size_tx
    };

    // Enrutador: traduce las órdenes de la app en avisos a cada hilo.
    std::thread::spawn(move || {
        while let Ok(c) = ctl_rx.recv() {
            let _ = match c {
                RefreshCmd::Now => snap_tx.send(()),
                RefreshCmd::Sizes => size_tx.send(()),
            };
        }
    });
    ctl_tx
}

/// Suspende la interfaz, ejecuta un programa interactivo (shell, editor) y la restaura.
fn run_external(term: &mut DefaultTerminal, ext: app::External) {
    teardown();
    let mut out = io::stdout();
    let _ = writeln!(
        out,
        "\x1b[2J\x1b[H\x1b[2m── dockTUI · {} ──\x1b[0m\n",
        ext.title
    );
    let _ = out.flush();
    let mut cmd = std::process::Command::new(&ext.program);
    cmd.args(&ext.args);
    if let Some(d) = ext.cwd.as_ref().filter(|d| d.is_dir()) {
        cmd.current_dir(d);
    }
    match cmd.status() {
        Ok(st) if !st.success() && ext.pause_after => {
            let _ = write!(
                out,
                "\n[código {}] pulsa Enter para volver…",
                st.code().unwrap_or(-1)
            );
            let _ = out.flush();
            let mut s = String::new();
            let _ = io::stdin().read_line(&mut s);
        }
        Err(e) => {
            let _ = write!(
                out,
                "\nNo pude ejecutar {}: {e}\nPulsa Enter para volver…",
                ext.program
            );
            let _ = out.flush();
            let mut s = String::new();
            let _ = io::stdin().read_line(&mut s);
        }
        _ => {}
    }
    *term = setup();
    let _ = term.clear();
}

fn run(term: &mut DefaultTerminal, app: &mut App, rx: Receiver<Msg>) -> io::Result<()> {
    while !app.quit {
        term.draw(|f| ui::draw(f, app))?;
        // ~30 fps con animaciones; sin ellas basta con despertar cada 250 ms.
        let wait = if app.anim || app.splash {
            Duration::from_millis(33)
        } else {
            Duration::from_millis(250)
        };
        if event::poll(wait)? {
            loop {
                match event::read()? {
                    Event::Key(k) if k.kind != KeyEventKind::Release => app.on_key(k),
                    Event::Mouse(m) => app.on_mouse(m),
                    Event::Paste(s) => app.on_paste(&s),
                    _ => {}
                }
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        while let Ok(m) = rx.try_recv() {
            app.handle(m);
        }
        app.on_tick();
        if let Some(ext) = app.external.take() {
            run_external(term, ext);
            app.request_refresh();
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let mut anim = std::env::var_os("DOCKTUI_NO_ANIM").is_none();
    let mut start: Option<PathBuf> = None;
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("docktui {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--no-anim" => anim = false,
            p if !p.starts_with('-') => start = Some(PathBuf::from(p)),
            other => {
                eprintln!("Opción desconocida: {other}\n\n{HELP}");
                std::process::exit(2);
            }
        }
    }
    // Sin carpeta explícita, mira la carpeta actual (así `docktui` funciona en cualquier proyecto).
    let start = start.or_else(|| std::env::current_dir().ok());

    let (tx, rx) = mpsc::channel();
    let refresh_tx = spawn_refresher(tx.clone());
    let mut app = App::new(tx, refresh_tx, anim, start);

    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        teardown();
        prev(info);
    }));

    let mut term = setup();
    let res = run(&mut term, &mut app, rx);
    teardown();
    res
}
