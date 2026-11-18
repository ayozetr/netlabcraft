//! `show ip cef`: the forwarding table as a view of the routing table, with the adjacency (the
//! ARP entry) each next hop needs, and `ip default-network`.

use std::fmt::Write as _;
use std::net::Ipv4Addr;

use netlabcraft_core::parse_ip;
use netlabcraft_model::MAX_DEFAULT_NETWORKS;
use netlabcraft_sim::RouteEntry;

use super::{Ctx, R};
use crate::grammar::Id;

/// What a table entry forwards to, in words.
fn target(r: &RouteEntry) -> String {
    match (r.code, r.via) {
        ('L', _) => "receive".to_string(),
        (_, Some(nh)) => nh.to_string(),
        _ => "attached".to_string(),
    }
}

fn adjacency(c: &Ctx, r: &RouteEntry) -> String {
    match (r.code, r.via) {
        ('L', _) => "this router".to_string(),
        (_, None) => "resolved for each destination on the link (ARP)".to_string(),
        (_, Some(nh)) => match c.sim.state(c.dev).and_then(|s| s.arp.get(&nh)) {
            Some(a) => format!("{} on {}", a.mac.dotted(), a.iface),
            None => "incomplete (no ARP entry yet)".to_string(),
        },
    }
}

fn row(c: &Ctx, r: &RouteEntry, detail: bool) -> String {
    let mut s = format!("{:<20} {:<16} {}\n", r.net.to_string(), target(r), r.iface.clone().unwrap_or_else(|| "-".into()));
    if detail {
        let _ = writeln!(s, "    adjacency: {}", adjacency(c, r));
    }
    s
}

/// The entry that carries traffic to `dst`: the longest prefix, then the lowest distance.
fn lookup(table: &[RouteEntry], dst: Ipv4Addr) -> Option<&RouteEntry> {
    table.iter().filter(|r| r.net.contains(dst)).max_by_key(|r| (r.net.prefix, std::cmp::Reverse(r.distance)))
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::IpDefaultNetwork => {
            let a = parse_ip(arg(0).trim()).map_err(|e| format!("% {e}"))?;
            if a.is_unspecified() || a.is_multicast() || a == Ipv4Addr::BROADCAST {
                return Err(format!("% {a} is not a network a default can lead to"));
            }
            let d = c.dm()?;
            if !d.default_networks.contains(&a) {
                if d.default_networks.len() >= MAX_DEFAULT_NETWORKS {
                    return Err(format!("% At most {MAX_DEFAULT_NETWORKS} candidate default networks"));
                }
                d.default_networks.push(a);
            }
            Ok(String::new())
        }
        Id::NoIpDefaultNetwork => {
            let a = parse_ip(arg(0).trim()).map_err(|e| format!("% {e}"))?;
            let d = c.dm()?;
            let before = d.default_networks.len();
            d.default_networks.retain(|x| *x != a);
            if d.default_networks.len() == before { Err(format!("% {a} is not a candidate default network")) } else { Ok(String::new()) }
        }
        _ => {
            let d = c.d()?.clone();
            let table = c.sim.routing_table(c.t, &d);
            let detail = matches!(id, Id::ShowIpCefDetail | Id::ShowIpCefAddressDetail);
            match id {
                Id::ShowIpCefSummary => {
                    let (mut receive, mut attached, mut hops) = (0usize, 0usize, 0usize);
                    for r in &table {
                        match (r.code, r.via) {
                            ('L', _) => receive += 1,
                            (_, None) => attached += 1,
                            _ => hops += 1,
                        }
                    }
                    let resolved = table
                        .iter()
                        .filter(|r| r.code != 'L')
                        .filter_map(|r| r.via)
                        .filter(|nh| c.sim.state(c.dev).is_some_and(|s| s.arp.contains_key(nh)))
                        .count();
                    let default = table
                        .iter()
                        .find(|r| r.net.prefix == 0)
                        .map_or("none".to_string(), |r| format!("{} out {}", target(r), r.iface.clone().unwrap_or_default()));
                    Ok(format!(
                        "Forwarding table: {} prefixes\n  receive (this router's own addresses): {receive}\n  attached (directly connected): {attached}\n  through a next hop: {hops} ({resolved} with a resolved adjacency)\n  default route: {default}",
                        table.len()
                    ))
                }
                Id::ShowIpCefAddress | Id::ShowIpCefAddressDetail => {
                    let a = parse_ip(arg(0).trim()).map_err(|e| format!("% {e}"))?;
                    match lookup(&table, a) {
                        Some(r) => Ok(format!("{:<20} {:<16} {}\n{}", "Prefix", "Next hop", "Interface", row(c, r, detail)).trim_end().to_string()),
                        None => Ok(format!("No forwarding entry covers {a}: packets to it are dropped.")),
                    }
                }
                _ => {
                    if table.is_empty() {
                        return Ok("The forwarding table is empty.".into());
                    }
                    let mut s = format!("{:<20} {:<16} {}\n", "Prefix", "Next hop", "Interface");
                    for r in &table {
                        s.push_str(&row(c, r, detail));
                    }
                    Ok(s.trim_end().to_string())
                }
            }
        }
    }
}
