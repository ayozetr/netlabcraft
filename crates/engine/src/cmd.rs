//! Command implementations.

use packetcraft_core::{DeviceId, parse_ip};
use packetcraft_model::{DeviceKind, End, Shape, file, models};
use packetcraft_sim::{PingOptions, Probe};
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
fn device_param(s: &Session, p: &Value, key: &str) -> Result<DeviceId> {
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
    let cmd = id;
    match id {
        "file.new" => {
            let t = match opt_str(p, "sample") {
                Some(name) => sample::build(name)?,
                None => packetcraft_model::Topology::new(),
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
            let bytes = s.pcap();
            if let Some(path) = opt_str(p, "path") {
                write_file(path, &bytes)?;
            }
            Ok(json!({"frames": s.sim.trace().len(), "bytes": bytes.len()}))
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
                    *dst = packetcraft_model::Port { name: dst.name.clone(), mac: dst.mac, ..from.clone() };
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
            use packetcraft_model::services::{MAX_FILE, MAX_ITEMS, Services};
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
            sv.email.mailboxes.truncate(packetcraft_model::services::MAX_MAILS);
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
                if packetcraft_core::mask_to_prefix(pool.mask).is_none() {
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
                "save" => Some(packetcraft_ios::running_config(dev)),
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
            let label: String = str_param(p, "label")?.trim().chars().take(packetcraft_model::MAX_NAME).collect();
            s.checkpoint();
            if let Some(dev) = s.topology.device_mut(d) {
                dev.label = if label.is_empty() { None } else { Some(label) };
            }
            s.changed();
            Ok(Value::Null)
        }
        "device.models" => Ok(Value::Array(
            models::MODELS.iter().map(|m| json!({"id": m.id, "kind": m.kind.id(), "label": m.label, "description": m.description, "ports": m.port_names()})).collect(),
        )),
                                "snmp.request" => {
            use packetcraft_wire::snmp::{PduKind, Snmp, Value as SnmpValue, parse_oid};
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
                            Some((if h == "md5" { packetcraft_wire::sha1::Hash::Md5 } else { packetcraft_wire::sha1::Hash::Sha1 }, pw))
                        }
                        other => return Err(bad(format!("unknown authentication `{other}` (none, md5, sha)"))),
                    };
                    s.sim.snmp_request_v3(&s.topology, d, target, req, &packetcraft_sim::snmp::V3Login { user, auth })
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
                packetcraft_wire::snmp::NO_ACCESS => json!("no access (a read-only community)"),
                packetcraft_wire::snmp::NOT_WRITABLE => json!("not writable"),
                packetcraft_wire::snmp::WRONG_TYPE => json!("wrong type"),
                16 => json!("not authorized (the user is in no group)"),
                n => json!(format!("error {n}")),
            };
            Ok(json!({
                "bindings": r.bindings.iter().map(|(o, v)| json!({"oid": packetcraft_wire::snmp::oid_text(o), "value": v.show()})).collect::<Vec<_>>(),
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
        "mail.configure" => {
            let d = device_param(s, p, "device")?;
            if !s.topology.device(d).is_some_and(|x| x.kind.is_host()) {
                return Err(bad("the Email app runs on PCs, laptops and servers"));
            }
            let f = |k: &str| opt_str(p, k).map_or(String::new(), |v| v.trim().chars().take(128).collect());
            let account = packetcraft_model::services::MailAccount {
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
            let mail = packetcraft_model::services::Mail {
                from: account.address.clone(),
                to,
                subject: opt_str(p, "subject").map_or(String::new(), |v| v.chars().take(200).collect()),
                body: opt_str(p, "body").map_or(String::new(), |v| v.chars().take(packetcraft_model::services::MAX_MAIL).collect()),
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
                    let over = x.inbox.len().saturating_sub(packetcraft_model::services::MAX_MAILS);
                    x.inbox.drain(..over);
                }
                s.changed();
            }
            Ok(json!({"received": n}))
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
            let failed = packetcraft_ios::apply_config(&mut t, &mut s.sim, d, &config);
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
                None => dev.ports.iter().position(|x| packetcraft_model::ifname::is_wireless(&x.name)).ok_or_else(|| bad(format!("{} has no radio", dev.name)))?,
            };
            if !dev.ports.get(port).is_some_and(|x| packetcraft_model::ifname::is_wireless(&x.name)) {
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
            let wifi = (!ssid.is_empty()).then_some(packetcraft_model::Wifi { ssid, key, channel });
            s.checkpoint();
            if let Some(x) = s.topology.device_mut(d).and_then(|x| x.ports.get_mut(port)) {
                x.wifi = wifi;
            }
            s.changed();
            let ap = s.topology.wifi_ap_of(packetcraft_model::End { device: d, port });
            Ok(json!({"associated_with": ap.map(|e| s.topology.end_label(e))}))
        }
                "activity.show" => {
            let mut a = s.topology.activity.clone();
            // A locked activity doesn't show its checks: their labels may give the answers away.
            if let Some(x) = a.as_mut().filter(|x| x.locked()) {
                x.checks.clear();
                x.lock = "set".into();
            }
            Ok(json!({
                "activity": a,
                "total_points": s.topology.activity.as_ref().map_or(0, packetcraft_model::activity::Activity::total_points),
                "checks": s.topology.activity.as_ref().map_or(0, |x| x.checks.len()),
                "locked": s.topology.activity.as_ref().is_some_and(packetcraft_model::activity::Activity::locked),
            }))
        }
        "activity.set" | "activity.capture" | "activity.add_check" | "activity.remove_check" | "activity.clear" | "activity.lock"
            if s.topology.activity.as_ref().is_some_and(packetcraft_model::activity::Activity::locked) =>
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
            a.lock = a.password_hash(password);
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
            if a.password_hash(password) != a.lock {
                return Err(bad("wrong password"));
            }
            a.lock.clear();
            s.checkpoint();
            s.topology = t;
            s.changed();
            Ok(json!({"locked": false}))
        }
        "activity.set" => {
            let mut t = s.topology.clone();
            let salt_title = opt_str(p, "title").unwrap_or("Activity").to_string();
            let fresh_salt = crate::activity::new_salt(&t, &salt_title);
            let a = t.activity.get_or_insert_with(|| packetcraft_model::activity::Activity { title: "Activity".into(), salt: fresh_salt, ..Default::default() });
            if let Some(title) = opt_str(p, "title") {
                a.title = title.chars().take(200).collect();
            }
            if let Some(vars) = p.get("variables").and_then(Value::as_object) {
                use packetcraft_model::activity::{MAX_VALUES, MAX_VARIABLES, Variable};
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
                a.instructions = text.chars().take(packetcraft_model::activity::MAX_TEXT).collect();
            }
                        if let Some(m) = p.get("time_limit").and_then(Value::as_u64) {
                a.time_limit = u32::try_from(m).unwrap_or(u32::MAX).min(packetcraft_model::activity::MAX_TIME_LIMIT);
            }
            if let Some(b) = p.get("score_only").and_then(Value::as_bool) {
                a.score_only = b;
            }
            if let Some(h) = p.get("hidden").and_then(Value::as_bool) {
                if !h && a.hidden && a.checks.iter().any(|c| matches!(c.kind, packetcraft_model::activity::CheckKind::ConfigHashed { .. })) {
                    return Err(bad("hidden answers can't be shown again: capture the answers again with `hidden` off"));
                }
                if h && !a.hidden {
                    // Hide what is already there.
                    let salt = a.salt.clone();
                    for c in &mut a.checks {
                        if let packetcraft_model::activity::CheckKind::Config { device, line } = &c.kind {
                            let hash = packetcraft_model::activity::line_hash(&salt, *device, line);
                            c.kind = packetcraft_model::activity::CheckKind::ConfigHashed { device: *device, hash };
                            c.label = c.label.split(':').next().map_or("configuration item".to_string(), |d| format!("{d}: configuration item"));
                        }
                    }
                }
                a.hidden = h;
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
                .filter(|c| matches!(c.kind, packetcraft_model::activity::CheckKind::Ping { .. } | packetcraft_model::activity::CheckKind::Output { .. }))
                .cloned()
                .collect();
            checks.extend(kept);
            checks.truncate(packetcraft_model::activity::MAX_CHECKS);
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
            use packetcraft_model::activity::{Check, CheckKind, normalize};
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
                        (format!("{name}: configuration item"), CheckKind::ConfigHashed { device: d, hash: packetcraft_model::activity::line_hash(&salt, d, &flat) })
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
                    address.parse::<packetcraft_core::Ipv4Net>().map_err(|e| bad(e.to_string()))?;
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
            let a = t.activity.get_or_insert_with(|| packetcraft_model::activity::Activity { title: "Activity".into(), salt: fresh_salt, ..Default::default() });
            if a.checks.len() >= packetcraft_model::activity::MAX_CHECKS {
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
            let start = s.activity_start.clone().ok_or_else(|| bad("this lab wasn't opened as an activity"))?;
            let path = s.path.clone();
            s.replace(start, path);
            Ok(json!({}))
        }
        "note.add" => {
            let (x, y) = (f32_param(p, "x").unwrap_or(0.0), f32_param(p, "y").unwrap_or(0.0));
                        let shape = match opt_str(p, "shape") {
                Some(sh) => packetcraft_model::Shape::from_id(sh).ok_or_else(|| bad(format!("unknown shape `{sh}` (text, rectangle, ellipse, line, freehand, polygon)")))?,
                None => packetcraft_model::Shape::Text,
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
                "note.edit" => t.annotation_mut(id)?.text = str_param(p, "text")?.chars().take(packetcraft_model::MAX_ANNOTATION_TEXT).collect(),
                "note.move" => {
                    let (x, y) = (f32_param(p, "x").ok_or_else(|| bad("`x` is required"))?, f32_param(p, "y").ok_or_else(|| bad("`y` is required"))?);
                    let a = t.annotation_mut(id)?;
                    a.x = x;
                    a.y = y;
                    transient = p.get("transient").and_then(Value::as_bool).unwrap_or(false);
                }
                "note.color" => {
                    let c = str_param(p, "color")?;
                    packetcraft_model::parse_color(c).ok_or_else(|| bad(format!("`{c}` is not a #rrggbb colour")))?;
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
                                "up": packetcraft_sim::port_up(&s.topology, end),
                                "peer": s.topology.peer(end).map(|e| s.topology.end_label(e)),
                            })
                        })
                        .collect();
                    json!({"id": d.id.0, "name": d.name, "kind": d.kind.id(), "x": d.x, "y": d.y, "cluster": s.topology.cluster_of(d.id), "gateway": d.gateway.map(|g| g.to_string()), "ports": ports})
                })
                .collect(),
        )),
        "link.add" => {
            let a = device_param(s, p, "a")?;
            let b = device_param(s, p, "b")?;
            let pa = match opt_str(p, "a_port") {
                Some(n) => port_param(s, a, n)?,
                None => s.topology.free_port(a).ok_or_else(|| EngineError::Failed(format!("{} has no free port", s.topology.end_label(End { device: a, port: 0 }))))?,
            };
                        let pb = match opt_str(p, "b_port") {
                Some(n) => port_param(s, b, n)?,
                None => {
                    let serial = s.topology.is_serial(End { device: a, port: pa });
                    s.topology.free_port_for(b, serial).ok_or_else(|| EngineError::Failed("the second device has no free port of the same type".into()))?
                }
            };
            let mut t = s.topology.clone();
            let (ea, eb) = (End { device: a, port: pa }, End { device: b, port: pb });
            t.connect(ea, eb)?;
            s.checkpoint();
            s.topology = t;
            s.changed();
            Ok(json!({"a": s.topology.end_label(ea), "b": s.topology.end_label(eb)}))
        }
        "link.remove" => {
            let d = device_param(s, p, "device")?;
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
            Ok(json!({"help": packetcraft_ios::help_line(&s.topology, d, &cli, line)}))
        }
        "cli.complete" => {
            let d = device_param(s, p, "device")?;
            let line = str_param(p, "line")?;
            let cli = s.console(d).cli.clone();
            Ok(json!({"line": packetcraft_ios::complete_line(&s.topology, d, &cli, line)}))
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
            let errors = packetcraft_ios::apply_config(&mut s.topology, &mut s.sim, d, &text);
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
            s.sim.run_for(&s.topology, (secs * 1_000_000.0) as u64);
            s.revision += 1;
            Ok(json!({"time_us": s.sim.now_us()}))
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
        "sim.status" => Ok(json!({"time_us": s.sim.now_us(), "events": s.sim.trace().len(), "storm": s.sim.storm_detected()})),
        "topology.get" => serde_json::to_value(&s.topology).map_err(|e| EngineError::Failed(e.to_string())),
        "sample.list" => Ok(Value::Array(sample::SAMPLES.iter().map(|x| json!({"id": x.id, "title": x.title, "description": x.description})).collect())),
        other => Err(EngineError::UnknownCommand(other.to_string())),
    }
}

/// A device made while looking inside a cluster goes into it.
fn join_view(s: &Session, t: &mut packetcraft_model::Topology, dev: DeviceId) {
    if let Some(c) = s.view
        && let Ok(c) = t.cluster_mut(c)
    {
        c.devices.push(dev);
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
        .take(packetcraft_model::drawing::MAX_RAW_POINTS)
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

fn prompt_of(s: &mut Session, d: DeviceId) -> String {
    let cli = s.console(d).cli.clone();
    packetcraft_ios::prompt(&s.topology, d, &cli)
}

/// Runs a console line, echoing it and its output into the device's scrollback.
pub fn cli_line(s: &mut Session, d: DeviceId, line: &str) -> Value {
    let line: String = line.chars().take(4_096).filter(|c| *c != '\r').collect();
    let before = s.topology.clone();
    let mut cli = std::mem::take(&mut s.console(d).cli);
    // What is typed at a password prompt is never echoed.
    let secret = packetcraft_ios::prompt(&s.topology, d, &cli) == packetcraft_ios::PASSWORD_PROMPT;
    let out = packetcraft_ios::execute(&mut s.topology, &mut s.sim, d, &mut cli, &line);
    if out.changed {
        let after = std::mem::replace(&mut s.topology, before);
        s.checkpoint();
        s.topology = after;
        s.changed();
    } else {
        s.revision += 1;
    }
    let prompt = packetcraft_ios::prompt(&s.topology, d, &cli);
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
fn read_file(path: &str) -> Result<String> {
    let meta = std::fs::metadata(path).map_err(|e| EngineError::Failed(format!("{path}: {e}")))?;
    if meta.len() > file::MAX_BYTES as u64 {
        return Err(EngineError::Failed(format!("{path}: too large to be a lab")));
    }
    std::fs::read_to_string(path).map_err(|e| EngineError::Failed(format!("{path}: {e}")))
}

#[cfg(target_arch = "wasm32")]
fn read_file(_: &str) -> Result<String> {
    Err(EngineError::Failed("the browser build opens files through the file picker".into()))
}

/// Writes through a temporary file and a rename, so a failed save never truncates the old file.
#[cfg(not(target_arch = "wasm32"))]
fn write_file(path: &str, bytes: &[u8]) -> Result<()> {
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
fn write_file(_: &str, _: &[u8]) -> Result<()> {
    Err(EngineError::Failed("the browser build saves through downloads".into()))
}

/// Applies to the lab what mail servers changed in their mailboxes.
fn apply_mail_changes(s: &mut Session) {
    use packetcraft_sim::email::MailChange;
    let changes = s.sim.take_mail_changes();
    if changes.is_empty() {
        return;
    }
    for (srv, change) in changes {
        let Some(d) = s.topology.device_mut(srv) else { continue };
        let boxes = &mut d.services.email.mailboxes;
        match change {
            MailChange::Deliver(user, mail) => {
                if boxes.len() < packetcraft_model::services::MAX_MAILS {
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
