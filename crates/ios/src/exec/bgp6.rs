//! BGP for IPv6 unicast: the `address-family ipv6` mode, `neighbor … activate` in it, the section of
//! the running configuration and `show bgp ipv6 unicast …`.

use std::fmt::Write as _;

use netlabcraft_model::Device;
use netlabcraft_model::bgp::Bgp;
use netlabcraft_sim::bgp::{BgpView, SessionState};
use netlabcraft_sim::bgp6::Peer;

use super::bgp::{hms, named, not_running, process};
use super::{Ctx, R};

/// `neighbor WHO activate` and its `no` form under `address-family ipv6`: the neighbour (reached over
/// IPv4 or IPv6) does or does not exchange IPv6 prefixes.
pub(super) fn activate(c: &mut Ctx, who: &str, on: bool) -> R {
    let n = named(process(c)?, who).map_err(|e| if e.contains("is not") { format!("% Peer groups have no IPv6 settings here: {who}") } else { e })?;
    n.af6.activated = on;
    Ok(String::new())
}

/// The `address-family ipv6` section of the BGP configuration.
pub(super) fn running_config(s: &mut String, b: &Bgp) {
    let mut lines: Vec<String> = Vec::new();
    for n in b.neighbors.iter().filter(|n| n.af6.activated) {
        lines.push(format!("  neighbor {} activate", n.addr));
    }
    for n in b.neighbors6.iter().filter(|n| n.af6.activated) {
        if let Some(a) = n.addr6 {
            lines.push(format!("  neighbor {a} activate"));
        }
    }
    if lines.is_empty() {
        return;
    }
    let _ = writeln!(s, " address-family ipv6");
    for l in lines {
        let _ = writeln!(s, "{l}");
    }
    let _ = writeln!(s, " exit-address-family");
}

fn state_text(admin_down: bool, state: SessionState) -> String {
    if admin_down { "Idle (Admin)".to_string() } else { state.name().to_string() }
}

/// `show bgp ipv6 unicast summary`.
pub(super) fn summary(d: &Device, v: &BgpView) -> String {
    if d.bgp.is_none() {
        return not_running();
    }
    let Some(r) = v.v6.routers.get(&d.id) else {
        return "IPv6 routing is not on here: BGP carries IPv6 prefixes once `ipv6 unicast-routing` is set.".to_string();
    };
    let mut s = format!("BGP router id {}, local AS {}\nAddress family: IPv6 unicast\n", r.router_id, r.asn);
    if r.neighbors.is_empty() {
        s.push_str("\n(no neighbour is activated for IPv6: under `address-family ipv6`, `neighbor ADDRESS activate`)");
        return s;
    }
    let _ = writeln!(s, "\n{:<40} {:>11} {:<9} {:<9} State/Prefixes", "Neighbor", "AS", "Transport", "Up/Down");
    for n in &r.neighbors {
        let up = match (n.peer, n.state) {
            (Peer::V4(a), SessionState::Established) => v.since.get(&(d.id, a)).map(|at| hms(v.now.saturating_sub(*at))),
            (p @ Peer::V6(_), SessionState::Established) => v.since6.get(&(d.id, p)).map(|at| hms(v.now.saturating_sub(*at))),
            _ => None,
        }
        .unwrap_or_else(|| "never".to_string());
        let last = if n.state == SessionState::Established {
            if n.carries6 { n.received.to_string() } else { "Established, IPv6 not negotiated".to_string() }
        } else {
            state_text(n.admin_down, n.state)
        };
        let remote = n.remote_as.map_or("-".to_string(), |a| a.to_string());
        let transport = if n.transport6 { "IPv6" } else { "IPv4" };
        let _ = writeln!(s, "{:<40} {remote:>11} {transport:<9} {up:<9} {last}", n.peer.to_string());
    }
    s.trim_end().to_string()
}
