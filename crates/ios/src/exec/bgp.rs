//! BGP: `router bgp N` and its mode (`bgp router-id`, `neighbor`, `network`, `redistribute`), the
//! `show ip bgp` views, `clear ip bgp` and `debug ip bgp`, and the section of the running
//! configuration.

use std::fmt::Write as _;
use std::net::Ipv4Addr;

use netlabcraft_core::{Ipv4Net, parse_ip};
use netlabcraft_model::bgp::{
    Aggregate, Bgp, DampParams, Dampening, HOLD_SECS, KEEPALIVE_SECS, LocalAs, MAX_AGGREGATES, MAX_CONFED_PEERS, MAX_MULTIPATH, MAX_NETWORKS,
    MAX_OPTIONS, MAX_PASSWORD, MAX_PEER_GROUPS, MAX_TEXT, Neighbor, PeerGroup, Source, follow,
};
use netlabcraft_model::policy::{self, community_text, parse_community};
use netlabcraft_model::{Device, DeviceKind, Rip, ifname, routing};
use netlabcraft_sim::DebugKind;
use netlabcraft_sim::bgp::{BgpView, NeighborView, Path, RouterView, SessionState};

use super::{Ctx, Mode, R, line_password};
use crate::grammar::{Id, Spec, Tok};

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

/// Changes what `neighbor WHO …` sets: one neighbour (WHO is its address) or a peer group (its name).
/// A change to a group reaches the members that follow it; after a change to a member, the settings
/// it left unset (a `no` form) come from its group again.
fn edit(b: &mut Bgp, who: &str, f: impl FnOnce(&mut Neighbor) -> Result<(), String>) -> Result<(), String> {
    let who = who.trim();
    if who.parse::<Ipv4Addr>().is_ok() {
        let a = peer_address(who)?;
        let conf = b.neighbor(a).and_then(|n| n.peer_group.as_deref()).and_then(|g| b.group(g)).map(|g| g.conf.clone());
        let n = existing(b, a)?;
        f(n)?;
        if let Some(conf) = conf {
            follow(n, &Neighbor::new(a), &conf);
        }
        return Ok(());
    }
    b.edit_group(who, f).unwrap_or_else(|| Err(format!("% No neighbour or peer group named {who}")))
}

/// A peer group's name: a policy-style name that cannot be mistaken for an address.
fn group_name(text: &str) -> Result<String, String> {
    let t = policy_name(text)?;
    if t.parse::<Ipv4Addr>().is_ok() {
        return Err("% A peer group's name cannot be an address".into());
    }
    Ok(t)
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

/// `redistribute SOURCE … [route-map NAME]`: the words before `route-map` and the map's name.
fn split_map(text: &str) -> Result<(String, Option<String>), String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let Some(at) = words.iter().position(|w| w.eq_ignore_ascii_case("route-map")) else {
        return Ok((words.join(" "), None));
    };
    let name = match (words.get(at + 1), words.len() == at + 2) {
        (Some(n), true) => *n,
        _ => return Err("% route-map takes one name, and it ends the line".into()),
    };
    if name.chars().count() > netlabcraft_model::policy::MAX_NAME || name.chars().any(char::is_control) {
        return Err(format!("% A name is 1-{} characters", netlabcraft_model::policy::MAX_NAME));
    }
    Ok((words.get(..at).unwrap_or(&[]).join(" "), Some(name.to_string())))
}

/// The four numbers of `bgp dampening` and `set dampening`: half-life, reuse, suppress, longest suppression.
pub(super) fn damp_params(args: &[String]) -> Result<DampParams, String> {
    let num = |i: usize, what: &str| -> Result<u16, String> {
        args.get(i).and_then(|t| t.trim().parse::<u16>().ok()).ok_or_else(|| format!("% The {what} is a number"))
    };
    DampParams {
        half_life: num(0, "half-life")?,
        reuse: num(1, "reuse limit")?,
        suppress: num(2, "suppress limit")?,
        max_suppress: num(3, "maximum suppress time")?,
    }
    .check()
    .map_err(|e| format!("% {e}"))
}

/// `local-as N [no-prepend [replace-as [dual-as]]]`: each option needs the one before it.
fn local_as(number: &str, words: &str) -> Result<LocalAs, String> {
    let lower: Vec<String> = words.split_whitespace().map(str::to_ascii_lowercase).collect();
    let words: Vec<&str> = lower.iter().map(String::as_str).collect();
    let (no_prepend, replace_as, dual_as) = match words.as_slice() {
        [] => (false, false, false),
        ["no-prepend"] => (true, false, false),
        ["no-prepend", "replace-as"] => (true, true, false),
        ["no-prepend", "replace-as", "dual-as"] => (true, true, true),
        _ => return Err("% After the AS number: no-prepend, then replace-as, then dual-as (each needs the one before it)".into()),
    };
    Ok(LocalAs { asn: asn(number)?, no_prepend, replace_as, dual_as })
}

/// `in` or `out`.
fn direction(text: &str) -> Result<bool, String> {
    match text.to_ascii_lowercase().as_str() {
        "in" => Ok(true),
        "out" => Ok(false),
        _ => Err("% Say in or out".into()),
    }
}

fn policy_name(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() || t.chars().count() > netlabcraft_model::policy::MAX_NAME || t.chars().any(char::is_control) {
        return Err(format!("% A name is 1-{} characters", netlabcraft_model::policy::MAX_NAME));
    }
    Ok(t.to_string())
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
            let who = arg(0).trim();
            let remote = asn(arg(1))?;
            let b = process(c)?;
            if who.parse::<Ipv4Addr>().is_ok() {
                // A neighbour is named by this line: it exists from here on.
                let a = peer_address(who)?;
                b.neighbor_mut(a).ok_or_else(|| format!("% At most {} neighbours", netlabcraft_model::bgp::MAX_NEIGHBORS))?;
            }
            edit(b, who, |n| {
                n.remote_as = Some(remote);
                Ok(())
            })?;
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
            let name = if id == Id::BgpNeighborUpdateSource { Some(source_name(arg(1))?) } else { None };
            edit(process(c)?, arg(0), |n| {
                n.update_source = name;
                Ok(())
            })?;
            Ok(String::new())
        }
        Id::BgpNeighborNextHopSelf | Id::NoBgpNeighborNextHopSelf => {
            let on = id == Id::BgpNeighborNextHopSelf;
            edit(process(c)?, arg(0), |n| {
                n.next_hop_self = on;
                Ok(())
            })?;
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
            let hops = if id == Id::BgpNeighborMultihop {
                if args.len() > 1 { Some(arg(1).trim().parse::<u8>().ok().filter(|h| *h >= 1).ok_or("% The hop count is 1-255")?) } else { Some(255) }
            } else {
                None
            };
            edit(process(c)?, arg(0), |n| {
                n.ebgp_multihop = hops.filter(|h| *h > 1);
                Ok(())
            })?;
            Ok(String::new())
        }
        Id::BgpNeighborWeight | Id::NoBgpNeighborWeight => {
            let w = if id == Id::BgpNeighborWeight {
                Some(arg(1).trim().parse::<u16>().map_err(|_| "% The weight is 0-65535".to_string())?)
            } else {
                None
            };
            edit(process(c)?, arg(0), |n| {
                n.weight = w.filter(|w| *w > 0);
                Ok(())
            })?;
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
            let (head, map) = split_map(arg(0))?;
            let src = source_of(&head, add)?;
            let b = process(c)?;
            b.redistribute.retain(|s| *s != src);
            b.redistribute_maps.remove(src.word());
            if add {
                b.redistribute.push(src);
                b.redistribute.sort();
                if let Some(m) = map {
                    b.redistribute_maps.insert(src.word().to_string(), m);
                }
            }
            Ok(String::new())
        }
        Id::BgpNeighborPrefixList | Id::NoBgpNeighborPrefixList | Id::BgpNeighborRouteMap | Id::NoBgpNeighborRouteMap => {
            let who = arg(0).trim().to_string();
            let name = policy_name(arg(1))?;
            let inbound = direction(arg(2))?;
            let set = matches!(id, Id::BgpNeighborPrefixList | Id::BgpNeighborRouteMap);
            let lists = matches!(id, Id::BgpNeighborPrefixList | Id::NoBgpNeighborPrefixList);
            edit(process(c)?, &who, |n| {
                let slot = match (lists, inbound) {
                    (true, true) => &mut n.prefix_list_in,
                    (true, false) => &mut n.prefix_list_out,
                    (false, true) => &mut n.route_map_in,
                    (false, false) => &mut n.route_map_out,
                };
                if set {
                    *slot = Some(name);
                } else if slot.as_deref() == Some(name.as_str()) {
                    *slot = None;
                } else {
                    return Err(format!("% {name} is not applied to {who} in that direction"));
                }
                Ok(())
            })?;
            Ok(String::new())
        }
        Id::BgpNeighborDistribute | Id::NoBgpNeighborDistribute => {
            let who = arg(0).trim().to_string();
            let name = policy_name(arg(1))?;
            let inbound = direction(arg(2))?;
            let set = id == Id::BgpNeighborDistribute;
            edit(process(c)?, &who, |n| {
                let slot = if inbound { &mut n.distribute_in } else { &mut n.distribute_out };
                if set {
                    *slot = Some(name);
                } else if slot.as_deref() == Some(name.as_str()) {
                    *slot = None;
                } else {
                    return Err(format!("% {name} is not applied to {who} in that direction"));
                }
                Ok(())
            })?;
            Ok(String::new())
        }
        Id::BgpNeighborSendCommunity | Id::NoBgpNeighborSendCommunity => {
            let on = id == Id::BgpNeighborSendCommunity;
            // Which kinds the line touches: without a word, `send-community` is the standard
            // communities and `no send-community` ends both.
            let (standard, extended) = match (on, arg(1).to_ascii_lowercase().as_str()) {
                (true, "" | "standard") | (false, "standard") => (true, false),
                (_, "extended") => (false, true),
                (true, "both") | (false, "" | "both") => (true, true),
                _ => return Err("% Say both, standard or extended".into()),
            };
            edit(process(c)?, arg(0), |n| {
                if standard {
                    n.send_community = on;
                }
                if extended {
                    n.send_extended = on;
                }
                Ok(())
            })?;
            Ok(String::new())
        }
        Id::BgpNeighborTimers | Id::NoBgpNeighborTimers => {
            let timers = if id == Id::BgpNeighborTimers {
                let keepalive = arg(1).trim().parse::<u16>().map_err(|_| "% The keepalive interval is 0-65535 seconds".to_string())?;
                let hold = arg(2)
                    .trim()
                    .parse::<u16>()
                    .ok()
                    .filter(|h| *h == 0 || *h >= 3)
                    .ok_or("% The hold time is 0 (never expires) or 3-65535 seconds")?;
                Some((keepalive, hold)).filter(|t| *t != (KEEPALIVE_SECS, HOLD_SECS))
            } else {
                None
            };
            edit(process(c)?, arg(0), |n| {
                n.timers = timers;
                Ok(())
            })?;
            Ok(String::new())
        }
        Id::BgpTimers | Id::NoBgpTimers => {
            let timers = if id == Id::BgpTimers {
                let keepalive = arg(0).trim().parse::<u16>().map_err(|_| "% The keepalive interval is 0-65535 seconds".to_string())?;
                let hold = arg(1)
                    .trim()
                    .parse::<u16>()
                    .ok()
                    .filter(|h| *h == 0 || *h >= 3)
                    .ok_or("% The hold time is 0 (never expires) or 3-65535 seconds")?;
                Some((keepalive, hold)).filter(|t| *t != (KEEPALIVE_SECS, HOLD_SECS))
            } else {
                None
            };
            process(c)?.timers = timers;
            Ok(String::new())
        }
        Id::BgpDampening | Id::NoBgpDampening => {
            let dampening = if id == Id::BgpDampening {
                Some(match args.len() {
                    0 => Dampening { params: DampParams::default(), route_map: None, explicit: false },
                    1 => Dampening { params: DampParams::default(), route_map: Some(policy_name(arg(0))?), explicit: false },
                    _ => Dampening { params: damp_params(args)?, route_map: None, explicit: true },
                })
            } else {
                None
            };
            process(c)?.dampening = dampening;
            Ok(String::new())
        }
        Id::ShowIpBgpDampened | Id::ShowIpBgpFlapStatistics => {
            let view = c.sim.bgp_state(c.t);
            Ok(flap_table(c.d()?, &view, id == Id::ShowIpBgpDampened))
        }
        Id::ClearIpBgpDampening => {
            let view = c.sim.bgp_state(c.t);
            let d = c.d()?;
            if d.bgp.is_none() {
                return Err("% BGP is not running on this router".into());
            }
            let only = match args.len() {
                0 => None,
                1 => {
                    let a = address(arg(0))?;
                    let found = view.flaps.keys().filter(|k| k.0 == d.id && k.2.contains(a)).map(|k| k.2).max_by_key(|n| n.prefix);
                    Some(found.ok_or_else(|| format!("% No dampening history for {a}"))?)
                }
                _ => {
                    let prefix = netlabcraft_core::mask_to_prefix(address(arg(1))?).ok_or("% The mask must be contiguous, like 255.255.0.0")?;
                    Some(Ipv4Net::new(address(arg(0))?, prefix).network())
                }
            };
            let dev = c.dev;
            let n = c.sim.bgp_clear_dampening(c.t, dev, only);
            Ok(format!("Dampening history of {n} route{} forgotten; held-down routes are usable again.", if n == 1 { "" } else { "s" }))
        }
        Id::BgpNeighborFilterList | Id::NoBgpNeighborFilterList => {
            let who = arg(0).trim().to_string();
            let name = arg(1)
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|n| (1..=500).contains(n))
                .map(|n| n.to_string())
                .ok_or("% An AS-path list is numbered 1-500")?;
            let inbound = direction(arg(2))?;
            let set = id == Id::BgpNeighborFilterList;
            edit(process(c)?, &who, |n| {
                let slot = if inbound { &mut n.filter_list_in } else { &mut n.filter_list_out };
                if set {
                    *slot = Some(name);
                } else if slot.as_deref() == Some(name.as_str()) {
                    *slot = None;
                } else {
                    return Err(format!("% AS-path list {name} is not applied to {who} in that direction"));
                }
                Ok(())
            })?;
            Ok(String::new())
        }
        Id::BgpAggregate | Id::NoBgpAggregate => {
            let a = address(arg(0))?;
            let prefix = netlabcraft_core::mask_to_prefix(address(arg(1))?).ok_or("% The mask must be contiguous, like 255.255.0.0")?;
            let net = Ipv4Net::new(a, prefix).network();
            let (mut summary_only, mut as_set) = (false, false);
            for w in arg(2).split_whitespace() {
                match w.to_ascii_lowercase().as_str() {
                    "summary-only" if !summary_only => summary_only = true,
                    "as-set" if !as_set => as_set = true,
                    _ => return Err(format!("% Unexpected `{w}`: only summary-only and as-set may follow, once each")),
                }
            }
            let b = process(c)?;
            if id == Id::BgpAggregate {
                if !b.aggregates.iter().any(|g| g.net == net) && b.aggregates.len() >= MAX_AGGREGATES {
                    return Err(format!("% At most {MAX_AGGREGATES} aggregate addresses"));
                }
                b.aggregates.retain(|g| g.net != net);
                b.aggregates.push(Aggregate { net, summary_only, as_set });
                b.aggregates.sort_by_key(|g| g.net);
                Ok(String::new())
            } else {
                let before = b.aggregates.len();
                b.aggregates.retain(|g| g.net != net);
                if b.aggregates.len() == before { Err("% No such aggregate address".into()) } else { Ok(String::new()) }
            }
        }
        Id::BgpMaximumPaths | Id::NoBgpMaximumPaths => {
            let n = if id == Id::BgpMaximumPaths {
                arg(0)
                    .trim()
                    .parse::<u8>()
                    .ok()
                    .filter(|n| (1..=MAX_MULTIPATH).contains(n))
                    .ok_or_else(|| format!("% The number of paths is 1-{MAX_MULTIPATH}"))?
            } else {
                1
            };
            process(c)?.maximum_paths = Some(n).filter(|n| *n > 1);
            Ok(String::new())
        }
        Id::BgpNeighborRrClient | Id::NoBgpNeighborRrClient => {
            let who = arg(0).trim().to_string();
            let on = id == Id::BgpNeighborRrClient;
            let b = process(c)?;
            let own = b.asn;
            edit(b, &who, |n| {
                if on && n.remote_as.is_some_and(|r| r != own) {
                    return Err(format!(
                        "% A route reflector client is an internal neighbour: {who} is in AS {}, not in AS {own}",
                        n.remote_as.unwrap_or(0)
                    ));
                }
                n.route_reflector_client = on;
                Ok(())
            })?;
            Ok(String::new())
        }
        Id::BgpClusterId | Id::NoBgpClusterId => {
            let v = if id == Id::BgpClusterId {
                let text = arg(0).trim();
                let a = if text.contains('.') {
                    address(text)?
                } else {
                    Ipv4Addr::from(text.parse::<u32>().ok().filter(|n| *n >= 1).ok_or("% The cluster id is A.B.C.D or 1-4294967295")?)
                };
                Some(a)
            } else {
                None
            };
            process(c)?.cluster_id = v;
            Ok(String::new())
        }
        Id::BgpConfedId | Id::NoBgpConfedId => {
            let v = if id == Id::BgpConfedId { Some(asn(arg(0))?) } else { None };
            let b = process(c)?;
            if v == Some(b.asn) {
                return Err("% The confederation identifier must differ from this router's member AS".into());
            }
            b.confed_id = v;
            Ok(String::new())
        }
        Id::BgpConfedPeers | Id::NoBgpConfedPeers => {
            let mut list = Vec::new();
            for w in arg(0).split_whitespace() {
                list.push(asn(w)?);
            }
            let b = process(c)?;
            if id == Id::BgpConfedPeers {
                if list.contains(&b.asn) {
                    return Err("% This router's own member AS is not a confederation peer".into());
                }
                for a in list {
                    if !b.confed_peers.contains(&a) {
                        if b.confed_peers.len() >= MAX_CONFED_PEERS {
                            return Err(format!("% At most {MAX_CONFED_PEERS} confederation peers"));
                        }
                        b.confed_peers.push(a);
                    }
                }
                b.confed_peers.sort_unstable();
            } else if list.is_empty() {
                b.confed_peers.clear();
            } else {
                b.confed_peers.retain(|a| !list.contains(a));
            }
            Ok(String::new())
        }
        Id::BgpNeighborDefaultOriginate | Id::NoBgpNeighborDefaultOriginate => {
            let on = id == Id::BgpNeighborDefaultOriginate;
            let map = if on && !arg(1).is_empty() { Some(policy_name(arg(1))?) } else { None };
            edit(process(c)?, arg(0), |n| {
                n.default_originate = on;
                n.default_originate_map = map;
                Ok(())
            })?;
            Ok(String::new())
        }
        Id::BgpNeighborPassword | Id::NoBgpNeighborPassword => {
            let key = if id == Id::BgpNeighborPassword { Some(line_password(arg(1))?) } else { None };
            if key.as_ref().is_some_and(|k| k.chars().count() > MAX_PASSWORD) {
                return Err(format!("% A neighbour password is at most {MAX_PASSWORD} characters"));
            }
            edit(process(c)?, arg(0), |n| {
                n.password = key;
                Ok(())
            })?;
            Ok(String::new())
        }
        Id::BgpNeighborRemovePrivate | Id::NoBgpNeighborRemovePrivate => {
            let on = id == Id::BgpNeighborRemovePrivate;
            if !arg(1).is_empty() && !arg(1).eq_ignore_ascii_case("all") {
                return Err("% Only `all` may follow remove-private-as".into());
            }
            let all = on && !arg(1).is_empty();
            edit(process(c)?, arg(0), |n| {
                n.remove_private_as = on;
                n.remove_private_all = all;
                Ok(())
            })?;
            Ok(String::new())
        }
        Id::BgpNeighborLocalAs | Id::NoBgpNeighborLocalAs => {
            let on = id == Id::BgpNeighborLocalAs;
            let local = if on { Some(local_as(arg(1), arg(2))?) } else { None };
            let b = process(c)?;
            let own = b.asn;
            edit(b, arg(0), |n| {
                if local.is_some_and(|l| l.asn == own) {
                    return Err(format!("% AS {own} is this router's own AS: local-as names a different one"));
                }
                if local.is_some() && n.remote_as == Some(own) {
                    return Err("% local-as is for external neighbours, and this one is in this router's AS".into());
                }
                n.local_as = local;
                Ok(())
            })?;
            Ok(String::new())
        }
        Id::BgpNeighborSoftReconf | Id::NoBgpNeighborSoftReconf => {
            let on = id == Id::BgpNeighborSoftReconf;
            edit(process(c)?, arg(0), |n| {
                n.soft_reconfiguration_in = on;
                Ok(())
            })?;
            Ok(String::new())
        }
        Id::BgpPeerGroup => {
            let name = group_name(arg(0))?;
            process(c)?.group_mut(&name).ok_or_else(|| format!("% At most {MAX_PEER_GROUPS} peer groups"))?;
            Ok(String::new())
        }
        Id::NoBgpPeerGroup => {
            let name = arg(0).trim().to_string();
            if process(c)?.remove_group(&name) { Ok(String::new()) } else { Err(format!("% No peer group named {name}")) }
        }
        Id::BgpNeighborPeerGroup => {
            let a = peer_address(arg(0))?;
            let name = group_name(arg(1))?;
            process(c)?.join_group(a, &name).map_err(|e| format!("% {e}"))?;
            Ok(String::new())
        }
        Id::NoBgpNeighborPeerGroup => {
            let a = peer_address(arg(0))?;
            let name = arg(1).trim().to_string();
            let b = process(c)?;
            if b.neighbor(a).and_then(|n| n.peer_group.as_deref()) != Some(name.as_str()) {
                return Err(format!("% {a} is not in peer group {name}"));
            }
            b.leave_group(a);
            Ok(String::new())
        }
        Id::ShowIpBgpPeerGroup => {
            let view = c.sim.bgp_state(c.t);
            peer_groups(c.d()?, &view, args.first().map(String::as_str))
        }
        Id::BgpAlwaysCompareMed | Id::NoBgpAlwaysCompareMed => {
            process(c)?.always_compare_med = id == Id::BgpAlwaysCompareMed;
            Ok(String::new())
        }
        Id::BgpAsPathIgnore | Id::NoBgpAsPathIgnore => {
            process(c)?.as_path_ignore = id == Id::BgpAsPathIgnore;
            Ok(String::new())
        }
        Id::BgpMaximumPathsIbgp | Id::NoBgpMaximumPathsIbgp => {
            let n = if id == Id::BgpMaximumPathsIbgp {
                arg(0)
                    .trim()
                    .parse::<u8>()
                    .ok()
                    .filter(|n| (1..=MAX_MULTIPATH).contains(n))
                    .ok_or_else(|| format!("% The number of paths is 1-{MAX_MULTIPATH}"))?
            } else {
                1
            };
            process(c)?.maximum_paths_ibgp = Some(n).filter(|n| *n > 1);
            Ok(String::new())
        }
        Id::ShowIpBgpReceived => {
            let view = c.sim.bgp_state(c.t);
            received_routes(c.d()?, &view, arg(0))
        }
        Id::ShowIpBgpAdvertised | Id::ShowIpBgpNeighborRoutes => {
            let view = c.sim.bgp_state(c.t);
            neighbor_table(c.d()?, &view, arg(0), id == Id::ShowIpBgpAdvertised)
        }
        Id::ShowIpBgpRegexp | Id::ShowIpBgpFilterList => {
            let view = c.sim.bgp_state(c.t);
            let d = c.d()?;
            if id == Id::ShowIpBgpRegexp { regexp_table(d, &view, arg(0)) } else { filter_list_table(d, &view, arg(0)) }
        }
        Id::ShowIpBgpCommunity | Id::ShowIpBgpCommunityList => {
            let view = c.sim.bgp_state(c.t);
            let d = c.d()?;
            if id == Id::ShowIpBgpCommunity { community_table(d, &view, arg(0)) } else { community_list_table(d, &view, arg(0)) }
        }
        Id::ClearIpBgpSoft => {
            let which = arg(0).trim();
            if !arg(1).is_empty() {
                direction(arg(1))?;
            }
            let d = c.d()?;
            let Some(b) = &d.bgp else { return Err("% BGP is not running on this router".into()) };
            if which != "*" && b.neighbor(address(which)?).is_none() {
                return Err(format!("% No neighbour {which}"));
            }
            Ok("No session was reset: prefix lists and route maps apply as soon as they change, so the paths already follow the policy.".into())
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

/// `show ip bgp vrf NAME [summary]`: the VRF's own BGP simulation.
pub(super) fn vrf_show(c: &mut Ctx, name: &str, brief: bool) -> R {
    let dev = c.dev;
    let (sub, topo) = c.sim.vrf_inner(c.t, name).ok_or_else(|| format!("% VRF {name} does not exist"))?;
    let view = sub.bgp_state(topo);
    let d = topo.device(dev).ok_or("% That device no longer exists")?;
    Ok(if brief { summary(d, &view) } else { table(d, &view) })
}

/// What the console says after it keeps a setting that changes nothing in the simulation.
const KEPT: &str = "Accepted: kept in the configuration; it does not change what the simulator does.";

/// Process settings that are on until turned off: the `no` line is what gets kept.
const PROCESS_ON: &[&str] = &["bgp fast-external-fallover", "bgp client-to-client reflection", "bgp default ipv4-unicast"];
/// Process settings the simulation reads (`Bgp::option`); the others are only kept and shown.
const PROCESS_ACTS: &[&str] = &[
    "bgp bestpath as-path multipath-relax",
    "bgp bestpath med missing-as-worst",
    "bgp deterministic-med",
    "bgp fast-external-fallover",
    "bgp client-to-client reflection",
    "bgp default ipv4-unicast",
];

/// The words of a spec and the key that names it: its keywords and its `{a|b}` choices, with the
/// arguments checked (the first one, when `skip_who` is set, is the neighbour and is not read here).
fn spec_words(spec: &Spec, args: &[String], skip: usize, skip_who: bool) -> Result<(String, String), String> {
    let mut words: Vec<String> = Vec::new();
    let mut key: Vec<String> = Vec::new();
    let mut next = args.iter();
    if skip_who {
        next.next();
    }
    for t in spec.path.iter().skip(skip) {
        match t {
            Tok::K(k) => {
                words.push((*k).to_string());
                key.push((*k).to_string());
            }
            Tok::Arg(p) => {
                let v = super::knobs::check(p, next.next().map_or("", String::as_str))?;
                if p.starts_with('{') {
                    key.push(v.clone());
                }
                words.push(v);
            }
            Tok::Rest(_) => {
                let v = netlabcraft_model::knobs::clean_value(next.next().map_or("", String::as_str));
                if v.is_empty() {
                    return Err("% Incomplete command.".into());
                }
                words.push(v);
            }
        }
    }
    Ok((key.join(" "), words.join(" ")))
}

/// Runs the settings typed as `bgp …` words, the `neighbor … version|maximum-prefix|…` ones,
/// `neighbor … activate` and the address-family lines.
pub(super) fn option_command(c: &mut Ctx, spec: &Spec, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match spec.id {
        Id::BgpOption | Id::NoBgpOption => {
            let no = spec.id == Id::NoBgpOption;
            let (_, line) = spec_words(spec, args, usize::from(no), false)?;
            let on_by_default = PROCESS_ON.contains(&line.as_str());
            let b = process(c)?;
            let kept = if on_by_default { no.then(|| format!("no {line}")) } else { (!no).then(|| line.clone()) };
            // The other spelling of a default-on setting goes away.
            b.options.remove(&format!("no {line}"));
            b.options.remove(&line);
            if let Some(k) = &kept {
                if b.options.len() >= MAX_OPTIONS {
                    return Err(format!("% At most {MAX_OPTIONS} settings of this kind"));
                }
                b.options.insert(k.clone());
            }
            Ok(if kept.is_some() && !PROCESS_ACTS.contains(&line.as_str()) { KEPT.to_string() } else { String::new() })
        }
        Id::BgpNeighborOption | Id::NoBgpNeighborOption => {
            let no = spec.id == Id::NoBgpNeighborOption;
            let who = arg(0).trim().to_string();
            // The words after the neighbour: `neighbor WHO` (and `no`) are not part of the kept line.
            let skip = usize::from(no) + 2;
            let (key, line) = spec_words(spec, args, skip, true)?;
            edit(process(c)?, &who, |n| {
                if no {
                    n.options.remove(&key);
                } else {
                    if !n.options.contains_key(&key) && n.options.len() >= MAX_OPTIONS {
                        return Err(format!("% At most {MAX_OPTIONS} settings of this kind"));
                    }
                    n.options.insert(key.clone(), line.clone());
                }
                Ok(())
            })?;
            // The prefix limit ends the session when it is passed (`warning-only` aside).
            Ok(if no || key == "maximum-prefix" { String::new() } else { KEPT.to_string() })
        }
        Id::BgpNeighborActivate | Id::NoBgpNeighborActivate => {
            let on = spec.id == Id::BgpNeighborActivate;
            edit(process(c)?, arg(0), |n| {
                n.activated = on;
                Ok(())
            })?;
            Ok(String::new())
        }
        Id::BgpAddressFamily | Id::BgpExitAddressFamily => {
            process(c)?;
            Ok(String::new())
        }
        _ => {
            Err("% Only the IPv4 unicast address family is simulated: BGP here carries IPv4 routes (no MP-BGP, no IPv6 or VPN address families)"
                .into())
        }
    }
}

/// The `neighbor WHO remote-as N` line, when the settings have one that `base` does not.
fn remote_as_line(who: &str, n: &Neighbor, base: &Neighbor) -> Option<String> {
    n.remote_as.filter(|r| base.remote_as != Some(*r)).map(|r| format!("neighbor {who} remote-as {r}"))
}

/// The configuration lines (`neighbor WHO …`, without the leading space) for what `n` sets that
/// `base` does not already give it: the unconfigured neighbour for a neighbour or a group, the
/// group's settings for a member (`pw` writes a password). The remote AS, the description and the shutdown are written apart.
fn neighbor_lines(who: &str, n: &Neighbor, base: &Neighbor, pw: &dyn Fn(&str) -> String) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(u) = n.update_source.as_ref().filter(|u| base.update_source.as_ref() != Some(*u)) {
        v.push(format!("neighbor {who} update-source {u}"));
    }
    if let Some(h) = n.ebgp_multihop.filter(|h| base.ebgp_multihop != Some(*h)) {
        v.push(format!("neighbor {who} ebgp-multihop {h}"));
    }
    if n.next_hop_self && !base.next_hop_self {
        v.push(format!("neighbor {who} next-hop-self"));
    }
    if let Some(w) = n.weight.filter(|w| base.weight != Some(*w)) {
        v.push(format!("neighbor {who} weight {w}"));
    }
    if let Some(l) = n.local_as.filter(|l| base.local_as != Some(*l)) {
        v.push(format!("neighbor {who} local-as {}{}", l.asn, l.words()));
    }
    if let Some((k, h)) = n.timers.filter(|t| base.timers != Some(*t)) {
        v.push(format!("neighbor {who} timers {k} {h}"));
    }
    if (n.send_community && !base.send_community) || (n.send_extended && !base.send_extended) {
        let word = match (n.send_community, n.send_extended) {
            (true, true) => " both",
            (false, true) => " extended",
            _ => "",
        };
        v.push(format!("neighbor {who} send-community{word}"));
    }
    if n.route_reflector_client && !base.route_reflector_client {
        v.push(format!("neighbor {who} route-reflector-client"));
    }
    if n.default_originate && (!base.default_originate || n.default_originate_map != base.default_originate_map) {
        let map = n.default_originate_map.as_ref().map_or(String::new(), |m| format!(" route-map {m}"));
        v.push(format!("neighbor {who} default-originate{map}"));
    }
    if let Some(p) = n.password.as_ref().filter(|p| base.password.as_ref() != Some(*p)) {
        v.push(format!("neighbor {who} password {}", pw(p)));
    }
    if n.remove_private_as && (!base.remove_private_as || n.remove_private_all != base.remove_private_all) {
        v.push(format!("neighbor {who} remove-private-as{}", if n.remove_private_all { " all" } else { "" }));
    }
    if n.soft_reconfiguration_in && !base.soft_reconfiguration_in {
        v.push(format!("neighbor {who} soft-reconfiguration inbound"));
    }
    for (word, dir, mine, theirs) in [
        ("distribute-list", "in", &n.distribute_in, &base.distribute_in),
        ("distribute-list", "out", &n.distribute_out, &base.distribute_out),
        ("prefix-list", "in", &n.prefix_list_in, &base.prefix_list_in),
        ("prefix-list", "out", &n.prefix_list_out, &base.prefix_list_out),
        ("filter-list", "in", &n.filter_list_in, &base.filter_list_in),
        ("filter-list", "out", &n.filter_list_out, &base.filter_list_out),
        ("route-map", "in", &n.route_map_in, &base.route_map_in),
        ("route-map", "out", &n.route_map_out, &base.route_map_out),
    ] {
        if let Some(name) = mine.as_ref().filter(|m| theirs.as_ref() != Some(*m)) {
            v.push(format!("neighbor {who} {word} {name} {dir}"));
        }
    }
    for (key, line) in &n.options {
        if base.options.get(key) != Some(line) {
            v.push(format!("neighbor {who} {line}"));
        }
    }
    v
}

/// The BGP section of `show running-config`.
pub(crate) fn running_config(s: &mut String, d: &Device) {
    let Some(b) = &d.bgp else { return };
    // With `service password-encryption` a password is written obscured, as a line password is.
    let pw = |p: &str| if d.access.password_encryption { format!("7 {}", netlabcraft_model::obscure(p)) } else { p.to_string() };
    let _ = writeln!(s, "router bgp {}", b.asn);
    if let Some(id) = b.router_id {
        let _ = writeln!(s, " bgp router-id {id}");
    }
    if b.log_neighbor_changes {
        let _ = writeln!(s, " bgp log-neighbor-changes");
    }
    if let Some((k, h)) = b.timers {
        let _ = writeln!(s, " timers bgp {k} {h}");
    }
    if let Some(dm) = &b.dampening {
        let _ = match (&dm.route_map, dm.explicit) {
            (Some(m), _) => writeln!(s, " bgp dampening route-map {m}"),
            (None, true) => {
                writeln!(s, " bgp dampening {} {} {} {}", dm.params.half_life, dm.params.reuse, dm.params.suppress, dm.params.max_suppress)
            }
            (None, false) => writeln!(s, " bgp dampening"),
        };
    }
    if let Some(l) = b.default_local_pref {
        let _ = writeln!(s, " bgp default local-preference {l}");
    }
    if let Some(c) = b.cluster_id {
        let _ = writeln!(s, " bgp cluster-id {c}");
    }
    if let Some(i) = b.confed_id {
        let _ = writeln!(s, " bgp confederation identifier {i}");
    }
    if !b.confed_peers.is_empty() {
        let _ = writeln!(s, " bgp confederation peers {}", b.confed_peers.iter().map(u32::to_string).collect::<Vec<_>>().join(" "));
    }
    if b.always_compare_med {
        let _ = writeln!(s, " bgp always-compare-med");
    }
    if b.as_path_ignore {
        let _ = writeln!(s, " bgp bestpath as-path ignore");
    }
    for o in &b.options {
        let _ = writeln!(s, " {o}");
    }
    for g in &b.peer_groups {
        let base = Neighbor::new(Ipv4Addr::UNSPECIFIED);
        let _ = writeln!(s, " neighbor {} peer-group", g.name);
        for line in remote_as_line(&g.name, &g.conf, &base).into_iter().chain(neighbor_lines(&g.name, &g.conf, &base, &pw)) {
            let _ = writeln!(s, " {line}");
        }
    }
    for n in &b.neighbors {
        let who = n.addr.to_string();
        // A member says which group it is in and then only what differs from the group.
        let base = match n.peer_group.as_deref().and_then(|g| b.group(g)) {
            Some(g) => {
                let _ = writeln!(s, " neighbor {who} peer-group {}", g.name);
                g.conf.clone()
            }
            None => Neighbor::new(n.addr),
        };
        if let Some(line) = remote_as_line(&who, n, &base) {
            let _ = writeln!(s, " {line}");
        }
        if let Some(text) = &n.description {
            let _ = writeln!(s, " neighbor {who} description {text}");
        }
        for line in neighbor_lines(&who, n, &base, &pw) {
            let _ = writeln!(s, " {line}");
        }
        if n.shutdown {
            let _ = writeln!(s, " neighbor {who} shutdown");
        }
    }
    for n in &b.networks {
        let _ = writeln!(s, " network {} mask {}", n.addr, n.mask());
    }
    for g in &b.aggregates {
        let _ = writeln!(
            s,
            " aggregate-address {} {}{}{}",
            g.net.addr,
            g.net.mask(),
            if g.summary_only { " summary-only" } else { "" },
            if g.as_set { " as-set" } else { "" }
        );
    }
    if let Some(m) = b.maximum_paths {
        let _ = writeln!(s, " maximum-paths {m}");
    }
    if let Some(m) = b.maximum_paths_ibgp {
        let _ = writeln!(s, " maximum-paths ibgp {m}");
    }
    // With `no bgp default ipv4-unicast` the neighbours that exchange IPv4 routes are listed apart.
    if b.option("no bgp default ipv4-unicast") {
        let named: Vec<String> = b.peer_groups.iter().filter(|g| g.conf.activated).map(|g| g.name.clone()).collect();
        let lone: Vec<String> = b
            .neighbors
            .iter()
            .filter(|n| n.activated && !n.peer_group.as_deref().and_then(|g| b.group(g)).is_some_and(|g| g.conf.activated))
            .map(|n| n.addr.to_string())
            .collect();
        if !named.is_empty() || !lone.is_empty() {
            let _ = writeln!(s, " address-family ipv4");
            for who in named.iter().chain(lone.iter()) {
                let _ = writeln!(s, "  neighbor {who} activate");
            }
            let _ = writeln!(s, " exit-address-family");
        }
    }
    for r in &b.redistribute {
        let map = b.redistribute_maps.get(r.word()).map_or(String::new(), |m| format!(" route-map {m}"));
        match r {
            Source::Ospf => {
                let _ = writeln!(s, " redistribute ospf 1{map}");
            }
            other => {
                let _ = writeln!(s, " redistribute {}{map}", other.word());
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

/// `show ip bgp dampened-paths` (only the routes held down) and `show ip bgp flap-statistics` (every
/// route that flapped).
fn flap_table(d: &Device, v: &BgpView, held_only: bool) -> String {
    let Some(r) = running(d, v) else { return not_running() };
    if d.bgp.as_ref().is_none_or(|b| b.dampening.is_none()) {
        return "Dampening is not configured: turn it on with `bgp dampening`.".to_string();
    }
    let rows: Vec<_> = v.flaps.iter().filter(|(k, f)| k.0 == d.id && if held_only { f.suppressed } else { f.flaps > 0 }).collect();
    if rows.is_empty() {
        let none = if held_only { "no route is held down by dampening" } else { "no route has flapped" };
        return format!("BGP router id {}, local AS {}\n({none})", r.router_id, r.asn);
    }
    let mut s = format!("BGP router id {}, local AS {}\n", r.router_id, r.asn);
    s.push_str("Status: d held down by dampening, * usable    Origin: i IGP, e EGP, ? incomplete\n\n");
    let _ = writeln!(s, "    {:<18} {:<16} {:>5} {:>8} {:>9} {:>9}  Path", "Network", "From", "Flaps", "Penalty", "Duration", "Reuse in");
    for ((_, from, net), f) in rows {
        let path = r.paths.iter().find(|p| p.from == Some(*from) && p.net == *net).map_or("(withdrawn)".to_string(), as_path_text);
        let status = if f.suppressed { "d " } else { "* " };
        let duration = hms(v.now.saturating_sub(f.since));
        let reuse = if f.suppressed { hms(f.reuse_at.saturating_sub(v.now)) } else { String::new() };
        let _ = writeln!(
            s,
            "{status}  {:<18} {:<16} {:>5} {:>8} {:>9} {:>9}  {path}",
            net.to_string(),
            from.to_string(),
            f.flaps,
            f.penalty_at(v.now),
            duration,
            reuse
        );
    }
    s.trim_end().to_string()
}

/// `show ip bgp summary`.
fn summary(d: &Device, v: &BgpView) -> String {
    let Some(r) = running(d, v) else { return not_running() };
    let best = r.paths.iter().filter(|p| p.best).count();
    let prefixes: std::collections::BTreeSet<Ipv4Net> = r.paths.iter().map(|p| p.net).collect();
    let mut s = format!("BGP router id {}, local AS {}\n", r.router_id, r.asn);
    if let Some(b) = d.bgp.as_ref() {
        if let Some(id) = b.confed_id {
            let peers =
                if b.confed_peers.is_empty() { "none".to_string() } else { b.confed_peers.iter().map(u32::to_string).collect::<Vec<_>>().join(" ") };
            let _ = writeln!(s, "Confederation identifier {id}, member AS {}, member peers: {peers}", b.asn);
        }
        let clients = b.neighbors.iter().filter(|n| n.route_reflector_client).count();
        if clients > 0 {
            let cluster = b.cluster_id.unwrap_or(r.router_id);
            let _ = writeln!(s, "Route reflector for cluster {cluster}, {clients} client{}", if clients == 1 { "" } else { "s" });
        }
    }
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
    path_text(&p.confed_path, &p.as_path, &p.as_set, p.origin)
}

/// An AS path as `show ip bgp` writes it: the member autonomous systems of a confederation in
/// parentheses first, then the path, an AS set in braces, and the origin letter.
fn path_text(confed: &[u32], as_path: &[u32], as_set: &[u32], origin: u8) -> String {
    let mut s = String::new();
    if !confed.is_empty() {
        s.push('(');
        s.push_str(&confed.iter().map(u32::to_string).collect::<Vec<_>>().join(" "));
        s.push(')');
    }
    let plain = as_path.iter().map(u32::to_string).collect::<Vec<_>>().join(" ");
    if !plain.is_empty() {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&plain);
    }
    if !as_set.is_empty() {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push('{');
        s.push_str(&as_set.iter().map(u32::to_string).collect::<Vec<_>>().join(","));
        s.push('}');
    }
    if !s.is_empty() {
        s.push(' ');
    }
    s.push(origin_char(origin));
    s
}

/// `show ip bgp`.
fn table(d: &Device, v: &BgpView) -> String {
    table_of(d, v, |_| true, "the BGP table is empty: nothing is advertised and nothing was learned")
}

/// The paths of the table that `keep` accepts, laid out as `show ip bgp` does; `none` says why
/// nothing is listed.
fn table_of(d: &Device, v: &BgpView, keep: impl Fn(&Path) -> bool, none: &str) -> String {
    let Some(r) = running(d, v) else { return not_running() };
    let shown: Vec<&Path> = r.paths.iter().filter(|p| keep(p)).collect();
    if shown.is_empty() {
        return format!("BGP router id {}, local AS {}\n({none})", r.router_id, r.asn);
    }
    let mut s = format!("BGP router id {}, local AS {}\n", r.router_id, r.asn);
    s.push_str(
        "Status: d held down by dampening, * usable, > best, m shares the load, i from an internal neighbour, c from another member AS    Origin: i IGP, e EGP, ? incomplete\n\n",
    );
    let _ = writeln!(s, "    {:<18} {:<16} {:>6} {:>6} {:>6}  Path", "Network", "Next hop", "Metric", "LocPrf", "Weight");
    let mut last: Option<Ipv4Net> = None;
    for p in shown {
        let status = format!(
            "{}{}{}",
            if p.damped {
                'd'
            } else if p.valid {
                '*'
            } else {
                ' '
            },
            if p.best {
                '>'
            } else if p.multi {
                'm'
            } else {
                ' '
            },
            if p.confed {
                'c'
            } else if p.internal() {
                'i'
            } else {
                ' '
            }
        );
        let net = if last == Some(p.net) { String::new() } else { p.net.to_string() };
        last = Some(p.net);
        let _ =
            writeln!(s, "{status} {net:<18} {:<16} {:>6} {:>6} {:>6}  {}", p.next_hop.to_string(), p.med, p.local_pref, p.weight, as_path_text(p));
    }
    s.trim_end().to_string()
}

/// `show ip bgp community COMMUNITY… [exact-match]`: the paths that carry all of them (or exactly
/// them).
fn community_table(d: &Device, v: &BgpView, text: &str) -> Result<String, String> {
    let mut words: Vec<&str> = text.split_whitespace().collect();
    let exact = words.last().is_some_and(|w| w.eq_ignore_ascii_case("exact-match"));
    if exact {
        words.pop();
    }
    if words.is_empty() {
        return Err("% Name at least one community".into());
    }
    let mut want: Vec<u32> = Vec::new();
    for w in words {
        want.push(
            parse_community(w)
                .ok_or_else(|| format!("% `{w}` is not a community: write AA:NN, a number, or no-export, no-advertise, local-AS or internet"))?,
        );
    }
    want.sort_unstable();
    want.dedup();
    let nf = d.bgp_community_new_format;
    let named = want.iter().map(|c| community_text(*c, nf)).collect::<Vec<_>>().join(" ");
    let none = format!("no path carries {}{named}", if exact { "exactly " } else { "" });
    let keep = |p: &Path| if exact { p.communities == want } else { want.iter().all(|c| *c == policy::INTERNET || p.communities.contains(c)) };
    Ok(table_of(d, v, keep, &none))
}

/// `show ip bgp community-list NAME`: the paths the list permits.
fn community_list_table(d: &Device, v: &BgpView, name: &str) -> Result<String, String> {
    let list = d.community_list(name).ok_or_else(|| format!("% No community list named {name}"))?;
    Ok(table_of(d, v, |p| list.permits(&p.communities), &format!("the community list {name} permits no path")))
}

fn cluster_text(list: &[Ipv4Addr]) -> String {
    if list.is_empty() { "empty".to_string() } else { list.iter().map(Ipv4Addr::to_string).collect::<Vec<_>>().join(" ") }
}

/// `show ip bgp regexp EXPRESSION`: the paths whose AS path the expression matches.
fn regexp_table(d: &Device, v: &BgpView, text: &str) -> Result<String, String> {
    let re = text.trim();
    if !netlabcraft_model::textmatch::valid(re) {
        return Err(format!("% `{re}` is not an expression the matcher reads (it knows . * + ? [] ^ $ and _, not alternation or groups)"));
    }
    Ok(table_of(
        d,
        v,
        |p| netlabcraft_model::textmatch::is_match(re, &policy::as_path_for_regex(&p.as_path, &p.as_set)),
        &format!("no AS path matches {re}"),
    ))
}

/// `show ip bgp filter-list N`: the paths an AS-path list permits.
fn filter_list_table(d: &Device, v: &BgpView, text: &str) -> Result<String, String> {
    let name =
        text.trim().parse::<u32>().ok().filter(|n| (1..=500).contains(n)).map(|n| n.to_string()).ok_or("% An AS-path list is numbered 1-500")?;
    let list = d.as_path_list(&name).ok_or_else(|| format!("% No AS-path list {name}"))?;
    Ok(table_of(d, v, |p| list.permits(&p.as_path, &p.as_set), &format!("the AS-path list {name} permits no path")))
}

/// `show ip bgp neighbors A.B.C.D received-routes`: what the neighbour sent, before the inbound
/// policy.
fn received_routes(d: &Device, v: &BgpView, addr: &str) -> Result<String, String> {
    let a = address(addr)?;
    let (Some(r), Some(b)) = (running(d, v), d.bgp.as_ref()) else { return Ok(not_running()) };
    let Some(conf) = b.neighbor(a) else { return Err(format!("% No neighbour {a}")) };
    if !conf.soft_reconfiguration_in {
        return Err(format!("% Nothing is kept for {a}: add `neighbor {a} soft-reconfiguration inbound` first"));
    }
    let list = r.received.get(&a).map_or(&[][..], Vec::as_slice);
    if list.is_empty() {
        return Ok(format!("BGP router id {}, local AS {}\n(nothing received from {a} yet)", r.router_id, r.asn));
    }
    let mut s = format!("BGP router id {}, local AS {}\nReceived from {a}, before the inbound policy:\n\n", r.router_id, r.asn);
    let _ = writeln!(s, "{:<18} {:<16} {:>6} {:>6}  Path", "Network", "Next hop", "Metric", "LocPrf");
    for (net, at) in list {
        let lp = at.local_pref.map_or("-".to_string(), |l| l.to_string());
        let _ = writeln!(
            s,
            "{:<18} {:<16} {:>6} {lp:>6}  {}",
            net.to_string(),
            at.next_hop.to_string(),
            at.med.unwrap_or(0),
            path_text(&at.confed_path, &at.as_path, &at.as_set, at.origin)
        );
    }
    Ok(s.trim_end().to_string())
}

fn attr_rows(s: &mut String, list: &[(Ipv4Net, netlabcraft_wire::bgp::PathAttrs)]) {
    let _ = writeln!(s, "{:<18} {:<16} {:>6} {:>6}  Path", "Network", "Next hop", "Metric", "LocPrf");
    for (net, at) in list {
        let lp = at.local_pref.map_or("-".to_string(), |l| l.to_string());
        let _ = writeln!(
            s,
            "{:<18} {:<16} {:>6} {lp:>6}  {}",
            net.to_string(),
            at.next_hop.to_string(),
            at.med.unwrap_or(0),
            path_text(&at.confed_path, &at.as_path, &at.as_set, at.origin)
        );
    }
}

/// `show ip bgp neighbors A.B.C.D advertised-routes|routes`: what this router sends the neighbour
/// after the outbound policy, or the paths it accepted from the neighbour after the inbound one.
fn neighbor_table(d: &Device, v: &BgpView, addr: &str, advertised: bool) -> Result<String, String> {
    let a = address(addr)?;
    let (Some(r), Some(b)) = (running(d, v), d.bgp.as_ref()) else { return Ok(not_running()) };
    if b.neighbor(a).is_none() {
        return Err(format!("% No neighbour {a}"));
    }
    let head = format!("BGP router id {}, local AS {}\n", r.router_id, r.asn);
    if advertised {
        let Some(list) = r.adv.get(&a) else {
            return Ok(format!("{head}Nothing is advertised to {a}: the session is not Established"));
        };
        if list.is_empty() {
            return Ok(format!("{head}(nothing is advertised to {a})"));
        }
        let mut s = format!("{head}Advertised to {a}, after the outbound policy:\n\n");
        attr_rows(&mut s, list);
        return Ok(s.trim_end().to_string());
    }
    let paths: Vec<&Path> = r.paths.iter().filter(|p| p.from == Some(a)).collect();
    if paths.is_empty() {
        return Ok(format!("{head}(no paths accepted from {a})"));
    }
    let mut s = format!("{head}Accepted from {a}, after the inbound policy:\n\n");
    let _ = writeln!(s, "{:<2} {:<18} {:<16} {:>6} {:>6} {:>6}  Path", "", "Network", "Next hop", "Metric", "LocPrf", "Weight");
    for p in paths {
        let _ = writeln!(
            s,
            "{:<2} {:<18} {:<16} {:>6} {:>6} {:>6}  {}",
            if p.best { "*>" } else { "* " },
            p.net.to_string(),
            p.next_hop.to_string(),
            p.med,
            p.local_pref,
            p.weight,
            path_text(&p.confed_path, &p.as_path, &p.as_set, p.origin)
        );
    }
    Ok(s.trim_end().to_string())
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
        let kind = match (p.from.is_some(), p.ebgp, p.confed) {
            (false, ..) => "local",
            (true, true, _) => "external",
            (true, false, true) => "confederation",
            (true, false, false) => "internal",
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
            if p.damped {
                "held down by dampening"
            } else if p.valid {
                "usable"
            } else {
                "next hop unreachable"
            }
        );
        if !p.as_set.is_empty() {
            let _ = writeln!(s, "    AS set {{{}}}", p.as_set.iter().map(u32::to_string).collect::<Vec<_>>().join(","));
        }
        if !p.confed_path.is_empty() {
            let _ = writeln!(s, "    confederation path: {}", p.confed_path.iter().map(u32::to_string).collect::<Vec<_>>().join(" "));
        }
        if let Some(o) = p.originator_id {
            let _ = writeln!(s, "    originator {o}, cluster list: {}", cluster_text(&p.cluster_list));
        }
        if !p.communities.is_empty() {
            let _ = writeln!(
                s,
                "    communities: {}",
                p.communities.iter().map(|c| community_text(*c, d.bgp_community_new_format)).collect::<Vec<_>>().join(" ")
            );
        }
        if !p.ext_communities.is_empty() {
            let _ = writeln!(
                s,
                "    extended communities: {}",
                p.ext_communities.iter().map(|c| netlabcraft_wire::bgp::ext_text(*c)).collect::<Vec<_>>().join(" ")
            );
        }
        if p.multi {
            let _ = writeln!(s, "    shares the load with the best path (maximum-paths)");
        }
        if p.suppressed {
            let _ = writeln!(s, "    not advertised: an aggregate with summary-only covers it");
        }
        if p.valid && p.from.is_some() && p.igp > 0 {
            let _ = writeln!(s, "    metric to the next hop: {}", p.igp);
        }
    }
    Ok(s.trim_end().to_string())
}

/// What a configuration line sets, without its `neighbor WHO ` start.
fn settings_of(who: &str, n: &Neighbor, base: &Neighbor) -> Vec<String> {
    let mask = |_: &str| "******".to_string();
    let start = format!("neighbor {who} ");
    remote_as_line(who, n, base)
        .into_iter()
        .chain(neighbor_lines(who, n, base, &mask))
        .map(|l| l.strip_prefix(&start).unwrap_or(&l).to_string())
        .collect()
}

/// `show ip bgp peer-group [NAME]`.
fn peer_groups(d: &Device, v: &BgpView, only: Option<&str>) -> Result<String, String> {
    let (Some(b), Some(r)) = (d.bgp.as_ref(), running(d, v)) else { return Ok(not_running()) };
    let list: Vec<&PeerGroup> = b.peer_groups.iter().filter(|g| only.is_none_or(|n| g.name == n)).collect();
    if list.is_empty() {
        return match only {
            Some(n) => Err(format!("% No peer group named {n}")),
            None => Ok("(no peer groups: create one with `neighbor NAME peer-group`)".to_string()),
        };
    }
    let none = Neighbor::new(Ipv4Addr::UNSPECIFIED);
    let mut blocks = Vec::new();
    for g in list {
        let mut s = format!("BGP peer group {}\n", g.name);
        let settings = settings_of(&g.name, &g.conf, &none);
        if settings.is_empty() {
            s.push_str("  Settings: none yet\n");
        } else {
            s.push_str("  Settings:\n");
            for l in &settings {
                let _ = writeln!(s, "    {l}");
            }
        }
        let members: Vec<&Neighbor> = b.members(&g.name).collect();
        let _ = writeln!(s, "  Members: {}", members.len());
        for m in members {
            let state = r.neighbors.iter().find(|x| x.addr == m.addr).map_or("-".to_string(), state_text);
            let own = settings_of(&m.addr.to_string(), m, &g.conf);
            let own = if own.is_empty() { String::new() } else { format!(" (its own: {})", own.join("; ")) };
            let _ = writeln!(s, "    {} {state}{own}", m.addr);
        }
        blocks.push(s.trim_end().to_string());
    }
    Ok(blocks.join("\n\n"))
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
            Some(_) if n.confed => "confederation link",
            Some(a) if a == r.asn => "internal link",
            Some(_) => "external link",
            None => "no remote AS",
        };
        let mut s = format!("BGP neighbour {}, remote AS {} ({link})\n", n.addr, n.remote_as.map_or("-".to_string(), |a| a.to_string()));
        if let Some(text) = conf.and_then(|c| c.description.as_ref()) {
            let _ = writeln!(s, "  Description: {text}");
        }
        if let Some(group) = conf.and_then(|c| c.peer_group.as_ref()) {
            let _ = writeln!(s, "  Member of peer group {group}");
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
        if let Some(l) = conf.and_then(|c| c.local_as) {
            let words = l.words();
            if n.local_as.is_some() {
                let sent = if l.replace_as {
                    "it replaces this router's AS in the paths sent"
                } else {
                    "it goes in front of this router's AS in the paths sent"
                };
                let got = if l.no_prepend { "the paths received are kept as they are" } else { "it is put in front of the paths received" };
                let _ = writeln!(s, "  Local AS {}{words}: {sent}; {got}", l.asn);
            } else {
                let _ = writeln!(
                    s,
                    "  Local AS {}{words}: not in use (it applies to an external session, and to the AS number the neighbour expects)",
                    l.asn
                );
            }
        }
        if n.hold_secs == 0 {
            let _ = writeln!(s, "  Hold time 0 s (no keepalives, the session never expires)");
        } else {
            let _ = writeln!(s, "  Hold time {} s, keepalive every {} s", n.hold_secs, n.keepalive_secs);
        }
        let mut options = Vec::new();
        match conf.map(|c| (c.send_community, c.send_extended)) {
            Some((true, true)) => options.push("send-community both".to_string()),
            Some((true, false)) => options.push("send-community".to_string()),
            Some((false, true)) => options.push("send-community extended".to_string()),
            _ => {}
        }
        if let Some((k, h)) = conf.and_then(|c| c.timers) {
            options.push(format!("timers {k} {h}"));
        }
        if conf.is_some_and(|c| c.next_hop_self) {
            options.push("next-hop-self".to_string());
        }
        if let Some(h) = conf.and_then(|c| c.ebgp_multihop) {
            options.push(format!("ebgp-multihop {h}"));
        }
        if let Some(w) = conf.and_then(|c| c.weight) {
            options.push(format!("weight {w}"));
        }
        if conf.is_some_and(|c| c.route_reflector_client) {
            options.push("route-reflector-client".to_string());
        }
        if let Some(c) = conf.filter(|c| c.default_originate) {
            options.push(match &c.default_originate_map {
                Some(m) => format!("default-originate route-map {m}"),
                None => "default-originate".to_string(),
            });
        }
        if conf.is_some_and(|c| c.password.is_some()) {
            options.push("MD5 password".to_string());
        }
        if let Some(c) = conf.filter(|c| c.remove_private_as) {
            options.push(format!("remove-private-as{}", if c.remove_private_all { " all" } else { "" }));
        }
        if conf.is_some_and(|c| c.soft_reconfiguration_in) {
            options.push("soft-reconfiguration inbound".to_string());
        }
        if !options.is_empty() {
            let _ = writeln!(s, "  Options: {}", options.join(", "));
        }
        if let Some(c) = conf {
            for (what, dir, name) in [
                ("access list", "inbound", &c.distribute_in),
                ("access list", "outbound", &c.distribute_out),
                ("prefix list", "inbound", &c.prefix_list_in),
                ("prefix list", "outbound", &c.prefix_list_out),
                ("AS-path list", "inbound", &c.filter_list_in),
                ("AS-path list", "outbound", &c.filter_list_out),
                ("route map", "inbound", &c.route_map_in),
                ("route map", "outbound", &c.route_map_out),
            ] {
                if let Some(name) = name {
                    let known = match what {
                        "prefix list" => d.prefix_list(name).is_some(),
                        "AS-path list" => d.as_path_list(name).is_some(),
                        "access list" => d.acls.iter().any(|a| a.name == *name),
                        _ => d.route_map(name).is_some(),
                    };
                    let _ = writeln!(s, "  Policy: {dir} {what} {name}{}", if known { "" } else { " (not defined: lets everything through)" });
                }
            }
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
        if conf.is_some_and(|c| c.soft_reconfiguration_in) {
            let kept = r.received.get(&n.addr).map_or(0, Vec::len);
            let _ = writeln!(s, "  Kept before the inbound policy: {kept} (show ip bgp neighbors {} received-routes)", n.addr);
        }
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
