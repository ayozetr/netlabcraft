//! The Netflow Collector of a computer's Desktop: `netflow.listen`, `netflow.stop`,
//! `netflow.flows` and `netflow.clear`.
//!
//! Listening opens a UDP port on the device (2055 unless told otherwise). Routers set up with
//! `ip flow-export destination` send it real NetFlow version 5 or 9 datagrams (see
//! `netlabcraft_sim::netflow`); the collector decodes them and keeps the newest records. The port
//! is saved with the lab, what was received is not.

use netlabcraft_sim::netflow::protocol_name;
use netlabcraft_wire::netflow::NETFLOW_PORT;
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

/// Most flows `netflow.flows` lists.
const MAX_LIST: usize = 500;
const DEFAULT_LIST: usize = 100;
/// Most rows in the top-talker list.
const TOP: usize = 10;

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// Runs one of these commands, or `None` when `id` is not one.
pub(crate) fn run(s: &mut Session, id: &str, p: &Value) -> Option<Result<Value>> {
    match id {
        "netflow.listen" => Some(listen(s, p)),
        "netflow.stop" => Some(stop(s, p)),
        "netflow.flows" => Some(flows(s, p)),
        "netflow.clear" => Some(clear(s, p)),
        _ => None,
    }
}

fn listen(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::hostapps::desktop_device(s, p)?;
    let saved = s.topology.device(d).and_then(|x| x.netflow.collector_port);
    let port = match p.get("port") {
        None | Some(Value::Null) => saved.unwrap_or(NETFLOW_PORT),
        Some(v) => v.as_u64().and_then(|n| u16::try_from(n).ok()).filter(|n| *n >= 1).ok_or_else(|| bad("`port` is 1-65535"))?,
    };
    s.checkpoint();
    if let Some(x) = s.topology.device_mut(d) {
        x.netflow.collector_port = Some(port);
    }
    s.changed();
    flows(s, p)
}

fn stop(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::hostapps::desktop_device(s, p)?;
    if s.topology.device(d).is_none_or(|x| x.netflow.collector_port.is_none()) {
        return Err(EngineError::Failed("the collector is not listening".into()));
    }
    s.checkpoint();
    if let Some(x) = s.topology.device_mut(d) {
        x.netflow.collector_port = None;
    }
    s.changed();
    flows(s, p)
}

fn clear(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::hostapps::desktop_device(s, p)?;
    s.sim.clear_netflow_collected(d);
    s.revision += 1;
    flows(s, p)
}

fn flows(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::hostapps::desktop_device(s, p)?;
    let limit = match p.get("limit") {
        None | Some(Value::Null) => DEFAULT_LIST,
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| (1..=MAX_LIST).contains(n))
            .ok_or_else(|| bad(format!("`limit` is 1-{MAX_LIST}")))?,
    };
    let port = s.topology.device(d).and_then(|x| x.netflow.collector_port);
    let (records, datagrams, bad_datagrams, sum) = s
        .sim
        .netflow_collected(d)
        .map_or((Vec::new(), 0, 0, Default::default()), |c| (c.records.iter().cloned().collect::<Vec<_>>(), c.datagrams, c.bad, c.summary()));
    let top: Vec<Value> = sum
        .talkers
        .iter()
        .take(TOP)
        .map(|(a, t)| json!({"address": a.to_string(), "flows": t.flows, "packets": t.packets, "bytes": t.bytes}))
        .collect();
    let protos: Vec<Value> = sum
        .protocols
        .iter()
        .map(|(n, t)| json!({"protocol": protocol_name(*n), "number": n, "flows": t.flows, "packets": t.packets, "bytes": t.bytes}))
        .collect();
    let total = sum.total;
    let skip = records.len().saturating_sub(limit);
    let listed: Vec<Value> = records
        .iter()
        .skip(skip)
        .map(|c| {
            let r = &c.rec;
            json!({
                "at_ms": c.at_us / 1_000,
                "exporter": c.exporter.to_string(),
                "version": c.version,
                "src": r.src.to_string(),
                "dst": r.dst.to_string(),
                "next_hop": r.next_hop.to_string(),
                "protocol": protocol_name(r.protocol),
                "src_port": r.src_port,
                "dst_port": r.dst_port,
                "packets": r.packets,
                "bytes": r.octets,
                "in_if": r.input,
                "out_if": r.output,
                "tcp_flags": r.tcp_flags,
                "first_ms": r.first,
                "last_ms": r.last,
            })
        })
        .collect();
    Ok(json!({
        "device": d.0,
        "listening": port.is_some(),
        "port": port,
        "default_port": NETFLOW_PORT,
        "datagrams": datagrams,
        "bad_datagrams": bad_datagrams,
        "flows_received": total.flows,
        "packets": total.packets,
        "bytes": total.bytes,
        "top_talkers": top,
        "protocols": protos,
        "flows": listed,
    }))
}
