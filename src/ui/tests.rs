//! Pruebas de dibujo sin terminal: `TestBackend` en varios tamaños y estados.
use super::*;
use crate::model::*;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;
use std::sync::mpsc;

fn container(id: &str, name: &str, status: &str, compose: bool) -> serde_json::Value {
    let labels = if compose {
        json!({"com.docker.compose.project":"demo","com.docker.compose.service":"web",
               "com.docker.compose.project.working_dir":"/nonexistent","com.docker.compose.project.config_files":"/nonexistent/compose.yaml"})
    } else {
        json!({})
    };
    json!({"Id": id, "Name": format!("/{name}"), "Image":"sha256:img1","Created":"2026-10-30T10:00:00Z",
        "Config":{"Image":"nginx:alpine","Cmd":["nginx"],"Env":["A=1","DB_PASSWORD=hunter2"],"Labels":labels},
        "State":{"Status":status,"StartedAt":"2026-10-30T10:00:05Z","FinishedAt":"0001-01-01T00:00:00Z","ExitCode":0},
        "HostConfig":{"RestartPolicy":{"Name":"always"}},
        "NetworkSettings":{"Ports":{"80/tcp":[{"HostIp":"0.0.0.0","HostPort":"8080"}]},"Networks":{"demo_default":{"IPAddress":"172.18.0.2"}}},
        "Mounts":[{"Type":"volume","Name":"datos","Destination":"/data","RW":true}]})
}

fn app() -> App {
    let (tx, _rx) = mpsc::channel();
    let (rtx, _rrx) = mpsc::channel();
    let mut a = App::new(tx, rtx, true, None);
    let containers = parse_containers(&json!([
        container(&"a".repeat(64), "web-1", "running", true),
        container(&"b".repeat(64), "viejo", "exited", false),
    ]));
    let images = parse_images(
        &json!([{"Id":"sha256:img1","RepoTags":["nginx:alpine"],"Size":50000000,"Created":"2026-10-01T00:00:00Z","Architecture":"amd64","Os":"linux"},
                                      {"Id":"sha256:img2","RepoTags":[],"Size":1000,"Created":"2026-10-01T00:00:00Z"}]),
        &containers,
    );
    let volumes = parse_volumes(
        &json!([{"Name":"datos","Driver":"local","Mountpoint":"/v","CreatedAt":"2026-10-01T00:00:00Z","Labels":{}}]),
        &containers,
    );
    let networks = parse_networks(
        &json!([{"Name":"bridge","Id":"abcdef123456789","Driver":"bridge","Scope":"local","IPAM":{"Config":[{"Subnet":"172.17.0.0/16"}]},"Containers":{}}]),
    );
    a.snap = Snapshot {
        info: EngineInfo {
            version: "29.0".into(),
            cpus: 4,
            ..Default::default()
        },
        containers,
        images,
        volumes,
        networks,
    };
    a.conn = Conn::Up;
    a.got_first = true;
    a.splash = false;
    a.rebuild_projects();
    for t in Tab::ALL {
        a.sync_cursor(t);
    }
    a
}

fn render(a: &mut App, w: u16, h: u16) -> Vec<String> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| draw(f, a)).unwrap();
    let b = t.backend().buffer().clone();
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| b[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect()
}

fn key(a: &mut App, c: KeyCode) {
    a.on_key(KeyEvent::new(c, KeyModifiers::NONE));
}

const SIZES: [(u16, u16); 7] = [
    (1, 1),
    (5, 3),
    (40, 10),
    (60, 18),
    (80, 24),
    (120, 40),
    (220, 60),
];

#[test]
fn todas_las_pestanas_en_todos_los_tamanos() {
    let mut a = app();
    for t in Tab::ALL {
        a.switch_tab(t);
        for (w, h) in SIZES {
            let rows = render(&mut a, w, h);
            assert_eq!(rows.len(), h as usize);
        }
    }
}

#[test]
fn la_lista_de_contenedores_muestra_estado_con_icono_y_texto() {
    let mut a = app();
    let txt = render(&mut a, 120, 30).join("\n");
    assert!(txt.contains("web-1") && txt.contains("viejo"));
    assert!(
        txt.contains("● activo") && txt.contains("○ detenido"),
        "estado = icono + texto, no solo color"
    );
    assert!(txt.contains("8080→80"));
}

#[test]
fn el_detalle_de_cada_tipo_y_pestana_no_rompe() {
    let mut a = app();
    for t in Tab::ALL {
        a.switch_tab(t);
        key(&mut a, KeyCode::Enter);
        let n = {
            let d = a.detail.as_ref().expect("abre el detalle");
            a.detail_subtabs(d).len()
        };
        for i in 0..n {
            a.set_sub(i);
            a.on_tick();
            for (w, h) in SIZES {
                render(&mut a, w, h);
            }
        }
        key(&mut a, KeyCode::Esc);
        assert!(a.detail.is_none());
    }
}

#[test]
fn el_resumen_oculta_secretos() {
    let mut a = app();
    key(&mut a, KeyCode::Enter);
    let txt = render(&mut a, 120, 40).join("\n");
    assert!(txt.contains("DB_PASSWORD=••••"), "{txt}");
    assert!(!txt.contains("hunter2"));
}

#[test]
fn los_dialogos_se_dibujan_y_cancelan() {
    let mut a = app();
    for k in ['d', 'n', 'P'] {
        key(&mut a, KeyCode::Char(k));
        for (w, h) in SIZES {
            render(&mut a, w, h);
        }
        key(&mut a, KeyCode::Esc);
        assert!(a.modal.is_none(), "Esc cierra el diálogo de «{k}»");
    }
    key(&mut a, KeyCode::Char('?'));
    for (w, h) in SIZES {
        render(&mut a, w, h);
    }
    key(&mut a, KeyCode::Esc);
    assert!(a.modal.is_none());
}

#[test]
fn estado_sin_conexion() {
    let mut a = app();
    a.snap = Snapshot::default();
    a.conn =
        Conn::Down("Cannot connect to the Docker daemon at unix:///var/run/docker.sock".into());
    let txt = render(&mut a, 100, 36).join("\n");
    assert!(txt.contains("El daemon de Docker no está en marcha"));
    assert!(txt.contains("systemctl start docker"));
    for (w, h) in SIZES {
        render(&mut a, w, h);
    }
}

#[test]
fn el_filtro_reduce_la_lista_y_conserva_la_seleccion() {
    let mut a = app();
    key(&mut a, KeyCode::Char('/'));
    for c in "viej".chars() {
        key(&mut a, KeyCode::Char(c));
    }
    assert_eq!(a.visible(Tab::Containers).len(), 1);
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.current_container().unwrap().name, "viejo");
}

#[test]
fn el_splash_se_dibuja() {
    let mut a = app();
    a.splash = true;
    for tick in [0, 5, 20, 40, 200] {
        a.tick = tick;
        for (w, h) in SIZES {
            render(&mut a, w, h);
        }
    }
}

#[test]
fn el_raton_cambia_de_pestana_y_selecciona_filas() {
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut a = app();
    render(&mut a, 120, 30);
    let ev = |col, row| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: col,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let (r, _) = *a.hits.tabs.iter().find(|(_, t)| *t == Tab::Images).unwrap();
    a.on_mouse(ev(r.x + 1, r.y));
    assert_eq!(a.tab, Tab::Images);
    a.switch_tab(Tab::Containers);
    render(&mut a, 120, 30);
    let row = a.hits.list_first_row + 1; // segunda fila
    a.on_mouse(ev(a.hits.list.x + 3, row));
    assert_eq!(a.cursor[Tab::Containers.idx()], 1);
    // Doble clic abre el detalle.
    a.on_mouse(ev(a.hits.list.x + 3, row));
    assert!(a.detail.is_some());
}
