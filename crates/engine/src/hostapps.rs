//! The computer apps of an end device's Desktop that keep state in the lab: the Text Editor's
//! files (`editor.list`, `editor.read`, `editor.write`, `editor.delete`) and the Firewall
//! (`host.firewall`, one call for the IPv4 and the IPv6 list). The Telnet/SSH Client needs no
//! command of its own: it types `telnet` and `ssh` into the device's Command Prompt (`cli.exec`).

use std::net::IpAddr;

use netlabcraft_model::hostfw::{FwProto, HostFwRule, MAX_RULES};
use netlabcraft_model::services::{MAX_HOST_FILE_BYTES, MAX_HOST_FILES, WebFile, valid_host_file_name};
use serde_json::{Value, json};

use crate::cmd::device_param;
use crate::{EngineError, Result, Session};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// Runs one of these commands, or `None` when `id` is not one.
pub(crate) fn run(s: &mut Session, id: &str, p: &Value) -> Option<Result<Value>> {
    match id {
        "editor.list" => Some(list(s, p)),
        "editor.read" => Some(read(s, p)),
        "editor.write" => Some(write(s, p)),
        "editor.delete" => Some(delete(s, p)),
        "host.firewall" => Some(firewall(s, p)),
        _ => None,
    }
}

/// The device the app runs on: a PC, laptop or server.
pub(crate) fn desktop_device(s: &Session, p: &Value) -> Result<netlabcraft_core::DeviceId> {
    let d = device_param(s, p, "device")?;
    if !s.topology.device(d).is_some_and(|x| x.has_desktop_apps()) {
        return Err(bad("this app runs on PCs, laptops and servers"));
    }
    Ok(d)
}

fn name_param(p: &Value) -> Result<&str> {
    let name = p.get("name").and_then(Value::as_str).ok_or_else(|| bad("`name` (text) is required"))?.trim();
    if !valid_host_file_name(name) {
        return Err(bad("a file name is 1 to 64 characters, without spaces or slashes"));
    }
    Ok(name)
}

fn limits() -> Value {
    json!({"max_files": MAX_HOST_FILES, "max_bytes": MAX_HOST_FILE_BYTES})
}

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let d = desktop_device(s, p)?;
    let files: Vec<Value> =
        s.topology.device(d).map(|x| x.files.iter().map(|f| json!({"name": f.name, "bytes": f.content.len()})).collect()).unwrap_or_default();
    let mut out = limits();
    out["files"] = Value::Array(files);
    Ok(out)
}

fn read(s: &mut Session, p: &Value) -> Result<Value> {
    let d = desktop_device(s, p)?;
    let name = name_param(p)?;
    let file = s
        .topology
        .device(d)
        .and_then(|x| x.files.iter().find(|f| f.name == name))
        .ok_or_else(|| EngineError::Failed(format!("there is no file `{name}`")))?;
    Ok(json!({"name": file.name, "text": file.content, "bytes": file.content.len()}))
}

fn write(s: &mut Session, p: &Value) -> Result<Value> {
    let d = desktop_device(s, p)?;
    let name = name_param(p)?.to_string();
    let text = p.get("text").and_then(Value::as_str).ok_or_else(|| bad("`text` (text) is required"))?.to_string();
    if text.len() > MAX_HOST_FILE_BYTES {
        return Err(bad(format!("a file holds up to {MAX_HOST_FILE_BYTES} bytes (this one has {})", text.len())));
    }
    let exists = s.topology.device(d).is_some_and(|x| x.files.iter().any(|f| f.name == name));
    if !exists && s.topology.device(d).is_some_and(|x| x.files.len() >= MAX_HOST_FILES) {
        return Err(EngineError::Failed(format!("a device keeps up to {MAX_HOST_FILES} files; delete one first")));
    }
    let bytes = text.len();
    s.checkpoint();
    if let Some(x) = s.topology.device_mut(d) {
        match x.files.iter_mut().find(|f| f.name == name) {
            Some(f) => f.content = text,
            None => x.files.push(WebFile { name: name.clone(), content: text }),
        }
    }
    s.changed();
    Ok(json!({"name": name, "bytes": bytes, "created": !exists}))
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let d = desktop_device(s, p)?;
    let name = name_param(p)?.to_string();
    if !s.topology.device(d).is_some_and(|x| x.files.iter().any(|f| f.name == name)) {
        return Err(EngineError::Failed(format!("there is no file `{name}`")));
    }
    s.checkpoint();
    if let Some(x) = s.topology.device_mut(d) {
        x.files.retain(|f| f.name != name);
    }
    s.changed();
    Ok(json!({}))
}

/// A remote address as the Firewall takes it: `any`, an address, or `address/length`.
fn parse_remote(v6: bool, text: &str) -> Result<Option<(IpAddr, u8)>> {
    let t = text.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("any") {
        return Ok(None);
    }
    let (addr, len) = match t.split_once('/') {
        Some((a, l)) => (a, Some(l)),
        None => (t, None),
    };
    let addr: IpAddr = addr.trim().parse().map_err(|_| bad(format!("`remote` `{t}` is not an address (10.0.0.0/24, 2001:db8::/32 or any)")))?;
    let len = match len {
        Some(l) => l.trim().parse::<u8>().map_err(|_| bad("the prefix length after `/` is a number"))?,
        None if v6 => 128,
        None => 32,
    };
    Ok(Some((addr, len)))
}

/// A port as the Firewall takes it: `any`, one port, or a range `1000-2000`.
fn parse_ports(v: &Value) -> Result<Option<(u16, u16)>> {
    let text = match v {
        Value::Null => return Ok(None),
        Value::Number(n) => n.to_string(),
        Value::String(t) => t.trim().to_string(),
        _ => return Err(bad("`port` is a number, a range such as 1000-2000, or any")),
    };
    if text.is_empty() || text.eq_ignore_ascii_case("any") {
        return Ok(None);
    }
    let one = |t: &str| t.trim().parse::<u16>().map_err(|_| bad(format!("`{text}` is not a port or a range such as 1000-2000")));
    match text.split_once('-') {
        Some((a, b)) => Ok(Some((one(a)?, one(b)?))),
        None => {
            let port = one(&text)?;
            Ok(Some((port, port)))
        }
    }
}

fn rule_json(n: usize, r: &HostFwRule) -> Value {
    json!({
        "n": n,
        "action": if r.allow { "allow" } else { "deny" },
        "protocol": r.proto.name(),
        "remote": r.remote.map_or_else(|| "any".to_string(), |(a, l)| format!("{a}/{l}")),
        "port": r.ports.map_or_else(|| "any".to_string(), |(lo, hi)| if lo == hi { lo.to_string() } else { format!("{lo}-{hi}") }),
    })
}

fn firewall(s: &mut Session, p: &Value) -> Result<Value> {
    let d = desktop_device(s, p)?;
    let v6 = match p.get("version") {
        None | Some(Value::Null) => false,
        Some(v) => match v.as_u64().or_else(|| v.as_str().and_then(|t| t.trim().parse().ok())) {
            Some(4) => false,
            Some(6) => true,
            _ => return Err(bad("`version` is 4 or 6")),
        },
    };
    let mut list = s.topology.device(d).map(|x| x.host_fw.list(v6).clone()).unwrap_or_default();
    let before = list.clone();
    if p.get("clear").and_then(Value::as_bool) == Some(true) {
        list.rules.clear();
    }
    if let Some(n) = p.get("remove") {
        let n = n
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| (1..=list.rules.len()).contains(n))
            .ok_or_else(|| bad("`remove` is the number of a rule, counting from 1"))?;
        list.rules.remove(n - 1);
    }
    if let Some(add) = p.get("add") {
        let allow = match add.get("action").and_then(Value::as_str).map(str::to_ascii_lowercase).as_deref() {
            Some("allow" | "permit") => true,
            Some("deny" | "block") => false,
            _ => return Err(bad("`add.action` is allow or deny")),
        };
        let proto = match add.get("protocol").and_then(Value::as_str) {
            None => FwProto::Any,
            Some(t) => FwProto::from_name(t).ok_or_else(|| bad("`add.protocol` is any, tcp, udp or icmp"))?,
        };
        let remote = match add.get("remote").and_then(Value::as_str) {
            None => None,
            Some(t) => parse_remote(v6, t)?,
        };
        let ports = match add.get("port") {
            None => None,
            Some(v) => parse_ports(v)?,
        };
        let rule = HostFwRule::new(v6, allow, proto, remote, ports).map_err(bad)?;
        if list.rules.len() >= MAX_RULES {
            return Err(EngineError::Failed(format!("a list holds up to {MAX_RULES} rules; remove one first")));
        }
        let at = match add.get("at") {
            None | Some(Value::Null) => list.rules.len(),
            Some(v) => v
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| (1..=list.rules.len() + 1).contains(n))
                .map(|n| n - 1)
                .ok_or_else(|| bad("`add.at` is a position from 1"))?,
        };
        list.rules.insert(at, rule);
    }
    if let Some(v) = p.get("on") {
        list.on = v.as_bool().ok_or_else(|| bad("`on` is true or false"))?;
    }
    if list != before {
        s.checkpoint();
        if let Some(x) = s.topology.device_mut(d) {
            *x.host_fw.list_mut(v6) = list.clone();
        }
        s.changed();
    }
    let rules: Vec<Value> = list.rules.iter().enumerate().map(|(i, r)| rule_json(i + 1, r)).collect();
    Ok(json!({"version": if v6 { 6 } else { 4 }, "on": list.on, "rules": rules}))
}
