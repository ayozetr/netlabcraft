//! BGP for IPv6 unicast: the `address-family ipv6` mode, `neighbor … activate` in it, the section of
//! the running configuration and `show bgp ipv6 unicast …`.

use std::fmt::Write as _;

use netlabcraft_core::Ipv6Net;
use netlabcraft_model::Device;
use netlabcraft_model::bgp::{Aggregate6, Bgp, Family6, MAX_AGGREGATES, MAX_NETWORKS, Source};
use netlabcraft_sim::bgp::{BgpView, SessionState};
use netlabcraft_sim::bgp6::{Path6, Peer, RouterView6};

use super::bgp::{hms, named, not_running, path_text, process};
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

fn router6<'a>(d: &Device, v: &'a BgpView) -> Result<&'a RouterView6, String> {
    if d.bgp.is_none() {
        return Err(not_running());
    }
    v.v6.routers.get(&d.id).ok_or_else(|| "IPv6 routing is not on here: BGP carries IPv6 prefixes once `ipv6 unicast-routing` is set.".to_string())
}

fn status(p: &Path6) -> String {
    format!(
        "{}{}{}",
        if p.valid { '*' } else { ' ' },
        if p.best {
            '>'
        } else if p.multi {
            'm'
        } else {
            ' '
        },
        if p.internal() { 'i' } else { ' ' }
    )
}

fn path_line(p: &Path6) -> String {
    let nh = if p.next_hop.is_unspecified() { "::".to_string() } else { p.next_hop.to_string() };
    format!(
        "{} {:<30} {:<28} {:>6} {:>6} {:>6}  {}",
        status(p),
        p.net.to_string(),
        nh,
        p.med,
        p.local_pref,
        p.weight,
        path_text(&[], &p.as_path, &p.as_set, p.origin)
    )
}

/// `show bgp ipv6 unicast`.
pub(super) fn table(d: &Device, v: &BgpView) -> R {
    let r = router6(d, v)?;
    let mut s = format!("BGP router id {}, local AS {}\nAddress family: IPv6 unicast\n", r.router_id, r.asn);
    s.push_str(
        "Status: * usable, > best, m one of several equal paths, i learned from an internal neighbour    Origin: i IGP, e EGP, ? incomplete\n\n",
    );
    if r.paths.is_empty() {
        s.push_str("(the IPv6 table is empty: nothing is advertised and nothing was learned)");
        return Ok(s);
    }
    let _ = writeln!(s, "    {:<30} {:<28} {:>6} {:>6} {:>6}  Path", "Network", "Next hop", "Metric", "LocPrf", "Weight");
    for p in &r.paths {
        let _ = writeln!(s, "{}", path_line(p));
    }
    Ok(s.trim_end().to_string())
}

/// `show bgp ipv6 unicast X:X::X[/LEN]`: the paths to one prefix, or to the most specific prefix
/// that covers an address.
pub(super) fn entry(d: &Device, v: &BgpView, text: &str) -> R {
    let r = router6(d, v)?;
    let text = text.trim();
    let wanted: Vec<&Path6> = if text.contains('/') {
        let net = text.parse::<Ipv6Net>().map_err(|e| format!("% {e}"))?.network();
        r.paths.iter().filter(|p| p.net == net).collect()
    } else {
        let a: std::net::Ipv6Addr = text.parse().map_err(|_| format!("% `{text}` is not an IPv6 address or prefix"))?;
        let longest = r.paths.iter().filter(|p| p.net.contains(a)).map(|p| p.net.prefix).max();
        r.paths.iter().filter(|p| p.net.contains(a) && Some(p.net.prefix) == longest).collect()
    };
    let Some(first) = wanted.first() else { return Ok(format!("% Network not in the BGP IPv6 table: {text}")) };
    let mut s = format!("BGP routing table entry for {}\n", first.net);
    let best = wanted.iter().position(|p| p.best).map_or("none".to_string(), |i| format!("#{}", i + 1));
    let _ = writeln!(s, "  Paths: {} available, best {best}", wanted.len());
    for p in wanted {
        let path = path_text(&[], &p.as_path, &p.as_set, p.origin);
        let from = match p.from {
            None => "local".to_string(),
            Some(f) => format!("from {f}, router id {}", p.from_id),
        };
        let _ = writeln!(s, "  {path}  ({from})");
        let ll = p.link_local.map_or(String::new(), |l| format!(" (link-local {l})"));
        let nh = if p.next_hop.is_unspecified() { "::".to_string() } else { p.next_hop.to_string() };
        let kind = if p.from.is_none() {
            "local"
        } else if p.ebgp {
            "external"
        } else {
            "internal"
        };
        let valid = if p.valid { "valid" } else { "not usable: the next hop is not in the IPv6 routing table" };
        let _ = writeln!(
            s,
            "    Next hop {nh}{ll}, metric {}, local preference {}, weight {}, {valid}, {kind}{}",
            p.med,
            p.local_pref,
            p.weight,
            if p.best { ", best" } else { "" }
        );
        if !p.communities.is_empty() {
            let c: Vec<String> = p.communities.iter().map(|c| netlabcraft_model::policy::community_text(*c, false)).collect();
            let _ = writeln!(s, "    Community: {}", c.join(" "));
        }
    }
    Ok(s.trim_end().to_string())
}

/// `show bgp ipv6 unicast neighbors [ADDRESS]`.
pub(super) fn neighbors(d: &Device, v: &BgpView, who: Option<&str>) -> R {
    let r = router6(d, v)?;
    let b = d.bgp.as_ref().ok_or_else(not_running)?;
    let chosen: Vec<_> = r.neighbors.iter().filter(|n| who.is_none_or(|w| n.peer.to_string() == w.trim())).collect();
    if chosen.is_empty() {
        return Err(match who {
            Some(w) => format!("% {w} is not a neighbour activated for IPv6"),
            None => "(no neighbour is activated for IPv6)".to_string(),
        });
    }
    let mut s = String::new();
    for n in chosen {
        let remote = n.remote_as.map_or("-".to_string(), |a| a.to_string());
        let kind = if n.ebgp { "external link" } else { "internal link" };
        let _ = writeln!(s, "BGP neighbor is {}, remote AS {remote}, {kind}", n.peer);
        let _ = writeln!(s, "  Session over {}, state {}", if n.transport6 { "IPv6" } else { "IPv4" }, state_text(n.admin_down, n.state));
        if let Some(id) = n.peer_router_id {
            let _ = writeln!(s, "  Remote router id {id}");
        }
        if let Some(e) = &n.error {
            let _ = writeln!(s, "  Last error: {e}");
        }
        let conf = match n.peer {
            Peer::V4(a) => b.neighbor(a),
            Peer::V6(a) => b.neighbor6(a),
        };
        let carries = if n.carries6 { "negotiated" } else { "not negotiated (the other end did not announce it)" };
        let _ = writeln!(s, "  Address family IPv6 unicast: {carries}, {} prefixes received, {} sent", n.received, n.sent);
        if let Some(f) = conf.map(|c| &c.af6) {
            for (word, dir, name) in [
                ("prefix list", "in", &f.prefix_list_in),
                ("prefix list", "out", &f.prefix_list_out),
                ("filter list", "in", &f.filter_list_in),
                ("filter list", "out", &f.filter_list_out),
                ("route map", "in", &f.route_map_in),
                ("route map", "out", &f.route_map_out),
            ] {
                if let Some(x) = name {
                    let _ = writeln!(s, "  Inbound and outbound policy: {word} {x} {dir}");
                }
            }
            if f.next_hop_self {
                s.push_str("  This router is the next hop of what it sends (next-hop-self)\n");
            }
        }
        s.push('\n');
    }
    Ok(s.trim_end().to_string())
}

/// `show bgp ipv6 unicast neighbors ADDRESS advertised-routes`.
pub(super) fn advertised(d: &Device, v: &BgpView, who: &str) -> R {
    let r = router6(d, v)?;
    let n = r.neighbors.iter().find(|n| n.peer.to_string() == who.trim()).ok_or_else(|| format!("% {who} is not a neighbour activated for IPv6"))?;
    let Some(list) = r.adv.get(&n.peer) else { return Ok(format!("(the session with {who} is not Established: nothing is advertised)")) };
    let mut s = format!("Advertised to {who}: {} prefix{}\n", list.len(), if list.len() == 1 { "" } else { "es" });
    for (net, at) in list {
        let nh = at.mp_reach6.as_ref().map_or("::".to_string(), |m| m.next_hop.to_string());
        let _ = writeln!(s, "  {:<30} next hop {nh:<28} {}", net.to_string(), path_text(&[], &at.as_path, &at.as_set, at.origin));
    }
    Ok(s.trim_end().to_string())
}
