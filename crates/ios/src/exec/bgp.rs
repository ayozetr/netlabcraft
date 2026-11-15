//! BGP: `router bgp N` and its mode (`bgp router-id`, `neighbor`, `network`, `redistribute`), the
//! `show ip bgp` views, `clear ip bgp` and `debug ip bgp`, and the section of the running
//! configuration.

use std::fmt::Write as _;
use std::net::Ipv4Addr;

use netlabcraft_core::{Ipv4Net, parse_ip};
use netlabcraft_model::bgp::{Bgp, HOLD_SECS, KEEPALIVE_SECS, MAX_NETWORKS, MAX_TEXT, Neighbor, Source};
use netlabcraft_model::{Device, DeviceKind, Rip, ifname, routing};
use netlabcraft_sim::DebugKind;
use netlabcraft_sim::bgp::{BgpView, NeighborView, Path, RouterView, SessionState};

use super::{Ctx, Mode, R};
use crate::grammar::Id;

fn asn(text: &str) -> Result<u32, String> {
    text.trim().parse::<u32>().ok().filter(|n| *n >= 1).ok_or_else(|| "% The autonomous system number is 1-4294967295".to_string())
}

fn address(text: &str) -> Result<Ipv4Addr, String> {
    parse_ip(text.trim()).map_err(|e| format!("% {e}"))
}

fn peer_address(text: &str) -> Result<Ipv4Addr, String> {
    let a = address(text)?;
    if a.is_unspecified() || a.is_multicast() || a == Ipv4Addr::BROADCAST {
        return Err(format!("% {a} is not an address a neighbour can have"));
    }
    Ok(a)
}

fn process<'a>(c: &'a mut Ctx<'_>) -> Result<&'a mut Bgp, String> {
    c.dm()?.bgp.as_mut().ok_or_else(|| "% Not in BGP mode".to_string())
}

/// A neighbour that was named with `remote-as` before.
fn existing(b: &mut Bgp, addr: Ipv4Addr) -> Result<&mut Neighbor, String> {
    b.neighbors.iter_mut().find(|n| n.addr == addr).ok_or_else(|| format!("% Name the neighbour first: neighbor {addr} remote-as <AS>"))
}

/// An interface name as the simulator spells it.
fn source_name(text: &str) -> Result<String, String> {
    let t = text.trim();
    if let Some(n) = routing::loopback_number(t) {
        return Ok(format!("Loopback{n}"));
    }
    let compact: String = t.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    if let Some(v) = compact.strip_prefix("vlan").and_then(|v| v.parse::<u16>().ok()) {
        return Ok(format!("Vlan{v}"));
    }
    ifname::parse(t).map(|n| n.to_string()).ok_or_else(|| format!("% `{t}` is not an interface name"))
}

/// `redistribute connected|static|ospf N`.
fn source_of(text: &str, needs_process: bool) -> Result<Source, String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let first = words.first().map(|w| w.to_ascii_lowercase()).unwrap_or_default();
    let pick = |name: &str| !first.is_empty() && name.starts_with(first.as_str());
    if pick("connected") || pick("static") {
        if words.len() != 1 {
            return Err("% `connected` and `static` take no more words".into());
        }
        return Ok(if pick("connected") { Source::Connected } else { Source::Static });
    }
    if pick("ospf") {
        if needs_process && words.get(1).and_then(|p| p.parse::<u32>().ok()).is_none_or(|p| !(1..=65_535).contains(&p)) {
            return Err("% redistribute ospf needs a process number (1-65535); any OSPF process counts".into());
        }
        return Ok(Source::Ospf);
    }
    Err("% redistribute connected, static or ospf <process>".into())
}

/// Runs one of this module's commands.
pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::BgpRouter => {
            let n = asn(arg(0))?;
            let d = c.dm()?;
            if d.kind == DeviceKind::Switch && !d.ip_routing {
                return Err("% BGP needs `ip routing` (multilayer switches)".into());
            }
            match &d.bgp {
                Some(b) if b.asn != n => {
                    return Err(format!("% BGP already runs here in AS {}; NetLabCraft runs one BGP process per router", b.asn));
                }
                Some(_) => {}
                None => d.bgp = Some(Bgp::new(n)),
            }
            c.cli.mode = Mode::Bgp;
            Ok(String::new())
        }
        Id::NoBgpRouter => {
            let n = asn(arg(0))?;
            let d = c.dm()?;
            if d.bgp.as_ref().is_none_or(|b| b.asn != n) {
                return Err(format!("% No BGP process for AS {n}"));
            }
            d.bgp = None;
            Ok(String::new())
        }
        Id::BgpRouterId | Id::NoBgpRouterId => {
            let rid = if id == Id::BgpRouterId { Some(address(arg(0))?) } else { None };
            process(c)?.router_id = rid;
            Ok(String::new())
        }
        Id::BgpLogChanges | Id::NoBgpLogChanges => {
            process(c)?.log_neighbor_changes = id == Id::BgpLogChanges;
            Ok(String::new())
        }
        Id::BgpDefaultLocalPref | Id::NoBgpDefaultLocalPref => {
            let v = if id == Id::BgpDefaultLocalPref {
                Some(arg(0).trim().parse::<u32>().map_err(|_| "% The local preference is 0-4294967295".to_string())?)
            } else {
                None
            };
            process(c)?.default_local_pref = v;
            Ok(String::new())
        }
        Id::BgpNeighborRemoteAs => {
            let a = peer_address(arg(0))?;
            let n = asn(arg(1))?;
            let b = process(c)?;
            b.neighbor_mut(a).ok_or_else(|| format!("% At most {} neighbours", netlabcraft_model::bgp::MAX_NEIGHBORS))?.remote_as = Some(n);
            Ok(String::new())
        }
        Id::NoBgpNeighbor => {
            let a = peer_address(arg(0))?;
            let b = process(c)?;
            let before = b.neighbors.len();
            b.neighbors.retain(|n| n.addr != a);
            if b.neighbors.len() == before { Err(format!("% No neighbour {a}")) } else { Ok(String::new()) }
        }
        Id::BgpNeighborUpdateSource | Id::NoBgpNeighborUpdateSource => {
            let a = peer_address(arg(0))?;
            let name = if id == Id::BgpNeighborUpdateSource { Some(source_name(arg(1))?) } else { None };
            existing(process(c)?, a)?.update_source = name;
            Ok(String::new())
        }
        Id::BgpNeighborNextHopSelf | Id::NoBgpNeighborNextHopSelf => {
            let a = peer_address(arg(0))?;
            existing(process(c)?, a)?.next_hop_self = id == Id::BgpNeighborNextHopSelf;
            Ok(String::new())
        }
        Id::BgpNeighborShutdown | Id::NoBgpNeighborShutdown => {
            let a = peer_address(arg(0))?;
            existing(process(c)?, a)?.shutdown = id == Id::BgpNeighborShutdown;
            Ok(String::new())
        }
        Id::BgpNeighborDescription | Id::NoBgpNeighborDescription => {
            let a = peer_address(arg(0))?;
            let text = if id == Id::BgpNeighborDescription {
                let t: String = arg(1).chars().filter(|ch| !ch.is_control()).take(MAX_TEXT).collect();
                Some(t)
            } else {
                None
            };
            existing(process(c)?, a)?.description = text;
            Ok(String::new())
        }
        Id::BgpNeighborMultihop | Id::NoBgpNeighborMultihop => {
            let a = peer_address(arg(0))?;
            let hops = if id == Id::BgpNeighborMultihop {
                if args.len() > 1 { Some(arg(1).trim().parse::<u8>().ok().filter(|h| *h >= 1).ok_or("% The hop count is 1-255")?) } else { Some(255) }
            } else {
                None
            };
            existing(process(c)?, a)?.ebgp_multihop = hops.filter(|h| *h > 1);
            Ok(String::new())
        }
        Id::BgpNeighborWeight | Id::NoBgpNeighborWeight => {
            let a = peer_address(arg(0))?;
            let w = if id == Id::BgpNeighborWeight {
                Some(arg(1).trim().parse::<u16>().map_err(|_| "% The weight is 0-65535".to_string())?)
            } else {
                None
            };
            existing(process(c)?, a)?.weight = w.filter(|w| *w > 0);
            Ok(String::new())
        }
        Id::BgpNetwork | Id::NoBgpNetwork => {
            let a = address(arg(0))?;
            let prefix = match args.get(1) {
                Some(m) => {
                    let mask = address(m)?;
                    netlabcraft_core::mask_to_prefix(mask).ok_or("% The mask must be contiguous, like 255.255.0.0")?
                }
                None => Rip::classful(a).prefix,
            };
            let net = Ipv4Net::new(a, prefix).network();
            let b = process(c)?;
            if id == Id::BgpNetwork {
                if !b.networks.contains(&net) {
                    if b.networks.len() >= MAX_NETWORKS {
                        return Err(format!("% At most {MAX_NETWORKS} network statements"));
                    }
                    b.networks.push(net);
                    b.networks.sort();
                }
                Ok(String::new())
            } else {
                let before = b.networks.len();
                b.networks.retain(|n| *n != net);
                if b.networks.len() == before { Err("% No such network statement".into()) } else { Ok(String::new()) }
            }
        }
        Id::BgpRedistribute | Id::NoBgpRedistribute => {
            let add = id == Id::BgpRedistribute;
            let src = source_of(arg(0), add)?;
            let b = process(c)?;
            b.redistribute.retain(|s| *s != src);
            if add {
                b.redistribute.push(src);
                b.redistribute.sort();
            }
            Ok(String::new())
        }
        Id::DebugIpBgp | Id::NoDebugIpBgp => {
            let on = id == Id::DebugIpBgp;
            c.sim.set_debug(c.dev, DebugKind::Bgp, on);
            Ok(if on { "BGP debugging is on: the messages this router hears and its session changes are reported." } else { "BGP debugging is off." }
                .into())
        }
        Id::ClearIpBgp => {
            let which = arg(0).trim();
            let only = if which == "*" { None } else { Some(address(which)?) };
            let d = c.d()?;
            let Some(b) = &d.bgp else { return Err("% BGP is not running on this router".into()) };
            if only.is_some_and(|a| b.neighbor(a).is_none()) {
                return Err(format!("% No neighbour {which}"));
            }
            let dev = c.dev;
            let n = c.sim.bgp_clear(c.t, dev, only);
            Ok(format!("{n} session{} reset; they start again.", if n == 1 { "" } else { "s" }))
        }
        Id::ShowIpBgp | Id::ShowIpBgpPrefix | Id::ShowIpBgpSummary | Id::ShowIpBgpNeighbors => {
            let view = c.sim.bgp_state(c.t);
            let d = c.d()?;
            Ok(match id {
                Id::ShowIpBgp => table(d, &view),
                Id::ShowIpBgpPrefix => entry(d, &view, arg(0), args.get(1).map(String::as_str))?,
                Id::ShowIpBgpSummary => summary(d, &view),
                _ => neighbors(d, &view, args.first().map(String::as_str))?,
            })
        }
        _ => Err("% Not a BGP command".into()),
    }
}

/// The BGP section of `show running-config`.
pub(crate) fn running_config(s: &mut String, d: &Device) {
    let Some(b) = &d.bgp else { return };
    let _ = writeln!(s, "router bgp {}", b.asn);
    if let Some(id) = b.router_id {
        let _ = writeln!(s, " bgp router-id {id}");
    }
    if b.log_neighbor_changes {
        let _ = writeln!(s, " bgp log-neighbor-changes");
    }
    if let Some(l) = b.default_local_pref {
        let _ = writeln!(s, " bgp default local-preference {l}");
    }
    for n in &b.neighbors {
        if let Some(r) = n.remote_as {
            let _ = writeln!(s, " neighbor {} remote-as {r}", n.addr);
        }
        if let Some(text) = &n.description {
            let _ = writeln!(s, " neighbor {} description {text}", n.addr);
        }
        if let Some(u) = &n.update_source {
            let _ = writeln!(s, " neighbor {} update-source {u}", n.addr);
        }
        if let Some(h) = n.ebgp_multihop {
            let _ = writeln!(s, " neighbor {} ebgp-multihop {h}", n.addr);
        }
        if n.next_hop_self {
            let _ = writeln!(s, " neighbor {} next-hop-self", n.addr);
        }
        if let Some(w) = n.weight {
            let _ = writeln!(s, " neighbor {} weight {w}", n.addr);
        }
        if n.shutdown {
            let _ = writeln!(s, " neighbor {} shutdown", n.addr);
        }
    }
    for n in &b.networks {
        let _ = writeln!(s, " network {} mask {}", n.addr, n.mask());
    }
    for r in &b.redistribute {
        match r {
            Source::Ospf => {
                let _ = writeln!(s, " redistribute ospf 1");
            }
            other => {
                let _ = writeln!(s, " redistribute {}", other.word());
            }
        }
    }
    let _ = writeln!(s, "!");
}

fn hms(us: u64) -> String {
    let secs = us / 1_000_000;
    format!("{:02}:{:02}:{:02}", secs / 3_600, secs / 60 % 60, secs % 60)
}

fn state_text(n: &NeighborView) -> String {
    if n.admin_down { "Idle (Admin)".to_string() } else { n.state.name().to_string() }
}

fn not_running() -> String {
    "BGP is not running on this router.".to_string()
}

fn running<'a>(d: &Device, v: &'a BgpView) -> Option<&'a RouterView> {
    d.bgp.as_ref()?;
    v.routers.get(&d.id)
}

/// `show ip bgp summary`.
fn summary(d: &Device, v: &BgpView) -> String {
    let Some(r) = running(d, v) else { return not_running() };
    let best = r.paths.iter().filter(|p| p.best).count();
    let prefixes: std::collections::BTreeSet<Ipv4Net> = r.paths.iter().map(|p| p.net).collect();
    let mut s = format!("BGP router id {}, local AS {}\n", r.router_id, r.asn);
    let _ = writeln!(
        s,
        "Paths in the table: {} ({best} best) for {} prefix{}",
        r.paths.len(),
        prefixes.len(),
        if prefixes.len() == 1 { "" } else { "es" }
    );
    if r.neighbors.is_empty() {
        s.push_str("\n(no neighbours: add one with `neighbor A.B.C.D remote-as <AS>`)");
        return s;
    }
    let _ = writeln!(s, "\n{:<16} {:>11} {:>7} {:>7} {:<9} State/Prefixes", "Neighbor", "AS", "Rcvd", "Sent", "Up/Down");
    for n in &r.neighbors {
        let c = v.counters.get(&(d.id, n.addr)).copied().unwrap_or_default();
        let up = match v.since.get(&(d.id, n.addr)) {
            Some(at) if n.state == SessionState::Established => hms(v.now.saturating_sub(*at)),
            _ => "never".to_string(),
        };
        let last = if n.state == SessionState::Established { n.received.to_string() } else { state_text(n) };
        let remote = n.remote_as.map_or("-".to_string(), |a| a.to_string());
        let _ = writeln!(s, "{:<16} {remote:>11} {:>7} {:>7} {up:<9} {last}", n.addr.to_string(), c.total_rcvd(), c.total_sent());
    }
    s.trim_end().to_string()
}

fn origin_char(o: u8) -> char {
    match o {
        0 => 'i',
        1 => 'e',
        _ => '?',
    }
}

fn as_path_text(p: &Path) -> String {
    let mut s = p.as_path.iter().map(u32::to_string).collect::<Vec<_>>().join(" ");
    if !s.is_empty() {
        s.push(' ');
    }
    s.push(origin_char(p.origin));
    s
}

/// `show ip bgp`.
fn table(d: &Device, v: &BgpView) -> String {
    let Some(r) = running(d, v) else { return not_running() };
    if r.paths.is_empty() {
        return format!("BGP router id {}, local AS {}\n(the BGP table is empty: nothing is advertised and nothing was learned)", r.router_id, r.asn);
    }
    let mut s = format!("BGP router id {}, local AS {}\n", r.router_id, r.asn);
    s.push_str("Status: * usable, > best, i from an internal neighbour    Origin: i IGP, e EGP, ? incomplete\n\n");
    let _ = writeln!(s, "    {:<18} {:<16} {:>6} {:>6} {:>6}  Path", "Network", "Next hop", "Metric", "LocPrf", "Weight");
    let mut last: Option<Ipv4Net> = None;
    for p in &r.paths {
        let status =
            format!("{}{}{}", if p.valid { '*' } else { ' ' }, if p.best { '>' } else { ' ' }, if p.from.is_some() && !p.ebgp { 'i' } else { ' ' });
        let net = if last == Some(p.net) { String::new() } else { p.net.to_string() };
        last = Some(p.net);
        let _ =
            writeln!(s, "{status} {net:<18} {:<16} {:>6} {:>6} {:>6}  {}", p.next_hop.to_string(), p.med, p.local_pref, p.weight, as_path_text(p));
    }
    s.trim_end().to_string()
}

/// `show ip bgp A.B.C.D [M.M.M.M]`.
fn entry(d: &Device, v: &BgpView, addr: &str, mask: Option<&str>) -> Result<String, String> {
    let Some(r) = running(d, v) else { return Ok(not_running()) };
    let a = address(addr)?;
    let net = match mask {
        Some(m) => {
            let prefix = netlabcraft_core::mask_to_prefix(address(m)?).ok_or("% The mask must be contiguous, like 255.255.0.0")?;
            Some(Ipv4Net::new(a, prefix).network())
        }
        None => r.paths.iter().filter(|p| p.net.contains(a)).map(|p| p.net).max_by_key(|n| n.prefix),
    };
    let Some(net) = net else { return Ok(format!("% {a} is not in the BGP table")) };
    let paths: Vec<&Path> = r.paths.iter().filter(|p| p.net == net).collect();
    if paths.is_empty() {
        return Ok(format!("% {net} is not in the BGP table"));
    }
    let best = paths.iter().position(|p| p.best);
    let mut s = format!("BGP entry for {net}: {} path{}", paths.len(), if paths.len() == 1 { "" } else { "s" });
    match best {
        Some(i) => {
            let _ = write!(s, ", the best is path {}", i + 1);
        }
        None => s.push_str(", none usable (no next hop resolves)"),
    }
    s.push('\n');
    for (i, p) in paths.iter().enumerate() {
        let path = if p.as_path.is_empty() { "local".to_string() } else { p.as_path.iter().map(u32::to_string).collect::<Vec<_>>().join(" ") };
        let source = match p.from {
            Some(f) => {
                let id =
                    r.neighbors.iter().find(|n| n.addr == f).and_then(|n| n.peer_router_id).map_or(String::new(), |x| format!(", router id {x}"));
                format!("learned from {f}{id}")
            }
            None => "originated here".to_string(),
        };
        let _ = writeln!(s, "  Path {}{}: AS path {path}, next hop {}, {source}", i + 1, if p.best { " (best)" } else { "" }, p.next_hop);
        let kind = match (p.from.is_some(), p.ebgp) {
            (false, _) => "local",
            (true, true) => "external",
            (true, false) => "internal",
        };
        let origin = match p.origin {
            0 => "IGP",
            1 => "EGP",
            _ => "incomplete",
        };
        let _ = writeln!(
            s,
            "    origin {origin}, metric {}, local preference {}, weight {}, {kind}, {}",
            p.med,
            p.local_pref,
            p.weight,
            if p.valid { "usable" } else { "next hop unreachable" }
        );
    }
    Ok(s.trim_end().to_string())
}

/// `show ip bgp neighbors [A.B.C.D]`.
fn neighbors(d: &Device, v: &BgpView, only: Option<&str>) -> Result<String, String> {
    let (Some(r), Some(b)) = (running(d, v), d.bgp.as_ref()) else { return Ok(not_running()) };
    let want = match only {
        Some(t) => Some(address(t)?),
        None => None,
    };
    let list: Vec<&NeighborView> = r.neighbors.iter().filter(|n| want.is_none_or(|a| a == n.addr)).collect();
    if list.is_empty() {
        return Ok(match want {
            Some(a) => format!("% No neighbour {a}"),
            None => "(no neighbours: add one with `neighbor A.B.C.D remote-as <AS>`)".to_string(),
        });
    }
    let mut blocks = Vec::new();
    for n in list {
        let conf = b.neighbor(n.addr);
        let link = match n.remote_as {
            Some(a) if a == r.asn => "internal link",
            Some(_) => "external link",
            None => "no remote AS",
        };
        let mut s = format!("BGP neighbour {}, remote AS {} ({link})\n", n.addr, n.remote_as.map_or("-".to_string(), |a| a.to_string()));
        if let Some(text) = conf.and_then(|c| c.description.as_ref()) {
            let _ = writeln!(s, "  Description: {text}");
        }
        let c = v.counters.get(&(d.id, n.addr)).copied().unwrap_or_default();
        match v.since.get(&(d.id, n.addr)) {
            Some(at) if n.state == SessionState::Established => {
                let _ = writeln!(s, "  State: Established, up for {}", hms(v.now.saturating_sub(*at)));
            }
            _ => {
                let _ = writeln!(s, "  State: {}", state_text(n));
            }
        }
        if let Some(id) = n.peer_router_id {
            let _ = writeln!(s, "  Neighbour router id: {id}");
        }
        if let Some(l) = n.local {
            let via = conf.and_then(|c| c.update_source.as_ref()).map_or(String::new(), |u| format!(" (update source {u})"));
            let _ = writeln!(s, "  Local address: {l}{via}");
        }
        let _ = writeln!(s, "  Hold time {HOLD_SECS} s, keepalive every {KEEPALIVE_SECS} s");
        let mut options = Vec::new();
        if conf.is_some_and(|c| c.next_hop_self) {
            options.push("next-hop-self".to_string());
        }
        if let Some(h) = conf.and_then(|c| c.ebgp_multihop) {
            options.push(format!("ebgp-multihop {h}"));
        }
        if let Some(w) = conf.and_then(|c| c.weight) {
            options.push(format!("weight {w}"));
        }
        if !options.is_empty() {
            let _ = writeln!(s, "  Options: {}", options.join(", "));
        }
        let _ = writeln!(
            s,
            "  Messages sent {} (opens {}, updates {}, notifications {}, keepalives {})",
            c.total_sent(),
            c.sent.first().copied().unwrap_or(0),
            c.sent.get(1).copied().unwrap_or(0),
            c.sent.get(2).copied().unwrap_or(0),
            c.sent.get(3).copied().unwrap_or(0)
        );
        let _ = writeln!(
            s,
            "  Messages received {} (opens {}, updates {}, notifications {}, keepalives {})",
            c.total_rcvd(),
            c.rcvd.first().copied().unwrap_or(0),
            c.rcvd.get(1).copied().unwrap_or(0),
            c.rcvd.get(2).copied().unwrap_or(0),
            c.rcvd.get(3).copied().unwrap_or(0)
        );
        let _ = writeln!(s, "  Prefixes received {}, sent {}", n.received, n.sent);
        if let Some(e) = &n.error {
            let _ = writeln!(s, "  Last error: {e}");
        }
        blocks.push(s.trim_end().to_string());
    }
    Ok(blocks.join("\n\n"))
}

/// `show ip protocols` with BGP's part added to what the other protocols printed.
pub(crate) fn with_protocol(d: &Device, v: &BgpView, others: String) -> String {
    let Some(r) = running(d, v) else { return others };
    let up = r.neighbors.iter().filter(|n| n.state == SessionState::Established).count();
    let mut s = format!("Routing protocol: BGP, autonomous system {}\n", r.asn);
    let _ = writeln!(s, "  Router id: {}", r.router_id);
    let _ = writeln!(s, "  Neighbours: {} configured, {up} established", r.neighbors.len());
    let _ = write!(
        s,
        "  Paths in the table: {} ({} best); administrative distance 20 (external) and 200 (internal)",
        r.paths.len(),
        r.paths.iter().filter(|p| p.best).count()
    );
    if others.starts_with("No routing protocol") { s } else { format!("{others}\n\n{s}") }
}
