//! `redistribute`, `default-metric` and `distribute-list` under `router rip`, `router eigrp` and
//! `router ospf`, and the lines they add to the running configuration and to
//! `show ip protocols`.

use std::fmt::Write as _;

use netlabcraft_model::redist::{DistKind, DistList, EigrpMetric, MAX_DIST_LISTS, MAX_REDISTRIBUTE, Redist, RedistSource, redistribute_line};

use super::{Ctx, Mode, R};
use crate::grammar::{Id, Tok};

/// The routing process a command is typed under.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Proto {
    Rip,
    Eigrp,
    Ospf,
}

fn proto(c: &Ctx) -> Result<Proto, String> {
    match c.cli.mode {
        Mode::Rip => Ok(Proto::Rip),
        Mode::Eigrp => Ok(Proto::Eigrp),
        Mode::Ospf => Ok(Proto::Ospf),
        _ => Err("% Only under router rip, router eigrp or router ospf (IPv4)".into()),
    }
}

/// The process's redistribution lines and distribute lists.
fn lists<'a>(c: &'a mut Ctx, p: Proto) -> Result<(&'a mut Vec<Redist>, &'a mut Vec<DistList>), String> {
    let d = c.dm()?;
    match p {
        Proto::Rip => {
            let r = d.rip.as_mut().ok_or("% Not in RIP mode")?;
            Ok((&mut r.redistribute, &mut r.distribute))
        }
        Proto::Eigrp => {
            let e = d.eigrp.as_mut().ok_or("% Not in EIGRP mode")?;
            Ok((&mut e.redistribute, &mut e.distribute))
        }
        Proto::Ospf => {
            let o = d.ospf.as_mut().ok_or("% Not in OSPF mode")?;
            Ok((&mut o.redistribute, &mut o.distribute))
        }
    }
}

fn number(word: Option<&String>, lo: u64, hi: u64, what: &str) -> Result<u64, String> {
    word.and_then(|w| w.parse::<u64>().ok()).filter(|n| (lo..=hi).contains(n)).ok_or_else(|| format!("% {what} is {lo}-{hi}"))
}

fn name_ok(name: &str) -> Result<String, String> {
    if name.is_empty() || name.chars().count() > netlabcraft_model::policy::MAX_NAME || name.chars().any(char::is_control) {
        return Err(format!("% A name is 1-{} characters", netlabcraft_model::policy::MAX_NAME));
    }
    Ok(name.to_string())
}

/// The source a `redistribute` line names, and the words after it.
fn source_of<'a>(p: Proto, words: &'a [String]) -> Result<(RedistSource, &'a [String]), String> {
    let first = words.first().map(|w| w.to_ascii_lowercase()).unwrap_or_default();
    let pick = |name: &str| !first.is_empty() && name.starts_with(first.as_str());
    let rest = words.get(1..).unwrap_or(&[]);
    let own = |what: &str| format!("% This router runs one {what} process: there is no other one to take routes from");
    if pick("connected") {
        return Ok((RedistSource::Connected, rest));
    }
    if pick("static") {
        return Ok((RedistSource::Static, rest));
    }
    if pick("rip") {
        if p == Proto::Rip {
            return Err(own("RIP"));
        }
        return Ok((RedistSource::Rip, rest));
    }
    let with_number = |max: u64| -> Result<(u64, &'a [String]), String> {
        let n = number(rest.first(), 1, max, "The process or autonomous system number")?;
        Ok((n, rest.get(1..).unwrap_or(&[])))
    };
    if pick("ospf") {
        if p == Proto::Ospf {
            return Err(own("OSPF"));
        }
        let (n, after) = with_number(65_535)?;
        return Ok((RedistSource::Ospf(u16::try_from(n).map_err(|_| "% 1-65535")?), after));
    }
    if pick("eigrp") {
        if p == Proto::Eigrp {
            return Err(own("EIGRP"));
        }
        let (n, after) = with_number(65_535)?;
        return Ok((RedistSource::Eigrp(u16::try_from(n).map_err(|_| "% 1-65535")?), after));
    }
    if pick("bgp") {
        let (n, after) = with_number(u64::from(u32::MAX))?;
        return Ok((RedistSource::Bgp(u32::try_from(n).map_err(|_| "% 1-4294967295")?), after));
    }
    Err("% redistribute connected, static, rip, ospf <n>, eigrp <n> or bgp <n> (not the protocol itself)".into())
}

/// A whole `redistribute` line as typed.
fn parse_line(p: Proto, text: &str) -> Result<Redist, String> {
    let typed: Vec<String> = text.split_whitespace().map(str::to_string).collect();
    let (source, rest) = source_of(p, &typed)?;
    let mut line = Redist::new(source);
    let mut i = 0usize;
    while let Some(w) = rest.get(i) {
        let lw = w.to_ascii_lowercase();
        if lw == "metric" {
            match p {
                Proto::Rip => {
                    line.metric = Some(number(rest.get(i + 1), 1, 16, "A RIP metric")? as u32);
                    i += 2;
                }
                Proto::Ospf => {
                    line.metric = Some(number(rest.get(i + 1), 0, 16_777_214, "An OSPF metric")? as u32);
                    i += 2;
                }
                Proto::Eigrp => {
                    line.eigrp_metric = Some(eigrp_metric(rest.get(i + 1..).unwrap_or(&[]))?);
                    i += 6;
                }
            }
        } else if lw == "metric-type" && p == Proto::Ospf {
            line.metric_type = Some(number(rest.get(i + 1), 1, 2, "The external type")? as u8);
            i += 2;
        } else if lw == "subnets" && p == Proto::Ospf {
            i += 1;
        } else if lw == "route-map" {
            line.route_map = Some(name_ok(rest.get(i + 1).map_or("", String::as_str))?);
            i += 2;
        } else {
            return Err(format!("% `{w}` is not part of a redistribute line here"));
        }
    }
    Ok(line)
}

/// The five values of an EIGRP metric: bandwidth, delay, reliability, load and MTU.
fn eigrp_metric(words: &[String]) -> Result<EigrpMetric, String> {
    if words.len() < 5 {
        return Err("% An EIGRP metric is five numbers: bandwidth delay reliability load mtu".into());
    }
    Ok(EigrpMetric {
        bandwidth: number(words.first(), 1, u64::from(u32::MAX), "The bandwidth (kbit/s)")? as u32,
        delay: number(words.get(1), 0, u64::from(u32::MAX), "The delay (tens of microseconds)")? as u32,
        reliability: number(words.get(2), 0, 255, "The reliability")? as u8,
        load: number(words.get(3), 1, 255, "The load")? as u8,
        mtu: number(words.get(4), 1, 65_535, "The MTU")? as u16,
    })
}

/// A hint when the line will not carry its routes yet.
fn advice(p: Proto, line: &Redist, default_given: bool) -> String {
    let needs = !matches!(line.source, RedistSource::Connected | RedistSource::Static);
    let has = match p {
        Proto::Rip | Proto::Ospf => line.metric.is_some(),
        Proto::Eigrp => line.eigrp_metric.is_some(),
    };
    match p {
        Proto::Rip if needs && !has && !default_given => {
            "Note: RIP needs a metric for the routes of another protocol (metric N or default-metric N); with none they are not advertised.".into()
        }
        Proto::Eigrp if needs && !has && !default_given => {
            "Note: EIGRP needs a metric for the routes of another protocol (metric or default-metric: bandwidth delay reliability load mtu); with none they are not advertised."
                .into()
        }
        _ => String::new(),
    }
}

/// Runs one of this module's commands.
pub(super) fn command(c: &mut Ctx, id: Id, path: &[Tok], args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    let p = proto(c)?;
    match id {
        Id::RipRedistribute | Id::EigrpRedistribute | Id::OspfRedistribute => {
            let line = parse_line(p, arg(0))?;
            let default_given = {
                let d = c.d()?;
                match p {
                    Proto::Rip => d.rip.as_ref().is_some_and(|r| r.default_metric.is_some()),
                    Proto::Eigrp => d.eigrp.as_ref().is_some_and(|e| e.default_metric.is_some()),
                    Proto::Ospf => true,
                }
            };
            let note = advice(p, &line, default_given);
            let (redist, _) = lists(c, p)?;
            redist.retain(|r| r.source != line.source);
            if redist.len() >= MAX_REDISTRIBUTE {
                return Err(format!("% At most {MAX_REDISTRIBUTE} redistribute lines"));
            }
            redist.push(line);
            redist.sort_by_key(|r| r.source);
            Ok(note)
        }
        Id::NoRipRedistribute | Id::NoEigrpRedistribute | Id::NoOspfRedistribute => {
            let typed: Vec<String> = arg(0).split_whitespace().map(str::to_string).collect();
            let (source, _) = source_of(p, &typed)?;
            let (redist, _) = lists(c, p)?;
            let before = redist.len();
            redist.retain(|r| r.source != source);
            if redist.len() == before { Err("% That source is not being redistributed".into()) } else { Ok(String::new()) }
        }
        Id::RipDefaultMetric => {
            let m = number(args.first(), 1, 16, "A RIP metric")? as u8;
            c.dm()?.rip.as_mut().ok_or("% Not in RIP mode")?.default_metric = Some(m);
            Ok(String::new())
        }
        Id::NoRipDefaultMetric => {
            c.dm()?.rip.as_mut().ok_or("% Not in RIP mode")?.default_metric = None;
            Ok(String::new())
        }
        Id::OspfDefaultMetric => {
            let m = number(args.first(), 1, 16_777_214, "An OSPF metric")? as u32;
            c.dm()?.ospf.as_mut().ok_or("% Not in OSPF mode")?.default_metric = Some(m);
            Ok(String::new())
        }
        Id::NoOspfDefaultMetric => {
            c.dm()?.ospf.as_mut().ok_or("% Not in OSPF mode")?.default_metric = None;
            Ok(String::new())
        }
        Id::EigrpDefaultMetric => {
            let m = eigrp_metric(args)?;
            c.dm()?.eigrp.as_mut().ok_or("% Not in EIGRP mode")?.default_metric = Some(m);
            Ok(String::new())
        }
        Id::NoEigrpDefaultMetric => {
            c.dm()?.eigrp.as_mut().ok_or("% Not in EIGRP mode")?.default_metric = None;
            Ok(String::new())
        }
        Id::DistList | Id::NoDistList => dist_list(c, p, id == Id::NoDistList, path, args),
        _ => Err("% Not a redistribution command".into()),
    }
}

/// `[no] distribute-list prefix NAME in|out [IF]`, `[no] distribute-list ACL in|out [IF]` and
/// `[no] distribute-list route-map NAME in [IF]`.
fn dist_list(c: &mut Ctx, p: Proto, no: bool, path: &[Tok], args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    let has = |k: &str| path.iter().any(|t| matches!(t, Tok::K(x) if *x == k));
    let (kind, name, inbound, iface_text) = if has("prefix") {
        (DistKind::Prefix, arg(0), direction(arg(1))?, arg(2))
    } else if has("route-map") {
        (DistKind::RouteMap, arg(0), true, arg(1))
    } else {
        (DistKind::Acl, arg(0), direction(arg(1))?, arg(2))
    };
    let name = name_ok(name)?;
    let iface = if iface_text.trim().is_empty() { None } else { Some(super::routing::ospf_if_name(c, iface_text.trim())?) };
    let (_, dist) = lists(c, p)?;
    if no {
        let before = dist.len();
        dist.retain(|l| !(l.kind == kind && l.name == name && l.inbound == inbound && l.iface == iface));
        return if dist.len() == before { Err("% No such distribute-list".into()) } else { Ok(String::new()) };
    }
    // One list per direction and interface: a new one takes the place of the old.
    dist.retain(|l| !(l.inbound == inbound && l.iface == iface));
    if dist.len() >= MAX_DIST_LISTS {
        return Err(format!("% At most {MAX_DIST_LISTS} distribute-list lines"));
    }
    dist.push(DistList { kind, name, inbound, iface });
    Ok(String::new())
}

fn direction(text: &str) -> Result<bool, String> {
    match text.to_ascii_lowercase().as_str() {
        "in" => Ok(true),
        "out" => Ok(false),
        _ => Err("% Say in or out".into()),
    }
}

/// The redistribution, default-metric and distribute-list lines of a routing process in the
/// running configuration (each starts with a space).
pub(crate) fn config_lines(s: &mut String, redist: &[Redist], default_metric: Option<String>, dist: &[DistList], ospf: bool) {
    for r in redist {
        let _ = writeln!(s, " {}", redistribute_line(r, ospf));
    }
    if let Some(m) = default_metric {
        let _ = writeln!(s, " default-metric {m}");
    }
    for l in dist {
        let _ = writeln!(s, " {}", l.line());
    }
}

/// The lines `show ip protocols` adds for a process: what it redistributes and its filters.
pub(crate) fn protocol_lines(s: &mut String, redist: &[Redist], default_metric: Option<String>, dist: &[DistList]) {
    if !redist.is_empty() {
        let words: Vec<String> = redist
            .iter()
            .map(|r| match &r.route_map {
                Some(m) => format!("{} (route-map {m})", r.source.words()),
                None => r.source.words(),
            })
            .collect();
        let _ = writeln!(s, "  Redistributing: {}", words.join(", "));
    }
    if let Some(m) = default_metric {
        let _ = writeln!(s, "  Default redistribution metric: {m}");
    }
    for l in dist {
        let what = match l.kind {
            DistKind::Prefix => format!("prefix list {}", l.name),
            DistKind::Acl => format!("access list {}", l.name),
            DistKind::RouteMap => format!("route map {}", l.name),
        };
        let on = l.iface.as_ref().map_or("every interface".to_string(), |i| netlabcraft_model::ifname::short(i));
        let _ = writeln!(s, "  {} filter: {what} on {on}", if l.inbound { "Incoming route" } else { "Outgoing route" });
    }
}
