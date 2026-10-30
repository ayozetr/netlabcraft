//! Simulation scenarios and their PDUs: simple PDUs (one echo request), complex PDUs built
//! field by field, fired once or periodically in simulated time (`pdu.*`, `scenario.*`).

use netlabcraft_core::{DeviceId, parse_ip};
use netlabcraft_model::scenario::{
    MAX_PDU_SIZE, MAX_PDU_TEXT, MAX_PDUS, MAX_PERIOD_S, MAX_SCENARIOS, MIN_PERIOD_S, PduApp, PduStatus, Scenario, UserPdu,
};
use netlabcraft_sim::pdu::PduSpec;
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

/// Most PDU firings one fast-forward runs (a short period over a long stretch of time).
pub const MAX_FIRES_PER_ADVANCE: usize = 2_000;

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

fn num_param(p: &Value) -> Result<u32> {
    p.get("num").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).ok_or_else(|| bad("`num` (the PDU's number in the list) is required"))
}

fn u16_param(p: &Value, key: &str) -> Result<Option<u16>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().and_then(|v| u16::try_from(v).ok()).map(Some).ok_or_else(|| bad(format!("`{key}` is a number from 0 to 65535"))),
    }
}

fn secs_param(p: &Value, key: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|v| v.is_finite() && *v >= 0.0).map(Some).ok_or_else(|| bad(format!("`{key}` is a number of seconds"))),
    }
}

/// A PDU as the list shows it.
pub(crate) fn pdu_json(s: &Session, p: &UserPdu) -> Value {
    let t = &s.topology;
    let name = |d: DeviceId| t.device(d).map_or("?".to_string(), |x| x.name.clone());
    json!({
        "num": p.num,
        "source": name(p.source),
        "port": p.port,
        "destination": p.destination,
        "target": p.target.map(name),
        "type": p.type_name(),
        "app": p.app.id(),
        "simple": p.simple,
        "src_port": p.src_port,
        "dst_port": p.dst_port,
        "ttl": p.ttl,
        "tos": p.tos,
        "seq": p.seq,
        "size": p.size,
        "text": p.text,
        "period": p.period_s,
        "start": p.start_s,
        "status": p.status.id(),
        "last": p.last_s,
        "fired": p.fired,
        "note": p.note,
    })
}

fn list_json(s: &Session) -> Value {
    let sc = &s.topology.scenarios;
    json!({
        "scenario": sc.current,
        "scenarios": sc.count(),
        "description": sc.list.get(sc.current).map_or("", |x| x.description.as_str()),
        "pdus": sc.pdus().iter().map(|p| pdu_json(s, p)).collect::<Vec<_>>(),
    })
}

/// The destination: an address, or a device whose first address is used.
fn destination(s: &Session, to: &str) -> Result<(String, Option<DeviceId>)> {
    let to = to.trim();
    if let Ok(a) = parse_ip(to) {
        let owner = s.topology.devices.iter().find(|d| netlabcraft_sim::l3_ifaces(d).iter().any(|i| i.net.addr == a)).map(|d| d.id);
        return Ok((a.to_string(), owner));
    }
    let id = s.topology.resolve(to).map_err(|_| bad(format!("`to`: {to} is neither an IPv4 address nor a device")))?;
    let d = s.topology.device(id).ok_or_else(|| EngineError::Failed("no such device".into()))?;
    let a = netlabcraft_sim::l3_ifaces(d)
        .first()
        .map(|i| i.net.addr)
        .ok_or_else(|| EngineError::Failed(format!("{} has no IP address to send a PDU to", d.name)))?;
    Ok((a.to_string(), Some(id)))
}

fn new_num(s: &mut Session) -> Result<u32> {
    let pdus = s.topology.scenarios.pdus();
    if pdus.len() >= MAX_PDUS {
        return Err(EngineError::Failed(format!("the scenario is full ({MAX_PDUS} PDUs)")));
    }
    Ok(pdus.iter().map(|p| p.num).max().unwrap_or(0).saturating_add(1))
}

/// Adds a PDU to the scenario in use (an undo step), and sends it now when it is due now.
fn add(s: &mut Session, pdu: UserPdu) -> Result<Value> {
    let now = s.sim.now_us();
    let mut pdu = pdu;
    pdu.next_us = Some(now.saturating_add(secs_us(pdu.start_s)));
    let num = pdu.num;
    let due_now = pdu.start_s <= 0.0;
    s.checkpoint();
    s.topology.scenarios.current_mut().pdus.push(pdu);
    s.dirty = true;
    s.revision += 1;
    if due_now {
        return fire(s, num);
    }
    let p = s.topology.scenarios.pdus().iter().find(|p| p.num == num).cloned();
    Ok(p.map_or(Value::Null, |p| pdu_json(s, &p)))
}

fn secs_us(secs: f64) -> u64 {
    if secs.is_finite() { (secs.clamp(0.0, MAX_PERIOD_S) * 1_000_000.0) as u64 } else { 0 }
}

/// Sends PDU `num` of the scenario in use now, and records how it did.
fn fire(s: &mut Session, num: u32) -> Result<Value> {
    let Some(p) = s.topology.scenarios.pdus().iter().find(|p| p.num == num).cloned() else {
        return Err(EngineError::Failed(format!("no PDU {num} in this scenario")));
    };
    let t = &s.topology;
    let port = match &p.port {
        Some(name) => {
            Some(t.device(p.source).and_then(|d| d.port_index(name)).ok_or_else(|| EngineError::Failed(format!("no port {name} on the source")))?)
        }
        None => None,
    };
    let dst = if p.app.broadcast() { None } else { Some(parse_ip(&p.destination).map_err(|e| EngineError::Failed(e.to_string()))?) };
    let spec = PduSpec {
        app: p.app,
        port,
        dst,
        src_port: p.src_port,
        dst_port: p.dst_port,
        ttl: p.ttl,
        tos: p.tos,
        seq: p.seq,
        size: if p.simple { 32 } else { p.size },
        text: p.text.clone(),
    };
    let sent_at = s.sim.now_us();
    let outcome = s.sim.send_pdu(&s.topology, p.source, &spec);
    s.revision += 1;
    let (ok, note, first) = match outcome {
        Ok(o) => (o.ok, o.note, Some(o.first_event)),
        Err(e) => (false, e.to_string(), None),
    };
    if let Some(x) = s.topology.scenarios.pdus_mut().iter_mut().find(|x| x.num == num) {
        x.status = if ok { PduStatus::Successful } else { PduStatus::Failed };
        x.last_s = Some(sent_at as f64 / 1e6);
        x.fired = x.fired.saturating_add(1);
        x.note = note.chars().take(MAX_PDU_TEXT).collect();
        x.next_us = x.period_s.map(|period| sent_at.saturating_add(secs_us(period)));
    }
    let mut v = s.topology.scenarios.pdus().iter().find(|x| x.num == num).map_or(Value::Null, |x| pdu_json(s, x));
    if let Some(o) = v.as_object_mut() {
        o.insert("first_event".into(), json!(first));
    }
    Ok(v)
}

/// Runs the simulation `us` microseconds ahead, sending scheduled and periodic PDUs as their
/// time comes. Returns how many were sent.
pub(crate) fn advance(s: &mut Session, us: u64) -> usize {
    let deadline = s.sim.now_us().saturating_add(us);
    let mut fired = 0;
    // A clock that went back (simulation reset) restarts the schedule from now.
    let now = s.sim.now_us();
    for p in s.topology.scenarios.pdus_mut() {
        // A reopened lab's periodic PDUs start again.
        if p.period_s.is_some() && p.next_us.is_none() {
            p.next_us = Some(now.saturating_add(secs_us(p.start_s)));
        }
        let span = secs_us(p.period_s.unwrap_or(0.0).max(p.start_s));
        if p.next_us.is_some_and(|n| n > now.saturating_add(span).saturating_add(1)) {
            p.next_us = Some(now);
        }
    }
    while fired < MAX_FIRES_PER_ADVANCE {
        let due = s.topology.scenarios.pdus().iter().filter_map(|p| p.next_us.filter(|n| *n <= deadline).map(|n| (n, p.num))).min();
        let Some((at, num)) = due else { break };
        let now = s.sim.now_us();
        if at > now {
            s.sim.run_for(&s.topology, at - now);
        }
        let _ = fire(s, num);
        fired += 1;
        // A one-shot PDU is done; a periodic one was rescheduled by `fire`, never earlier than
        // now (so a PDU that takes longer than its period cannot spin).
        if let Some(p) = s.topology.scenarios.pdus_mut().iter_mut().find(|p| p.num == num) {
            p.next_us = p.next_us.filter(|_| p.period_s.is_some()).map(|n| n.max(at.saturating_add(1)));
        }
    }
    let now = s.sim.now_us();
    if deadline > now {
        s.sim.run_for(&s.topology, deadline - now);
    }
    s.revision += 1;
    fired
}

fn scenario_json(s: &Session) -> Value {
    let sc = &s.topology.scenarios;
    let list: Vec<Value> = (0..sc.count())
        .map(|i| {
            let x = sc.list.get(i);
            json!({"index": i, "description": x.map_or("", |x| x.description.as_str()), "pdus": x.map_or(0, |x| x.pdus.len())})
        })
        .collect();
    json!({"current": sc.current, "scenarios": list})
}

pub(crate) fn run(s: &mut Session, cmd: &str, p: &Value) -> Result<Value> {
    match cmd {
        "pdu.simple" => {
            let from = crate::cmd::device_param(s, p, "from")?;
            let to = p.get("to").and_then(Value::as_str).ok_or_else(|| bad("`to` (address or device) is required"))?;
            let (destination, target) = destination(s, to)?;
            let num = new_num(s)?;
            add(
                s,
                UserPdu {
                    num,
                    source: from,
                    port: None,
                    destination,
                    target,
                    app: PduApp::Ping,
                    simple: true,
                    src_port: None,
                    dst_port: None,
                    ttl: None,
                    tos: 0,
                    seq: None,
                    size: 32,
                    text: String::new(),
                    period_s: None,
                    start_s: 0.0,
                    status: PduStatus::Waiting,
                    last_s: None,
                    fired: 0,
                    note: String::new(),
                    next_us: None,
                },
            )
        }
        "pdu.complex" => {
            let from = crate::cmd::device_param(s, p, "from")?;
            let app_text = p.get("app").and_then(Value::as_str).unwrap_or("ping");
            let app = PduApp::parse(app_text).ok_or_else(|| {
                bad(format!("`app`: {app_text} is not one of {}", PduApp::ALL.iter().map(|a| a.id()).collect::<Vec<_>>().join(", ")))
            })?;
            let port = match p.get("port").and_then(Value::as_str).map(str::trim).filter(|x| !x.is_empty()) {
                Some(name) => {
                    let d = s.topology.device(from).ok_or_else(|| EngineError::Failed("no such device".into()))?;
                    let i = d.port_index(name).ok_or_else(|| EngineError::Failed(format!("{} has no port {name}", d.name)))?;
                    d.ports.get(i).map(|x| x.name.clone())
                }
                None => None,
            };
            let (destination, target) = match p.get("to").and_then(Value::as_str).map(str::trim).filter(|x| !x.is_empty()) {
                Some(to) => destination(s, to)?,
                None if app.broadcast() => (String::new(), None),
                None => return Err(bad("`to` (address or device) is required")),
            };
            let ttl = match p.get("ttl") {
                None | Some(Value::Null) => None,
                Some(v) => Some(v.as_u64().filter(|v| (1..=255).contains(v)).ok_or_else(|| bad("`ttl` is a number from 1 to 255"))? as u8),
            };
            let tos = match (p.get("tos"), p.get("dscp")) {
                (Some(v), _) if !v.is_null() => v.as_u64().filter(|v| *v <= 255).ok_or_else(|| bad("`tos` is a number from 0 to 255"))? as u8,
                (_, Some(Value::String(name))) => {
                    netlabcraft_wire::qos::parse_dscp(name).ok_or_else(|| bad("`dscp` is 0-63 or a name such as ef or af31"))? << 2
                }
                (_, Some(v)) if !v.is_null() => v.as_u64().filter(|v| *v <= 63).ok_or_else(|| bad("`dscp` is a number from 0 to 63"))? as u8 * 4,
                _ => 0,
            };
            let seq = match p.get("seq") {
                None | Some(Value::Null) => None,
                Some(v) => Some(v.as_u64().and_then(|v| u32::try_from(v).ok()).ok_or_else(|| bad("`seq` is a number from 0 to 4294967295"))?),
            };
            let size = u16_param(p, "size")?.unwrap_or(32);
            if size > MAX_PDU_SIZE {
                return Err(bad(format!("`size` is at most {MAX_PDU_SIZE} bytes")));
            }
            let period_s = secs_param(p, "period")?;
            if period_s.is_some_and(|x| !(MIN_PERIOD_S..=MAX_PERIOD_S).contains(&x)) {
                return Err(bad(format!("`period` is from {MIN_PERIOD_S} to {MAX_PERIOD_S} seconds")));
            }
            let start_s = secs_param(p, "start")?.unwrap_or(0.0).min(MAX_PERIOD_S);
            let text: String = p.get("text").and_then(Value::as_str).unwrap_or_default().trim().chars().take(MAX_PDU_TEXT).collect();
            let num = new_num(s)?;
            add(
                s,
                UserPdu {
                    num,
                    source: from,
                    port,
                    destination,
                    target,
                    app,
                    simple: false,
                    src_port: u16_param(p, "src_port")?,
                    dst_port: u16_param(p, "dst_port")?,
                    ttl,
                    tos,
                    seq,
                    size,
                    text,
                    period_s,
                    start_s,
                    status: PduStatus::Waiting,
                    last_s: None,
                    fired: 0,
                    note: String::new(),
                    next_us: None,
                },
            )
        }
        "pdu.fire" => {
            let num = num_param(p)?;
            fire(s, num)
        }
        "pdu.fire_all" => {
            let nums: Vec<u32> = s.topology.scenarios.pdus().iter().map(|p| p.num).collect();
            let results: Vec<Value> = nums.into_iter().map(|n| fire(s, n).unwrap_or(Value::Null)).collect();
            Ok(json!({"pdus": results}))
        }
        "pdu.delete" => {
            let all = p.get("all").and_then(Value::as_bool).unwrap_or(false);
            let num = if all { None } else { Some(num_param(p)?) };
            if let Some(n) = num
                && !s.topology.scenarios.pdus().iter().any(|x| x.num == n)
            {
                return Err(EngineError::Failed(format!("no PDU {n} in this scenario")));
            }
            s.checkpoint();
            s.topology.scenarios.current_mut().pdus.retain(|x| num.is_some_and(|n| x.num != n));
            s.dirty = true;
            s.revision += 1;
            Ok(list_json(s))
        }
        "pdu.list" => Ok(list_json(s)),
        "sim.event" => {
            let id = p.get("id").and_then(Value::as_u64).ok_or_else(|| bad("`id` (event number) is required"))?;
            let e = s.sim.trace().iter().find(|e| e.id == id).ok_or_else(|| EngineError::Failed(format!("no event {id} in the event list")))?;
            let t = &s.topology;
            let osi = netlabcraft_sim::explain::osi(t, &s.sim, e);
            let (inbound, outbound) = netlabcraft_sim::explain::osi_lines(&osi);
            let side = |l: &netlabcraft_sim::explain::Layers| -> Vec<Value> {
                l.iter().enumerate().filter(|(_, x)| !x.is_empty()).map(|(i, x)| json!({"layer": i + 1, "lines": x})).collect()
            };
            Ok(json!({
                "id": e.id,
                "at_us": e.at_us,
                "from": t.end_label(e.from),
                "to": t.end_label(e.to),
                "protocol": e.protocol.name(),
                "summary": e.summary,
                "dropped": e.dropped,
                "cause": e.cause,
                "explain": e.explain.iter().map(netlabcraft_sim::explain::Step::describe).collect::<Vec<_>>(),
                "osi": {"in": side(&inbound), "out": side(&outbound)},
                "sent": osi.sent,
            }))
        }
        "scenario.list" => Ok(scenario_json(s)),
        "scenario.new" => {
            let sc = &s.topology.scenarios;
            if sc.count() >= MAX_SCENARIOS {
                return Err(EngineError::Failed(format!("a lab holds at most {MAX_SCENARIOS} scenarios")));
            }
            let description: String = p.get("description").and_then(Value::as_str).unwrap_or_default().trim().chars().take(MAX_PDU_TEXT).collect();
            s.checkpoint();
            let sc = &mut s.topology.scenarios;
            if sc.list.is_empty() {
                sc.list.push(Scenario::default());
            }
            sc.list.push(Scenario { description, pdus: Vec::new() });
            sc.current = sc.list.len() - 1;
            s.dirty = true;
            s.revision += 1;
            Ok(scenario_json(s))
        }
        "scenario.delete" => {
            let sc = &s.topology.scenarios;
            let index = match p.get("index") {
                None | Some(Value::Null) => sc.current,
                Some(v) => v.as_u64().and_then(|v| usize::try_from(v).ok()).ok_or_else(|| bad("`index` is a scenario number"))?,
            };
            if index >= sc.count() {
                return Err(EngineError::Failed(format!("no scenario {index}")));
            }
            s.checkpoint();
            let sc = &mut s.topology.scenarios;
            if sc.list.len() <= 1 {
                // The last scenario stays, emptied.
                sc.list.clear();
                sc.current = 0;
            } else {
                sc.list.remove(index);
                if sc.current > index || sc.current >= sc.list.len() {
                    sc.current = sc.current.saturating_sub(1);
                }
            }
            s.dirty = true;
            s.revision += 1;
            Ok(scenario_json(s))
        }
        "scenario.select" => {
            let index = p
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| bad("`index` (scenario number) is required"))?;
            if index >= s.topology.scenarios.count() {
                return Err(EngineError::Failed(format!("no scenario {index}")));
            }
            s.topology.scenarios.current = index;
            let now = s.sim.now_us();
            // The scenario's periodic PDUs start again from now.
            for x in s.topology.scenarios.pdus_mut() {
                x.next_us = x.period_s.map(|_| now.saturating_add(secs_us(x.start_s)));
            }
            s.dirty = true;
            s.revision += 1;
            Ok(list_json(s))
        }
        other => Err(EngineError::UnknownCommand(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use netlabcraft_wire::tcp::{self, Tcp};
    use netlabcraft_wire::{ETHERTYPE_ARP, ETHERTYPE_IPV4, Frame, Icmp, Ipv4, PROTO_ICMP, PROTO_TCP, PROTO_UDP, Udp};
    use serde_json::json;

    use crate::Session;

    fn lab(sample: &str) -> Session {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": sample})).unwrap();
        s
    }

    /// The first frame `from` put on a cable for the PDU (ARP for its gateway skipped, unless
    /// `arp`), decoded.
    fn first_frame(s: &Session, from: &str, since: u64, arp: bool) -> Frame {
        let d = s.topology.resolve(from).unwrap();
        s.sim
            .trace()
            .iter()
            .filter(|e| e.id >= since && e.from.device == d)
            .map(|e| Frame::decode(&e.frame).unwrap())
            .find(|f| arp || f.ethertype != ETHERTYPE_ARP)
            .unwrap_or_else(|| panic!("{from} sent nothing"))
    }

    fn next_id(s: &Session) -> u64 {
        s.sim.trace().back().map_or(0, |e| e.id) + 1
    }

    #[test]
    fn each_application_puts_its_own_first_frame_on_the_wire() {
        // (app, extra params, IP protocol, destination port, answered)
        let cases = [
            ("dns", json!({"text": "www.netlabcraft.lab"}), PROTO_UDP, Some(53), true),
            ("http", json!({}), PROTO_TCP, Some(80), true),
            ("telnet", json!({}), PROTO_TCP, Some(23), false),
            ("ssh", json!({}), PROTO_TCP, Some(22), false),
            ("ftp", json!({}), PROTO_TCP, Some(21), false),
            ("smtp", json!({}), PROTO_TCP, Some(25), false),
            ("pop3", json!({}), PROTO_TCP, Some(110), false),
            ("tftp", json!({"text": "notes.txt"}), PROTO_UDP, Some(69), false),
            ("snmp", json!({}), PROTO_UDP, Some(161), false),
            ("udp", json!({"src_port": 4000, "dst_port": 5000, "size": 100}), PROTO_UDP, Some(5000), false),
            ("tcp", json!({"src_port": 4001, "dst_port": 8080}), PROTO_TCP, Some(8080), false),
            ("ping", json!({"ttl": 9, "seq": 7, "size": 64, "dscp": "ef"}), PROTO_ICMP, None, true),
        ];
        for (app, extra, proto, port, answered) in cases {
            let mut s = lab("services");
            let since = next_id(&s);
            let mut p = json!({"from": "PC1", "to": "Server0", "app": app});
            for (k, v) in extra.as_object().unwrap() {
                p[k] = v.clone();
            }
            let v = s.execute("pdu.complex", p).unwrap();
            assert_eq!(v["type"], netlabcraft_model::scenario::PduApp::parse(app).unwrap().type_name());
            let f = first_frame(&s, "PC1", since, false);
            assert_eq!(f.ethertype, ETHERTYPE_IPV4, "{app}");
            let ip = Ipv4::decode(&f.payload).unwrap();
            assert_eq!(ip.protocol, proto, "{app}");
            assert_eq!(ip.dst.to_string(), "192.168.100.2", "{app}");
            match proto {
                PROTO_UDP => assert_eq!(Some(Udp::decode(&ip.payload, ip.src, ip.dst).unwrap().dst_port), port, "{app}"),
                PROTO_TCP => {
                    let seg = Tcp::decode(&ip.payload, ip.src, ip.dst).unwrap();
                    assert_eq!(Some(seg.dst_port), port, "{app}");
                    assert_eq!(seg.flags, tcp::SYN, "{app}: a connection opens with a SYN");
                }
                _ => {
                    assert_eq!(ip.ttl, 9);
                    assert_eq!(netlabcraft_wire::qos::dscp(&f.payload), Some(46), "EF is DSCP 46");
                    match Icmp::decode(&ip.payload).unwrap() {
                        Icmp::EchoRequest { seq, data, .. } => assert_eq!((seq, data.len()), (7, 64)),
                        other => panic!("{other:?}"),
                    }
                }
            }
            if answered {
                assert_eq!(v["status"], "successful", "{app}: {v}");
            }
            assert_eq!(v["fired"], 1);
        }
    }

    #[test]
    fn dhcp_and_arp_pdus_broadcast() {
        let mut s = lab("services");
        let since = next_id(&s);
        let v = s.execute("pdu.complex", json!({"from": "PC1", "app": "dhcp"})).unwrap();
        let f = first_frame(&s, "PC1", since, true);
        assert!(f.dst.is_broadcast());
        let ip = Ipv4::decode(&f.payload).unwrap();
        assert_eq!(Udp::decode(&ip.payload, ip.src, ip.dst).unwrap().dst_port, 67);
        assert_eq!(v["status"], "successful", "{v}");
        let since = next_id(&s);
        let v = s.execute("pdu.complex", json!({"from": "PC1", "app": "arp", "to": "192.168.100.2"})).unwrap();
        let f = first_frame(&s, "PC1", since, true);
        assert_eq!(f.ethertype, ETHERTYPE_ARP);
        assert_eq!(v["status"], "successful", "{v}");
    }

    #[test]
    fn a_blocked_pdu_fails_and_a_bad_one_is_refused() {
        let mut s = lab("nat-acl");
        let v = s.execute("pdu.simple", json!({"from": "PC2", "to": "Web"})).unwrap();
        assert_eq!(v["status"], "failed", "{v}");
        let v = s.execute("pdu.simple", json!({"from": "PC1", "to": "Web"})).unwrap();
        assert_eq!(v["status"], "successful", "{v}");
        assert!(s.execute("pdu.complex", json!({"from": "PC1", "to": "Web", "app": "gopher"})).is_err());
        assert!(s.execute("pdu.complex", json!({"from": "PC1", "to": "Web", "ttl": 0})).is_err());
        assert!(s.execute("pdu.complex", json!({"from": "PC1", "to": "Web", "size": 60_000})).is_err());
        assert!(s.execute("pdu.complex", json!({"from": "PC1", "to": "Web", "period": 0.0})).is_err());
        assert!(s.execute("pdu.complex", json!({"from": "PC1", "app": "ping"})).is_err());
        let list = s.execute("pdu.list", json!({})).unwrap();
        assert_eq!(list["pdus"].as_array().unwrap().len(), 2);
        // Fire again, then delete.
        let v = s.execute("pdu.fire", json!({"num": 2})).unwrap();
        assert_eq!(v["fired"], 2);
        s.execute("pdu.delete", json!({"num": 1})).unwrap();
        assert!(s.execute("pdu.fire", json!({"num": 1})).is_err());
        s.execute("pdu.delete", json!({"all": true})).unwrap();
        assert!(s.topology.scenarios.pdus().is_empty());
    }

    #[test]
    fn periodic_pdus_fire_in_simulated_time() {
        let mut s = lab("two-subnets");
        let v = s.execute("pdu.complex", json!({"from": "PC1", "to": "10.0.2.10", "period": 2.0})).unwrap();
        assert_eq!(v["fired"], 1);
        let v = s.execute("sim.advance", json!({"seconds": 10})).unwrap();
        assert_eq!(v["pdus_fired"], 5);
        let p = &s.topology.scenarios.pdus()[0];
        assert_eq!(p.fired, 6);
        assert_eq!(p.status, netlabcraft_model::scenario::PduStatus::Successful);
        // A one-shot PDU scheduled for later waits for its time.
        let v = s.execute("pdu.complex", json!({"from": "PC3", "to": "10.0.1.10", "start": 3.0})).unwrap();
        assert_eq!(v["status"], "waiting");
        s.execute("sim.advance", json!({"seconds": 1})).unwrap();
        assert_eq!(s.topology.scenarios.pdus()[1].fired, 0);
        s.execute("sim.advance", json!({"seconds": 5})).unwrap();
        assert_eq!(s.topology.scenarios.pdus()[1].fired, 1);
        // A tiny period over a long time is bounded.
        let mut s = lab("lan");
        s.execute("pdu.complex", json!({"from": "PC1", "to": "PC2", "period": 0.1})).unwrap();
        let v = s.execute("sim.advance", json!({"seconds": 86_400})).unwrap();
        assert_eq!(v["pdus_fired"], super::MAX_FIRES_PER_ADVANCE);
    }

    #[test]
    fn scenarios_keep_their_own_pdus_and_are_saved() {
        let mut s = lab("two-subnets");
        s.execute("pdu.simple", json!({"from": "PC1", "to": "PC3"})).unwrap();
        let v = s.execute("scenario.new", json!({"description": "Across the router"})).unwrap();
        assert_eq!(v["current"], 1);
        assert!(s.topology.scenarios.pdus().is_empty());
        s.execute("pdu.complex", json!({"from": "PC2", "to": "PC4", "app": "udp", "dst_port": 9000, "period": 5})).unwrap();
        s.execute("pdu.complex", json!({"from": "PC2", "to": "PC4", "app": "tcp"})).unwrap();
        let text = netlabcraft_model::file::to_json(&s.topology).unwrap();
        let back = netlabcraft_model::file::from_json(&text).unwrap();
        assert_eq!(back.scenarios.count(), 2);
        assert_eq!(back.scenarios.current, 1);
        assert_eq!(back.scenarios.list[0].pdus.len(), 1);
        assert_eq!(back.scenarios.list[1].pdus.len(), 2);
        assert_eq!(back.scenarios.list[1].description, "Across the router");
        assert_eq!(back.scenarios.list[1].pdus[0].period_s, Some(5.0));
        let v = s.execute("scenario.select", json!({"index": 0})).unwrap();
        assert_eq!(v["pdus"].as_array().unwrap().len(), 1);
        assert!(s.execute("scenario.select", json!({"index": 5})).is_err());
        s.execute("scenario.delete", json!({"index": 0})).unwrap();
        assert_eq!(s.topology.scenarios.count(), 1);
        assert_eq!(s.topology.scenarios.pdus().len(), 2);
        s.execute("scenario.delete", json!({})).unwrap();
        assert!(s.topology.scenarios.pdus().is_empty());
        assert!(s.topology.scenarios.is_default());
        // A lab with no scenarios writes none (older files open the same).
        assert!(!netlabcraft_model::file::to_json(&s.topology).unwrap().contains("scenarios"));
        // Undo brings a deleted scenario back.
        assert!(s.undo());
        assert_eq!(s.topology.scenarios.pdus().len(), 2);
    }

    /// The details of the last event delivered to `at` whose summary has `what`.
    fn details(s: &mut Session, at: &str, what: &str) -> serde_json::Value {
        let d = s.topology.resolve(at).unwrap();
        let id = s
            .sim
            .trace()
            .iter()
            .rev()
            .find(|e| e.to.device == d && e.summary.contains(what))
            .map(|e| e.id)
            .unwrap_or_else(|| panic!("no {what} at {at}"));
        s.execute("sim.event", json!({"id": id})).unwrap()
    }

    fn lines(v: &serde_json::Value) -> String {
        v["explain"].as_array().unwrap().iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn events_explain_switching_routing_arp_nat_and_acl_drops() {
        // Across a switch and a router.
        let mut s = lab("two-subnets");
        s.execute("pdu.simple", json!({"from": "PC1", "to": "PC3"})).unwrap();
        s.execute("pdu.simple", json!({"from": "PC1", "to": "PC3"})).unwrap();
        let sw = details(&mut s, "S1", "echo request");
        let text = lines(&sw);
        assert!(text.contains("The MAC table has") || text.contains("floods"), "{text}");
        assert!(!sw["sent"].as_array().unwrap().is_empty());
        let r = details(&mut s, "R1", "echo request");
        let text = lines(&r);
        assert!(text.contains("destination MAC is the MAC of GigabitEthernet0/0"), "{text}");
        assert!(text.contains("looks up 10.0.2.10 in its routing table: directly connected, out GigabitEthernet0/1. TTL drops to 127"), "{text}");
        assert!(text.contains("ARP table gives") || text.contains("is not in the ARP table"), "{text}");
        let osi = &r["osi"];
        assert_eq!(osi["in"][0]["layer"], 1);
        assert!(osi["in"].as_array().unwrap().iter().any(|l| l["layer"] == 3), "{osi}");
        assert!(osi["out"].as_array().unwrap().iter().any(|l| l["layer"] == 3 && l["lines"].to_string().contains("TTL 127")), "{osi}");
        let host = details(&mut s, "PC3", "echo request");
        assert!(lines(&host).contains("10.0.2.10 is this device's own address"), "{}", lines(&host));
        // NAT on the way out, and its undoing on the way back.
        let mut s = lab("nat-acl");
        s.execute("pdu.simple", json!({"from": "PC1", "to": "Web"})).unwrap();
        let office = details(&mut s, "Office", "echo request");
        let text = lines(&office);
        assert!(text.contains("NAT translates the inside source 192.168.0.10 to 203.0.113.1"), "{text}");
        assert!(text.contains("Access list GUESTS on GigabitEthernet0/0 (in) permits the packet"), "{text}");
        assert!(text.contains("next hop 203.0.113.2"), "{text}");
        let back = details(&mut s, "Office", "echo reply");
        assert!(lines(&back).contains("NAT translates the destination 203.0.113.1 back to the inside address 192.168.0.10"), "{}", lines(&back));
        // An access list drop.
        s.execute("pdu.simple", json!({"from": "PC2", "to": "Web"})).unwrap();
        let d = s.topology.resolve("Office").unwrap();
        let e = s.sim.trace().iter().rev().find(|e| e.to.device == d && e.dropped.is_some()).unwrap().id;
        let v = s.execute("sim.event", json!({"id": e})).unwrap();
        let osi = v["osi"]["in"].to_string();
        assert!(osi.contains("The device drops the frame: denied by access list GUESTS (in)"), "{osi}");
        assert!(s.execute("sim.event", json!({"id": 0})).is_err());
        // The records stay small.
        assert!(s.sim.trace().iter().all(|e| e.explain.len() <= netlabcraft_sim::explain::MAX_STEPS));
    }
}
