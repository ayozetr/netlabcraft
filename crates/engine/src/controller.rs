//! A small controller, in the spirit of the SDN controllers CCNA 3 describes: a host keeps a
//! list of managed routers (address and login), reads their inventory through RESTCONF and
//! pushes one configuration template to all of them. Every exchange is a real RESTCONF request
//! across the lab, made with the same machinery as `pc.http_request`.

use netlabcraft_model::automation::{MAX_MANAGED, Managed};
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// An address as a URL's host: IPv6 in brackets.
fn url_host(a: &str) -> String {
    if a.contains(':') { format!("[{a}]") } else { a.to_string() }
}

fn managed(s: &Session, d: netlabcraft_core::DeviceId) -> Vec<Managed> {
    s.topology.device(d).map(|x| x.automation.managed.clone()).unwrap_or_default()
}

fn as_json(m: &Managed) -> Value {
    json!({"address": m.address, "user": m.user})
}

/// One RESTCONF request from the controller to a managed router.
fn call(s: &mut Session, d: netlabcraft_core::DeviceId, m: &Managed, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
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
fn inventory(s: &mut Session, d: netlabcraft_core::DeviceId, m: &Managed) -> Value {
    let mut out = as_json(m);
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
    if let Ok(r) = call(s, d, m, "GET", "/restconf/data/ietf-interfaces:interfaces", None) {
        let list = r["json"]["ietf-interfaces:interfaces"]["interface"].as_array().cloned().unwrap_or_default();
        out["interfaces"] = json!(list.len());
        out["up"] = json!(list.iter().filter(|i| i["oper-status"] == "up").count());
    }
    out
}

pub(crate) fn run(s: &mut Session, cmd: &str, p: &Value) -> Result<Value> {
    let d = crate::cmd::device_param(s, p, "host")?;
    if s.topology.device(d).is_none_or(|x| !x.kind.is_host()) {
        return Err(bad("the controller runs on a PC, laptop or server"));
    }
    match cmd {
        "controller.devices" => {
            let mut list = managed(s, d);
            let mut changed = false;
            if let Some(a) = p.get("remove").and_then(Value::as_str) {
                let before = list.len();
                list.retain(|m| m.address != a.trim());
                if list.len() == before {
                    return Err(EngineError::Failed(format!("{a} is not managed here")));
                }
                changed = true;
            }
            if let Some(x) = p.get("add") {
                let address = x.get("address").and_then(Value::as_str).ok_or_else(|| bad("a managed router needs its `address`"))?.trim().to_string();
                let address = address.trim_start_matches('[').trim_end_matches(']').to_string();
                address.parse::<std::net::IpAddr>().map_err(|_| bad(format!("`{address}` is not an IPv4 or IPv6 address")))?;
                let user: String = x.get("user").and_then(Value::as_str).unwrap_or("").trim().chars().take(64).collect();
                let password: String = x.get("password").and_then(Value::as_str).unwrap_or("").chars().take(128).collect();
                if user.is_empty() || user.contains(char::is_whitespace) || password.chars().any(char::is_control) {
                    return Err(bad("a managed router needs a one-word `user` and a `password`"));
                }
                let m = Managed { address: address.clone(), user, password };
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
            if changed {
                s.checkpoint();
                if let Some(x) = s.topology.device_mut(d) {
                    x.automation.managed = list.clone();
                }
                s.changed();
            }
            let devices: Vec<Value> = if p.get("refresh").and_then(Value::as_bool) == Some(true) {
                list.iter().map(|m| inventory(s, d, m)).collect()
            } else {
                list.iter().map(as_json).collect()
            };
            Ok(json!({"devices": devices}))
        }
        _ => {
            let method = p.get("method").and_then(Value::as_str).unwrap_or("PATCH").to_ascii_uppercase();
            if !["PUT", "PATCH", "POST", "DELETE"].contains(&method.as_str()) {
                return Err(bad("`method` is PUT, PATCH, POST or DELETE"));
            }
            let path =
                p.get("path").and_then(Value::as_str).ok_or_else(|| bad("`path` (such as /restconf/data/ietf-interfaces:interfaces) is required"))?;
            if !path.starts_with("/restconf/data") {
                return Err(bad("`path` starts with /restconf/data"));
            }
            let body = match p.get("body") {
                None | Some(Value::Null) => None,
                Some(Value::String(t)) => Some(serde_json::from_str::<Value>(t).map_err(|e| bad(format!("the template is not valid JSON: {e}")))?),
                Some(v) => Some(v.clone()),
            };
            if method != "DELETE" && body.is_none() {
                return Err(bad("a template (`body`) is required"));
            }
            let only: Option<Vec<String>> =
                p.get("devices").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect());
            let targets: Vec<Managed> = managed(s, d).into_iter().filter(|m| only.as_ref().is_none_or(|o| o.contains(&m.address))).collect();
            if targets.is_empty() {
                return Err(EngineError::Failed("no managed routers to push to (controller.devices with add)".into()));
            }
            let mut results = Vec::new();
            for m in &targets {
                let mut r = as_json(m);
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
    }
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
}
