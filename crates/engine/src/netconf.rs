//! NETCONF (RFC 6241), engine side: what a router answers in a session, and the `netconf.session`
//! command a host uses to open one.
//!
//! The data is the same subset RESTCONF serves ([`crate::automation`]), in XML: `system`
//! (ietf-system), `interfaces` (ietf-interfaces, with ietf-ip's `ipv4` and `ipv6`) and `static-routes`
//! (netlabcraft-routing). Operations: `<get-config>` and `<get>` (with subtree filters on the top
//! containers and on an interface's name), `<edit-config>` on `<running/>` (default operation
//! merge, replace or none; `operation` attributes merge, replace, create, delete and remove on
//! interfaces, routes and an interface's description and ipv4), `<lock>`/`<unlock>` (one
//! datastore, so they always succeed), and `<close-session>`. Only the running datastore
//! exists: `<commit>`, `<validate>` and the candidate are refused honestly.

use netlabcraft_core::DeviceId;
use netlabcraft_model::Topology;
use netlabcraft_sim::automation::{NetconfEvent, NetconfSession, NetconfStep};
use netlabcraft_wire::netconf::{self, BASE_10, BASE_11, Framing, NS};
use netlabcraft_wire::xml::{self, Element, escape};
use serde_json::{Map, Value, json};

use crate::automation::{self as rc, ApiError, IF, IP, MODULES, Op, RT, SYS, Target};
use crate::{EngineError, Result, Session};

const WRITABLE_RUNNING: &str = "urn:ietf:params:netconf:capability:writable-running:1.0";
const IANA_IF: &str = "urn:ietf:params:xml:ns:yang:iana-if-type";

/// The server's `<hello>`.
pub fn server_hello(session: u32) -> String {
    let mut caps = vec![BASE_10.to_string(), BASE_11.to_string(), WRITABLE_RUNNING.to_string()];
    for (m, rev) in MODULES {
        caps.push(format!("{}?module={m}&amp;revision={rev}", rc::namespace(m)));
    }
    let caps: String = caps.iter().map(|c| format!("<capability>{c}</capability>")).collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<hello xmlns=\"{NS}\"><capabilities>{caps}</capabilities><session-id>{session}</session-id></hello>"
    )
}

/// A `<hello>` from a client.
pub fn client_hello(chunked: bool) -> String {
    let caps = if chunked {
        format!("<capability>{BASE_10}</capability><capability>{BASE_11}</capability>")
    } else {
        format!("<capability>{BASE_10}</capability>")
    };
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<hello xmlns=\"{NS}\"><capabilities>{caps}</capabilities></hello>")
}

/// An `<rpc-error>` (RFC 6241 §4.3).
fn rpc_error(id: Option<&str>, kind: &str, tag: &str, message: &str) -> String {
    let attr = id.map_or(String::new(), |i| format!(" message-id=\"{}\"", escape(i)));
    format!(
        "<rpc-reply xmlns=\"{NS}\"{attr}><rpc-error><error-type>{kind}</error-type><error-tag>{tag}</error-tag><error-severity>error</error-severity><error-message xml:lang=\"en\">{}</error-message></rpc-error></rpc-reply>",
        escape(message)
    )
}

fn reply(id: &str, body: &str) -> String {
    format!("<rpc-reply xmlns=\"{NS}\" message-id=\"{}\">{body}</rpc-reply>", escape(id))
}

fn api_error(id: &str, e: &ApiError, deleting: bool) -> String {
    let tag = if deleting && e.status == 404 { "data-missing" } else { e.tag };
    rpc_error(Some(id), "application", tag, &e.message)
}

/// JSON data as XML: `module:name` members start a new default namespace.
pub(crate) fn to_xml(name: &str, v: &Value, module: &str, out: &mut String) {
    let (m, local) = match name.split_once(':') {
        Some((m, l)) => (m, l),
        None => (module, name),
    };
    let ns = if m != module { format!(" xmlns=\"{}\"", rc::namespace(m)) } else { String::new() };
    match v {
        Value::Array(a) => {
            for x in a {
                to_xml(name, x, module, out);
            }
        }
        Value::Object(o) => {
            out.push_str(&format!("<{local}{ns}>"));
            for (k, x) in o {
                to_xml(k, x, m, out);
            }
            out.push_str(&format!("</{local}>"));
        }
        Value::String(s) if local == "type" && s.starts_with("iana-if-type:") => {
            out.push_str(&format!("<type{ns} xmlns:ianaift=\"{IANA_IF}\">ianaift:{}</type>", escape(s.trim_start_matches("iana-if-type:"))));
        }
        Value::String(s) => out.push_str(&format!("<{local}{ns}>{}</{local}>", escape(s))),
        other => out.push_str(&format!("<{local}{ns}>{other}</{local}>")),
    }
}

/// The lab as this session sees it: with the session's earlier edits applied.
fn view(t: &Topology, dev: DeviceId, applied: &[String]) -> Topology {
    let mut v = t.clone();
    if !applied.is_empty() {
        let mut sim = netlabcraft_sim::Sim::default();
        let _ = netlabcraft_ios::apply_config(&mut v, &mut sim, dev, &applied.join("\n"));
    }
    v
}

fn module_of(local: &str) -> Option<&'static str> {
    match local {
        "system" => Some(SYS),
        "interfaces" => Some(IF),
        "static-routes" => Some(RT),
        _ => None,
    }
}

/// `<data>` for `<get-config>` / `<get>`, with an optional subtree filter.
fn data(t: &Topology, dev: DeviceId, filter: Option<&Element>, config_only: bool) -> std::result::Result<String, String> {
    let d = t.device(dev).ok_or("the router is gone")?;
    let mut out = String::from("<data>");
    let wanted: Vec<(&'static str, Option<&Element>)> = match filter {
        None => MODULES.iter().map(|(m, _)| (*m, None)).collect(),
        Some(f) => {
            if f.attr("type").is_some_and(|t| t != "subtree") {
                return Err("only subtree filters are supported".into());
            }
            f.children.iter().filter_map(|c| module_of(&c.name).map(|m| (m, Some(c)))).collect()
        }
    };
    for (m, sel) in wanted {
        let Some((k, mut v)) = rc::module_data(t, d, m, config_only) else { continue };
        // An interface filter with a name keeps that interface only.
        if m == IF
            && let Some(name) = sel.and_then(|s| s.child("interface")).and_then(|i| i.text_of("name"))
            && let Some(list) = v.get_mut("interface").and_then(Value::as_array_mut)
        {
            list.retain(|i| i.get("name").and_then(Value::as_str).is_some_and(|n| n.eq_ignore_ascii_case(name)));
        }
        to_xml(&k, &v, "", &mut out);
    }
    out.push_str("</data>");
    Ok(out)
}

fn has_running(e: Option<&Element>) -> bool {
    e.is_some_and(|x| x.child("running").is_some())
}

fn op_of(e: &Element, default: Option<Op>) -> std::result::Result<Option<Op>, String> {
    match e.attr("operation") {
        None => Ok(default),
        Some("merge") => Ok(Some(Op::Merge)),
        Some("replace") => Ok(Some(Op::Replace)),
        Some("create") => Ok(Some(Op::Create)),
        Some("delete") | Some("remove") => Ok(Some(Op::Delete)),
        Some(o) => Err(format!("unknown operation `{o}`")),
    }
}

/// An `<interface>` element as the JSON entry RESTCONF takes.
fn iface_entry(e: &Element) -> std::result::Result<Map<String, Value>, String> {
    let mut m = Map::new();
    for c in &e.children {
        match c.name.as_str() {
            "name" | "description" => {
                m.insert(c.name.clone(), json!(c.text));
            }
            "type" => {
                let local = c.text.rsplit(':').next().unwrap_or("");
                m.insert("type".into(), json!(format!("iana-if-type:{local}")));
            }
            "enabled" => {
                let b = match c.text.as_str() {
                    "true" => true,
                    "false" => false,
                    _ => return Err("`enabled` is true or false".into()),
                };
                m.insert("enabled".into(), json!(b));
            }
            "ipv4" | "ipv6" if c.ns.as_deref() == Some(rc::namespace(IP)) => {
                let mut addrs = Vec::new();
                for a in c.children_named("address") {
                    let mut x = Map::new();
                    for f in &a.children {
                        match f.name.as_str() {
                            "prefix-length" => x.insert(f.name.clone(), f.text.parse::<u64>().map_or_else(|_| json!(f.text), |n| json!(n))),
                            _ => x.insert(f.name.clone(), json!(f.text)),
                        };
                    }
                    addrs.push(Value::Object(x));
                }
                if let Some(other) = c.children.iter().find(|x| x.name != "address") {
                    return Err(format!("`{}` here holds only `address`, not `{}`", c.name, other.name));
                }
                m.insert(format!("{IP}:{}", c.name), json!({"address": addrs}));
            }
            "ipv4" | "ipv6" => return Err(format!("`{}` belongs to the ietf-ip namespace (urn:ietf:params:xml:ns:yang:ietf-ip)", c.name)),
            other => {
                m.insert(other.to_string(), json!(c.text));
            }
        }
    }
    Ok(m)
}

fn route_entry(e: &Element) -> Map<String, Value> {
    let mut m = Map::new();
    if let Some(p) = e.text_of("prefix") {
        m.insert("prefix".into(), json!(p));
    }
    let hops: Vec<Value> = e
        .children_named("next-hop")
        .map(|h| {
            let mut x = Map::new();
            for f in &h.children {
                let v = if f.name == "distance" { f.text.parse::<u64>().map_or_else(|_| json!(f.text), |n| json!(n)) } else { json!(f.text) };
                x.insert(f.name.clone(), v);
            }
            Value::Object(x)
        })
        .collect();
    if !hops.is_empty() {
        m.insert("next-hop".into(), Value::Array(hops));
    }
    for c in &e.children {
        if !["prefix", "next-hop"].contains(&c.name.as_str()) {
            m.insert(c.name.clone(), json!(c.text));
        }
    }
    m
}

/// The configuration lines of an `<edit-config>`'s `<config>`, checked one edit at a time
/// against the lab as the edits so far left it.
fn edit_config(t: &Topology, dev: DeviceId, config: &Element, default: Option<Op>) -> std::result::Result<Vec<String>, (ApiError, bool)> {
    let bad = |m: String| (ApiError { status: 400, tag: "invalid-value", message: m }, false);
    let mut view_t = t.clone();
    let mut lines: Vec<String> = Vec::new();
    let mut step =
        |view_t: &mut Topology, target: Target, op: Op, body: Option<Value>, tolerant: bool| -> std::result::Result<(), (ApiError, bool)> {
            let d = view_t.device(dev).ok_or_else(|| bad("the router is gone".into()))?;
            match rc::edit(view_t, d, &target, op, body.as_ref()) {
                Ok(e) => {
                    rc::check(view_t, dev, &e.lines).map_err(|x| (x, false))?;
                    let mut sim = netlabcraft_sim::Sim::default();
                    let _ = netlabcraft_ios::apply_config(view_t, &mut sim, dev, &e.lines.join("\n"));
                    lines.extend(e.lines);
                    Ok(())
                }
                Err(e) if tolerant && e.status == 404 => Ok(()),
                Err(e) => Err((e, op == Op::Delete)),
            }
        };
    for top in &config.children {
        match top.name.as_str() {
            "system" => {
                if let Some(h) = top.text_of("hostname") {
                    let op = op_of(top, default).map_err(bad)?;
                    if op.is_some() {
                        step(&mut view_t, Target::Hostname, Op::Merge, Some(json!({"ietf-system:hostname": h})), false)?;
                    }
                }
            }
            "interfaces" => {
                for i in top.children_named("interface") {
                    let name = i
                        .text_of("name")
                        .ok_or_else(|| (ApiError { status: 400, tag: "missing-element", message: "an interface needs its <name>".into() }, false))?
                        .to_string();
                    let op = op_of(i, default).map_err(bad)?;
                    let mut entry = iface_entry(i).map_err(bad)?;
                    // Leaf-level deletes first.
                    for (leaf, key) in [("description", "description"), ("ipv4", "ietf-ip:ipv4"), ("ipv6", "ietf-ip:ipv6")] {
                        if let Some(c) = i.child(leaf)
                            && let Some(Op::Delete) = op_of(c, None).map_err(bad)?
                        {
                            step(&mut view_t, Target::IfChild(name.clone(), key), Op::Delete, None, c.attr("operation") == Some("remove"))?;
                            entry.remove(key);
                        }
                    }
                    match op {
                        None => {}
                        Some(Op::Delete) => step(&mut view_t, Target::Interface(name), Op::Delete, None, i.attr("operation") == Some("remove"))?,
                        Some(Op::Create) => {
                            step(&mut view_t, Target::Interfaces, Op::Create, Some(json!({"ietf-interfaces:interface": entry})), false)?
                        }
                        // A merge naming only the interface changes nothing.
                        Some(op) if entry.len() > 1 || op == Op::Replace => {
                            step(&mut view_t, Target::Interface(name), op, Some(json!({"ietf-interfaces:interface": entry})), false)?
                        }
                        Some(_) => {}
                    }
                }
            }
            "static-routes" => {
                for r in top.children_named("route") {
                    let entry = route_entry(r);
                    let prefix = entry.get("prefix").and_then(Value::as_str).unwrap_or("").to_string();
                    match op_of(r, default).map_err(bad)? {
                        None => {}
                        Some(Op::Delete) => step(&mut view_t, Target::Route(prefix), Op::Delete, None, r.attr("operation") == Some("remove"))?,
                        Some(Op::Create) => step(&mut view_t, Target::Routes, Op::Create, Some(json!({"netlabcraft-routing:route": entry})), false)?,
                        Some(op) => step(&mut view_t, Target::Route(prefix), op, Some(json!({"netlabcraft-routing:route": entry})), false)?,
                    }
                }
            }
            other => {
                return Err((
                    ApiError {
                        status: 400,
                        tag: "unknown-element",
                        message: format!("no data named <{other}> here (system, interfaces, static-routes)"),
                    },
                    false,
                ));
            }
        }
    }
    Ok(lines)
}

/// The router's side of a session.
pub fn serve(t: &Topology, dev: DeviceId, s: &mut NetconfSession, ev: NetconfEvent<'_>) -> NetconfStep {
    let msg = match ev {
        NetconfEvent::Open => return NetconfStep { reply: Some(server_hello(s.id)), ..NetconfStep::default() },
        NetconfEvent::Message(m) => m,
    };
    let close = |reply: Option<String>| NetconfStep { reply, close: true, lines: Vec::new() };
    let answer = |reply: String| NetconfStep { reply: Some(reply), ..NetconfStep::default() };
    let doc = match xml::parse(msg) {
        Ok(d) => d,
        // Before the hellos a broken message ends the session (RFC 6241 §8.1).
        Err(_) if !s.hello_done => return close(None),
        Err(e) => return answer(rpc_error(None, "rpc", "malformed-message", &e.to_string())),
    };
    if !s.hello_done {
        if doc.name != "hello" || doc.child("session-id").is_some() {
            return close(None);
        }
        let caps: Vec<&str> =
            doc.child("capabilities").map(|c| c.children_named("capability").map(|x| x.text.as_str()).collect()).unwrap_or_default();
        if !caps.contains(&BASE_10) && !caps.contains(&BASE_11) {
            return close(None);
        }
        s.hello_done = true;
        s.chunked = caps.contains(&BASE_11);
        return NetconfStep::default();
    }
    if doc.name != "rpc" {
        return answer(rpc_error(None, "rpc", "unknown-element", &format!("expected <rpc>, got <{}>", doc.name)));
    }
    let Some(id) = doc.attr("message-id").map(str::to_string) else {
        return answer(rpc_error(None, "rpc", "missing-attribute", "an <rpc> needs a message-id attribute"));
    };
    s.rpcs = s.rpcs.saturating_add(1);
    let Some(op) = doc.children.first() else {
        return answer(rpc_error(Some(&id), "rpc", "missing-element", "the <rpc> names no operation"));
    };
    let only_running = "this router has only the running datastore: use <running/>";
    match op.name.as_str() {
        "get-config" | "get" => {
            if op.name == "get-config" && !has_running(op.child("source")) {
                return answer(rpc_error(Some(&id), "protocol", "invalid-value", only_running));
            }
            let v = view(t, dev, &s.applied);
            match data(&v, dev, op.child("filter"), op.name == "get-config") {
                Ok(d) => answer(reply(&id, &d)),
                Err(e) => answer(rpc_error(Some(&id), "protocol", "operation-not-supported", &e)),
            }
        }
        "edit-config" => {
            if !has_running(op.child("target")) {
                return answer(rpc_error(Some(&id), "protocol", "invalid-value", only_running));
            }
            let default = match op.text_of("default-operation") {
                None | Some("merge") => Some(Op::Merge),
                Some("replace") => Some(Op::Replace),
                Some("none") => None,
                Some(o) => return answer(rpc_error(Some(&id), "protocol", "invalid-value", &format!("unknown default-operation `{o}`"))),
            };
            let Some(config) = op.child("config") else {
                return answer(rpc_error(Some(&id), "protocol", "missing-element", "<edit-config> needs a <config>"));
            };
            let v = view(t, dev, &s.applied);
            match edit_config(&v, dev, config, default) {
                Ok(lines) => {
                    s.applied.extend(lines.iter().cloned());
                    NetconfStep { reply: Some(reply(&id, "<ok/>")), close: false, lines }
                }
                Err((e, deleting)) => answer(api_error(&id, &e, deleting)),
            }
        }
        "lock" | "unlock" => {
            if has_running(op.child("target")) {
                answer(reply(&id, "<ok/>"))
            } else {
                answer(rpc_error(Some(&id), "protocol", "invalid-value", only_running))
            }
        }
        "close-session" => close(Some(reply(&id, "<ok/>"))),
        "commit" | "discard-changes" | "validate" | "copy-config" | "delete-config" | "cancel-commit" => {
            answer(rpc_error(Some(&id), "protocol", "operation-not-supported", only_running))
        }
        other => answer(rpc_error(Some(&id), "protocol", "operation-not-supported", &format!("<{other}> is not an operation this router supports"))),
    }
}

// ---- the client command ----

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// Most RPCs in one `netconf.session`.
const MAX_RPCS: usize = 32;

/// `netconf.session`: opens a session, sends the RPCs and a `<close-session>`, and returns what
/// the router answered.
pub(crate) fn session(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::cmd::device_param(s, p, "pc")?;
    if s.topology.device(d).is_none_or(|x| !x.kind.is_host()) {
        return Err(bad("NETCONF sessions are opened from PCs, laptops and servers"));
    }
    let host = p.get("target").and_then(Value::as_str).ok_or_else(|| bad("`target` (the router's address or name) is required"))?.trim().to_string();
    let user = p.get("user").and_then(Value::as_str).ok_or_else(|| bad("`user` is required"))?;
    let password = p.get("password").and_then(Value::as_str).ok_or_else(|| bad("`password` is required"))?;
    if user.is_empty() || user.len() > 64 || password.len() > 128 || user.contains(char::is_whitespace) || password.chars().any(char::is_control) {
        return Err(bad("a user is one word, and neither has line breaks"));
    }
    let chunked = match p.get("framing").and_then(Value::as_str).unwrap_or("1.1") {
        "1.1" | "chunked" => true,
        "1.0" | "end-of-message" => false,
        other => return Err(bad(format!("framing `{other}`: 1.1 (chunked) or 1.0 (end-of-message)"))),
    };
    let rpcs: Vec<String> = match p.get("rpcs") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(x)) => vec![x.clone()],
        Some(Value::Array(a)) => {
            a.iter().map(|x| x.as_str().map(str::to_string).ok_or_else(|| bad("each RPC is XML text"))).collect::<Result<_>>()?
        }
        Some(_) => return Err(bad("`rpcs` is XML text or a list of them")),
    };
    if rpcs.len() > MAX_RPCS || rpcs.iter().any(|r| r.len() > netconf::MAX_MESSAGE) {
        return Err(bad(format!("at most {MAX_RPCS} RPCs, each under {} bytes", netconf::MAX_MESSAGE)));
    }
    let framing = if chunked { Framing::Chunked } else { Framing::EndOfMessage };
    let mut stream = netconf::auth_line(user, password).into_bytes();
    stream.extend(netconf::encode(&client_hello(chunked), Framing::EndOfMessage));
    // Plain operations may be given bare (`<get-config>…`): they are wrapped in an <rpc>.
    let mut next_id = 100;
    for r in &rpcs {
        let t = r.trim();
        let wrapped = if t.starts_with("<rpc") || t.starts_with("<?xml") || t.starts_with("<nc:rpc") {
            t.to_string()
        } else {
            next_id += 1;
            format!("<rpc message-id=\"{next_id}\" xmlns=\"{NS}\">{t}</rpc>")
        };
        stream.extend(netconf::encode(&wrapped, framing));
    }
    let close_id = next_id + 1;
    stream.extend(netconf::encode(&format!("<rpc message-id=\"{close_id}\" xmlns=\"{NS}\"><close-session/></rpc>"), framing));
    let server = rc::resolve(s, d, &host)?;
    let got = s.sim.tcp_session(&s.topology, d, server, netconf::PORT, &stream);
    let applied = rc::apply_changes(s);
    s.revision += 1;
    let got = got.map_err(|e| EngineError::Failed(e.to_string()))?;
    // The server's identification line, then (if the login worked) its framed messages.
    let text_start = got.windows(2).position(|w| w == b"\r\n").map_or(0, |i| i + 2);
    let rest = got.get(text_start..).unwrap_or_default();
    if !rest.starts_with(b"<") {
        let why = String::from_utf8_lossy(rest).trim().to_string();
        return Ok(
            json!({"connected": false, "error": if why.is_empty() { "the server closed the session".to_string() } else { why }, "hello": null, "replies": [], "applied": applied}),
        );
    }
    let mut buf = rest.to_vec();
    let mut hello = Value::Null;
    let mut replies = Vec::new();
    let mut f = Framing::EndOfMessage;
    for _ in 0..(MAX_RPCS + 4) {
        match netconf::decode(&buf, f) {
            Ok(Some((m, used))) => {
                buf.drain(..used.min(buf.len()));
                if hello.is_null() {
                    hello = json!(m);
                    f = framing;
                } else {
                    replies.push(json!(m));
                }
            }
            _ => break,
        }
    }
    Ok(json!({"connected": true, "framing": if chunked { "1.1" } else { "1.0" }, "hello": hello, "replies": replies, "applied": applied}))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn lab() -> Session {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "automation"})).unwrap();
        s
    }

    fn nc(s: &mut Session, rpcs: &[&str], framing: &str) -> Value {
        s.execute(
            "netconf.session",
            json!({"pc": "PC1", "target": "192.168.1.1", "user": "admin", "password": "NetLab2026", "rpcs": rpcs, "framing": framing}),
        )
        .unwrap()
    }

    fn running(s: &Session) -> String {
        netlabcraft_ios::running_config(s.topology.device(s.topology.resolve("R1").unwrap()).unwrap())
    }

    #[test]
    fn hello_get_config_and_close_in_both_framings() {
        for framing in ["1.1", "1.0"] {
            let mut s = lab();
            let r = nc(&mut s, &["<get-config><source><running/></source></get-config>"], framing);
            assert_eq!(r["connected"], true, "{r}");
            let hello = r["hello"].as_str().unwrap();
            assert!(hello.contains(BASE_11) && hello.contains("<session-id>") && hello.contains("module=ietf-interfaces"), "{hello}");
            let replies = r["replies"].as_array().unwrap();
            assert_eq!(replies.len(), 2, "{r}");
            let data = xml::parse(replies[0].as_str().unwrap()).unwrap();
            assert_eq!(data.attr("message-id"), Some("101"));
            let ifs = data.child("data").and_then(|d| d.child("interfaces")).unwrap();
            assert_eq!(ifs.ns.as_deref(), Some(rc::namespace(IF)));
            let g0 = ifs.children_named("interface").find(|i| i.text_of("name") == Some("GigabitEthernet0/0")).unwrap();
            assert_eq!(g0.text_of("description"), Some("LAN"));
            assert_eq!(g0.child("ipv4").and_then(|x| x.child("address")).and_then(|a| a.text_of("ip")), Some("192.168.1.1"));
            assert!(g0.child("oper-status").is_none(), "get-config has no state");
            assert!(replies[1].as_str().unwrap().contains("<ok/>"));
            // Real NETCONF segments crossed the lab on port 830.
            assert!(s.sim.trace().iter().any(|e| e.summary.starts_with("NETCONF") && e.summary.contains("<hello>")), "{framing}");
            assert!(s.execute("cli.exec", json!({"device": "R1", "line": "enable"})).is_ok());
            let out = s.execute("cli.exec", json!({"device": "R1", "line": "show netconf-yang sessions"})).unwrap();
            assert!(out["output"].as_str().unwrap().contains(if framing == "1.1" { "chunked" } else { "end-mark" }), "{out}");
        }
    }

    #[test]
    fn edit_config_changes_the_running_configuration() {
        let mut s = lab();
        let edit = format!(
            "<edit-config><target><running/></target><config>\
             <interfaces xmlns=\"{}\"><interface><name>GigabitEthernet0/1</name><description>Set by NETCONF</description><enabled>true</enabled>\
             <ipv4 xmlns=\"{}\"><address><ip>10.20.0.1</ip><prefix-length>24</prefix-length></address></ipv4></interface>\
             <interface xmlns:nc=\"{NS}\" nc:operation=\"create\"><name>Loopback5</name></interface></interfaces>\
             <static-routes xmlns=\"{}\"><route><prefix>10.30.0.0/16</prefix><next-hop><address>10.20.0.2</address></next-hop></route></static-routes>\
             </config></edit-config>",
            rc::namespace(IF),
            rc::namespace(IP),
            rc::namespace(RT)
        );
        let filter = format!(
            "<get-config><source><running/></source><filter type=\"subtree\"><interfaces xmlns=\"{}\"><interface><name>GigabitEthernet0/1</name></interface></interfaces></filter></get-config>",
            rc::namespace(IF)
        );
        let r = nc(&mut s, &[&edit, &filter], "1.1");
        let replies = r["replies"].as_array().unwrap();
        assert!(replies[0].as_str().unwrap().contains("<ok/>"), "{r}");
        // The session's next read sees its edit, and only the filtered interface.
        let got = replies[1].as_str().unwrap();
        assert!(got.contains("Set by NETCONF") && !got.contains("GigabitEthernet0/0") && !got.contains("static-routes"), "{got}");
        let run = running(&s);
        for l in [
            "interface GigabitEthernet0/1\n description Set by NETCONF\n ip address 10.20.0.1 255.255.255.0",
            "interface Loopback5",
            "ip route 10.30.0.0 255.255.0.0 10.20.0.2",
        ] {
            assert!(run.contains(l), "{l}:\n{run}");
        }
        // Deletes: a leaf, an interface, a route; deleting again is data-missing, remove is quiet.
        let del = format!(
            "<edit-config xmlns:nc=\"{NS}\"><target><running/></target><config><interfaces xmlns=\"{}\"><interface><name>GigabitEthernet0/1</name><description nc:operation=\"delete\"/></interface>\
             <interface nc:operation=\"delete\"><name>Loopback5</name></interface></interfaces>\
             <static-routes xmlns=\"{}\"><route nc:operation=\"delete\"><prefix>10.30.0.0/16</prefix></route></static-routes></config></edit-config>",
            rc::namespace(IF),
            rc::namespace(RT)
        );
        let again = del.replace("Loopback5", "Loopback6");
        let quiet = again.replace("nc:operation=\"delete\"", "nc:operation=\"remove\"");
        let r = nc(&mut s, &[&del, &again, &quiet], "1.0");
        let replies = r["replies"].as_array().unwrap();
        assert!(replies[0].as_str().unwrap().contains("<ok/>"), "{r}");
        assert!(replies[1].as_str().unwrap().contains("data-missing"), "{r}");
        assert!(replies[2].as_str().unwrap().contains("<ok/>"), "{r}");
        let run = running(&s);
        assert!(!run.contains("Set by NETCONF") && !run.contains("Loopback5") && !run.contains("10.30.0.0"), "{run}");
    }

    #[test]
    fn ipv6_addresses_in_netconf_data() {
        let mut s = lab();
        let edit = format!(
            "<edit-config><target><running/></target><config><interfaces xmlns=\"{}\"><interface><name>GigabitEthernet0/0</name>\
             <ipv6 xmlns=\"{}\"><address><ip>2001:db8:10::1</ip><prefix-length>64</prefix-length></address></ipv6></interface></interfaces></config></edit-config>",
            rc::namespace(IF),
            rc::namespace(IP)
        );
        let r = nc(&mut s, &[&edit, "<get-config><source><running/></source></get-config>"], "1.1");
        let replies = r["replies"].as_array().unwrap();
        assert!(replies[0].as_str().unwrap().contains("<ok/>"), "{r}");
        let data = xml::parse(replies[1].as_str().unwrap()).unwrap();
        let g0 = data.child("data").and_then(|d| d.child("interfaces")).unwrap();
        let g0 = g0.children_named("interface").find(|i| i.text_of("name") == Some("GigabitEthernet0/0")).unwrap();
        let v6 = g0.child("ipv6").unwrap();
        assert_eq!(v6.ns.as_deref(), Some(rc::namespace(IP)));
        assert_eq!(v6.child("address").and_then(|a| a.text_of("prefix-length")), Some("64"));
        assert!(running(&s).contains(" ipv6 address 2001:db8:10::1/64"));
        let del = format!(
            "<edit-config xmlns:nc=\"{NS}\"><target><running/></target><config><interfaces xmlns=\"{}\"><interface><name>GigabitEthernet0/0</name>\
             <ipv6 xmlns=\"{}\" nc:operation=\"delete\"/></interface></interfaces></config></edit-config>",
            rc::namespace(IF),
            rc::namespace(IP)
        );
        let r = nc(&mut s, &[&del], "1.1");
        assert!(r["replies"][0].as_str().unwrap().contains("<ok/>"), "{r}");
        assert!(!running(&s).contains("2001:db8:10::1"));
    }

    #[test]
    fn errors_are_rpc_errors() {
        let mut s = lab();
        let r = nc(
            &mut s,
            &[
                "<get-config><source><candidate/></source></get-config>",
                "<commit/>",
                "<kill-session><session-id>9</session-id></kill-session>",
                "<rpc><get/></rpc>",
                "<get-config><source><running/></source><filter type=\"xpath\" select=\"/x\"/></get-config>",
                "<edit-config><target><running/></target><config><system xmlns=\"urn:ietf:params:xml:ns:yang:ietf-system\"><hostname>two words</hostname></system></config></edit-config>",
                "<edit-config><target><running/></target><config><widgets/></config></edit-config>",
                "<lock><target><running/></target></lock>",
                "<not xml",
            ],
            "1.1",
        );
        let replies: Vec<&str> = r["replies"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
        assert_eq!(replies.len(), 10, "{r}");
        for (i, tag) in [
            "invalid-value",
            "operation-not-supported",
            "operation-not-supported",
            "missing-attribute",
            "operation-not-supported",
            "invalid-value",
            "unknown-element",
        ]
        .iter()
        .enumerate()
        {
            assert!(replies[i].contains(&format!("<error-tag>{tag}</error-tag>")), "{i}: {}", replies[i]);
        }
        assert!(replies[7].contains("<ok/>"));
        assert!(replies[8].contains("malformed-message"));
        assert!(!running(&s).contains("two words"));
    }

    #[test]
    fn logins_and_the_service_are_checked() {
        let mut s = lab();
        let r = s.execute("netconf.session", json!({"pc": "PC1", "target": "192.168.1.1", "user": "admin", "password": "nope"})).unwrap();
        assert_eq!(r["connected"], false);
        assert!(r["error"].as_str().unwrap().contains("Permission denied"), "{r}");
        s.execute("device.configure", json!({"device": "R1", "lines": "no netconf-yang"})).unwrap();
        let e = s.execute("netconf.session", json!({"pc": "PC1", "target": "192.168.1.1", "user": "admin", "password": "NetLab2026"})).unwrap_err();
        assert!(e.to_string().contains("refused"), "{e}");
        for bad in [
            json!({"pc": "PC1", "target": "192.168.1.1", "user": "a b", "password": "x"}),
            json!({"pc": "PC1", "target": "192.168.1.1", "user": "a", "password": "x", "framing": "2.0"}),
            json!({"pc": "PC1", "target": "192.168.1.1", "user": "a", "password": "x", "rpcs": 5}),
            json!({"pc": "R1", "target": "192.168.1.1", "user": "a", "password": "x"}),
        ] {
            assert!(s.execute("netconf.session", bad.clone()).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_server_survives_hostile_streams() {
        // Straight at the handler: any bytes, any order.
        let s = lab();
        let r1 = s.topology.resolve("R1").unwrap();
        let junk = [
            "",
            "<hello/>",
            "<hello xmlns=\"x\"><session-id>1</session-id></hello>",
            "<rpc message-id=\"1\"/>",
            "<!DOCTYPE x [<!ENTITY a \"b\">]><rpc>&a;</rpc>",
            "<rpc message-id='1'><edit-config><target><running/></target><config><interfaces><interface nc:operation='x'/></interfaces></config></edit-config></rpc>",
            "<rpc message-id='1'><edit-config><target><running/></target><config><interfaces><interface operation='bogus'><name>Loopback1</name></interface></interfaces></config></edit-config></rpc>",
            "<rpc message-id='1'><edit-config><target><running/></target><config><static-routes><route><prefix>junk</prefix></route></static-routes></config></edit-config></rpc>",
        ];
        for first_hello in [false, true] {
            for j in junk {
                let mut sess = NetconfSession { id: 1, hello_done: first_hello, ..NetconfSession::default() };
                let step = serve(&s.topology, r1, &mut sess, NetconfEvent::Message(j));
                assert!(step.lines.is_empty(), "{j}");
            }
        }
    }
}
