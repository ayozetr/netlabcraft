//! The Traffic Generator of an end device's Desktop: sends a bounded burst of ICMP, UDP or TCP
//! packets to one destination at a fixed interval of simulated time and counts what was answered
//! (`traffic.send`, `traffic.status`). Every packet is a PDU of the Simulation mode's machinery
//! (`Sim::send_pdu`), so it takes the same path, meets the same firewalls and access lists and
//! shows in the event list like a Complex PDU does. A burst runs to its end inside the command: it
//! sends at most [`MAX_COUNT`] packets, [`MAX_INTERVAL_S`] apart, so it cannot stall the simulator.

use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, parse_ip};
use netlabcraft_model::scenario::{MAX_PDU_SIZE, PduApp};
use netlabcraft_sim::pdu::PduSpec;
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

/// Most packets one burst sends.
pub const MAX_COUNT: u32 = 100;
/// Longest wait between two packets (simulated seconds).
pub const MAX_INTERVAL_S: f64 = 5.0;
/// What the form starts with.
pub const DEFAULT_COUNT: u32 = 4;
pub const DEFAULT_INTERVAL_S: f64 = 1.0;
pub const DEFAULT_SIZE: u16 = 32;

/// What became of one packet of a burst.
#[derive(Debug, Clone, PartialEq)]
pub struct TrafficResult {
    pub n: u32,
    pub ok: bool,
    pub note: String,
    /// Simulated time the packet was sent, in seconds.
    pub at_s: f64,
}

/// The last burst a device sent.
#[derive(Debug, Clone, PartialEq)]
pub struct TrafficRun {
    pub destination: String,
    pub protocol: &'static str,
    pub port: Option<u16>,
    pub size: u16,
    pub count: u32,
    pub interval_s: f64,
    pub results: Vec<TrafficResult>,
}

impl TrafficRun {
    fn received(&self) -> usize {
        self.results.iter().filter(|r| r.ok).count()
    }

    fn json(&self, device: DeviceId) -> Value {
        let sent = self.results.len();
        let received = self.received();
        json!({
            "device": device.0,
            "ran": true,
            "destination": self.destination,
            "protocol": self.protocol,
            "port": self.port,
            "size": self.size,
            "count": self.count,
            "interval": self.interval_s,
            "sent": sent,
            "received": received,
            "lost": sent.saturating_sub(received),
            "results": self.results.iter().map(|r| json!({"n": r.n, "ok": r.ok, "note": r.note, "at": r.at_s})).collect::<Vec<_>>(),
        })
    }
}

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// Runs one of these commands, or `None` when `id` is not one.
pub(crate) fn run(s: &mut Session, id: &str, p: &Value) -> Option<Result<Value>> {
    match id {
        "traffic.send" => Some(send(s, p)),
        "traffic.status" => Some(status(s, p)),
        _ => None,
    }
}

fn protocol(p: &Value) -> Result<(PduApp, &'static str)> {
    match p.get("protocol").and_then(Value::as_str).map(|t| t.trim().to_ascii_lowercase()).as_deref() {
        None | Some("icmp" | "ping") => Ok((PduApp::Ping, "ICMP")),
        Some("udp") => Ok((PduApp::Udp, "UDP")),
        Some("tcp") => Ok((PduApp::Tcp, "TCP")),
        Some(_) => Err(bad("`protocol` is icmp, udp or tcp")),
    }
}

fn send(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::hostapps::desktop_device(s, p)?;
    let to = p.get("to").and_then(Value::as_str).ok_or_else(|| bad("`to` (an address or a device name) is required"))?;
    let (app, protocol_name) = protocol(p)?;
    let port = match p.get("port") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().and_then(|n| u16::try_from(n).ok()).filter(|n| *n > 0).ok_or_else(|| bad("`port` is a number from 1 to 65535"))?),
    };
    if app != PduApp::Ping && port.is_none() {
        return Err(bad("`port` is required for udp and tcp"));
    }
    let size = match p.get("size") {
        None | Some(Value::Null) => DEFAULT_SIZE,
        Some(v) => v
            .as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .filter(|n| *n <= MAX_PDU_SIZE)
            .ok_or_else(|| bad(format!("`size` is a number of bytes from 0 to {MAX_PDU_SIZE}")))?,
    };
    let count = match p.get("count") {
        None | Some(Value::Null) => DEFAULT_COUNT,
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| (1..=MAX_COUNT).contains(n))
            .ok_or_else(|| bad(format!("`count` is a number from 1 to {MAX_COUNT}")))?,
    };
    let interval_s = match p.get("interval") {
        None | Some(Value::Null) => DEFAULT_INTERVAL_S,
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite() && (0.0..=MAX_INTERVAL_S).contains(x))
            .ok_or_else(|| bad(format!("`interval` is a number of seconds from 0 to {MAX_INTERVAL_S}")))?,
    };
    let (destination, _) = crate::pdu::destination(s, to)?;
    let dst: Ipv4Addr = parse_ip(&destination).map_err(|e| EngineError::Failed(e.to_string()))?;
    let interval_us = (interval_s * 1_000_000.0) as u64;
    let mut results = Vec::new();
    for n in 1..=count {
        let spec = PduSpec {
            app,
            port: None,
            dst: Some(dst),
            src_port: None,
            dst_port: port,
            ttl: None,
            tos: 0,
            seq: (app == PduApp::Ping).then_some(n),
            size,
            text: String::new(),
        };
        let at_s = s.sim.now_us() as f64 / 1e6;
        let (ok, note) = match s.sim.send_pdu(&s.topology, d, &spec) {
            Ok(o) => (o.ok, o.note),
            Err(e) => (false, e.to_string()),
        };
        results.push(TrafficResult { n, ok, note, at_s });
        if n < count && interval_us > 0 {
            s.sim.run_for(&s.topology, interval_us);
        }
    }
    s.revision += 1;
    let run = TrafficRun { destination, protocol: protocol_name, port, size, count, interval_s, results };
    let out = run.json(d);
    s.traffic.insert(d, run);
    Ok(out)
}

fn status(s: &mut Session, p: &Value) -> Result<Value> {
    let d = crate::hostapps::desktop_device(s, p)?;
    Ok(s.traffic.get(&d).map_or_else(
        || json!({"device": d.0, "ran": false, "max_count": MAX_COUNT, "max_interval": MAX_INTERVAL_S, "max_size": MAX_PDU_SIZE}),
        |r| r.json(d),
    ))
}
