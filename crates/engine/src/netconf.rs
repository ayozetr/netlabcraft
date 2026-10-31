//! NETCONF (RFC 6241), engine side: what a router answers in a session, and the `netconf.session`
//! command a host uses to open one.
//!
//! The data is the same subset RESTCONF serves ([`crate::automation`]), in XML: `system`
//! (ietf-system), `interfaces` (ietf-interfaces, with ietf-ip's `ipv4` and `ipv6`) and `static-routes`
//! (netlabcraft-routing). Operations: `<get-config>` and `<get>` (with subtree filters on the top
//! containers and on an interface's name), `<edit-config>` on `<running/>` or `<candidate/>`
//! (default operation merge, replace or none; `operation` attributes merge, replace, create,
//! delete and remove on interfaces, routes and an interface's description, ipv4 and ipv6),
//! `<commit>`, `<discard-changes>`, `<validate>`, `<lock>`/`<unlock>` on either datastore,
//! and `<close-session>`.
//!
//! # Datastores
//!
//! `running` is the router's configuration. The `candidate` (RFC 6241 §8.3) is shared by the
//! router's sessions and kept as the edits made to it since the last commit, replayed on top of
//! running when it is read, validated or committed; `<commit>` applies them through the
//! console grammar. Locks (§7.5) belong to a session: another session's edits, commits and
//! locks on a locked datastore are refused (`in-use`, `lock-denied` with the holder's session
//! id), the candidate can't be locked while it holds someone's uncommitted changes, and when the
//! session holding the candidate's lock unlocks it or ends, its uncommitted changes are
//! discarded. Not supported: confirmed commits, `<copy-config>`, `<delete-config>`,
//! `<kill-session>`, the startup datastore.

use netlabcraft_core::DeviceId;
use netlabcraft_model::Topology;
use netlabcraft_sim::automation::{Datastores, NetconfEvent, NetconfSession, NetconfStep, OpenClient};
use netlabcraft_wire::netconf::{self, BASE_10, BASE_11, Framing, NS};
use netlabcraft_wire::xml::{self, Element, escape};
use serde_json::{Map, Value, json};

use crate::automation::{self as rc, ApiError, IF, IP, MODULES, Op, RT, SYS, Target};
use crate::{EngineError, Result, Session};

pub const WRITABLE_RUNNING: &str = "urn:ietf:params:netconf:capability:writable-running:1.0";
pub const CANDIDATE: &str = "urn:ietf:params:netconf:capability:candidate:1.0";
pub const VALIDATE: &str = "urn:ietf:params:netconf:capability:validate:1.1";
const IANA_IF: &str = "urn:ietf:params:xml:ns:yang:iana-if-type";

/// The server's `<hello>`.
pub fn server_hello(session: u32) -> String {
    let mut caps = vec![BASE_10.to_string(), BASE_11.to_string(), WRITABLE_RUNNING.to_string(), CANDIDATE.to_string(), VALIDATE.to_string()];
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

/// An `<rpc-error>` naming the session that holds a lock (RFC 6241 §7.5).
fn lock_error(id: &str, tag: &str, message: &str, holder: u32) -> String {
    rpc_error(Some(id), "protocol", tag, message)
        .replace("</error-message></rpc-error>", &format!("</error-message><error-info><session-id>{holder}</session-id></error-info></rpc-error>"))
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

/// Which datastore a `<source>` or `<target>` names.
fn datastore_of(e: Option<&Element>) -> Option<&'static str> {
    let e = e?;
    if e.child("running").is_some() {
        Some("running")
    } else if e.child("candidate").is_some() {
        Some("candidate")
    } else {
        None
    }
}

/// Another session holds this datastore's lock: its id.
fn locked_by_other(ds: &Datastores, store: &str, me: u32) -> Option<u32> {
    ds.locks.get(store).copied().filter(|h| *h != me)
}

/// The router's side of a session.
pub fn serve(t: &Topology, dev: DeviceId, s: &mut NetconfSession, ds: &mut Datastores, ev: NetconfEvent<'_>) -> NetconfStep {
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
    let which = "name a datastore: <running/> or <candidate/>";
    let in_use = |store: &str, holder: u32| answer(lock_error(&id, "in-use", &format!("session {holder} holds the {store} lock"), holder));
    // The candidate as this session sees it: running with the session's edits, then the
    // candidate's pending ones.
    let candidate_view = |s: &NetconfSession, ds: &Datastores| view(t, dev, &[s.applied.clone(), ds.candidate.clone()].concat());
    match op.name.as_str() {
        "get-config" | "get" => {
            let store = if op.name == "get" { Some("running") } else { datastore_of(op.child("source")) };
            let v = match store {
                Some("running") => view(t, dev, &s.applied),
                Some(_) => candidate_view(s, ds),
                None => return answer(rpc_error(Some(&id), "protocol", "invalid-value", which)),
            };
            match data(&v, dev, op.child("filter"), op.name == "get-config") {
                Ok(d) => answer(reply(&id, &d)),
                Err(e) => answer(rpc_error(Some(&id), "protocol", "operation-not-supported", &e)),
            }
        }
        "edit-config" => {
            let Some(store) = datastore_of(op.child("target")) else {
                return answer(rpc_error(Some(&id), "protocol", "invalid-value", which));
            };
            if let Some(h) = locked_by_other(ds, store, s.id) {
                return in_use(store, h);
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
            let v = if store == "running" { view(t, dev, &s.applied) } else { candidate_view(s, ds) };
            match edit_config(&v, dev, config, default) {
                Ok(lines) if store == "candidate" => {
                    if ds.candidate.len().saturating_add(lines.len()) > MAX_CANDIDATE {
                        return answer(rpc_error(
                            Some(&id),
                            "application",
                            "resource-denied",
                            "the candidate holds too many changes: commit or discard them",
                        ));
                    }
                    ds.candidate.extend(lines);
                    answer(reply(&id, "<ok/>"))
                }
                Ok(lines) => {
                    s.applied.extend(lines.iter().cloned());
                    NetconfStep { reply: Some(reply(&id, "<ok/>")), close: false, lines }
                }
                Err((e, deleting)) => answer(api_error(&id, &e, deleting)),
            }
        }
        "commit" => {
            if op.child("confirmed").is_some() || op.child("persist-id").is_some() {
                return answer(rpc_error(Some(&id), "protocol", "operation-not-supported", "confirmed commits are not supported"));
            }
            if let Some(h) = locked_by_other(ds, "running", s.id).or_else(|| locked_by_other(ds, "candidate", s.id)) {
                return in_use("running or candidate", h);
            }
            let lines = std::mem::take(&mut ds.candidate);
            if let Err(e) = rc::check(&view(t, dev, &s.applied), dev, &lines) {
                ds.candidate = lines;
                return answer(rpc_error(Some(&id), "application", "operation-failed", &format!("the candidate does not apply: {}", e.message)));
            }
            s.applied.extend(lines.iter().cloned());
            NetconfStep { reply: Some(reply(&id, "<ok/>")), close: false, lines }
        }
        "discard-changes" => {
            if let Some(h) = locked_by_other(ds, "candidate", s.id) {
                return in_use("candidate", h);
            }
            ds.candidate.clear();
            answer(reply(&id, "<ok/>"))
        }
        "validate" => match datastore_of(op.child("source")) {
            Some("running") => answer(reply(&id, "<ok/>")),
            Some(_) => match rc::check(&view(t, dev, &s.applied), dev, &ds.candidate) {
                Ok(()) => answer(reply(&id, "<ok/>")),
                Err(e) => answer(rpc_error(Some(&id), "application", "operation-failed", &format!("the candidate does not apply: {}", e.message))),
            },
            None => answer(rpc_error(Some(&id), "protocol", "invalid-value", which)),
        },
        "lock" => {
            let Some(store) = datastore_of(op.child("target")) else {
                return answer(rpc_error(Some(&id), "protocol", "invalid-value", which));
            };
            if let Some(&h) = ds.locks.get(store) {
                return answer(lock_error(&id, "lock-denied", &format!("the {store} lock is already held by session {h}"), h));
            }
            if store == "candidate" && !ds.candidate.is_empty() {
                return answer(lock_error(&id, "lock-denied", "the candidate holds uncommitted changes: commit or discard them first", 0));
            }
            ds.locks.insert(store.to_string(), s.id);
            answer(reply(&id, "<ok/>"))
        }
        "unlock" => {
            let Some(store) = datastore_of(op.child("target")) else {
                return answer(rpc_error(Some(&id), "protocol", "invalid-value", which));
            };
            if ds.locks.get(store) != Some(&s.id) {
                return answer(rpc_error(Some(&id), "protocol", "operation-failed", &format!("this session does not hold the {store} lock")));
            }
            ds.locks.remove(store);
            if store == "candidate" {
                ds.candidate.clear();
            }
            answer(reply(&id, "<ok/>"))
        }
        "close-session" => close(Some(reply(&id, "<ok/>"))),
        "copy-config" | "delete-config" | "cancel-commit" | "kill-session" => {
            answer(rpc_error(Some(&id), "protocol", "operation-not-supported", &format!("<{}> is not supported by this router", op.name)))
        }
        other => answer(rpc_error(Some(&id), "protocol", "operation-not-supported", &format!("<{other}> is not an operation this router supports"))),
    }
}

/// Most configuration lines the candidate holds.
const MAX_CANDIDATE: usize = 2_048;

// ---- the client command ----

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// Most RPCs in one `netconf.session`.
const MAX_RPCS: usize = 32;

/// The framed messages at the front of `buf` (end-of-message framing for the first
/// `eom_first` of them), and the bytes they used.
fn messages(buf: &[u8], eom_first: usize, framing: Framing) -> (Vec<String>, usize) {
    let mut out = Vec::new();
    let mut used = 0usize;
    for _ in 0..(MAX_RPCS + 8) {
        let f = if out.len() < eom_first { Framing::EndOfMessage } else { framing };
        match netconf::decode(buf.get(used..).unwrap_or_default(), f) {
            Ok(Some((m, n))) => {
                out.push(m);
                used += n;
            }
            _ => break,
        }
    }
    (out, used)
}

/// Sends RPCs on an open session (each wrapped in an `<rpc>` if given bare) after `prefix`
/// (the client's hello, on a new session), and collects their replies; `then_close` adds a
/// `<close-session>`. Whether the server closed.
fn run_rpcs(s: &mut Session, c: &mut OpenClient, prefix: &[u8], rpcs: &[String], then_close: bool) -> (Vec<Value>, bool) {
    let framing = if c.chunked { Framing::Chunked } else { Framing::EndOfMessage };
    let mut stream = prefix.to_vec();
    for r in rpcs {
        let t = r.trim();
        let wrapped = if t.starts_with("<rpc") || t.starts_with("<?xml") || t.starts_with("<nc:rpc") {
            t.to_string()
        } else {
            c.next_id = c.next_id.saturating_add(1);
            format!("<rpc message-id=\"{}\" xmlns=\"{NS}\">{t}</rpc>", c.next_id)
        };
        stream.extend(netconf::encode(&wrapped, framing));
    }
    if then_close {
        c.next_id = c.next_id.saturating_add(1);
        stream.extend(netconf::encode(&format!("<rpc message-id=\"{}\" xmlns=\"{NS}\"><close-session/></rpc>", c.next_id), framing));
    }
    let want = rpcs.len() + usize::from(then_close);
    if want == 0 {
        if !stream.is_empty() {
            s.sim.ssh_push(&s.topology, c, &stream);
        }
        return (Vec::new(), c.tcp.closed);
    }
    let _ = s.sim.ssh_exchange(&s.topology, c, &stream, &|b: &[u8]| messages(b, 0, framing).0.len() >= want);
    let (msgs, used) = messages(&c.buf, 0, framing);
    c.buf.drain(..used.min(c.buf.len()));
    (msgs.into_iter().map(Value::String).collect(), c.tcp.closed)
}

/// What the session's SSH transport is, for the answer.
fn ssh_info() -> Value {
    use netlabcraft_wire::ssh;
    json!({"version": ssh::IDENT, "kex": ssh::KEX, "cipher": ssh::CIPHER, "mac": ssh::MAC, "host_key": ssh::HOST_KEY})
}

/// `netconf.session`: opens a session (or goes on with one kept open), sends the RPCs and
/// returns what the router answered. Without `keep_open` the session ends with a
/// `<close-session>`. It runs as the `netconf` subsystem of an SSH session on port 830.
pub(crate) fn session(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::cmd::device_param(s, p, "pc")?;
    if s.topology.device(d).is_none_or(|x| !x.kind.is_host()) {
        return Err(bad("NETCONF sessions are opened from PCs, laptops and servers"));
    }
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
    let keep = p.get("keep_open").and_then(Value::as_bool).unwrap_or(false);
    // Going on with a kept session.
    if let Some(h) = p.get("session") {
        let h = h.as_u64().and_then(|h| u32::try_from(h).ok()).ok_or_else(|| bad("`session` is the number a kept session was given"))?;
        let mut c =
            s.sim.take_client(h).ok_or_else(|| EngineError::Failed(format!("no open NETCONF session {h} (it was closed, or the lab was reset)")))?;
        if c.tcp.dev != d {
            s.sim.return_client(h, c);
            return Err(bad(format!("session {h} belongs to another host")));
        }
        let close = p.get("close").and_then(Value::as_bool).unwrap_or(false);
        let (replies, closed) = run_rpcs(s, &mut c, &[], &rpcs, close);
        let session_id = c.session_id;
        let open = !closed && !close;
        if open {
            s.sim.return_client(h, c);
        } else {
            s.sim.ssh_close(&s.topology, &mut c);
        }
        let applied = rc::apply_changes(s);
        s.revision += 1;
        return Ok(
            json!({"connected": true, "session": if open { json!(h) } else { Value::Null }, "session_id": session_id, "replies": replies, "applied": applied, "ssh": ssh_info()}),
        );
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
    let server = rc::resolve(s, d, &host)?;
    let mut c = match s.sim.ssh_open(&s.topology, d, server, netconf::PORT, user, password) {
        Ok(c) => c,
        Err(e) if e.to_string().contains("Permission denied") => {
            let applied = rc::apply_changes(s);
            return Ok(json!({"connected": false, "error": e.to_string(), "hello": null, "replies": [], "applied": applied}));
        }
        Err(e) => return Err(EngineError::Failed(e.to_string())),
    };
    c.chunked = chunked;
    // The server's hello came with the subsystem's start.
    let (hello, used) = messages(&c.buf, 1, Framing::EndOfMessage);
    let Some(hello) = hello.into_iter().next() else {
        s.sim.ssh_close(&s.topology, &mut c);
        let applied = rc::apply_changes(s);
        return Ok(json!({"connected": false, "error": "the server sent no <hello>", "hello": null, "replies": [], "applied": applied}));
    };
    c.buf.drain(..used.min(c.buf.len()));
    c.session_id = xml::parse(&hello).ok().and_then(|h| h.text_of("session-id").and_then(|x| x.parse().ok())).unwrap_or(0);
    let mine = netconf::encode(&client_hello(chunked), Framing::EndOfMessage);
    let (replies, closed) = run_rpcs(s, &mut c, &mine, &rpcs, !keep);
    let session_id = c.session_id;
    let handle = if keep && !closed {
        Some(s.sim.keep_client(c))
    } else {
        s.sim.ssh_close(&s.topology, &mut c);
        None
    };
    let applied = rc::apply_changes(s);
    s.revision += 1;
    Ok(
        json!({"connected": true, "framing": if chunked { "1.1" } else { "1.0" }, "session": handle, "session_id": session_id, "hello": hello, "replies": replies, "applied": applied, "ssh": ssh_info()}),
    )
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
            assert!(s.sim.trace().iter().any(|e| e.protocol == netlabcraft_wire::Protocol::Ssh && e.summary.contains("KEXINIT")), "{framing}");
            assert!(s.sim.trace().iter().all(|e| !e.summary.contains("<hello")), "NETCONF is inside encrypted SSH packets");
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

    fn desc_edit(store: &str, text: &str) -> String {
        format!(
            "<edit-config><target><{store}/></target><config><interfaces xmlns=\"{}\"><interface><name>GigabitEthernet0/1</name><description>{text}</description></interface></interfaces></config></edit-config>",
            rc::namespace(IF)
        )
    }

    #[test]
    fn the_candidate_holds_edits_until_commit() {
        let mut s = lab();
        let r = nc(
            &mut s,
            &[
                &desc_edit("candidate", "Staged"),
                "<get-config><source><candidate/></source></get-config>",
                "<get-config><source><running/></source></get-config>",
                "<validate><source><candidate/></source></validate>",
            ],
            "1.1",
        );
        let hello = r["hello"].as_str().unwrap();
        assert!(hello.contains(CANDIDATE) && hello.contains(VALIDATE) && hello.contains(WRITABLE_RUNNING), "{hello}");
        let replies: Vec<&str> = r["replies"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
        assert!(replies[0].contains("<ok/>"), "{r}");
        assert!(replies[1].contains("Staged") && !replies[2].contains("Staged"), "{r}");
        assert!(replies[3].contains("<ok/>"));
        assert!(!running(&s).contains("Staged"), "the candidate is not running");
        // Discard throws it away; commit applies it.
        let r = nc(&mut s, &["<discard-changes/>", "<get-config><source><candidate/></source></get-config>"], "1.1");
        assert!(!r["replies"][1].as_str().unwrap().contains("Staged"));
        let r = nc(&mut s, &[&desc_edit("candidate", "Committed"), "<commit/>"], "1.0");
        assert!(r["replies"][1].as_str().unwrap().contains("<ok/>"), "{r}");
        assert!(running(&s).contains(" description Committed"), "{}", running(&s));
        assert!(s.sim.datastores(s.topology.resolve("R1").unwrap()).candidate.is_empty());
    }

    #[test]
    fn locks_keep_other_sessions_out() {
        let mut s = lab();
        let open = |s: &mut Session, pc: &str, rpcs: &[&str]| {
            s.execute(
                "netconf.session",
                json!({"pc": pc, "target": "192.168.1.1", "user": "admin", "password": "NetLab2026", "rpcs": rpcs, "keep_open": true}),
            )
            .unwrap()
        };
        let a = open(&mut s, "PC1", &["<lock><target><candidate/></target></lock>", &desc_edit("candidate", "From A")]);
        let ha = a["session"].as_u64().unwrap();
        let a_id = a["session_id"].as_u64().unwrap();
        assert!(a["replies"][0].as_str().unwrap().contains("<ok/>"), "{a}");
        s.execute("cli.exec", json!({"device": "R1", "line": "enable"})).unwrap();
        let out = s.execute("cli.exec", json!({"device": "R1", "line": "show netconf-yang sessions"})).unwrap();
        let out = out["output"].as_str().unwrap();
        assert!(out.contains(&format!("The candidate datastore is locked by session {a_id}.")) && out.contains("uncommitted"), "{out}");
        // B: lock denied with A's session id; edits and commits refused while A holds it.
        let b = open(
            &mut s,
            "PC2",
            &[
                "<lock><target><candidate/></target></lock>",
                &desc_edit("candidate", "From B"),
                "<commit/>",
                "<discard-changes/>",
                "<lock><target><running/></target></lock>",
            ],
        );
        let hb = b["session"].as_u64().unwrap();
        let br: Vec<&str> = b["replies"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
        assert!(br[0].contains("<error-tag>lock-denied</error-tag>") && br[0].contains(&format!("<session-id>{a_id}</session-id>")), "{}", br[0]);
        for x in &br[1..4] {
            assert!(x.contains("<error-tag>in-use</error-tag>"), "{x}");
        }
        assert!(br[4].contains("<ok/>"), "B may lock running");
        // Now A can't commit (B holds running); B can't unlock what it doesn't hold.
        let a2 = s.execute("netconf.session", json!({"pc": "PC1", "session": ha, "rpcs": ["<commit/>"]})).unwrap();
        assert!(a2["replies"][0].as_str().unwrap().contains("in-use"), "{a2}");
        let b2 = s.execute("netconf.session", json!({"pc": "PC2", "session": hb, "rpcs": ["<unlock><target><candidate/></target></unlock>", "<unlock><target><running/></target></unlock>"]})).unwrap();
        assert!(b2["replies"][0].as_str().unwrap().contains("operation-failed") && b2["replies"][1].as_str().unwrap().contains("<ok/>"), "{b2}");
        // A commits, unlocks and closes.
        let a3 = s
            .execute(
                "netconf.session",
                json!({"pc": "PC1", "session": ha, "rpcs": ["<commit/>", "<unlock><target><candidate/></target></unlock>"], "close": true}),
            )
            .unwrap();
        assert!(a3["replies"][0].as_str().unwrap().contains("<ok/>"), "{a3}");
        assert!(a3["session"].is_null());
        assert!(running(&s).contains(" description From A"));
        assert!(s.execute("netconf.session", json!({"pc": "PC1", "session": ha, "rpcs": []})).is_err(), "a closed session is gone");
        assert!(s.execute("netconf.session", json!({"pc": "PC1", "session": hb, "rpcs": []})).is_err(), "another host's session");
        // A session that ends holding the candidate's lock releases it and drops its changes.
        let b3 = s.execute("netconf.session", json!({"pc": "PC2", "session": hb, "rpcs": ["<lock><target><candidate/></target></lock>", desc_edit("candidate", "Lost")], "close": true})).unwrap();
        assert!(b3["replies"][0].as_str().unwrap().contains("<ok/>"), "{b3}");
        let r1 = s.topology.resolve("R1").unwrap();
        let ds = s.sim.datastores(r1);
        assert!(ds.locks.is_empty() && ds.candidate.is_empty(), "{ds:?}");
        // The candidate can't be locked while it holds someone's uncommitted changes.
        let r = nc(&mut s, &[&desc_edit("candidate", "Pending")], "1.1");
        assert!(r["replies"][0].as_str().unwrap().contains("<ok/>"));
        let r = nc(&mut s, &["<lock><target><candidate/></target></lock>"], "1.1");
        assert!(r["replies"][0].as_str().unwrap().contains("lock-denied"), "{r}");
    }

    #[test]
    fn netconf_runs_inside_encrypted_ssh() {
        let mut s = lab();
        let r = nc(&mut s, &["<get-config><source><running/></source></get-config>"], "1.1");
        assert_eq!(r["ssh"]["kex"], netlabcraft_wire::ssh::KEX, "{r}");
        let mut kinds = Vec::new();
        for e in s.sim.trace() {
            for secret in [&b"NetLab2026"[..], b"get-config", b"GigabitEthernet"] {
                assert!(!e.frame.windows(secret.len()).any(|w| w == secret), "{}", e.summary);
            }
            if e.protocol == netlabcraft_wire::Protocol::Ssh {
                kinds.push(e.summary.clone());
            }
        }
        for want in ["identification SSH-2.0-", "KEXINIT", "KEXDH_INIT", "KEXDH_REPLY", "NEWKEYS", "encrypted packets"] {
            assert!(kinds.iter().any(|k| k.contains(want)), "{want}: {kinds:?}");
        }
        // The old clear-text login, or any non-SSH bytes, is refused.
        let pc1 = s.topology.resolve("PC1").unwrap();
        let got = s.sim.tcp_session(&s.topology, pc1, "192.168.1.1".parse().unwrap(), netconf::PORT, b"user admin NetLab2026\r\n").unwrap();
        assert_eq!(got, b"Protocol mismatch.\r\n");
        // A wrong password ends the session before any NETCONF.
        let r = s.execute("netconf.session", json!({"pc": "PC1", "target": "192.168.1.1", "user": "admin", "password": "nope"})).unwrap();
        assert_eq!(r["connected"], false);
        assert!(r["error"].as_str().unwrap().contains("Permission denied"), "{r}");
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
                "<get-config><source><startup/></source></get-config>",
                "<commit><confirmed/></commit>",
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
                let step = serve(&s.topology, r1, &mut sess, &mut Datastores::default(), NetconfEvent::Message(j));
                assert!(step.lines.is_empty(), "{j}");
            }
        }
    }
}
