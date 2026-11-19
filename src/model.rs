//! Modelo de datos: lo que Docker devuelve (`inspect`) convertido a structs
//! pequeños. Se parsea a mano desde `serde_json::Value` para tolerar campos
//! ausentes entre versiones del motor.
use crate::util::{parse_rfc3339, parse_size};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

fn s(v: &Value, ptr: &str) -> String {
    v.pointer(ptr)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}
fn b(v: &Value, ptr: &str) -> bool {
    v.pointer(ptr).and_then(Value::as_bool).unwrap_or(false)
}
fn strings(v: &Value, ptr: &str) -> Vec<String> {
    v.pointer(ptr)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}
fn labels(v: &Value, ptr: &str) -> BTreeMap<String, String> {
    v.pointer(ptr)
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default()
}

// ───────────────────────── Contenedores ─────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Running,
    Paused,
    Restarting,
    Exited,
    Created,
    Dead,
    Removing,
}

impl State {
    fn parse(x: &str) -> State {
        match x {
            "running" => State::Running,
            "paused" => State::Paused,
            "restarting" => State::Restarting,
            "created" => State::Created,
            "dead" => State::Dead,
            "removing" => State::Removing,
            _ => State::Exited,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ComposeRef {
    pub project: String,
    pub service: String,
    pub working_dir: String,
    pub files: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Port {
    pub host: Option<u16>,
    pub container: String, // «80/tcp»
}

impl Port {
    pub fn show(&self) -> String {
        match self.host {
            Some(h) => format!("{h}→{}", self.container.trim_end_matches("/tcp")),
            None => self.container.trim_end_matches("/tcp").to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct MountInfo {
    pub kind: String, // volume | bind | tmpfs
    pub name: String, // nombre del volumen o ruta origen
    pub dest: String,
    pub rw: bool,
}

#[derive(Clone, Debug)]
pub struct Container {
    pub id: String,
    pub short: String,
    pub name: String,
    pub image: String,
    pub image_id: String,
    pub state: State,
    pub health: String,
    pub exit_code: i64,
    pub created: i64,
    pub started: Option<i64>,
    pub finished: Option<i64>,
    pub ports: Vec<Port>,
    pub mounts: Vec<MountInfo>,
    pub networks: Vec<(String, String)>, // (red, ip)
    pub labels: BTreeMap<String, String>,
    pub env: Vec<String>,
    pub cmd: String,
    pub restart: String,
    pub compose: Option<ComposeRef>,
    pub raw: Value,
}

pub fn parse_containers(arr: &Value) -> Vec<Container> {
    let Some(list) = arr.as_array() else {
        return vec![];
    };
    list.iter().map(parse_container).collect()
}

fn parse_container(v: &Value) -> Container {
    let id = s(v, "/Id");
    let labels = labels(v, "/Config/Labels");
    let compose = labels
        .get("com.docker.compose.project")
        .map(|p| ComposeRef {
            project: p.clone(),
            service: labels
                .get("com.docker.compose.service")
                .cloned()
                .unwrap_or_default(),
            working_dir: labels
                .get("com.docker.compose.project.working_dir")
                .cloned()
                .unwrap_or_default(),
            files: labels
                .get("com.docker.compose.project.config_files")
                .map(|f| {
                    f.split(',')
                        .map(|x| x.trim().to_string())
                        .filter(|x| !x.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
        });

    let mut ports: Vec<Port> = vec![];
    if let Some(map) = v
        .pointer("/NetworkSettings/Ports")
        .and_then(Value::as_object)
    {
        for (cport, binds) in map {
            match binds.as_array().filter(|a| !a.is_empty()) {
                Some(bs) => {
                    for bnd in bs {
                        let host = bnd
                            .get("HostPort")
                            .and_then(Value::as_str)
                            .and_then(|p| p.parse().ok());
                        // IPv4 e IPv6 repiten el mismo puerto: una sola vez.
                        if !ports
                            .iter()
                            .any(|p| p.host == host && p.container == *cport)
                        {
                            ports.push(Port {
                                host,
                                container: cport.clone(),
                            });
                        }
                    }
                }
                None => ports.push(Port {
                    host: None,
                    container: cport.clone(),
                }),
            }
        }
    }
    ports.sort_by(|a, b| {
        (a.host.is_none(), a.host, &a.container).cmp(&(b.host.is_none(), b.host, &b.container))
    });

    let mounts = v
        .pointer("/Mounts")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|m| {
                    let kind = s(m, "/Type");
                    let name = if kind == "volume" {
                        s(m, "/Name")
                    } else {
                        s(m, "/Source")
                    };
                    MountInfo {
                        kind,
                        name,
                        dest: s(m, "/Destination"),
                        rw: b(m, "/RW"),
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let mut networks: Vec<(String, String)> = v
        .pointer("/NetworkSettings/Networks")
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .map(|(k, n)| (k.clone(), s(n, "/IPAddress")))
                .collect()
        })
        .unwrap_or_default();
    networks.sort();

    let ep = strings(v, "/Config/Entrypoint");
    let cmd = strings(v, "/Config/Cmd");
    let cmd = ep.into_iter().chain(cmd).collect::<Vec<_>>().join(" ");

    let ts = |p: &str| parse_rfc3339(&s(v, p));
    Container {
        short: id.chars().take(12).collect(),
        id,
        name: s(v, "/Name").trim_start_matches('/').to_string(),
        image: s(v, "/Config/Image"),
        image_id: s(v, "/Image"),
        state: State::parse(&s(v, "/State/Status")),
        health: s(v, "/State/Health/Status"),
        exit_code: v
            .pointer("/State/ExitCode")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        created: ts("/Created").unwrap_or(0),
        started: ts("/State/StartedAt"),
        finished: ts("/State/FinishedAt"),
        ports,
        mounts,
        networks,
        labels,
        env: strings(v, "/Config/Env"),
        cmd,
        restart: s(v, "/HostConfig/RestartPolicy/Name"),
        compose,
        raw: v.clone(),
    }
}

// ───────────────────────── Imágenes ─────────────────────────

#[derive(Clone, Debug)]
pub struct Image {
    pub id: String,
    pub short: String,
    pub tags: Vec<String>,
    pub size: u64,
    pub created: i64,
    pub arch: String,
    pub os: String,
    pub used_by: Vec<String>,
    pub raw: Value,
}

impl Image {
    pub fn title(&self) -> String {
        self.tags
            .first()
            .cloned()
            .unwrap_or_else(|| format!("<sin etiqueta> {}", self.short))
    }
    pub fn dangling(&self) -> bool {
        self.tags.is_empty()
    }
}

pub fn parse_images(arr: &Value, containers: &[Container]) -> Vec<Image> {
    let Some(list) = arr.as_array() else {
        return vec![];
    };
    list.iter()
        .map(|v| {
            let id = s(v, "/Id");
            let used_by = containers
                .iter()
                .filter(|c| c.image_id == id)
                .map(|c| c.name.clone())
                .collect();
            Image {
                short: id.trim_start_matches("sha256:").chars().take(12).collect(),
                tags: strings(v, "/RepoTags"),
                size: v.pointer("/Size").and_then(Value::as_u64).unwrap_or(0),
                created: parse_rfc3339(&s(v, "/Created")).unwrap_or(0),
                arch: s(v, "/Architecture"),
                os: s(v, "/Os"),
                used_by,
                id,
                raw: v.clone(),
            }
        })
        .collect()
}

// ───────────────────────── Volúmenes ─────────────────────────

#[derive(Clone, Debug)]
pub struct Volume {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    pub created: i64,
    pub labels: BTreeMap<String, String>,
    pub used_by: Vec<String>,
    pub raw: Value,
}

impl Volume {
    pub fn anonymous(&self) -> bool {
        self.labels.contains_key("com.docker.volume.anonymous")
            || (self.name.len() == 64 && self.name.chars().all(|c| c.is_ascii_hexdigit()))
    }
    pub fn project(&self) -> Option<&String> {
        self.labels.get("com.docker.compose.project")
    }
}

pub fn parse_volumes(arr: &Value, containers: &[Container]) -> Vec<Volume> {
    let Some(list) = arr.as_array() else {
        return vec![];
    };
    list.iter()
        .map(|v| {
            let name = s(v, "/Name");
            let used_by = containers
                .iter()
                .filter(|c| {
                    c.mounts
                        .iter()
                        .any(|m| m.kind == "volume" && m.name == name)
                })
                .map(|c| c.name.clone())
                .collect();
            Volume {
                driver: s(v, "/Driver"),
                mountpoint: s(v, "/Mountpoint"),
                created: parse_rfc3339(&s(v, "/CreatedAt")).unwrap_or(0),
                labels: labels(v, "/Labels"),
                used_by,
                name,
                raw: v.clone(),
            }
        })
        .collect()
}

/// `docker system df -v --format json` → tamaño por volumen.
pub fn parse_volume_sizes(v: &Value) -> HashMap<String, u64> {
    v.pointer("/Volumes")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| Some((s(x, "/Name"), parse_size(&s(x, "/Size"))?)))
                .collect()
        })
        .unwrap_or_default()
}

// ───────────────────────── Redes ─────────────────────────

#[derive(Clone, Debug)]
pub struct Network {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub subnets: Vec<String>,
    pub gateway: String,
    pub internal: bool,
    pub created: i64,
    pub members: Vec<(String, String)>, // (contenedor, ip)
    pub raw: Value,
}

impl Network {
    pub fn builtin(&self) -> bool {
        matches!(self.name.as_str(), "bridge" | "host" | "none")
    }
}

pub fn parse_networks(arr: &Value) -> Vec<Network> {
    let Some(list) = arr.as_array() else {
        return vec![];
    };
    list.iter()
        .map(|v| {
            let cfg = v
                .pointer("/IPAM/Config")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut members: Vec<(String, String)> = v
                .pointer("/Containers")
                .and_then(Value::as_object)
                .map(|o| {
                    o.values()
                        .map(|c| (s(c, "/Name"), s(c, "/IPv4Address")))
                        .collect()
                })
                .unwrap_or_default();
            members.sort();
            Network {
                id: s(v, "/Id").chars().take(12).collect(),
                name: s(v, "/Name"),
                driver: s(v, "/Driver"),
                scope: s(v, "/Scope"),
                subnets: cfg
                    .iter()
                    .map(|c| s(c, "/Subnet"))
                    .filter(|x| !x.is_empty())
                    .collect(),
                gateway: cfg
                    .iter()
                    .map(|c| s(c, "/Gateway"))
                    .find(|x| !x.is_empty())
                    .unwrap_or_default(),
                internal: b(v, "/Internal"),
                created: parse_rfc3339(&s(v, "/Created")).unwrap_or(0),
                members,
                raw: v.clone(),
            }
        })
        .collect()
}

// ───────────────────────── Motor ─────────────────────────

#[derive(Clone, Debug, Default)]
pub struct EngineInfo {
    pub version: String,
    pub cpus: u64,
    pub context: String,
}

pub fn parse_info(v: &Value, context: String) -> EngineInfo {
    EngineInfo {
        version: s(v, "/ServerVersion"),
        cpus: v.pointer("/NCPU").and_then(Value::as_u64).unwrap_or(0),
        context,
    }
}

/// Foto completa del motor en un instante.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub info: EngineInfo,
    pub containers: Vec<Container>,
    pub images: Vec<Image>,
    pub volumes: Vec<Volume>,
    pub networks: Vec<Network>,
}

// ───────────────────────── Estadísticas ─────────────────────────

#[derive(Clone, Debug, Default)]
pub struct StatSample {
    pub short: String,
    pub cpu: f32,
    pub mem_pct: f32,
    pub mem_used: u64,
    pub mem_limit: u64,
    pub net: String,
    pub block: String,
    pub pids: String,
}

pub fn parse_stat(line: &str) -> Option<StatSample> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    let pct = |p: &str| s(&v, p).trim_end_matches('%').parse::<f32>().unwrap_or(0.0);
    let mem = s(&v, "/MemUsage");
    let (used, limit) = mem.split_once('/').unwrap_or((&mem, ""));
    Some(StatSample {
        short: s(&v, "/ID"),
        cpu: pct("/CPUPerc"),
        mem_pct: pct("/MemPerc"),
        mem_used: parse_size(used).unwrap_or(0),
        mem_limit: parse_size(limit).unwrap_or(0),
        net: s(&v, "/NetIO"),
        block: s(&v, "/BlockIO"),
        pids: s(&v, "/PIDs"),
    })
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub created_by: String,
    pub size: u64,
}

pub fn parse_history(text: &str) -> Vec<Layer> {
    text.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .map(|v| Layer {
            created_by: s(&v, "/CreatedBy"),
            size: parse_size(&s(&v, "/Size")).unwrap_or(0),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contenedor_con_compose_y_puertos() {
        let v: Value = serde_json::from_str(
            r#"[{"Id":"abc123abc123abc123","Name":"/web-1","Image":"sha256:1","Created":"2026-10-30T10:00:00Z",
            "Config":{"Image":"nginx","Cmd":["nginx","-g"],"Env":["A=1"],"Labels":{
              "com.docker.compose.project":"demo","com.docker.compose.service":"web",
              "com.docker.compose.project.working_dir":"/srv/demo",
              "com.docker.compose.project.config_files":"/srv/demo/compose.yaml,/srv/demo/over.yaml"}},
            "State":{"Status":"running","StartedAt":"2026-10-30T10:00:05Z","FinishedAt":"0001-01-01T00:00:00Z","Health":{"Status":"healthy"}},
            "HostConfig":{"RestartPolicy":{"Name":"always"}},
            "NetworkSettings":{"Ports":{"80/tcp":[{"HostIp":"0.0.0.0","HostPort":"8080"},{"HostIp":"::","HostPort":"8080"}],"443/tcp":null},
              "Networks":{"demo_default":{"IPAddress":"172.18.0.2"}}},
            "Mounts":[{"Type":"volume","Name":"datos","Destination":"/data","RW":true}]}]"#,
        )
        .unwrap();
        let c = &parse_containers(&v)[0];
        assert_eq!(c.name, "web-1");
        assert_eq!(c.state, State::Running);
        assert_eq!(c.short, "abc123abc123");
        let cr = c.compose.as_ref().unwrap();
        assert_eq!((cr.project.as_str(), cr.service.as_str()), ("demo", "web"));
        assert_eq!(cr.files.len(), 2);
        let shown: Vec<_> = c.ports.iter().map(Port::show).collect();
        assert_eq!(shown, ["8080→80", "443"]);
        assert_eq!(c.mounts[0].name, "datos");
        assert_eq!(c.finished, None);
        assert_eq!(c.cmd, "nginx -g");
    }

    #[test]
    fn stats_con_formato_real() {
        let l = r#"{"BlockIO":"89.3MB / 0B","CPUPerc":"1.67%","Container":"0c41","ID":"0c4185866b8d","MemPerc":"0.61%","MemUsage":"95.16MiB / 15.31GiB","Name":"x","NetIO":"41.8kB / 38.9kB","PIDs":"37"}"#;
        let s = parse_stat(l).unwrap();
        assert_eq!(s.short, "0c4185866b8d");
        assert!((s.cpu - 1.67).abs() < 1e-3);
        assert!(s.mem_limit > 16_000_000_000);
    }
}
