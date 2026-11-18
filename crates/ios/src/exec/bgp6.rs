//! BGP for IPv6 unicast: the `address-family ipv6` mode, `neighbor … activate` in it, the section of
//! the running configuration and `show bgp ipv6 unicast …`.

use std::fmt::Write as _;

use netlabcraft_core::Ipv6Net;
use netlabcraft_model::Device;
use netlabcraft_model::bgp::{Aggregate6, Bgp, Family6, MAX_AGGREGATES, MAX_NETWORKS, Source};
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

/// The lines (two spaces in) one neighbour has in the IPv6 family.
fn family_lines(who: &str, f: &Family6) -> Vec<String> {
    let mut v = Vec::new();
    if f.activated {
        v.push(format!("  neighbor {who} activate"));
    }
    if f.next_hop_self {
        v.push(format!("  neighbor {who} next-hop-self"));
    }
    if f.send_community || f.send_extended {
        let word = match (f.send_community, f.send_extended) {
            (true, true) => " both",
            (false, true) => " extended",
            _ => "",
        };
        v.push(format!("  neighbor {who} send-community{word}"));
    }
    if f.route_reflector_client {
        v.push(format!("  neighbor {who} route-reflector-client"));
    }
    if f.default_originate {
        v.push(format!("  neighbor {who} default-originate"));
    }
    if let Some(w) = f.weight {
        v.push(format!("  neighbor {who} weight {w}"));
    }
    for (word, dir, name) in [
        ("prefix-list", "in", &f.prefix_list_in),
        ("prefix-list", "out", &f.prefix_list_out),
        ("filter-list", "in", &f.filter_list_in),
        ("filter-list", "out", &f.filter_list_out),
        ("route-map", "in", &f.route_map_in),
        ("route-map", "out", &f.route_map_out),
    ] {
        if let Some(n) = name {
            v.push(format!("  neighbor {who} {word} {n} {dir}"));
        }
    }
    v
}

/// The `address-family ipv6` section of the BGP configuration.
pub(super) fn running_config(s: &mut String, b: &Bgp) {
    let mut lines: Vec<String> = Vec::new();
    for n in &b.neighbors {
        lines.extend(family_lines(&n.addr.to_string(), &n.af6));
    }
    for n in &b.neighbors6 {
        if let Some(a) = n.addr6 {
            lines.extend(family_lines(&a.to_string(), &n.af6));
        }
    }
    for n in &b.networks6 {
        lines.push(format!("  network {n}"));
    }
    for g in &b.aggregates6 {
        lines.push(format!(
            "  aggregate-address {}{}{}",
            g.net,
            if g.summary_only { " summary-only" } else { "" },
            if g.as_set { " as-set" } else { "" }
        ));
    }
    if let Some(m) = b.maximum_paths6 {
        lines.push(format!("  maximum-paths {m}"));
    }
    for r in &b.redistribute6 {
        let map = b.redistribute_maps6.get(r.word()).map_or(String::new(), |m| format!(" route-map {m}"));
        match r {
            Source::Ospf => lines.push(format!("  redistribute ospf 1{map}")),
            other => lines.push(format!("  redistribute {}{map}", other.word())),
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

/// `network X:X::X/LEN` under `address-family ipv6`.
pub(super) fn network(c: &mut Ctx, add: bool, text: &str) -> R {
    let text = text.trim();
    if !text.contains('/') {
        return Err("% Write the prefix as X:X::X/LEN, like 2001:db8:1::/48".into());
    }
    let net = text.parse::<Ipv6Net>().map_err(|e| format!("% {e}"))?.network();
    let b = process(c)?;
    if add {
        if !b.networks6.contains(&net) {
            if b.networks6.len() >= MAX_NETWORKS {
                return Err(format!("% At most {MAX_NETWORKS} network statements"));
            }
            b.networks6.push(net);
            b.networks6.sort();
        }
        Ok(String::new())
    } else {
        let before = b.networks6.len();
        b.networks6.retain(|n| *n != net);
        if b.networks6.len() == before { Err("% No such network statement".into()) } else { Ok(String::new()) }
    }
}

/// `aggregate-address X:X::X/LEN [summary-only] [as-set]` under `address-family ipv6`.
pub(super) fn aggregate(c: &mut Ctx, add: bool, text: &str, words: &[&str]) -> R {
    let text = text.trim();
    if !text.contains('/') {
        return Err("% Write the prefix as X:X::X/LEN, like 2001:db8::/32".into());
    }
    let net = text.parse::<Ipv6Net>().map_err(|e| format!("% {e}"))?.network();
    let (mut summary_only, mut as_set) = (false, false);
    for w in words {
        match w.to_ascii_lowercase().as_str() {
            "summary-only" if !summary_only => summary_only = true,
            "as-set" if !as_set => as_set = true,
            _ => return Err(format!("% Unexpected `{w}`: only summary-only and as-set may follow, once each")),
        }
    }
    let b = process(c)?;
    if add {
        if !b.aggregates6.iter().any(|g| g.net == net) && b.aggregates6.len() >= MAX_AGGREGATES {
            return Err(format!("% At most {MAX_AGGREGATES} aggregate addresses"));
        }
        b.aggregates6.retain(|g| g.net != net);
        b.aggregates6.push(Aggregate6 { net, summary_only, as_set });
        b.aggregates6.sort_by_key(|g| g.net);
        Ok(String::new())
    } else {
        let before = b.aggregates6.len();
        b.aggregates6.retain(|g| g.net != net);
        if b.aggregates6.len() == before { Err("% No such aggregate address".into()) } else { Ok(String::new()) }
    }
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
    let prefixes: std::collections::BTreeSet<Ipv6Net> = r.paths.iter().map(|p| p.net).collect();
    let _ = writeln!(
        s,
        "Paths in the table: {} ({} best) for {} prefix{}",
        r.paths.len(),
        r.paths.iter().filter(|p| p.best).count(),
        prefixes.len(),
        if prefixes.len() == 1 { "" } else { "es" }
    );
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
