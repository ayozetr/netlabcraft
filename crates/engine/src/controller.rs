//! A small controller, in the spirit of the SDN controllers CCNA 3 describes: a host keeps a
//! list of managed routers (address, login and per-router template variables), discovers them
//! from a seed router by walking CDP and LLDP neighbours, reads their inventory (hostname,
//! model, software, interfaces) and pushes a configuration template whose `{{variables}}` are
//! filled in per router. Every exchange is a real RESTCONF request over HTTPS across the lab,
//! made with the same machinery as `pc.http_request`:
//!
//! - inventory: `ietf-system:system` (hostname), `ietf-system:system-state/platform`
//!   (RFC 7317: `os-name`, `os-release`, `machine`), `ietf-interfaces:interfaces`;
//! - discovery: `netlabcraft-discovery:neighbors` (each router's CDP and LLDP neighbours,
//!   with the management address they advertise), breadth first from the seed.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use netlabcraft_core::DeviceId;
use netlabcraft_model::automation::{MAX_MANAGED, MAX_VARS, Managed};
use serde_json::{Map, Value, json};

use crate::{EngineError, Result, Session};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// An address as a URL's host: IPv6 in brackets.
fn url_host(a: &str) -> String {
    if a.contains(':') { format!("[{a}]") } else { a.to_string() }
}

fn managed(s: &Session, d: DeviceId) -> Vec<Managed> {
    s.topology.device(d).map(|x| x.automation.managed.clone()).unwrap_or_default()
}

fn as_json(m: &Managed) -> Value {
    let vars: Map<String, Value> = m.vars.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    json!({"address": m.address, "user": m.user, "vars": vars})
}

/// One RESTCONF request from the controller to a managed router.
fn call(s: &mut Session, d: DeviceId, m: &Managed, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
    let mut p = json!({
        "pc": d.0, "method": method, "url": format!("https://{}{path}", url_host(&m.address)),
        "headers": {"Accept": crate::automation::YANG_JSON}, "user": m.user, "password": m.password,
    });
    if let Some(b) = body {
        p["body"] = b.clone();
    }
    crate::automation::http_request(s, &p)
}

/// What the inventory says of one router.
fn inventory(s: &mut Session, d: DeviceId, m: &Managed) -> Value {
    let mut out = json!({"address": m.address, "user": m.user});
    match call(s, d, m, "GET", "/restconf/data/ietf-system:system", None) {
        Err(e) => {
            out["reachable"] = json!(false);
            out["error"] = json!(e.to_string());
            return out;
        }
        Ok(r) if r["status"] != 200 => {
            out["reachable"] = json!(true);
            out["error"] = json!(format!("{} {}", r["status"], r["reason"].as_str().unwrap_or("")));
            return out;
        }
        Ok(r) => {
            out["reachable"] = json!(true);
            out["hostname"] = r["json"]["ietf-system:system"]["hostname"].clone();
        }
    }
    if let Ok(r) = call(s, d, m, "GET", "/restconf/data/ietf-system:system-state", None) {
        let p = &r["json"]["ietf-system:system-state"]["platform"];
        out["model"] = p["machine"].clone();
        out["os"] = json!(format!("{} {}", p["os-name"].as_str().unwrap_or(""), p["os-release"].as_str().unwrap_or("")).trim());
    }
    if let Ok(r) = call(s, d, m, "GET", "/restconf/data/ietf-interfaces:interfaces", None) {
        let list = r["json"]["ietf-interfaces:interfaces"]["interface"].as_array().cloned().unwrap_or_default();
        out["interfaces"] = json!(list.len());
        out["up"] = json!(list.iter().filter(|i| i["oper-status"] == "up").count());
    }
    out
}

/// A login from params: a one-word user and a password.
fn login(x: &Value) -> Result<(String, String)> {
    let user: String = x.get("user").and_then(Value::as_str).unwrap_or("").trim().chars().take(64).collect();
    let password: String = x.get("password").and_then(Value::as_str).unwrap_or("").chars().take(128).collect();
    if user.is_empty() || user.contains(char::is_whitespace) || password.chars().any(char::is_control) {
        return Err(bad("a managed router needs a one-word `user` and a `password`"));
    }
    Ok((user, password))
}

fn address(x: &Value) -> Result<String> {
    let a = x.get("address").and_then(Value::as_str).ok_or_else(|| bad("a managed router needs its `address`"))?.trim();
    let a = a.trim_start_matches('[').trim_end_matches(']').to_string();
    a.parse::<std::net::IpAddr>().map_err(|_| bad(format!("`{a}` is not an IPv4 or IPv6 address")))?;
    Ok(a)
}

/// Template variables: `{"name": "value"}`.
fn vars_param(v: Option<&Value>) -> Result<Vec<(String, String)>> {
    let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let m = v.as_object().ok_or_else(|| bad("`vars` is {\"name\": \"value\"}"))?;
    if m.len() > MAX_VARS {
        return Err(bad(format!("at most {MAX_VARS} variables per router")));
    }
    m.iter()
        .map(|(k, v)| {
            if k.is_empty() || k.len() > 32 || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                return Err(bad(format!("`{k}` is not a variable name (letters, digits, _ and -)")));
            }
            let v = v.as_str().map_or_else(|| v.to_string(), str::to_string);
            if v.chars().count() > 240 || v.chars().any(char::is_control) {
                return Err(bad(format!("the value of {k} is too long or has line breaks")));
            }
            Ok((k.clone(), v))
        })
        .collect()
}

/// Fills `{{name}}` in a template; values are escaped for a JSON string. The first variable
/// without a value is an error.
pub fn fill(template: &str, vars: &BTreeMap<String, String>) -> std::result::Result<String, String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(i) = rest.find("{{") {
        out.push_str(rest.get(..i).unwrap_or_default());
        let after = rest.get(i + 2..).unwrap_or_default();
        let end = after.find("}}").ok_or("a {{ without its }}")?;
        let name = after.get(..end).unwrap_or_default().trim();
        let value = vars.get(name).ok_or_else(|| format!("no value for {{{{{name}}}}}"))?;
        let escaped = serde_json::to_string(value).unwrap_or_default();
        out.push_str(escaped.get(1..escaped.len().saturating_sub(1)).unwrap_or_default());
        rest = after.get(end + 2..).unwrap_or_default();
    }
    out.push_str(rest);
    Ok(out)
}

/// Most routers one discovery visits.
const MAX_DISCOVER: usize = 32;

pub(crate) fn run(s: &mut Session, cmd: &str, p: &Value) -> Result<Value> {
    let d = crate::cmd::device_param(s, p, "host")?;
    if s.topology.device(d).is_none_or(|x| !x.kind.is_host()) {
        return Err(bad("the controller runs on a PC, laptop or server"));
    }
    match cmd {
        "controller.devices" => devices(s, d, p),
        "controller.discover" => discover(s, d, p),
        _ => push(s, d, p),
    }
}

fn save(s: &mut Session, d: DeviceId, list: Vec<Managed>) {
    s.checkpoint();
    if let Some(x) = s.topology.device_mut(d) {
        x.automation.managed = list;
    }
    s.changed();
}

fn devices(s: &mut Session, d: DeviceId, p: &Value) -> Result<Value> {
    let mut list = managed(s, d);
    let mut changed = false;
    if let Some(a) = p.get("remove").and_then(Value::as_str) {
        let a = a.trim().trim_start_matches('[').trim_end_matches(']');
        let before = list.len();
        list.retain(|m| m.address != a);
        if list.len() == before {
            return Err(EngineError::Failed(format!("{a} is not managed here")));
        }
        changed = true;
    }
    if let Some(x) = p.get("add") {
        let address = address(x)?;
        let (user, password) = login(x)?;
        let vars = vars_param(x.get("vars"))?;
        let m = Managed { address: address.clone(), user, password, vars };
        match list.iter_mut().find(|x| x.address == address) {
            Some(old) => *old = m,
            None => {
                if list.len() >= MAX_MANAGED {
                    return Err(EngineError::Failed(format!("at most {MAX_MANAGED} managed routers")));
                }
                list.push(m);
            }
        }
        changed = true;
    }
    if let Some(x) = p.get("set_vars") {
        let address = address(x)?;
        let vars = vars_param(x.get("vars"))?;
        let m = list.iter_mut().find(|m| m.address == address).ok_or_else(|| EngineError::Failed(format!("{address} is not managed here")))?;
        m.vars = vars;
        changed = true;
    }
    if changed {
        save(s, d, list.clone());
    }
    let devices: Vec<Value> = if p.get("refresh").and_then(Value::as_bool) == Some(true) {
        list.iter()
            .map(|m| {
                let mut x = inventory(s, d, m);
                x["vars"] = as_json(m)["vars"].clone();
                x
            })
            .collect()
    } else {
        list.iter().map(as_json).collect()
    };
    Ok(json!({"devices": devices}))
}

/// `controller.discover`: breadth first from a seed router over its CDP and LLDP neighbours'
/// management addresses; every router that answers RESTCONF with the login is inventoried (and
/// managed, unless `"add": false`).
fn discover(s: &mut Session, d: DeviceId, p: &Value) -> Result<Value> {
    let seed = address(&json!({"address": p.get("seed").cloned().unwrap_or(Value::Null)}))
        .map_err(|_| bad("`seed` (the first router's address) is required"))?;
    let (user, password) = login(p)?;
    let max = p.get("max").and_then(Value::as_u64).map_or(16, |m| (m as usize).clamp(1, MAX_DISCOVER));
    let add = p.get("add").and_then(Value::as_bool).unwrap_or(true);
    let mut queue = VecDeque::from([seed.clone()]);
    let mut seen = BTreeSet::from([seed]);
    let (mut found, mut links, mut unreachable): (Vec<Value>, Vec<Value>, Vec<Value>) = (Vec::new(), Vec::new(), Vec::new());
    let mut reached: Vec<Managed> = Vec::new();
    let mut names: BTreeSet<String> = BTreeSet::new();
    while let Some(a) = queue.pop_front() {
        if found.len() >= max {
            break;
        }
        let m = Managed { address: a.clone(), user: user.clone(), password: password.clone(), vars: Vec::new() };
        let r = match call(s, d, &m, "GET", "/restconf/data/netlabcraft-discovery:neighbors", None) {
            Ok(r) if r["status"] == 200 => r,
            Ok(r) => {
                unreachable.push(json!({"address": a, "error": format!("{} {}", r["status"], r["reason"].as_str().unwrap_or(""))}));
                continue;
            }
            Err(e) => {
                unreachable.push(json!({"address": a, "error": e.to_string()}));
                continue;
            }
        };
        let inv = inventory(s, d, &m);
        let me = inv["hostname"].as_str().unwrap_or(&a).to_string();
        // The same router seen again under another of its addresses.
        if !names.insert(me.clone()) {
            continue;
        }
        let neighbors = r["json"]["netlabcraft-discovery:neighbors"]["neighbor"].as_array().cloned().unwrap_or_default();
        for n in &neighbors {
            let (to, local, remote) =
                (n["device-id"].as_str().unwrap_or(""), n["local-interface"].as_str().unwrap_or(""), n["remote-interface"].as_str().unwrap_or(""));
            let proto = n["protocol"].as_str().unwrap_or("");
            // A link is listed once, whichever end and protocol saw it.
            let n_links = links.len();
            let existing = links.iter_mut().find(|l| {
                (l["from"] == me.as_str() && l["from_interface"] == local && l["to"] == to && l["to_interface"] == remote)
                    || (l["to"] == me.as_str() && l["to_interface"] == local && l["from"] == to && l["from_interface"] == remote)
            });
            match existing {
                Some(l) => {
                    if let Some(ps) = l["protocols"].as_array_mut()
                        && !ps.iter().any(|x| x == proto)
                    {
                        ps.push(json!(proto));
                    }
                }
                None if n_links < 256 => links.push(
                    json!({"from": me, "from_interface": local, "to": to, "to_interface": remote, "platform": n["platform"], "protocols": [proto]}),
                ),
                None => {}
            }
            if let Some(next) = n["management-address"].as_str()
                && seen.len() < 4 * MAX_DISCOVER
                && seen.insert(next.to_string())
            {
                queue.push_back(next.to_string());
            }
        }
        let mut entry = inv;
        entry["neighbors"] = json!(neighbors.len());
        found.push(entry);
        reached.push(m);
    }
    if add && !reached.is_empty() {
        let mut list = managed(s, d);
        for m in reached {
            let n = list.len();
            match list.iter_mut().find(|x| x.address == m.address) {
                Some(old) => {
                    old.user = m.user;
                    old.password = m.password;
                }
                None if n < MAX_MANAGED => list.push(m),
                None => {}
            }
        }
        save(s, d, list);
    }
    Ok(json!({"devices": found, "links": links, "unreachable": unreachable}))
}

/// `controller.push`: one request per managed router, its template filled in with the router's
/// variables (`{{address}}` and, when used, `{{hostname}}` are built in).
fn push(s: &mut Session, d: DeviceId, p: &Value) -> Result<Value> {
    let method = p.get("method").and_then(Value::as_str).unwrap_or("PATCH").to_ascii_uppercase();
    if !["PUT", "PATCH", "POST", "DELETE"].contains(&method.as_str()) {
        return Err(bad("`method` is PUT, PATCH, POST or DELETE"));
    }
    let path = p.get("path").and_then(Value::as_str).ok_or_else(|| bad("`path` (such as /restconf/data/ietf-interfaces:interfaces) is required"))?;
    if !path.starts_with("/restconf/data") {
        return Err(bad("`path` starts with /restconf/data"));
    }
    let template: Option<String> = match p.get("body") {
        None | Some(Value::Null) => None,
        Some(Value::String(t)) => Some(t.clone()),
        Some(v) => Some(v.to_string()),
    };
    if method != "DELETE" && template.is_none() {
        return Err(bad("a template (`body`) is required"));
    }
    // A template without variables must be JSON already.
    if let Some(t) = &template
        && !t.contains("{{")
    {
        serde_json::from_str::<Value>(t).map_err(|e| bad(format!("the template is not valid JSON: {e}")))?;
    }
    let extra: BTreeMap<String, BTreeMap<String, String>> = match p.get("variables") {
        None | Some(Value::Null) => BTreeMap::new(),
        Some(Value::Object(m)) => m
            .iter()
            .map(|(a, v)| Ok((a.trim_start_matches('[').trim_end_matches(']').to_string(), vars_param(Some(v))?.into_iter().collect())))
            .collect::<Result<_>>()?,
        Some(_) => return Err(bad("`variables` is {\"router address\": {\"name\": \"value\"}}")),
    };
    let only: Option<Vec<String>> =
        p.get("devices").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect());
    let targets: Vec<Managed> = managed(s, d).into_iter().filter(|m| only.as_ref().is_none_or(|o| o.contains(&m.address))).collect();
    if targets.is_empty() {
        return Err(EngineError::Failed("no managed routers to push to (controller.devices with add, or controller.discover)".into()));
    }
    let mut results = Vec::new();
    for m in &targets {
        let mut r = json!({"address": m.address, "user": m.user});
        let mut vars: BTreeMap<String, String> = m.vars.iter().cloned().collect();
        vars.extend(extra.get(&m.address).cloned().unwrap_or_default());
        vars.insert("address".into(), m.address.clone());
        let body = match &template {
            None => None,
            Some(t) => {
                if (t.contains("{{hostname}}") || t.contains("{{ hostname }}"))
                    && let Ok(x) = call(s, d, m, "GET", "/restconf/data/ietf-system:system", None)
                    && let Some(h) = x["json"]["ietf-system:system"]["hostname"].as_str()
                {
                    vars.entry("hostname".into()).or_insert_with(|| h.to_string());
                }
                match fill(t, &vars)
                    .and_then(|x| serde_json::from_str::<Value>(&x).map_err(|e| format!("the filled template is not valid JSON: {e}")))
                {
                    Ok(v) => Some(v),
                    Err(e) => {
                        r["error"] = json!(e);
                        results.push(r);
                        continue;
                    }
                }
            }
        };
        match call(s, d, m, &method, path, body.as_ref()) {
            Ok(x) => {
                r["status"] = x["status"].clone();
                if x["status"].as_u64().is_some_and(|c| c >= 300) {
                    r["error"] = x["json"]["ietf-restconf:errors"]["error"][0]["error-message"].clone();
                }
                r["applied"] = x["applied"].clone();
            }
            Err(e) => r["error"] = json!(e.to_string()),
        }
        results.push(r);
    }
    Ok(json!({"results": results}))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_controller_lists_its_routers_and_pushes_a_template() {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "automation"})).unwrap();
        let add = |a: &str, pw: &str| json!({"host": "PC2", "add": {"address": a, "user": "admin", "password": pw}});
        s.execute("controller.devices", add("192.168.1.1", "NetLab2026")).unwrap();
        s.execute("controller.devices", add("192.168.1.99", "x")).unwrap();
        let r = s.execute("controller.devices", json!({"host": "PC2", "refresh": true})).unwrap();
        let list = r["devices"].as_array().unwrap();
        assert_eq!(list[0]["hostname"], "R1", "{r}");
        assert!(list[0]["interfaces"].as_u64().unwrap() >= 3 && list[0]["up"].as_u64().unwrap() >= 1, "{r}");
        assert!(list[0]["model"].as_str().is_some_and(|m| !m.is_empty()) && list[0]["os"].as_str().unwrap().starts_with("NetLabCraft"), "{r}");
        assert_eq!(list[1]["reachable"], false, "{r}");
        assert!(!r.to_string().contains("NetLab2026"), "passwords stay in the lab, not in answers");
        let r = s
            .execute(
                "controller.push",
                json!({"host": "PC2", "devices": ["192.168.1.1"], "path": "/restconf/data/ietf-interfaces:interfaces",
                       "body": {"ietf-interfaces:interfaces": {"interface": [{"name": "Loopback9", "description": "pushed by the controller"}]}}}),
            )
            .unwrap();
        assert_eq!(r["results"][0]["status"], 204, "{r}");
        let run = netlabcraft_ios::running_config(s.topology.device(s.topology.resolve("R1").unwrap()).unwrap());
        assert!(run.contains("interface Loopback9\n description pushed by the controller"), "{run}");
        // A bad template comes back per router.
        let r = s
            .execute(
                "controller.push",
                json!({"host": "PC2", "devices": ["192.168.1.1"], "path": "/restconf/data/ietf-system:system", "body": {"bogus": 1}}),
            )
            .unwrap();
        assert_eq!(r["results"][0]["status"], 400, "{r}");
        assert!(s.execute("controller.push", json!({"host": "PC2", "path": "/x", "body": {}})).is_err());
        assert!(s.execute("controller.push", json!({"host": "PC2", "path": "/restconf/data", "body": "{nope"})).is_err());
        assert!(s.execute("controller.devices", json!({"host": "R1"})).is_err());
        assert!(s.execute("controller.devices", json!({"host": "PC2", "add": {"address": "nope", "user": "a", "password": "b"}})).is_err());
        let r = s.execute("controller.devices", json!({"host": "PC2", "remove": "192.168.1.99"})).unwrap();
        assert_eq!(r["devices"].as_array().unwrap().len(), 1);
    }

    /// R1 — R2 — R3 in a line behind R1's LAN, all with the same admin login and RESTCONF.
    fn three_routers() -> Session {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "automation"})).unwrap();
        for r in ["R2", "R3"] {
            s.execute("device.add", json!({"kind": "router", "name": r})).unwrap();
        }
        s.execute("link.add", json!({"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/0"})).unwrap();
        s.execute("link.add", json!({"a": "R2", "a_port": "g0/1", "b": "R3", "b_port": "g0/0"})).unwrap();
        let common = "username admin privilege 15 secret NetLab2026\nip http secure-server\nip http authentication local\nrestconf";
        s.execute("device.configure", json!({"device": "R1", "lines": "interface g0/1\n ip address 10.0.12.1 255.255.255.0\n no shutdown\nexit\nip route 10.0.23.0 255.255.255.0 10.0.12.2"})).unwrap();
        s.execute(
            "device.configure",
            json!({"device": "R2", "lines": format!("interface g0/0\n ip address 10.0.12.2 255.255.255.0\n no shutdown\ninterface g0/1\n ip address 10.0.23.2 255.255.255.0\n no shutdown\nexit\nip route 192.168.1.0 255.255.255.0 10.0.12.1\n{common}")}),
        )
        .unwrap();
        s.execute(
            "device.configure",
            json!({"device": "R3", "lines": format!("interface g0/0\n ip address 10.0.23.3 255.255.255.0\n no shutdown\nexit\nip route 0.0.0.0 0.0.0.0 10.0.23.2\n{common}")}),
        )
        .unwrap();
        s
    }

    #[test]
    fn discovery_walks_cdp_and_lldp_neighbours_from_a_seed() {
        let mut s = three_routers();
        let r = s.execute("controller.discover", json!({"host": "PC2", "seed": "192.168.1.1", "user": "admin", "password": "NetLab2026"})).unwrap();
        let names: Vec<&str> = r["devices"].as_array().unwrap().iter().filter_map(|x| x["hostname"].as_str()).collect();
        assert_eq!(names, ["R1", "R2", "R3"], "{r}");
        let links = r["links"].as_array().unwrap();
        assert!(
            links.iter().any(|l| l["from"] == "R1"
                && l["from_interface"] == "GigabitEthernet0/1"
                && l["to"] == "R2"
                && l["to_interface"] == "GigabitEthernet0/0"),
            "{r}"
        );
        assert!(links.iter().any(|l| l["from"] == "R1" && l["to"] == "S1"), "the switch shows as a neighbour: {r}");
        assert_eq!(
            links.iter().filter(|l| (l["from"] == "R2" && l["to"] == "R3") || (l["from"] == "R3" && l["to"] == "R2")).count(),
            1,
            "each link once: {r}"
        );
        // The switch doesn't speak RESTCONF: it is listed as unreachable, not managed.
        assert!(r["unreachable"].as_array().unwrap().iter().all(|u| u["error"].is_string()), "{r}");
        let managed = s.execute("controller.devices", json!({"host": "PC2"})).unwrap();
        assert_eq!(managed["devices"].as_array().unwrap().len(), 3, "{managed}");
        // A wrong password reaches nothing.
        let r =
            s.execute("controller.discover", json!({"host": "PC2", "seed": "192.168.1.1", "user": "admin", "password": "no", "add": false})).unwrap();
        assert!(r["devices"].as_array().unwrap().is_empty() && r["unreachable"].as_array().unwrap().len() == 1, "{r}");
        for bad in [
            json!({"host": "PC2", "user": "a", "password": "b"}),
            json!({"host": "PC2", "seed": "x", "user": "a", "password": "b"}),
            json!({"host": "PC2", "seed": "10.0.0.1"}),
        ] {
            assert!(s.execute("controller.discover", bad.clone()).is_err(), "{bad}");
        }
    }

    #[test]
    fn templates_take_each_routers_variables() {
        let mut s = three_routers();
        s.execute("controller.discover", json!({"host": "PC2", "seed": "192.168.1.1", "user": "admin", "password": "NetLab2026"})).unwrap();
        s.execute("controller.devices", json!({"host": "PC2", "set_vars": {"address": "10.0.12.2", "vars": {"site": "Branch \"B\"", "lo": "2"}}}))
            .unwrap();
        let template =
            r#"{"ietf-interfaces:interfaces": {"interface": [{"name": "Loopback{{lo}}", "description": "{{hostname}} at {{site}} ({{address}})"}]}}"#;
        let r = s
            .execute(
                "controller.push",
                json!({"host": "PC2", "path": "/restconf/data/ietf-interfaces:interfaces", "body": template,
                       "variables": {"192.168.1.1": {"site": "HQ", "lo": "1"}, "10.0.23.3": {"site": "Edge", "lo": "3"}}}),
            )
            .unwrap();
        let results = r["results"].as_array().unwrap();
        assert!(results.iter().all(|x| x["status"] == 204), "{r}");
        let run = |s: &Session, n: &str| netlabcraft_ios::running_config(s.topology.device(s.topology.resolve(n).unwrap()).unwrap());
        assert!(run(&s, "R1").contains("interface Loopback1\n description R1 at HQ (192.168.1.1)"), "{}", run(&s, "R1"));
        assert!(run(&s, "R2").contains("interface Loopback2\n description R2 at Branch \"B\" (10.0.12.2)"), "{}", run(&s, "R2"));
        assert!(run(&s, "R3").contains("interface Loopback3\n description R3 at Edge (10.0.23.3)"), "{}", run(&s, "R3"));
        // A router missing a variable gets an error of its own; the others go ahead.
        let r = s
            .execute("controller.push", json!({"host": "PC2", "path": "/restconf/data/ietf-system:system", "body": r#"{"ietf-system:system": {"hostname": "{{newname}}"}}"#, "variables": {"10.0.23.3": {"newname": "Edge3"}}}))
            .unwrap();
        let errors = r["results"].as_array().unwrap().iter().filter(|x| x["error"].as_str().is_some_and(|e| e.contains("{{newname}}"))).count();
        assert_eq!(errors, 2, "{r}");
        assert!(s.topology.resolve("Edge3").is_ok());
        let mut v = BTreeMap::new();
        v.insert("a".to_string(), "x".to_string());
        assert_eq!(fill("{{a}}{{ a }}", &v).unwrap(), "xx");
        assert!(fill("{{a", &v).is_err() && fill("{{b}}", &v).is_err());
        assert!(s.execute("controller.devices", json!({"host": "PC2", "set_vars": {"address": "10.9.9.9", "vars": {}}})).is_err());
        assert!(s.execute("controller.devices", json!({"host": "PC2", "set_vars": {"address": "10.0.12.2", "vars": {"bad name": "x"}}})).is_err());
    }
}
