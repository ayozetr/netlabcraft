//! Command implementations.

use netlabcraft_core::{DeviceId, parse_ip};
use netlabcraft_model::cable::{CONSOLE_PORT, Cable, RS232_PORT, is_console_port_name};
use netlabcraft_model::{DeviceKind, End, Shape, file, models};
use netlabcraft_sim::{PingOptions, Probe};
use serde_json::{Value, json};

use crate::{EngineError, Result, Session, catalog, sample};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

fn str_param<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p.get(key).and_then(Value::as_str).ok_or_else(|| bad(format!("`{key}` (text) is required")))
}

fn opt_str<'a>(p: &'a Value, key: &str) -> Option<&'a str> {
    p.get(key).and_then(Value::as_str)
}

fn f32_param(p: &Value, key: &str) -> Option<f32> {
    p.get(key).and_then(Value::as_f64).filter(|v| v.is_finite()).map(|v| v.clamp(-1.0e6, 1.0e6) as f32)
}

/// A device given by name (`"R1"`), `"#id"` or numeric id.
pub(crate) fn device_param(s: &Session, p: &Value, key: &str) -> Result<DeviceId> {
    match p.get(key) {
        Some(Value::Number(n)) => {
            let id = n.as_u64().and_then(|v| u32::try_from(v).ok()).ok_or_else(|| bad(format!("`{key}` is not a device id")))?;
            s.topology.device(DeviceId(id)).map(|d| d.id).ok_or_else(|| EngineError::Failed(format!("no device #{id}")))
        }
        Some(Value::String(name)) => Ok(s.topology.resolve(name)?),
        _ => Err(bad(format!("`{key}` (device name or id) is required"))),
    }
}

fn port_param(s: &Session, dev: DeviceId, name: &str) -> Result<usize> {
    let d = s.topology.device(dev).ok_or_else(|| EngineError::Failed("no such device".into()))?;
    d.port_index(name).ok_or_else(|| EngineError::Failed(format!("{} has no port {name}", d.name)))
}

pub fn run(s: &mut Session, id: &str, p: &Value) -> Result<Value> {
    let info = catalog::find(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
    let _ = info;
    if let Some(r) = crate::instructor::run(s, id, p) {
        return r;
    }
    let cmd = id;
    match id {
        "file.new" => {
            let t = match opt_str(p, "sample") {
                Some(name) => sample::build(name)?,
                None => netlabcraft_model::Topology::new(),
            };
            s.replace(t, None);
            Ok(json!({"devices": s.topology.devices.len()}))
        }
        "file.open" => {
            let (text, path) = match (opt_str(p, "json"), opt_str(p, "path")) {
                (Some(j), _) => (j.to_string(), opt_str(p, "path").map(str::to_string)),
                (None, Some(path)) => (read_file(path)?, Some(path.to_string())),
                (None, None) => return Err(bad("give `json` (the file's contents) or `path`")),
            };
            let t = file::from_json(&text)?;
            s.replace(t, path);
            Ok(json!({"devices": s.topology.devices.len(), "links": s.topology.links.len()}))
        }
        "file.save" => {
            let text = file::to_json(&s.topology)?;
            if let Some(path) = opt_str(p, "path") {
                write_file(path, text.as_bytes())?;
                s.path = Some(path.to_string());
            }
            s.dirty = false;
            Ok(json!({"json": text}))
        }
        "file.export_pcap" => {
            // A pcap file has one link type: Ethernet (every wired hop, and what each air hop
            // carries), or 802.11 (the air hops only, as they crossed the air).
            let air = match opt_str(p, "link").unwrap_or("ethernet") {
                "ethernet" => false,
                "802.11" | "wlan" => true,
                other => return Err(bad(format!("link `{other}`: ethernet or 802.11"))),
            };
            let bytes = if air { s.sim.pcap_wlan() } else { s.pcap() };
            if let Some(path) = opt_str(p, "path") {
                write_file(path, &bytes)?;
            }
            let frames = s.sim.trace().iter().filter(|e| if air { e.air.is_some() } else { !e.frame.is_empty() }).count();
            Ok(json!({"frames": frames, "bytes": bytes.len()}))
        }
        "edit.undo" => Ok(json!({"undone": s.undo()})),
        "edit.redo" => Ok(json!({"redone": s.redo()})),
        "edit.checkpoint" => {
            s.checkpoint();
            Ok(Value::Null)
        }
        "lab.notes" => {
            let text: String = str_param(p, "text")?.chars().take(100_000).collect();
            s.checkpoint();
            s.topology.notes = text;
            s.changed();
            Ok(Value::Null)
        }
        "device.add" => {
            let model = match (opt_str(p, "model"), opt_str(p, "kind")) {
                (Some(m), _) => models::find(m).ok_or_else(|| bad(format!("unknown model `{m}` (see device.models)")))?,
                (None, Some(k)) => models::default_for(DeviceKind::from_id(k).ok_or_else(|| bad(format!("unknown device kind `{k}` (router, switch, hub, pc, laptop, server, printer)")))?),
                (None, None) => return Err(bad("`kind` or `model` is required")),
            };
            let (x, y) = (f32_param(p, "x").unwrap_or(0.0), f32_param(p, "y").unwrap_or(0.0));
            let mut t = s.topology.clone();
                        let id = t.add_model(model, opt_str(p, "name"), x, y)?;
            crate::home::factory(&mut t, id);
            join_view(s, &mut t, id);
            s.checkpoint();
            s.topology = t;
            s.changed();
            let name = s.topology.device(id).map(|d| d.name.clone()).unwrap_or_default();
            Ok(json!({"id": id.0, "name": name}))
        }
        "device.duplicate" => {
            let d = device_param(s, p, "device")?;
            let src = s.topology.device(d).cloned().ok_or_else(|| EngineError::Failed("no such device".into()))?;
            let (x, y) = (f32_param(p, "x").unwrap_or(src.x + 60.0), f32_param(p, "y").unwrap_or(src.y + 60.0));
            let mut t = s.topology.clone();
            let id = t.add_model(src.catalogue_model(), None, x, y)?;
            if let Some(copy) = t.device_mut(id) {
                // Same configuration, fresh identity: new name and MAC addresses.
                for (dst, from) in copy.ports.iter_mut().zip(&src.ports) {
                    *dst = netlabcraft_model::Port { name: dst.name.clone(), mac: dst.mac, ..from.clone() };
                }
                copy.gateway = src.gateway;
                copy.routes = src.routes.clone();
                copy.vlans = src.vlans.clone();
            }
            join_view(s, &mut t, id);
            s.checkpoint();
            s.topology = t;
            s.changed();
            let name = s.topology.device(id).map(|d| d.name.clone()).unwrap_or_default();
            Ok(json!({"id": id.0, "name": name}))
        }
                                "server.services" => {
            use netlabcraft_model::services::{MAX_FILE, MAX_ITEMS, Services};
            let d = device_param(s, p, "device")?;
            if s.topology.device(d).is_none_or(|x| x.kind != DeviceKind::Server) {
                return Err(bad("services run on Server devices"));
            }
            let mut sv: Services = serde_json::from_value(p.get("services").cloned().unwrap_or(Value::Null)).map_err(|e| bad(format!("`services`: {e}")))?;
            sv.http.files.truncate(MAX_ITEMS);
            for f in &mut sv.http.files {
                f.name = f.name.trim().chars().filter(|c| !c.is_whitespace() && *c != '/').take(64).collect();
                f.content = f.content.chars().take(MAX_FILE).collect();
            }
            sv.http.files.retain(|f| !f.name.is_empty());
            sv.dns.records.truncate(MAX_ITEMS);
                        sv.dhcp.pools.truncate(MAX_ITEMS);
                                    sv.email.users.truncate(MAX_ITEMS);
            sv.email.domain = sv.email.domain.trim().chars().take(64).collect();
            sv.email.mailboxes.truncate(netlabcraft_model::services::MAX_MAILS);
            sv.ftp.users.truncate(MAX_ITEMS);
            sv.ftp.files.truncate(MAX_ITEMS);
            for f in &mut sv.ftp.files {
                f.name = f.name.trim().chars().filter(|c| !c.is_whitespace() && *c != '/').take(64).collect();
                f.content = f.content.chars().take(MAX_FILE).collect();
            }
            sv.ftp.files.retain(|f| !f.name.is_empty());
            sv.aaa.clients.truncate(MAX_ITEMS);
            sv.aaa.users.truncate(MAX_ITEMS);
            for c in &mut sv.aaa.clients {
                c.name = c.name.chars().take(64).collect();
                c.secret = c.secret.chars().take(63).collect();
            }
            for u in &mut sv.aaa.users {
                u.name = u.name.chars().take(64).collect();
                u.password = u.password.chars().take(64).collect();
            }
            sv.tftp.files.truncate(MAX_ITEMS);
            for f in &mut sv.tftp.files {
                f.content = f.content.chars().take(MAX_FILE).collect();
            }
            for pool in &mut sv.dhcp.pools {
                pool.max_users = pool.max_users.min(65_536);
                if netlabcraft_core::mask_to_prefix(pool.mask).is_none() {
                    return Err(bad(format!("pool {}: {} is not a subnet mask", pool.name, pool.mask)));
                }
            }
            s.checkpoint();
            if let Some(dev) = s.topology.device_mut(d) {
                dev.services = sv;
            }
            s.changed();
            Ok(Value::Null)
        }
        "browser.get" => {
            let d = device_param(s, p, "device")?;
            if s.topology.device(d).is_none_or(|x| !x.kind.is_host()) {
                return Err(bad("web pages are opened from PCs, laptops and servers"));
            }
            let url = str_param(p, "url")?.trim().to_string();
            let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("HTTP://")).unwrap_or(&url);
            if url.to_ascii_lowercase().starts_with("https://") {
                return Err(EngineError::Failed("https is not simulated yet; use http://".into()));
            }
            let (host, path) = match rest.split_once('/') {
                Some((h, p)) => (h.to_string(), format!("/{p}")),
                None => (rest.to_string(), "/".to_string()),
            };
            if host.is_empty() {
                return Err(bad("give an address or a name, such as http://192.168.1.10"));
            }
            let server = s.sim.dns_resolve(&s.topology, d, &host).map_err(|e| EngineError::Failed(e.to_string()))?;
            let (status, body) = s.sim.http_get(&s.topology, d, server, &host, &path).map_err(|e| EngineError::Failed(e.to_string()))?;
            s.revision += 1;
            Ok(json!({"status": status, "body": body, "url": format!("http://{host}{path}")}))
        }
        "device.startup" => {
            let d = device_param(s, p, "device")?;
            let dev = s.topology.device(d).ok_or_else(|| EngineError::Failed("no such device".into()))?;
            if dev.kind.is_host() || dev.kind == DeviceKind::Hub {
                return Err(bad("only routers and switches keep a startup configuration"));
            }
            let new = match str_param(p, "action")? {
                "save" => Some(netlabcraft_ios::running_config(dev)),
                "erase" => None,
                "load" => Some(str_param(p, "text")?.chars().take(1_000_000).collect()),
                other => return Err(bad(format!("unknown action `{other}` (save, erase, load)"))),
            };
            s.checkpoint();
            if let Some(dev) = s.topology.device_mut(d) {
                dev.startup = new;
            }
            s.changed();
            Ok(Value::Null)
        }
        "device.label" => {
            let d = device_param(s, p, "device")?;
            let label: String = str_param(p, "label")?.trim().chars().take(netlabcraft_model::MAX_NAME).collect();
            s.checkpoint();
            if let Some(dev) = s.topology.device_mut(d) {
                dev.label = if label.is_empty() { None } else { Some(label) };
            }
            s.changed();
            Ok(Value::Null)
        }
        "device.models" => Ok(Value::Array(
            models::MODELS.iter().map(|m| json!({"id": m.id, "kind": m.kind.id(), "label": m.label, "description": m.description, "ports": m.port_names(), "slots": m.slots()})).collect(),
        )),
                                "snmp.request" => {
            use netlabcraft_wire::snmp::{PduKind, Snmp, Value as SnmpValue, parse_oid};
            let d = device_param(s, p, "device")?;
            let target = parse_ip(str_param(p, "target")?).map_err(|e| bad(e.to_string()))?;
            let community: String = opt_str(p, "community").unwrap_or("public").chars().take(32).collect();
            let kind = match opt_str(p, "operation").unwrap_or("get") {
                "get" => PduKind::Get,
                "get-next" => PduKind::GetNext,
                "set" => PduKind::Set,
                other => return Err(bad(format!("unknown operation `{other}` (get, get-next, set)"))),
            };
            let oid = parse_oid(str_param(p, "oid")?).ok_or_else(|| bad("an OID is numbers and dots: 1.3.6.1.2.1.1.5.0"))?;
            let value = match (kind, opt_str(p, "value")) {
                (PduKind::Set, Some(v)) => v.parse::<i64>().map_or_else(|_| SnmpValue::Text(v.chars().take(255).collect()), SnmpValue::Integer),
                (PduKind::Set, None) => return Err(bad("a set needs a value")),
                _ => SnmpValue::Null,
            };
            // Any id does: the manager waits for one answer at a time.
            let request_id = i32::try_from(s.sim.now_us() % 1_000_000).unwrap_or(0) + 1;
            let req = Snmp { community, kind, request_id, error_status: 0, error_index: 0, bindings: vec![(oid, value)] };
            let r = match opt_str(p, "version").unwrap_or("2c") {
                "2c" | "2" => s.sim.snmp_request(&s.topology, d, target, req),
                "3" => {
                    let user: String = str_param(p, "user")?.chars().take(32).collect();
                    let auth = match opt_str(p, "auth").unwrap_or("none") {
                        "none" | "" => None,
                        h @ ("md5" | "sha") => {
                            let pw: String = str_param(p, "password")?.chars().take(64).collect();
                            if pw.chars().count() < 8 {
                                return Err(bad("an SNMPv3 password has at least 8 characters"));
                            }
                            Some((if h == "md5" { netlabcraft_wire::sha1::Hash::Md5 } else { netlabcraft_wire::sha1::Hash::Sha1 }, pw))
                        }
                        other => return Err(bad(format!("unknown authentication `{other}` (none, md5, sha)"))),
                    };
                    s.sim.snmp_request_v3(&s.topology, d, target, req, &netlabcraft_sim::snmp::V3Login { user, auth })
                }
                other => return Err(bad(format!("unknown SNMP version `{other}` (2c, 3)"))),
            }
            .map_err(|e| EngineError::Failed(e.to_string()))?;
            // Sets the agent accepted change the lab.
            for (dev, what, v) in s.sim.take_snmp_changes() {
                match what {
                    "name" => {
                        let _ = s.topology.rename(dev, &v);
                    }
                    _ => {
                        if let Some(x) = s.topology.device_mut(dev) {
                            if what == "contact" {
                                x.snmp.contact = v;
                            } else {
                                x.snmp.location = v;
                            }
                        }
                    }
                }
                s.changed();
            }
            let error = match r.error_status {
                0 => Value::Null,
                netlabcraft_wire::snmp::NO_ACCESS => json!("no access (a read-only community)"),
                netlabcraft_wire::snmp::NOT_WRITABLE => json!("not writable"),
                netlabcraft_wire::snmp::WRONG_TYPE => json!("wrong type"),
                16 => json!("not authorized (the user is in no group)"),
                n => json!(format!("error {n}")),
            };
            Ok(json!({
                "bindings": r.bindings.iter().map(|(o, v)| json!({"oid": netlabcraft_wire::snmp::oid_text(o), "value": v.show()})).collect::<Vec<_>>(),
                "error": error,
            }))
        }
        "snmp.traps" => {
            let d = device_param(s, p, "device")?;
            if p.get("clear").and_then(Value::as_bool) == Some(true) {
                s.sim.clear_traps(d);
                return Ok(json!({"traps": []}));
            }
            let traps: Vec<Value> = s
                .sim
                .traps(d)
                .iter()
                .map(|x| {
                    json!({
                        "at_ms": x.at_us / 1_000,
                        "from": x.from.to_string(),
                        "community": x.community,
                        "trap": x.name,
                        "bindings": x.bindings.iter().map(|(o, v)| json!({"oid": o, "value": v})).collect::<Vec<_>>(),
                    })
                })
                .collect();
            Ok(json!({"traps": traps}))
        }
        "server.syslog" => {
            let d = device_param(s, p, "device")?;
            if s.topology.device(d).is_none_or(|x| x.kind != DeviceKind::Server) {
                return Err(bad("the Syslog service runs on Server devices"));
            }
            if p.get("clear").and_then(Value::as_bool) == Some(true) {
                s.sim.clear_syslog(d);
                return Ok(json!({"messages": []}));
            }
            let msgs: Vec<Value> = s
                .sim
                .state(d)
                .map(|st| st.syslog.iter().map(|(at, from, text)| json!({"at_ms": at / 1_000, "from": from.to_string(), "text": text})).collect())
                .unwrap_or_default();
            Ok(json!({"messages": msgs}))
        }
        "mail.configure" => {
            let d = device_param(s, p, "device")?;
            if !s.topology.device(d).is_some_and(|x| x.kind.is_host()) {
                return Err(bad("the Email app runs on PCs, laptops and servers"));
            }
            let f = |k: &str| opt_str(p, k).map_or(String::new(), |v| v.trim().chars().take(128).collect());
            let account = netlabcraft_model::services::MailAccount {
                name: f("name"),
                address: f("address"),
                incoming: f("incoming"),
                outgoing: f("outgoing"),
                user: f("user"),
                password: opt_str(p, "password").map_or(String::new(), |v| v.chars().take(64).collect()),
            };
            if !account.address.contains('@') {
                return Err(bad("the email address needs an @ (ana@lab.local)"));
            }
            s.checkpoint();
            if let Some(x) = s.topology.device_mut(d) {
                x.mail = Some(account);
            }
            s.changed();
            Ok(json!({}))
        }
        "mail.send" => {
            let d = device_param(s, p, "device")?;
            let account = s.topology.device(d).and_then(|x| x.mail.clone()).ok_or_else(|| bad("set up the Email app first (mail.configure)"))?;
            let to: String = str_param(p, "to")?.trim().chars().take(128).collect();
            let mail = netlabcraft_model::services::Mail {
                from: account.address.clone(),
                to,
                subject: opt_str(p, "subject").map_or(String::new(), |v| v.chars().take(200).collect()),
                body: opt_str(p, "body").map_or(String::new(), |v| v.chars().take(netlabcraft_model::services::MAX_MAIL).collect()),
            };
            let server = s.sim.dns_resolve(&s.topology, d, &account.outgoing).map_err(|e| EngineError::Failed(e.to_string()))?;
            let r = s.sim.smtp_send(&s.topology, d, server, &mail);
            // Messages for other domains go on to their servers.
            let relayed = s.sim.relay_mail(&s.topology);
            apply_mail_changes(s);
            let done = r.map_err(|e| EngineError::Failed(e.to_string()))?;
            let relayed: Vec<Value> = relayed
                .iter()
                .map(|x| {
                    let server = s.topology.device(x.server).map(|d| d.name.clone()).unwrap_or_default();
                    match &x.result {
                        Ok(a) => json!({"server": server, "to": x.to, "answer": a}),
                        Err(e) => json!({"server": server, "to": x.to, "error": e}),
                    }
                })
                .collect();
            Ok(json!({"answer": done, "relayed": relayed}))
        }
        "mail.receive" => {
            let d = device_param(s, p, "device")?;
            let account = s.topology.device(d).and_then(|x| x.mail.clone()).ok_or_else(|| bad("set up the Email app first (mail.configure)"))?;
            let server = s.sim.dns_resolve(&s.topology, d, &account.incoming).map_err(|e| EngineError::Failed(e.to_string()))?;
            let r = s.sim.pop3_fetch(&s.topology, d, server, &account.user, &account.password);
            apply_mail_changes(s);
            let got = r.map_err(|e| EngineError::Failed(e.to_string()))?;
            let n = got.len();
            if n > 0 {
                if let Some(x) = s.topology.device_mut(d) {
                    x.inbox.extend(got);
                    let over = x.inbox.len().saturating_sub(netlabcraft_model::services::MAX_MAILS);
                    x.inbox.drain(..over);
                }
                s.changed();
            }
            Ok(json!({"received": n}))
        }
        "wlc.wlan" | "lap.controller" => {
            let d = device_param(s, p, "device")?;
            let line = if id == "wlc.wlan" {
                let n = p.get("id").and_then(Value::as_u64).ok_or_else(|| bad("give the WLAN's id (1-16)"))?;
                if p.get("remove").and_then(Value::as_bool).unwrap_or(false) {
                    format!("no wlan {n}")
                } else {
                    let ssid = str_param(p, "ssid")?.trim().to_string();
                    let sec = match opt_str(p, "security").unwrap_or("open") {
                        "open" => "open".to_string(),
                        "psk" => format!("psk {}", str_param(p, "key")?.trim()),
                        "enterprise" => "enterprise".to_string(),
                        other => return Err(bad(format!("security `{other}`: open, psk or enterprise"))),
                    };
                    if ssid.contains(char::is_whitespace) {
                        return Err(bad("an SSID here has no spaces"));
                    }
                    let vlan = match p.get("vlan").filter(|v| !v.is_null()) {
                        Some(v) => format!(" vlan {}", v.as_u64().ok_or_else(|| bad("vlan is a number, 1-4094"))?),
                        None => String::new(),
                    };
                    let central = match opt_str(p, "switching").unwrap_or("local") {
                        "local" => "",
                        "central" => " central",
                        other => return Err(bad(format!("switching `{other}`: local or central"))),
                    };
                    format!("wlan {n} {ssid} {sec}{vlan}{central}")
                }
            } else {
                match opt_str(p, "address").map(str::trim).filter(|a| !a.is_empty()) {
                    Some(a) => format!("capwap controller {a}"),
                    None => "no capwap controller".to_string(),
                }
            };
            let mut t = s.topology.clone();
            let failed = netlabcraft_ios::apply_config(&mut t, &mut s.sim, d, &line);
            if let Some(f) = failed.first() {
                return Err(bad(f.clone()));
            }
            s.checkpoint();
            s.topology = t;
            s.changed();
            let joined = s.topology.lap_controller(d).and_then(|w| s.topology.device(w)).map(|w| w.name.clone());
            Ok(json!({"config": line, "joined": joined}))
        }
        "home.settings" => {
            let d = device_param(s, p, "device")?;
            let pair = |a: &str, b: &str| -> Option<(String, String)> { Some((opt_str(p, a)?.to_string(), opt_str(p, b)?.to_string())) };
            let settings = crate::home::Settings {
                lan: pair("lan", "lan_mask"),
                dhcp: p.get("dhcp").and_then(Value::as_bool),
                dhcp_start: opt_str(p, "dhcp_start").map(str::to_string),
                internet: pair("internet", "internet_mask").and_then(|(a, m)| Some((a, m, opt_str(p, "gateway")?.to_string()))),
                internet_dhcp: p.get("internet_dhcp").and_then(Value::as_bool).unwrap_or(false),
            };
            let config = crate::home::settings_config(&s.topology, d, &settings).map_err(bad)?;
            let mut t = s.topology.clone();
            let failed = netlabcraft_ios::apply_config(&mut t, &mut s.sim, d, &config);
            if let Some(f) = failed.first() {
                return Err(bad(format!("the router refused a setting: {f}")));
            }
            s.checkpoint();
            s.topology = t;
            s.changed();
            Ok(json!({"config": config}))
        }
                        "device.wireless" => {
            let d = device_param(s, p, "device")?;
            let dev = s.topology.device(d).ok_or_else(|| bad("no such device"))?;
            let port = match opt_str(p, "port") {
                Some(n) => port_param(s, d, n)?,
                None => dev.ports.iter().position(|x| netlabcraft_model::ifname::is_wireless(&x.name)).ok_or_else(|| bad(format!("{} has no radio", dev.name)))?,
            };
            if !dev.ports.get(port).is_some_and(|x| netlabcraft_model::ifname::is_wireless(&x.name)) {
                return Err(bad("that port is not a radio"));
            }
            let ssid: String = str_param(p, "ssid")?.trim().chars().take(32).collect();
            let key = opt_str(p, "key").map(str::to_string).filter(|k| !k.is_empty());
            if let Some(k) = &key
                && !(8..=63).contains(&k.chars().count())
            {
                return Err(bad("a WPA2 passphrase has 8-63 characters"));
            }
            let channel = p.get("channel").and_then(Value::as_u64).map_or(6, |c| c.clamp(1, 13) as u8);
            // WPA2-Enterprise login (hosts).
            let user: String = opt_str(p, "user").unwrap_or("").trim().chars().take(64).collect();
            let password: String = opt_str(p, "password").unwrap_or("").chars().take(64).collect();
            if !user.is_empty() && !dev.kind.is_host() {
                return Err(bad("only hosts log in to a WPA2-Enterprise network; set WLANs on the controller (device.wlan)"));
            }
            let login = (!user.is_empty()).then_some(netlabcraft_model::wlan::WifiLogin { user, password });
            let wifi = (!ssid.is_empty()).then_some(netlabcraft_model::Wifi { ssid, key, channel, login });
            s.checkpoint();
            if let Some(x) = s.topology.device_mut(d).and_then(|x| x.ports.get_mut(port)) {
                x.wifi = wifi;
            }
            s.changed();
            let end = netlabcraft_model::End { device: d, port };
            let ap = s.topology.wifi_ap_of(end);
            let problem = s.topology.wifi_attempt(end).and_then(|a| a.outcome.err());
            Ok(json!({"associated_with": ap.map(|e| s.topology.end_label(e)), "problem": problem}))
        }
                "activity.show" => {
            let mut a = s.topology.activity.clone();
            // A locked activity doesn't show its checks: their labels may give the answers away.
            if let Some(x) = a.as_mut().filter(|x| x.locked()) {
                x.checks.clear();
                x.lock = "set".into();
                x.assessment.clear();
            }
            // The networks are large: they are reached with their own commands.
            let answer = a.as_mut().and_then(|x| x.answer.take()).map(|x| x.scheme);
            let initial = a.as_mut().and_then(|x| x.initial.take()).is_some();
            let text = a.as_ref().map(|x| x.resolve(x.instructions_in(opt_str(p, "lang").unwrap_or(""))));
            Ok(json!({
                "answer": answer,
                "initial": initial,
                "languages": a.as_ref().map(netlabcraft_model::activity::Activity::languages),
                "text": text,
                "id": a.as_ref().map(crate::instructor::activity_id),
                "locks_active": a.as_ref().is_some_and(netlabcraft_model::activity::Activity::locks_active),
                "activity": a,
                "total_points": s.topology.activity.as_ref().map_or(0, netlabcraft_model::activity::Activity::total_points),
                "checks": s.topology.activity.as_ref().map_or(0, |x| x.checks.len()),
                "locked": s.topology.activity.as_ref().is_some_and(netlabcraft_model::activity::Activity::locked),
            }))
        }
        "activity.set" | "activity.capture" | "activity.add_check" | "activity.remove_check" | "activity.clear" | "activity.lock"
            if s.topology.activity.as_ref().is_some_and(netlabcraft_model::activity::Activity::locked) =>
        {
            Err(bad("the activity is locked: `activity.unlock` with the instructor's password first"))
        }
        "activity.start" => {
            let who: String = str_param(p, "student")?.trim().chars().take(64).collect();
            if who.is_empty() {
                return Err(bad("give the student's name"));
            }
            let mut t = s.topology.clone();
            let a = t.activity.as_mut().ok_or_else(|| bad("this lab has no activity"))?;
            if a.student.is_some() {
                return Err(bad("this activity was already started"));
            }
            a.student = Some(who);
            let values: serde_json::Map<String, Value> = a.values().into_iter().map(|(k, v)| (k, json!(v))).collect();
            let instructions = a.resolve(&a.instructions);
            s.checkpoint();
            s.topology = t;
            s.changed();
            Ok(json!({"values": values, "instructions": instructions}))
        }
        "activity.lock" => {
            let password = str_param(p, "password")?;
            if password.is_empty() {
                return Err(bad("the password can't be empty"));
            }
            let mut t = s.topology.clone();
            let a = t.activity.as_mut().ok_or_else(|| bad("this lab has no activity"))?;
            a.lock = crate::instructor::lock_hash(a, password);
            // The answer network goes under the password.
            crate::instructor::reseal(a, None, Some(password)).map_err(EngineError::Failed)?;
            s.checkpoint();
            s.topology = t;
            s.changed();
            Ok(json!({"locked": true}))
        }
        "activity.unlock" => {
            let password = str_param(p, "password")?;
            let mut t = s.topology.clone();
            let a = t.activity.as_mut().ok_or_else(|| bad("this lab has no activity"))?;
            if !a.locked() {
                return Ok(json!({"locked": false}));
            }
            if !crate::instructor::lock_matches(a, password) {
                return Err(bad("wrong password"));
            }
            a.lock.clear();
            crate::instructor::reseal(a, Some(password), None).map_err(EngineError::Failed)?;
            s.checkpoint();
            s.topology = t;
            s.changed();
            Ok(json!({"locked": false}))
        }
        "activity.set" => {
            let mut t = s.topology.clone();
            let salt_title = opt_str(p, "title").unwrap_or("Activity").to_string();
            let fresh_salt = crate::activity::new_salt(&t, &salt_title);
            let a = t.activity.get_or_insert_with(|| netlabcraft_model::activity::Activity { title: "Activity".into(), salt: fresh_salt, ..Default::default() });
            if let Some(title) = opt_str(p, "title") {
                a.title = title.chars().take(200).collect();
            }
            if let Some(vars) = p.get("variables").and_then(Value::as_object) {
                use netlabcraft_model::activity::{MAX_VALUES, MAX_VARIABLES, Variable};
                a.variables = vars
                    .iter()
                    .take(MAX_VARIABLES)
                    .filter(|(k, _)| !k.trim().is_empty())
                    .map(|(k, v)| Variable {
                        name: k.trim().chars().take(64).collect(),
                        values: v.as_array().into_iter().flatten().filter_map(Value::as_str).take(MAX_VALUES).map(|x| x.chars().take(64).collect()).collect(),
                    })
                    .filter(|v| !v.values.is_empty())
                    .collect();
            }
            if let Some(text) = opt_str(p, "instructions") {
                a.instructions = text.chars().take(netlabcraft_model::activity::MAX_TEXT).collect();
            }
                        if let Some(m) = p.get("time_limit").and_then(Value::as_u64) {
                a.time_limit = u32::try_from(m).unwrap_or(u32::MAX).min(netlabcraft_model::activity::MAX_TIME_LIMIT);
            }
            if let Some(b) = p.get("score_only").and_then(Value::as_bool) {
                a.score_only = b;
            }
            if let Some(h) = p.get("hidden").and_then(Value::as_bool) {
                if !h && a.hidden && a.checks.iter().any(|c| matches!(c.kind, netlabcraft_model::activity::CheckKind::ConfigHashed { .. })) {
                    return Err(bad("hidden answers can't be shown again: capture the answers again with `hidden` off"));
                }
                if h && !a.hidden {
                    // Hide what is already there.
                    let salt = a.salt.clone();
                    for c in &mut a.checks {
                        if let netlabcraft_model::activity::CheckKind::Config { device, line } = &c.kind {
                            let hash = netlabcraft_model::activity::line_hash(&salt, *device, line);
                            c.kind = netlabcraft_model::activity::CheckKind::ConfigHashed { device: *device, hash };
                            c.label = c.label.split(':').next().map_or("configuration item".to_string(), |d| format!("{d}: configuration item"));
                        }
                        if let netlabcraft_model::activity::CheckKind::Link { a: x, a_port, b: y, b_port } = &c.kind {
                            let (device, text) = netlabcraft_model::activity::link_text(*x, a_port, *y, b_port);
                            let hash = netlabcraft_model::activity::line_hash(&salt, device, &text);
                            c.kind = netlabcraft_model::activity::CheckKind::LinkHashed { device, hash };
                            c.label = format!("{}: cable", c.section);
                        }
                    }
                }
                a.hidden = h;
                crate::instructor::reseal(a, None, None).map_err(EngineError::Failed)?;
            }
            s.checkpoint();
            s.topology = t;
            s.changed();
            Ok(json!({"title": s.topology.activity.as_ref().map(|a| a.title.clone())}))
        }
        "activity.capture" => {
            let mut t = s.topology.clone();
            let Some(a) = t.activity.as_ref() else { return Err(bad("no activity yet: `activity.set` first")) };
            let mut checks = crate::activity::capture(&s.topology, a.hidden, &a.salt);
            let kept: Vec<_> = a
                .checks
                .iter()
                .filter(|c| matches!(c.kind, netlabcraft_model::activity::CheckKind::Ping { .. } | netlabcraft_model::activity::CheckKind::Output { .. }))
                .cloned()
                .collect();
            checks.extend(kept);
            checks.truncate(netlabcraft_model::activity::MAX_CHECKS);
            let n = checks.len();
            if let Some(a) = t.activity.as_mut() {
                a.checks = checks;
            }
            s.checkpoint();
            s.topology = t;
            s.changed();
            Ok(json!({"checks": n}))
        }
        "activity.add_check" => {
            use netlabcraft_model::activity::{Check, CheckKind, normalize};
            let ty = str_param(p, "type")?;
            let dev_key = if ty == "ping" { "from" } else { "device" };
            let d = device_param(s, p, dev_key)?;
            let name = s.topology.device(d).map_or(String::new(), |x| x.name.clone());
            let hidden = s.topology.activity.as_ref().is_some_and(|a| a.hidden);
            let salt = s.topology.activity.as_ref().map(|a| a.salt.clone()).unwrap_or_default();
            let (label, kind) = match ty {
                "config" => {
                    let line = str_param(p, "line")?;
                    let flat = match line.split_once('|') {
                        Some((b, l)) => format!("{} | {}", normalize(b), normalize(l)),
                        None => normalize(line),
                    };
                    if hidden {
                        (format!("{name}: configuration item"), CheckKind::ConfigHashed { device: d, hash: netlabcraft_model::activity::line_hash(&salt, d, &flat) })
                    } else {
                        (format!("{name}: {flat}"), CheckKind::Config { device: d, line: flat })
                    }
                }
                "ping" => {
                    let to = str_param(p, "to")?;
                    to.parse::<std::net::IpAddr>().map_err(|_| bad(format!("`{to}` is not an address")))?;
                    let succeed = p.get("succeed").and_then(Value::as_bool).unwrap_or(true);
                    (format!("{name} {} {to}", if succeed { "reaches" } else { "can't reach" }), CheckKind::Ping { from: d, to: to.to_string(), succeed })
                }
                "host-address" => {
                    let address = str_param(p, "address")?;
                    address.parse::<netlabcraft_core::Ipv4Net>().map_err(|e| bad(e.to_string()))?;
                    let gateway = opt_str(p, "gateway").map(str::to_string);
                    (format!("{name}: address {address}"), CheckKind::HostAddress { device: d, address: address.to_string(), gateway })
                }
                "output" => {
                    let command = str_param(p, "command")?.chars().take(512).collect::<String>();
                    let contains = str_param(p, "contains")?.chars().take(512).collect::<String>();
                    (format!("{name}: `{command}` shows \"{contains}\""), CheckKind::Output { device: d, command, contains })
                }
                other => return Err(bad(format!("unknown check type `{other}` (config, ping, host-address, output)"))),
            };
            let label = opt_str(p, "label").map_or(label, |l| l.chars().take(300).collect());
                        let points = p.get("points").and_then(Value::as_u64).map_or(1, |n| n.clamp(1, 1_000) as u32);
            let feedback = opt_str(p, "feedback").map_or(String::new(), |f| f.chars().take(1_000).collect());
            let section = opt_str(p, "section").map(str::trim).filter(|f| !f.is_empty()).map_or(name.clone(), |f| f.chars().take(64).collect());
            let mut t = s.topology.clone();
            let fresh_salt = crate::activity::new_salt(&t, "Activity");
            let a = t.activity.get_or_insert_with(|| netlabcraft_model::activity::Activity { title: "Activity".into(), salt: fresh_salt, ..Default::default() });
            if a.checks.len() >= netlabcraft_model::activity::MAX_CHECKS {
                return Err(bad("too many checks"));
            }
            a.checks.push(Check { label, points, section, feedback, kind });
            let n = a.checks.len();
            s.checkpoint();
            s.topology = t;
            s.changed();
            Ok(json!({"checks": n}))
        }
        "activity.remove_check" => {
            let i = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("`index` is required"))? as usize;
            let mut t = s.topology.clone();
            let a = t.activity.as_mut().ok_or_else(|| bad("no activity"))?;
            if i >= a.checks.len() {
                return Err(bad(format!("no check {i}")));
            }
            a.checks.remove(i);
            s.checkpoint();
            s.topology = t;
            s.changed();
            Ok(json!({}))
        }
        "activity.clear" => {
            let mut t = s.topology.clone();
            t.activity = None;
            s.checkpoint();
            s.topology = t;
            s.changed();
            Ok(json!({}))
        }
        "activity.check" => {
            let a = s.topology.activity.as_ref().ok_or_else(|| bad("this lab has no activity"))?;
            if a.needs_student() {
                return Err(bad("start the activity first: `activity.start` with the student's name"));
            }
            let results = crate::activity::evaluate(&s.topology, &s.sim, &a.resolved_checks(), &a.salt, a.hidden);
            let total: u32 = results.iter().map(|r| r.points).sum();
            let score: u32 = results.iter().filter(|r| r.passed).map(|r| r.points).sum();
                        let percent = (score * 100).checked_div(total).unwrap_or(0);
            let detail = !(a.locked() && a.score_only);
            let shown: Vec<Value> = if detail {
                results.iter().map(|r| json!({"label": r.label, "points": r.points, "passed": r.passed, "feedback": r.feedback, "section": r.section, "expected": r.expected, "actual": r.actual})).collect()
            } else {
                Vec::new()
            };
            // Per section, in the order sections first appear.
            let mut sections: Vec<(String, u32, u32)> = Vec::new();
            for r in &results {
                let i = match sections.iter().position(|x| x.0 == r.section) {
                    Some(i) => i,
                    None => {
                        sections.push((r.section.clone(), 0, 0));
                        sections.len() - 1
                    }
                };
                if let Some(x) = sections.get_mut(i) {
                    x.2 += r.points;
                    if r.passed {
                        x.1 += r.points;
                    }
                }
            }
            let sections: Vec<Value> = if detail {
                sections.iter().map(|(n, sc, tot)| json!({"name": n, "score": sc, "total": tot, "percent": (sc * 100).checked_div(*tot).unwrap_or(0)})).collect()
            } else {
                Vec::new()
            };
            Ok(json!({"results": shown, "sections": sections, "score": score, "total": total, "percent": percent, "detail": detail}))
        }
        "activity.reset" => {
            let path = s.path.clone();
            // An activity with an initial network starts over from it (the student's name
            // and the activity as they are now are kept).
            if let Some(a) = s.topology.activity.clone()
                && let Some(initial) = crate::instructor::initial_network(&a)
            {
                let mut t = initial.map_err(EngineError::Failed)?;
                t.activity = Some(a);
                s.replace(t, path);
                return Ok(json!({"from": "initial"}));
            }
            let start = s.activity_start.clone().ok_or_else(|| bad("this lab wasn't opened as an activity"))?;
            s.replace(start, path);
            Ok(json!({"from": "opened"}))
        }
        "note.add" => {
            let (x, y) = (f32_param(p, "x").unwrap_or(0.0), f32_param(p, "y").unwrap_or(0.0));
                        let shape = match opt_str(p, "shape") {
                Some(sh) => netlabcraft_model::Shape::from_id(sh).ok_or_else(|| bad(format!("unknown shape `{sh}` (text, rectangle, ellipse, line, freehand, polygon)")))?,
                None => netlabcraft_model::Shape::Text,
            };
            let mut t = s.topology.clone();
            let id = match shape {
                Shape::Text => t.add_annotation(x, y, opt_str(p, "text").unwrap_or("Note"))?,
                Shape::Freehand | Shape::Polygon => t.add_path(shape, &points_param(p)?, opt_str(p, "color"), s.view)?,
                _ => t.add_drawing(shape, x, y, f32_param(p, "w").unwrap_or(80.0), f32_param(p, "h").unwrap_or(60.0), opt_str(p, "color"))?,
            };
            t.annotation_mut(id)?.cluster = s.view;
            s.checkpoint();
            s.topology = t;
            s.changed();
            Ok(json!({"id": id}))
        }
        "note.edit" | "note.move" | "note.remove" | "note.color" => {
            let id = p.get("id").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).ok_or_else(|| bad("`id` (note number) is required"))?;
            let mut t = s.topology.clone();
            t.annotation_mut(id)?;
            let mut transient = false;
            match cmd {
                "note.edit" => t.annotation_mut(id)?.text = str_param(p, "text")?.chars().take(netlabcraft_model::MAX_ANNOTATION_TEXT).collect(),
                "note.move" => {
                    let (x, y) = (f32_param(p, "x").ok_or_else(|| bad("`x` is required"))?, f32_param(p, "y").ok_or_else(|| bad("`y` is required"))?);
                    let a = t.annotation_mut(id)?;
                    a.x = x;
                    a.y = y;
                    transient = p.get("transient").and_then(Value::as_bool).unwrap_or(false);
                }
                "note.color" => {
                    let c = str_param(p, "color")?;
                    netlabcraft_model::parse_color(c).ok_or_else(|| bad(format!("`{c}` is not a #rrggbb colour")))?;
                    t.annotation_mut(id)?.color = Some(c.to_string());
                }
                _ => t.annotations.retain(|a| a.id != id),
            }
            if !transient {
                s.checkpoint();
            }
            s.topology = t;
            s.dirty = true;
            s.revision += 1;
            Ok(Value::Null)
        }
        "cluster.create" => {
            let mut devices = Vec::new();
            for v in list_param(p, "devices")? {
                devices.push(device_param(s, &json!({"d": v}), "d")?);
            }
            let clusters: Vec<u32> = match p.get("clusters") {
                None | Some(Value::Null) => Vec::new(),
                Some(_) => list_param(p, "clusters")?.iter().map(|v| id_value(v, "clusters")).collect::<Result<_>>()?,
            };
            let mut t = s.topology.clone();
            let id = t.create_cluster(&devices, &clusters, opt_str(p, "name"))?;
            s.checkpoint();
            s.topology = t;
            s.changed();
            let name = s.topology.cluster(id).map(|c| c.name.clone()).unwrap_or_default();
            Ok(json!({"id": id, "name": name}))
        }
        "cluster.rename" | "cluster.move" | "cluster.dissolve" => {
            let id = cluster_param(s, p)?;
            let mut t = s.topology.clone();
            let mut transient = false;
            match cmd {
                "cluster.rename" => t.rename_cluster(id, str_param(p, "name")?)?,
                "cluster.move" => {
                    let (x, y) = (f32_param(p, "x").ok_or_else(|| bad("`x` is required"))?, f32_param(p, "y").ok_or_else(|| bad("`y` is required"))?);
                    let c = t.cluster_mut(id)?;
                    c.x = x;
                    c.y = y;
                    transient = p.get("transient").and_then(Value::as_bool).unwrap_or(false);
                }
                _ => {
                    let c = t.dissolve_cluster(id)?;
                    if s.view == Some(id) {
                        s.view = c.parent;
                    }
                }
            }
            if !transient {
                s.checkpoint();
            }
            s.topology = t;
            s.dirty = true;
            s.revision += 1;
            if s.view.is_some_and(|c| s.topology.cluster(c).is_none()) {
                s.view = None;
            }
            Ok(Value::Null)
        }
        "view.physical" | "view.logical" | "view.enter_place" | "view.place_up" | "view.intercity" | "place.add" | "place.rename" | "place.move"
        | "place.remove" | "place.list" | "device.place" | "link.length" => physical(s, cmd, p),
        "device.power" | "device.module.add" | "device.module.remove" | "device.module.list" => crate::hardware::run(s, cmd, p),
        "device.adapter" | "device.power.source" | "phone.configure" | "cloud.map" => crate::families::run(s, cmd, p),
        "phone.status" | "phone.dial" | "phone.answer" | "phone.hangup" => crate::voice::run(s, cmd, p),
        "iot.sync" | "iot.configure" | "iot.server" | "iot.switch" => crate::iot::run(s, cmd, p),
        "board.program" | "board.status" | "board.wire" => crate::board::run(s, cmd, p),
        "pdu.simple" | "pdu.complex" | "pdu.fire" | "pdu.fire_all" | "pdu.delete" | "pdu.list" | "scenario.list" | "scenario.new" | "scenario.delete"
        | "scenario.select" | "sim.event" => crate::pdu::run(s, cmd, p),
        "pc.http_request" => crate::automation::http_request(s, p),
        "pc.http_saved" => crate::automation::http_saved(s, p),
        "netconf.session" => crate::netconf::session(s, p),
        "controller.devices" | "controller.push" => crate::controller::run(s, cmd, p),
        "view.enter_cluster" => {
            s.view = Some(cluster_param(s, p)?);
            s.revision += 1;
            Ok(view_json(s))
        }
        "view.up" | "view.root" => {
            s.view = if cmd == "view.up" { s.view.and_then(|c| s.topology.cluster(c)).and_then(|c| c.parent) } else { None };
            s.revision += 1;
            Ok(view_json(s))
        }
        "device.remove" => {
            let d = device_param(s, p, "device")?;
            s.checkpoint();
            s.topology.remove_device(d)?;
            s.consoles.remove(&d);
            s.changed();
            Ok(Value::Null)
        }
        "device.move" => {
            let d = device_param(s, p, "device")?;
            let (x, y) = (f32_param(p, "x").ok_or_else(|| bad("`x` is required"))?, f32_param(p, "y").ok_or_else(|| bad("`y` is required"))?);
            if !p.get("transient").and_then(Value::as_bool).unwrap_or(false) {
                s.checkpoint();
            }
            if let Some(dev) = s.topology.device_mut(d) {
                dev.x = x;
                dev.y = y;
            }
            s.dirty = true;
            s.revision += 1;
            Ok(Value::Null)
        }
        "device.rename" => {
            let d = device_param(s, p, "device")?;
            let name = str_param(p, "name")?;
            let mut t = s.topology.clone();
            t.rename(d, name)?;
            s.checkpoint();
            s.topology = t;
            s.changed();
            Ok(Value::Null)
        }
        "device.list" => Ok(Value::Array(
            s.topology
                .devices
                .iter()
                .map(|d| {
                    let ports: Vec<Value> = d
                        .ports
                        .iter()
                        .enumerate()
                        .map(|(i, port)| {
                            let end = End { device: d.id, port: i };
                            json!({
                                "name": port.name,
                                "ip": port.ip.map(|ip| ip.to_string()),
                                "shutdown": port.shutdown,
                                "up": netlabcraft_sim::port_up(&s.topology, end),
                                "problem": netlabcraft_sim::cable_problem(&s.topology, end),
                                "peer": s.topology.peer(end).map(|e| s.topology.end_label(e)),
                                "medium": d.port_medium(i).map(|m| m.id()),
                                "cable": s.topology.link_at(end).map(|l| l.cable.id()),
                            })
                        })
                        .collect();
                    let console = d.console_port_name().map(|port| json!({"port": port, "peer": console_peer_label(&s.topology, d.id)}));
                    json!({"id": d.id.0, "name": d.name, "kind": d.kind.id(), "x": d.x, "y": d.y, "cluster": s.topology.cluster_of(d.id), "place": s.topology.home_of(d.id).map(|h| h.place), "gateway": d.gateway.map(|g| g.to_string()), "ports": ports, "console": console})
                })
                .collect(),
        )),
        "link.add" => {
            let a = device_param(s, p, "a")?;
            let b = device_param(s, p, "b")?;
            let cable = match opt_str(p, "cable") {
                Some(c) => Cable::parse(c).ok_or_else(|| {
                    bad(format!(
                        "`cable` must be one of: {}",
                        Cable::ALL.iter().map(|c| c.id()).collect::<Vec<_>>().join(", ")
                    ))
                })?,
                None => Cable::Auto,
            };
            if cable == Cable::IotCustom {
                return crate::board::link_add(s, a, b, p);
            }
            let console_named = [opt_str(p, "a_port"), opt_str(p, "b_port")].into_iter().flatten().any(is_console_port_name);
            if cable == Cable::Console || console_named {
                if a == b {
                    return Err(EngineError::Failed("a cable needs two different devices".into()));
                }
                if !matches!(cable, Cable::Console | Cable::Auto) {
                    return Err(EngineError::Failed(format!(
                        "a {} cable doesn't fit a Console or RS-232 port: pick the console cable",
                        cable.label().to_lowercase()
                    )));
                }
                let mut t = s.topology.clone();
                let c = t.connect_console(a, b)?;
                s.checkpoint();
                s.topology = t;
                s.changed();
                let name = |id| s.topology.device(id).map_or(String::new(), |d| d.name.clone());
                return Ok(json!({
                    "a": format!("{} {RS232_PORT}", name(c.pc)),
                    "b": format!("{} {CONSOLE_PORT}", name(c.device)),
                    "cable": "console",
                    "resolved": "console",
                }));
            }
            let pa = match opt_str(p, "a_port") {
                Some(n) => port_param(s, a, n)?,
                None => s.topology.free_port_for_cable(a, cable, None).ok_or_else(|| {
                    EngineError::Failed(format!("{} has no free port for a {} cable", s.topology.end_label(End { device: a, port: 0 }), cable.label().to_lowercase()))
                })?,
            };
            let pb = match opt_str(p, "b_port") {
                Some(n) => port_param(s, b, n)?,
                None => s
                    .topology
                    .free_port_for_cable(b, cable, Some(End { device: a, port: pa }))
                    .ok_or_else(|| EngineError::Failed(format!("the second device has no free port this {} cable fits", cable.label().to_lowercase())))?,
            };
            let mut t = s.topology.clone();
            let (ea, eb) = (End { device: a, port: pa }, End { device: b, port: pb });
            t.connect_with(ea, eb, cable)?;
            s.checkpoint();
            s.topology = t;
            s.changed();
            let resolved = s.topology.link_at(ea).map_or(cable, |l| s.topology.resolved_cable(l));
            Ok(json!({"a": s.topology.end_label(ea), "b": s.topology.end_label(eb), "cable": cable.id(), "resolved": resolved.id()}))
        }
        "link.list" => {
            let t = &s.topology;
            let mut out: Vec<Value> = t
                .links
                .iter()
                .map(|l| {
                    json!({
                        "a": t.end_label(l.a),
                        "b": t.end_label(l.b),
                        "cable": l.cable.id(),
                        "resolved": t.resolved_cable(l).id(),
                        "up": netlabcraft_sim::port_up(t, l.a) && netlabcraft_sim::port_up(t, l.b),
                        "problem": netlabcraft_sim::cable_problem(t, l.a),
                    })
                })
                .collect();
            let name = |id| t.device(id).map_or(String::new(), |d| d.name.clone());
            out.extend(t.consoles.iter().map(|c| {
                let off = t.device(c.pc).is_some_and(|d| d.powered_off) || t.device(c.device).is_some_and(|d| d.powered_off);
                json!({
                    "a": format!("{} {RS232_PORT}", name(c.pc)),
                    "b": format!("{} {CONSOLE_PORT}", name(c.device)),
                    "cable": "console",
                    "resolved": "console",
                    "up": !off,
                    "problem": Value::Null,
                })
            }));
            out.extend(crate::board::links(s));
            Ok(Value::Array(out))
        }
        "pc.terminal" => {
            let pc = device_param(s, p, "pc")?;
            let dev = s.topology.device(pc).ok_or_else(|| EngineError::Failed("no such device".into()))?;
            if !dev.has_rs232() {
                return Err(EngineError::Failed(format!("{} has no {RS232_PORT} port: the Terminal runs on a PC or laptop", dev.name)));
            }
            let pc_name = dev.name.clone();
            let target = s.topology.console_peer(pc).ok_or_else(|| {
                EngineError::Failed(format!(
                    "{pc_name} has no console cable: join its {RS232_PORT} port to a router's or switch's {CONSOLE_PORT} port (link.add with \"cable\": \"console\")"
                ))
            })?;
            let target_name = s.topology.device(target).map_or(String::new(), |d| d.name.clone());
            let prompt = prompt_of(s, target);
            let log = s.console(target).log.clone();
            Ok(json!({"device": target_name, "prompt": prompt, "log": log}))
        }
        "link.remove" => {
            let d = device_param(s, p, "device")?;
            if let Some(dev) = s.topology.device(d).filter(|x| netlabcraft_model::board::is_board(x))
                && let Some(pin) = netlabcraft_model::board::find_pin(dev, str_param(p, "port")?)
            {
                return crate::board::unwire(s, d, &pin);
            }
            if is_console_port_name(str_param(p, "port")?) {
                if s.topology.console_at(d).is_none() {
                    return Err(EngineError::Failed(format!("{} has no console cable", s.topology.device(d).map_or("?", |x| x.name.as_str()))));
                }
                s.checkpoint();
                s.topology.disconnect_console(d);
                s.changed();
                return Ok(Value::Null);
            }
            let port = port_param(s, d, str_param(p, "port")?)?;
            let end = End { device: d, port };
            if s.topology.link_at(end).is_none() {
                return Err(EngineError::Failed(format!("{} has no cable", s.topology.end_label(end))));
            }
            s.checkpoint();
            s.topology.disconnect(end);
            s.changed();
            Ok(Value::Null)
        }
        "cli.exec" => {
            let d = device_param(s, p, "device")?;
            let line = str_param(p, "line")?;
            Ok(cli_line(s, d, line))
        }
        "cli.script" => {
            let d = device_param(s, p, "device")?;
            let lines: Vec<String> = match p.get("lines") {
                Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
                Some(Value::String(t)) => t.lines().map(str::to_string).collect(),
                _ => return Err(bad("`lines` (array of text, or text) is required")),
            };
            if lines.len() > 10_000 {
                return Err(bad("at most 10000 lines per script"));
            }
            let outputs: Vec<Value> = lines.iter().map(|l| cli_line(s, d, l)).map(|v| v.get("output").cloned().unwrap_or(Value::Null)).collect();
            Ok(json!({"outputs": outputs, "prompt": prompt_of(s, d)}))
        }
        "cli.help" => {
            let d = device_param(s, p, "device")?;
            let line = opt_str(p, "line").unwrap_or("");
            let cli = s.console(d).cli.clone();
            Ok(json!({"help": netlabcraft_ios::help_line(&s.topology, d, &cli, line)}))
        }
        "cli.complete" => {
            let d = device_param(s, p, "device")?;
            let line = str_param(p, "line")?;
            let cli = s.console(d).cli.clone();
            Ok(json!({"line": netlabcraft_ios::complete_line(&s.topology, d, &cli, line)}))
        }
        "cli.prompt" => {
            let d = device_param(s, p, "device")?;
            Ok(json!({"prompt": prompt_of(s, d)}))
        }
        "net.ping" => {
            let d = device_param(s, p, "from")?;
                        let to = str_param(p, "to")?;
            let count = p.get("count").and_then(Value::as_u64).map_or(4, |c| c.clamp(1, 1_000) as u16);
            let opts = PingOptions { count, ..PingOptions::default() };
            let probes = match to.parse::<std::net::Ipv6Addr>() {
                Ok(dst6) => s.sim.ping6(&s.topology, d, dst6, opts),
                Err(_) => s.sim.ping(&s.topology, d, parse_ip(to).map_err(|e| bad(e.to_string()))?, opts),
            }
            .map_err(|e| EngineError::Failed(e.to_string()))?;
            s.revision += 1;
            let marks: String = probes.iter().map(|p| p.mark()).collect();
            let replies = probes.iter().filter(|p| matches!(p, Probe::Reply { .. })).count();
            Ok(json!({"marks": marks, "sent": probes.len(), "replies": replies, "probes": probes.iter().map(probe_json).collect::<Vec<_>>()}))
        }
        "net.traceroute" => {
            let d = device_param(s, p, "from")?;
                        let to = str_param(p, "to")?;
            let hops = match to.parse::<std::net::Ipv6Addr>() {
                Ok(dst6) => s.sim.traceroute6(&s.topology, d, dst6, 30, 2_000_000),
                Err(_) => s.sim.traceroute(&s.topology, d, parse_ip(to).map_err(|e| bad(e.to_string()))?, 30, 2_000_000),
            }
            .map_err(|e| EngineError::Failed(e.to_string()))?;
            s.revision += 1;
            Ok(json!({"hops": hops.iter().map(|h| json!({"ttl": h.ttl, "probes": h.probes.iter().map(probe_json).collect::<Vec<_>>()})).collect::<Vec<_>>()}))
        }
        "device.configure" => {
            let d = device_param(s, p, "device")?;
            let text = match p.get("lines") {
                Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"),
                Some(Value::String(t)) => t.clone(),
                _ => return Err(bad("`lines` (array of text, or text) is required")),
            };
            if text.len() > 1_000_000 {
                return Err(bad("configuration too long"));
            }
            if s.topology.device(d).is_some_and(|x| x.kind.is_host()) {
                return Err(bad("hosts are configured with `ipconfig` through cli.exec"));
            }
            let before = s.topology.clone();
            let errors = netlabcraft_ios::apply_config(&mut s.topology, &mut s.sim, d, &text);
            if s.topology != before {
                let after = std::mem::replace(&mut s.topology, before);
                s.checkpoint();
                s.topology = after;
                s.changed();
            }
            Ok(json!({"applied": text.lines().filter(|l| !l.trim().is_empty()).count(), "errors": errors}))
        }
        "sim.advance" => {
            let secs = p.get("seconds").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(30.0).clamp(0.0, 86_400.0);
            // Scheduled and periodic PDUs go out as their time comes.
            let fired = crate::pdu::advance(s, (secs * 1_000_000.0) as u64);
            Ok(json!({"time_us": s.sim.now_us(), "pdus_fired": fired}))
        }
        "sim.reset" => {
            s.sim.reset();
            s.revision += 1;
            Ok(Value::Null)
        }
        "sim.clear_events" => {
            s.sim.clear_trace();
            s.revision += 1;
            Ok(Value::Null)
        }
        "sim.events" => {
            let since = p.get("since").and_then(Value::as_u64).unwrap_or(0);
            let limit = p.get("limit").and_then(Value::as_u64).map_or(200, |l| l.clamp(1, 10_000)) as usize;
            let t = &s.topology;
            let evs: Vec<Value> = s
                .sim
                .trace()
                .iter()
                .filter(|e| e.id > since)
                .take(limit)
                .map(|e| json!({"id": e.id, "at_us": e.at_us, "from": t.end_label(e.from), "to": t.end_label(e.to), "protocol": e.protocol.name(), "summary": e.summary, "dropped": e.dropped}))
                .collect();
            Ok(Value::Array(evs))
        }
        "sim.status" => Ok(json!({
            "time_us": s.sim.now_us(),
            "events": s.sim.trace().len(),
            "storm": s.sim.storm_detected(),
            "constant_delay_us": s.sim.constant_delay(),
            "capture": s.sim.capture_filter(),
        })),
        "sim.constant_delay" => {
            let on = p.get("on").and_then(Value::as_bool).ok_or_else(|| bad("`on` (true or false) is required"))?;
            let us = match p.get("us") {
                None | Some(Value::Null) => netlabcraft_sim::simmode::DEFAULT_CONSTANT_DELAY_US,
                Some(v) => v
                    .as_u64()
                    .filter(|v| (1..=netlabcraft_sim::simmode::MAX_CONSTANT_DELAY_US).contains(v))
                    .ok_or_else(|| bad("`us` is a whole number of microseconds from 1 to 1000000"))?,
            };
            s.sim.set_constant_delay(on.then_some(us));
            s.revision += 1;
            Ok(json!({"on": on, "us": s.sim.constant_delay()}))
        }
        "sim.capture_filter" => {
            let on = p.get("on").and_then(Value::as_bool).ok_or_else(|| bad("`on` (true or false) is required"))?;
            let protocols = if on {
                let list = p.get("protocols").and_then(Value::as_array).ok_or_else(|| bad("`protocols` (a list of protocol names) is required"))?;
                if list.len() > netlabcraft_sim::simmode::MAX_CAPTURE_PROTOCOLS {
                    return Err(bad(format!("`protocols` names at most {} protocols", netlabcraft_sim::simmode::MAX_CAPTURE_PROTOCOLS)));
                }
                let names: Vec<String> = list.iter().filter_map(Value::as_str).map(|x| x.chars().take(32).collect()).collect();
                if names.len() != list.len() {
                    return Err(bad("`protocols` holds names such as \"ICMP\" or \"ARP\""));
                }
                Some(names)
            } else {
                None
            };
            s.sim.set_capture_filter(protocols);
            let removed = if p.get("prune").and_then(Value::as_bool).unwrap_or(false) { s.sim.prune_to_capture_filter() } else { 0 };
            s.revision += 1;
            Ok(json!({"on": on, "protocols": s.sim.capture_filter(), "removed": removed}))
        }
        "topology.get" => serde_json::to_value(&s.topology).map_err(|e| EngineError::Failed(e.to_string())),
        "sample.list" => Ok(Value::Array(sample::SAMPLES.iter().map(|x| json!({"id": x.id, "title": x.title, "description": x.description})).collect())),
        other => Err(EngineError::UnknownCommand(other.to_string())),
    }
}

/// A device made while looking inside a cluster goes into it; one made while looking at a
/// closet or building in the physical view stands there.
fn join_view(s: &Session, t: &mut netlabcraft_model::Topology, dev: DeviceId) {
    if let Some(c) = s.view
        && let Ok(c) = t.cluster_mut(c)
    {
        c.devices.push(dev);
    }
    if s.physical
        && let Some(place) = s.place_view
        && let Some((x, y)) = t.device(dev).map(|d| (d.x, d.y))
    {
        // A switch made while looking at a building's rooms stays in the default closet.
        let _ = t.set_home(dev, place, x, y);
    }
}

fn place_id(s: &Session, p: &Value, key: &str) -> Result<u32> {
    let id =
        p.get(key).and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).ok_or_else(|| bad(format!("`{key}` (place number) is required")))?;
    s.topology.place(id).map(|x| x.id).ok_or_else(|| EngineError::Failed(format!("no place {id}")))
}

/// Where the physical view looks: the place (or null for the Intercity map) and the path to it.
fn place_view_json(s: &Session) -> Value {
    let t = &s.topology;
    let path: Vec<Value> = t.place_path(s.place_view).iter().map(|x| json!({"id": x.id, "name": x.name, "kind": x.kind.id()})).collect();
    let kind = s.place_view.and_then(|x| t.place(x)).map(|x| x.kind.id());
    json!({"physical": s.physical, "place": s.place_view, "kind": kind, "path": path})
}

/// A free spot for a new icon on the map of `place` (none: the Intercity map): the first point
/// of a loose grid clear of the places and devices already there.
fn spot(t: &netlabcraft_model::Topology, place: Option<u32>) -> (f32, f32) {
    let taken: Vec<(f32, f32)> =
        t.places_in(place).iter().map(|x| (x.x, x.y)).chain(t.homes().iter().filter(|h| place == Some(h.place)).map(|h| (h.x, h.y))).collect();
    let grid = (0..40u16).map(|n| (160.0 + 180.0 * f32::from(n % 5), 140.0 + 170.0 * f32::from(n / 5)));
    let mut last = (160.0, 140.0);
    for (x, y) in grid {
        last = (x, y);
        if taken.iter().all(|(a, b)| (a - x).powi(2) + (b - y).powi(2) > 130.0f32.powi(2)) {
            break;
        }
    }
    last
}

/// The physical view: places, where devices stand, cable lengths.
fn physical(s: &mut Session, cmd: &str, p: &Value) -> Result<Value> {
    use netlabcraft_model::physical::PlaceKind;
    match cmd {
        "view.physical" | "view.logical" => {
            s.physical = cmd == "view.physical";
            s.revision += 1;
            Ok(place_view_json(s))
        }
        "view.enter_place" => {
            s.place_view = Some(place_id(s, p, "id")?);
            s.physical = true;
            s.revision += 1;
            Ok(place_view_json(s))
        }
        "view.place_up" | "view.intercity" => {
            s.place_view = if cmd == "view.place_up" { s.place_view.and_then(|x| s.topology.place(x)).and_then(|x| x.parent) } else { None };
            s.physical = true;
            s.revision += 1;
            Ok(place_view_json(s))
        }
        "place.add" => {
            let kind = str_param(p, "kind")?;
            let kind = PlaceKind::from_id(kind).ok_or_else(|| bad(format!("unknown place kind `{kind}` (city, building, closet)")))?;
            let parent = match p.get("parent") {
                None | Some(Value::Null) => {
                    // The place being looked at, when it is the right kind.
                    let viewed = s.place_view.and_then(|x| s.topology.place(x)).filter(|x| Some(x.kind) == kind.parent_kind()).map(|x| x.id);
                    // Else the only place of the right kind, if there is just one.
                    viewed.or_else(|| {
                        kind.parent_kind().and_then(|pk| {
                            let places = s.topology.places();
                            let mut it = places.iter().filter(|x| x.kind == pk);
                            match (it.next(), it.next()) {
                                (Some(only), None) => Some(only.id),
                                _ => None,
                            }
                        })
                    })
                }
                Some(_) => Some(place_id(s, p, "parent")?),
            };
            let (dx, dy) = spot(&s.topology, parent);
            let (x, y) = (f32_param(p, "x").unwrap_or(dx), f32_param(p, "y").unwrap_or(dy));
            let mut t = s.topology.clone();
            let id = t.add_place(kind, opt_str(p, "name"), parent, x, y)?;
            s.checkpoint();
            s.topology = t;
            s.changed();
            let name = s.topology.place(id).map(|x| x.name).unwrap_or_default();
            Ok(json!({"id": id, "name": name, "parent": parent}))
        }
        "place.rename" | "place.move" | "place.remove" => {
            let id = place_id(s, p, "id")?;
            let mut t = s.topology.clone();
            let mut transient = false;
            match cmd {
                "place.rename" => t.rename_place(id, str_param(p, "name")?)?,
                "place.move" => {
                    let (x, y) = (f32_param(p, "x").ok_or_else(|| bad("`x` is required"))?, f32_param(p, "y").ok_or_else(|| bad("`y` is required"))?);
                    t.move_place(id, x, y)?;
                    transient = p.get("transient").and_then(Value::as_bool).unwrap_or(false);
                }
                _ => {
                    let gone = t.place_subtree(id);
                    let removed = t.remove_place(id)?;
                    if s.place_view.is_some_and(|v| gone.contains(&v)) {
                        s.place_view = removed.parent;
                    }
                }
            }
            if !transient {
                s.checkpoint();
            }
            s.topology = t;
            s.changed();
            Ok(Value::Null)
        }
        "place.list" => {
            let t = &s.topology;
            let homes = t.homes();
            let name = |d: DeviceId| t.device(d).map(|x| x.name.clone()).unwrap_or_default();
            let places: Vec<Value> = t
                .places()
                .iter()
                .map(|x| {
                    let devices: Vec<String> = homes.iter().filter(|h| h.place == x.id).map(|h| name(h.device)).collect();
                    json!({"id": x.id, "kind": x.kind.id(), "name": x.name, "x": x.x, "y": x.y, "parent": x.parent, "devices": devices})
                })
                .collect();
            let cables: Vec<Value> = t
                .links
                .iter()
                .map(|l| {
                    let len = t.cable_length(l);
                    json!({"a": t.end_label(l.a), "b": t.end_label(l.b), "meters": len.meters, "manual": len.manual, "problem": netlabcraft_sim::cable_problem(t, l.a)})
                })
                .collect();
            Ok(json!({"implicit": t.implicit_layout(), "places": places, "cables": cables}))
        }
        "device.place" => {
            let mut devices = Vec::new();
            match p.get("devices") {
                Some(Value::Array(a)) => {
                    for v in a.iter().take(netlabcraft_model::MAX_DEVICES) {
                        devices.push(device_param(s, &json!({"d": v}), "d")?);
                    }
                }
                _ => devices.push(device_param(s, p, "device")?),
            }
            if devices.is_empty() {
                return Err(bad("`devices` names no device"));
            }
            let place = place_id(s, p, "place")?;
            let mut t = s.topology.clone();
            for (i, d) in devices.iter().enumerate() {
                let (dx, dy) = match t.home_of(*d) {
                    Some(h) if h.place == place => (h.x, h.y),
                    _ => spot(&t, Some(place)),
                };
                let x = f32_param(p, "x").map_or(dx, |x| x + 90.0 * i as f32);
                let y = f32_param(p, "y").unwrap_or(dy);
                t.set_home(*d, place, x, y)?;
            }
            if !p.get("transient").and_then(Value::as_bool).unwrap_or(false) {
                s.checkpoint();
            }
            s.topology = t;
            s.changed();
            Ok(Value::Null)
        }
        _ => {
            // link.length
            let d = device_param(s, p, "device")?;
            let port = port_param(s, d, str_param(p, "port")?)?;
            let max = netlabcraft_model::physical::MAX_CABLE_M;
            let meters = match p.get("meters") {
                Some(Value::Null) => None,
                Some(v) => Some(
                    v.as_f64()
                        .filter(|m| m.is_finite() && *m >= 1.0 && *m <= f64::from(max))
                        .map(|m| m.round() as u32)
                        .ok_or_else(|| bad(format!("`meters` is 1 to {max}")))?,
                ),
                None => return Err(bad("`meters` (a number of metres, or null for the length the places give) is required")),
            };
            let end = End { device: d, port };
            let mut t = s.topology.clone();
            let len = t.set_cable_length(end, meters)?;
            s.checkpoint();
            s.topology = t;
            s.changed();
            Ok(json!({"meters": len.meters, "manual": len.manual, "problem": netlabcraft_sim::cable_problem(&s.topology, end)}))
        }
    }
}

fn list_param<'a>(p: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    p.get(key).and_then(Value::as_array).ok_or_else(|| bad(format!("`{key}` (a list) is required")))
}

fn id_value(v: &Value, key: &str) -> Result<u32> {
    v.as_u64().and_then(|v| u32::try_from(v).ok()).ok_or_else(|| bad(format!("`{key}` holds cluster numbers")))
}

fn cluster_param(s: &Session, p: &Value) -> Result<u32> {
    let id = p.get("id").ok_or_else(|| bad("`id` (cluster number) is required")).and_then(|v| id_value(v, "id"))?;
    s.topology.cluster(id).map(|c| c.id).ok_or_else(|| EngineError::Failed(format!("no cluster {id}")))
}

/// Points as `[[x, y], …]` or `[{"x", "y"}, …]` (only the first `MAX_RAW_POINTS` are read).
fn points_param(p: &Value) -> Result<Vec<[f32; 2]>> {
    let num = |v: Option<&Value>| v.and_then(Value::as_f64).filter(|v| v.is_finite()).map(|v| v.clamp(-1.0e6, 1.0e6) as f32);
    list_param(p, "points")?
        .iter()
        .take(netlabcraft_model::drawing::MAX_RAW_POINTS)
        .map(|v| match v {
            Value::Array(a) => num(a.first()).zip(num(a.get(1))),
            Value::Object(_) => num(v.get("x")).zip(num(v.get("y"))),
            _ => None,
        })
        .map(|pt| pt.map(|(x, y)| [x, y]).ok_or_else(|| bad(r#"each point is [x, y] or {"x", "y"}"#)))
        .collect()
}

/// Where the workspace looks: the cluster (or null) and the path from the top.
fn view_json(s: &Session) -> Value {
    let t = &s.topology;
    let path: Vec<Value> = t.cluster_path(s.view).iter().filter_map(|c| t.cluster(*c)).map(|c| json!({"id": c.id, "name": c.name})).collect();
    json!({"cluster": s.view, "path": path})
}

fn probe_json(p: &Probe) -> Value {
    match p {
        Probe::Reply { from, rtt_us } => json!({"result": "reply", "from": from.to_string(), "rtt_us": rtt_us}),
        Probe::TtlExceeded { from, rtt_us } => json!({"result": "ttl-exceeded", "from": from.to_string(), "rtt_us": rtt_us}),
        Probe::Unreachable { from, code } => json!({"result": "unreachable", "from": from.to_string(), "code": code}),
        Probe::Timeout => json!({"result": "timeout"}),
    }
}

/// `PC1 RS-232` (or `R1 Console`): the far end of a device's console cable.
fn console_peer_label(t: &netlabcraft_model::Topology, id: DeviceId) -> Option<String> {
    let c = t.console_at(id)?;
    let (far, port) = if c.pc == id { (c.device, CONSOLE_PORT) } else { (c.pc, RS232_PORT) };
    t.device(far).map(|d| format!("{} {port}", d.name))
}

fn prompt_of(s: &mut Session, d: DeviceId) -> String {
    let cli = s.console(d).cli.clone();
    netlabcraft_ios::prompt(&s.topology, d, &cli)
}

/// Runs a console line, echoing it and its output into the device's scrollback.
pub fn cli_line(s: &mut Session, d: DeviceId, line: &str) -> Value {
    let line: String = line.chars().take(4_096).filter(|c| *c != '\r').collect();
    if s.topology.device(d).is_some_and(|x| x.powered_off) {
        let output = "% The device is switched off: switch it on in its Physical tab (device.power).".to_string();
        let cli = s.console(d).cli.clone();
        let prompt = netlabcraft_ios::prompt(&s.topology, d, &cli);
        return json!({"output": output, "prompt": prompt, "clear": false});
    }
    let before = s.topology.clone();
    let mut cli = std::mem::take(&mut s.console(d).cli);
    // What is typed at a password prompt is never echoed.
    let secret = netlabcraft_ios::prompt(&s.topology, d, &cli) == netlabcraft_ios::PASSWORD_PROMPT;
    let out = netlabcraft_ios::execute(&mut s.topology, &mut s.sim, d, &mut cli, &line);
    if out.changed {
        let after = std::mem::replace(&mut s.topology, before);
        s.checkpoint();
        s.topology = after;
        s.changed();
    } else {
        s.revision += 1;
    }
    let prompt = netlabcraft_ios::prompt(&s.topology, d, &cli);
    let console = s.console(d);
    console.cli = cli;
    if out.clear {
        console.log.clear();
    } else {
        console.append(&if secret { "\n".to_string() } else { format!("{line}\n") });
        if !out.output.is_empty() {
            console.append(&out.output);
            console.append("\n");
        }
    }
    console.append(&prompt);
    json!({"output": out.output, "prompt": prompt, "clear": out.clear})
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn read_file(path: &str) -> Result<String> {
    let meta = std::fs::metadata(path).map_err(|e| EngineError::Failed(format!("{path}: {e}")))?;
    if meta.len() > file::MAX_BYTES as u64 {
        return Err(EngineError::Failed(format!("{path}: too large to be a lab")));
    }
    std::fs::read_to_string(path).map_err(|e| EngineError::Failed(format!("{path}: {e}")))
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn read_file(_: &str) -> Result<String> {
    Err(EngineError::Failed("the browser build opens files through the file picker".into()))
}

/// Writes through a temporary file and a rename, so a failed save never truncates the old file.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write_file(path: &str, bytes: &[u8]) -> Result<()> {
    let target = std::path::Path::new(path);
    let name = target.file_name().and_then(|n| n.to_str()).ok_or_else(|| EngineError::Failed(format!("{path}: not a file path")))?;
    let tmp = target.with_file_name(format!(".{name}.part"));
    std::fs::write(&tmp, bytes).map_err(|e| EngineError::Failed(format!("{path}: {e}")))?;
    std::fs::rename(&tmp, target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        EngineError::Failed(format!("{path}: {e}"))
    })
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn write_file(_: &str, _: &[u8]) -> Result<()> {
    Err(EngineError::Failed("the browser build saves through downloads".into()))
}

/// Applies to the lab what mail servers changed in their mailboxes.
fn apply_mail_changes(s: &mut Session) {
    use netlabcraft_sim::email::MailChange;
    let changes = s.sim.take_mail_changes();
    if changes.is_empty() {
        return;
    }
    for (srv, change) in changes {
        let Some(d) = s.topology.device_mut(srv) else { continue };
        let boxes = &mut d.services.email.mailboxes;
        match change {
            MailChange::Deliver(user, mail) => {
                if boxes.len() < netlabcraft_model::services::MAX_MAILS {
                    boxes.push((user, mail));
                }
            }
            MailChange::Remove(user, n) => {
                let mut left = n;
                boxes.retain(|(u, _)| {
                    if left > 0 && u.eq_ignore_ascii_case(&user) {
                        left -= 1;
                        false
                    } else {
                        true
                    }
                });
            }
        }
    }
    s.changed();
}
