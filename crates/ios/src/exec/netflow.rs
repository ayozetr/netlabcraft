//! NetFlow on routers: `ip flow-export destination|version`, `ip flow ingress` (and the older
//! `ip route-cache flow`) on interfaces, `show ip flow export`, `show ip cache flow`,
//! `clear ip flow stats`, and the lines of the running configuration.

use std::fmt::Write as _;

use netlabcraft_core::parse_ip;
use netlabcraft_model::Device;
use netlabcraft_model::netflow::{Destination, MAX_DESTINATIONS, MAX_INGRESS};
use netlabcraft_sim::netflow::MAX_FLOWS;

use super::{Ctx, R};
use crate::grammar::Id;

fn port(text: &str) -> Result<u16, String> {
    text.trim().parse::<u16>().ok().filter(|p| *p >= 1).ok_or_else(|| "% The port is 1-65535".to_string())
}

/// The name of an interface from its SNMP-style index (1 is the first port).
fn if_name(d: &Device, index: u16) -> String {
    index.checked_sub(1).and_then(|i| d.ports.get(usize::from(i))).map_or_else(|| "-".to_string(), |p| p.name.clone())
}

fn protocol_name(p: u8) -> String {
    match p {
        1 => "icmp".into(),
        6 => "tcp".into(),
        17 => "udp".into(),
        n => n.to_string(),
    }
}

/// `ip flow-export` lines of the running configuration.
pub fn running_config(s: &mut String, d: &Device) {
    let n = &d.netflow;
    if n.export_version() == 9 {
        let _ = writeln!(s, "ip flow-export version 9");
    }
    for x in &n.destinations {
        let _ = writeln!(s, "ip flow-export destination {} {}", x.addr, x.port);
    }
    if n.version == 9 || !n.destinations.is_empty() {
        let _ = writeln!(s, "!");
    }
}

/// The line under one interface.
pub(crate) fn iface_lines(s: &mut String, d: &Device, name: &str) {
    if d.netflow.counts(name) {
        let _ = writeln!(s, " ip flow ingress");
    }
}

pub(super) fn command(c: &mut Ctx<'_>, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::FlowExportDestination | Id::NoFlowExportDestination => {
            let addr = parse_ip(arg(0)).map_err(|e| format!("% {e}"))?;
            let p = port(arg(1))?;
            let n = &mut c.dm()?.netflow;
            n.destinations.retain(|x| !(x.addr == addr && x.port == p));
            if id == Id::FlowExportDestination {
                if n.destinations.len() >= MAX_DESTINATIONS {
                    return Err(format!("% At most {MAX_DESTINATIONS} flow export destinations"));
                }
                n.destinations.push(Destination { addr, port: p });
            }
            Ok(String::new())
        }
        Id::FlowExportVersion => {
            let v = match arg(0).trim() {
                "5" => 5,
                "9" => 9,
                _ => return Err("% The export version is 5 or 9".into()),
            };
            c.dm()?.netflow.version = v;
            Ok(String::new())
        }
        Id::NoFlowExportVersion => {
            c.dm()?.netflow.version = 0;
            Ok(String::new())
        }
        Id::FlowIngress | Id::NoFlowIngress => {
            let names = c.mode_iface_names()?;
            let n = &mut c.dm()?.netflow;
            for name in names {
                n.ingress.retain(|x| *x != name);
                if id == Id::FlowIngress {
                    if n.ingress.len() >= MAX_INGRESS {
                        return Err("% Too many interfaces count flows".into());
                    }
                    n.ingress.push(name);
                }
            }
            Ok(String::new())
        }
        Id::ClearIpFlowStats => {
            c.sim.clear_netflow_cache(c.dev);
            Ok(String::new())
        }
        Id::ShowIpFlowExport => {
            let d = c.d()?;
            let n = &d.netflow;
            let mut s = format!("Flow export version {}\n", n.export_version());
            if n.destinations.is_empty() {
                s.push_str("Destinations: none\n");
            } else {
                s.push_str("Destinations:\n");
                for x in &n.destinations {
                    let _ = writeln!(s, "  {} port {}", x.addr, x.port);
                }
            }
            let counting = if n.ingress.is_empty() { "none".to_string() } else { n.ingress.join(", ") };
            let _ = writeln!(s, "Counting flows on: {counting}");
            let cache = c.sim.netflow_cache(c.dev);
            let (flows, datagrams, dropped) = cache.map_or((0, 0, 0), |k| (k.exported_flows, k.exported_datagrams, k.dropped));
            let _ =
                write!(s, "Exported {flows} flow{} in {datagrams} datagram{}\nNot cached (cache full): {dropped}", plural(flows), plural(datagrams));
            Ok(s)
        }
        Id::ShowIpCacheFlow => {
            let d = c.d()?;
            let Some(cache) = c.sim.netflow_cache(c.dev) else {
                return Ok(format!("Flow cache: 0 flows (at most {MAX_FLOWS})"));
            };
            let mut s = format!(
                "Flow cache: {} active (at most {MAX_FLOWS}), {} aged out, {} not cached because the cache was full\nPackets counted: {}\n",
                cache.flows.len(),
                cache.aged,
                cache.dropped,
                cache.packets
            );
            if cache.flows.is_empty() {
                return Ok(s.trim_end().to_string());
            }
            let _ = writeln!(
                s,
                "{:<20} {:<16} {:<20} {:<16} {:<5} {:>5} {:>5} {:>8} {:>10}",
                "In", "Source", "Out", "Destination", "Proto", "SPort", "DPort", "Packets", "Bytes"
            );
            for f in &cache.flows {
                let r = &f.rec;
                let _ = writeln!(
                    s,
                    "{:<20} {:<16} {:<20} {:<16} {:<5} {:>5} {:>5} {:>8} {:>10}",
                    if_name(d, r.input),
                    r.src,
                    if_name(d, r.output),
                    r.dst,
                    protocol_name(r.protocol),
                    r.src_port,
                    r.dst_port,
                    r.packets,
                    r.octets
                );
            }
            Ok(s.trim_end().to_string())
        }
        _ => Err("% Not a NetFlow command".into()),
    }
}

fn plural(n: u64) -> &'static str {
    if n == 1 { "" } else { "s" }
}
