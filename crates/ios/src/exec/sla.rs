//! IP SLA: `ip sla N`, the operation's test (`icmp-echo`, `tcp-connect`, `udp-echo`,
//! `udp-jitter`, `http get`), `frequency`, `timeout`, `threshold`, `ip sla schedule`,
//! `ip sla responder`, `ip sla reaction-configuration` and the `show ip sla` views. The probes
//! are sent by `netlabcraft_sim::sla` as simulated time passes; tracked objects follow the
//! outcome.

use std::fmt::Write as _;
use std::net::Ipv4Addr;

use netlabcraft_core::parse_ip;
use netlabcraft_model::Device;
use netlabcraft_model::sla::{MAX_OP_ID, MAX_OPS, MAX_REACTIONS, MAX_TEXT, Op, Probe, Schedule, Start, Stats};
use netlabcraft_model::track::{Kind, SlaOutcome};

use super::{Ctx, Mode, R};
use crate::grammar::Id;

fn op_number(text: &str) -> Result<u32, String> {
    text.trim().parse::<u32>().ok().filter(|n| (1..=MAX_OP_ID).contains(n)).ok_or_else(|| format!("% The operation number is 1-{MAX_OP_ID}"))
}

fn current(c: &Ctx) -> Result<u32, String> {
    match c.cli.mode {
        Mode::IpSla(n) => Ok(n),
        _ => Err("% Not in IP SLA mode".into()),
    }
}

/// A text of printable characters, at most [`MAX_TEXT`] long.
fn clean(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() || t.chars().count() > MAX_TEXT || !t.chars().all(|ch| ch.is_ascii_graphic() || ch == ' ') {
        return Err(format!("% Give 1-{MAX_TEXT} printable characters"));
    }
    Ok(t.to_string())
}

/// `hh:mm:ss` as seconds.
fn clock_seconds(text: &str) -> Result<u32, String> {
    let parts: Vec<Option<u32>> = text.split(':').map(|p| p.parse::<u32>().ok()).collect();
    match parts.as_slice() {
        [Some(h), Some(m), Some(s)] if *m < 60 && *s < 60 && *h < 24 * 7 => Ok(h * 3600 + m * 60 + s),
        _ => Err("% The time is hh:mm:ss".into()),
    }
}

/// Puts a new test on the operation the session is editing and forgets what the old one measured.
fn set_probe(c: &mut Ctx, probe: Probe) -> R {
    let n = current(c)?;
    let d = c.dm()?;
    let op = d.sla.ops.get_mut(&n).ok_or("% That operation no longer exists")?;
    op.probe = Some(probe);
    let keep = d.sla.stats.get(&n).and_then(|s| s.started_us);
    restart(d, n, keep);
    Ok(String::new())
}

/// Clears an operation's results (and the outcome tracked objects read), then sets when it started.
fn restart(d: &mut Device, n: u32, started_us: Option<u64>) {
    d.sla.stats.insert(n, Stats { started_us, ..Stats::default() });
    d.track.sla_outcome.remove(&n);
}

fn set_op(c: &mut Ctx, f: impl FnOnce(&mut Op)) -> R {
    let n = current(c)?;
    let op = c.dm()?.sla.ops.get_mut(&n).ok_or("% That operation no longer exists")?;
    f(op);
    Ok(String::new())
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::IpSla => {
            let n = op_number(arg(0))?;
            let d = c.dm()?;
            if !d.sla.ops.contains_key(&n) && d.sla.ops.len() >= MAX_OPS {
                return Err(format!("% At most {MAX_OPS} IP SLA operations"));
            }
            d.sla.ops.entry(n).or_default();
            c.cli.mode = Mode::IpSla(n);
            Ok(String::new())
        }
        Id::NoIpSla => {
            let n = op_number(arg(0))?;
            let d = c.dm()?;
            if !d.sla.remove(n) {
                return Err(format!("% There is no IP SLA operation {n}"));
            }
            d.track.sla_outcome.remove(&n);
            if c.cli.mode == Mode::IpSla(n) {
                c.cli.mode = Mode::Config;
            }
            Ok(String::new())
        }
        Id::IpSlaSchedule => {
            let n = op_number(arg(0))?;
            let now = c.sim.now_us();
            let words: Vec<&str> = arg(1).split_whitespace().collect();
            let (mut life, mut start) = (Some(3600u32), Start::Now);
            let mut i = 0;
            while let Some(w) = words.get(i) {
                match w.to_ascii_lowercase().as_str() {
                    "life" => {
                        life = match words.get(i + 1).map(|x| x.to_ascii_lowercase()).as_deref() {
                            Some("forever") => None,
                            Some(v) => Some(v.parse::<u32>().ok().filter(|s| *s >= 1).ok_or("% The life is `forever` or 1-4294967295 seconds")?),
                            None => return Err("% Incomplete command.".into()),
                        };
                        i += 2;
                    }
                    "start-time" => {
                        match words.get(i + 1).map(|x| x.to_ascii_lowercase()).as_deref() {
                            Some("now") => start = Start::Now,
                            Some("pending") => start = Start::Pending,
                            Some("after") => {
                                start = Start::After(clock_seconds(words.get(i + 2).copied().unwrap_or(""))?);
                                i += 1;
                            }
                            _ => return Err("% Say `now`, `pending` or `after hh:mm:ss`".into()),
                        }
                        i += 2;
                    }
                    "recurring" => i += 1,
                    other => return Err(format!("% `{other}` is not part of the schedule; use life and start-time")),
                }
            }
            let d = c.dm()?;
            let op = d.sla.ops.get_mut(&n).ok_or_else(|| format!("% There is no IP SLA operation {n}; create it with `ip sla {n}`"))?;
            if op.probe.is_none() {
                return Err(format!("% Operation {n} has no test yet (`icmp-echo`, `tcp-connect`, …)"));
            }
            op.schedule = Some(Schedule { life_s: life, start });
            let started = match start {
                Start::Now => Some(now),
                Start::Pending => None,
                Start::After(s) => Some(now.saturating_add(u64::from(s).saturating_mul(1_000_000))),
            };
            restart(d, n, started);
            Ok(String::new())
        }
        Id::NoIpSlaSchedule => {
            let n = op_number(arg(0))?;
            let d = c.dm()?;
            let op = d.sla.ops.get_mut(&n).ok_or_else(|| format!("% There is no IP SLA operation {n}"))?;
            op.schedule = None;
            restart(d, n, None);
            Ok(String::new())
        }
        Id::IpSlaResponder | Id::NoIpSlaResponder => {
            c.dm()?.sla.responder = id == Id::IpSlaResponder;
            Ok(String::new())
        }
        Id::IpSlaReaction => {
            let n = op_number(arg(0))?;
            let text = clean(arg(1))?;
            let words: Vec<&str> = text.split_whitespace().collect();
            let conditions = ["timeout", "rtt", "connectionloss", "verifyerror", "jitteravg"];
            match words.as_slice() {
                [react, cond, ..] if react.eq_ignore_ascii_case("react") && conditions.contains(&cond.to_ascii_lowercase().as_str()) => {}
                _ => return Err("% Say `react timeout|rtt|connectionLoss|verifyError|jitterAvg`, then the options".into()),
            }
            let key = words.get(..2).map(|k| k.join(" ").to_ascii_lowercase()).unwrap_or_default();
            let line = words.join(" ");
            let op = c.dm()?.sla.ops.get_mut(&n).ok_or_else(|| format!("% There is no IP SLA operation {n}"))?;
            op.reactions.retain(|r| !r.to_ascii_lowercase().starts_with(&key));
            if op.reactions.len() >= MAX_REACTIONS {
                return Err(format!("% At most {MAX_REACTIONS} reactions per operation"));
            }
            op.reactions.push(line);
            Ok(String::new())
        }
        Id::NoIpSlaReaction => {
            let n = op_number(arg(0))?;
            let op = c.dm()?.sla.ops.get_mut(&n).ok_or_else(|| format!("% There is no IP SLA operation {n}"))?;
            op.reactions.clear();
            Ok(String::new())
        }
        Id::SlaIcmpEcho | Id::SlaIcmpEchoSource => {
            let dest = parse_ip(arg(0)).map_err(|e| format!("% {e}"))?;
            let source = if id == Id::SlaIcmpEchoSource { Some(parse_ip(arg(1)).map_err(|e| format!("% {e}"))?) } else { None };
            set_probe(c, Probe::IcmpEcho { dest, source })
        }
        Id::SlaTcpConnect | Id::SlaUdpEcho | Id::SlaUdpJitter => {
            let dest = parse_ip(arg(0)).map_err(|e| format!("% {e}"))?;
            let port = arg(1).trim().parse::<u16>().ok().filter(|p| *p >= 1).ok_or("% The port is 1-65535")?;
            set_probe(
                c,
                match id {
                    Id::SlaTcpConnect => Probe::TcpConnect { dest, port },
                    Id::SlaUdpEcho => Probe::UdpEcho { dest, port },
                    _ => Probe::UdpJitter { dest, port },
                },
            )
        }
        Id::SlaHttpGet => {
            let url = arg(0).trim();
            let rest = url
                .get(..7)
                .filter(|p| p.eq_ignore_ascii_case("http://"))
                .and_then(|_| url.get(7..))
                .ok_or("% The address is http://A.B.C.D/path (a name needs a DNS lookup, which operations do not do)")?;
            let (host, path) = match rest.find('/') {
                Some(i) => (rest.get(..i).unwrap_or(""), rest.get(i..).unwrap_or("/")),
                None => (rest, "/"),
            };
            let dest = host.parse::<Ipv4Addr>().map_err(|_| "% The host must be an IPv4 address")?;
            let path = clean(path)?;
            if path.contains(' ') {
                return Err("% The path has no spaces".into());
            }
            set_probe(c, Probe::HttpGet { dest, path })
        }
        Id::SlaFrequency => {
            let v = arg(0).trim().parse::<u32>().ok().filter(|v| (1..=604_800).contains(v)).ok_or("% The frequency is 1-604800 seconds")?;
            set_op(c, |o| o.frequency_s = v)
        }
        Id::NoSlaFrequency => set_op(c, |o| o.frequency_s = 60),
        Id::SlaTimeout => {
            let v = arg(0).trim().parse::<u32>().ok().filter(|v| *v <= 604_800_000).ok_or("% The timeout is 0-604800000 milliseconds")?;
            set_op(c, |o| o.timeout_ms = v)
        }
        Id::NoSlaTimeout => set_op(c, |o| o.timeout_ms = 5_000),
        Id::SlaThreshold => {
            let v = arg(0).trim().parse::<u32>().ok().filter(|v| *v <= 60_000).ok_or("% The threshold is 0-60000 milliseconds")?;
            set_op(c, |o| o.threshold_ms = v)
        }
        Id::NoSlaThreshold => set_op(c, |o| o.threshold_ms = 5_000),
        Id::SlaTag => {
            let t = clean(arg(0))?;
            set_op(c, |o| o.tag = t)
        }
        Id::NoSlaTag => set_op(c, |o| o.tag.clear()),
        Id::ShowIpSlaStatistics | Id::ShowIpSlaConfiguration | Id::ShowIpSlaSummary => {
            let only = if arg(0).is_empty() { None } else { Some(op_number(arg(0))?) };
            let now = c.sim.now_us();
            let d = c.d()?;
            Ok(match id {
                Id::ShowIpSlaStatistics => statistics(d, only, now),
                Id::ShowIpSlaConfiguration => configuration(d, only, now),
                _ => summary(d, now),
            })
        }
        _ => Err("% Not an IP SLA command".into()),
    }
}

fn age(us: u64) -> String {
    let s = us / 1_000_000;
    format!("{:02}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
}

fn millis(us: u64) -> String {
    format!("{}.{} ms", us / 1_000, us % 1_000 / 100)
}

/// Where an operation is in its schedule.
fn state_word(op: &Op, stats: Option<&Stats>, now: u64) -> &'static str {
    let Some(sch) = &op.schedule else { return "not scheduled" };
    let Some(start) = stats.and_then(|s| s.started_us) else { return "pending" };
    if sch.start == Start::Pending {
        return "pending";
    }
    if now < start {
        return "waiting to start";
    }
    match sch.life_s {
        Some(life) if now >= start.saturating_add(u64::from(life).saturating_mul(1_000_000)) => "ended",
        _ => "running",
    }
}

fn outcome_text(stats: Option<&Stats>) -> String {
    match stats.and_then(|s| s.last.map(|o| (o, s.last_rtt_us))) {
        None => "no probe has run yet".into(),
        Some((SlaOutcome::Ok, Some(r))) => format!("OK, round trip {}", millis(r)),
        Some((SlaOutcome::Ok, None)) => "OK".into(),
        Some((SlaOutcome::OverThreshold, r)) => format!("over threshold{}", r.map_or(String::new(), |r| format!(", round trip {}", millis(r)))),
        Some((SlaOutcome::Failed, _)) => "failed (no answer)".into(),
    }
}

fn test_text(op: &Op) -> String {
    op.probe.as_ref().map_or("not set".to_string(), Probe::line)
}

fn ids(d: &Device, only: Option<u32>) -> Vec<u32> {
    d.sla.ops.keys().copied().filter(|i| only.is_none_or(|o| o == *i)).collect()
}

fn none_text(only: Option<u32>) -> String {
    match only {
        Some(n) => format!("There is no IP SLA operation {n}."),
        None => "No IP SLA operation is configured here.".into(),
    }
}

fn statistics(d: &Device, only: Option<u32>, now: u64) -> String {
    let list = ids(d, only);
    if list.is_empty() {
        return none_text(only);
    }
    let mut s = String::new();
    for id in list {
        let Some(op) = d.sla.ops.get(&id) else { continue };
        let st = d.sla.stats.get(&id);
        let _ = writeln!(s, "IP SLA operation {id}");
        let _ = writeln!(s, "  Test: {}", test_text(op));
        let _ = writeln!(s, "  Latest outcome: {}", outcome_text(st));
        if let Some(st) = st {
            let _ = writeln!(s, "  Probes: {} sent, {} answered, {} over threshold, {} failed", st.sent, st.answered, st.over_threshold, st.failed);
            if let (Some(min), Some(avg)) = (st.min_rtt_us, st.average_rtt_us()) {
                let _ = writeln!(s, "  Round trip: min {}, average {}, max {}", millis(min), millis(avg), millis(st.max_rtt_us));
            }
            if let Some(at) = st.last_run_us {
                let _ = writeln!(s, "  Last probe: {} ago", age(now.saturating_sub(at)));
            }
        }
        let _ = writeln!(s, "  Schedule: {}, every {} s", state_word(op, st, now), op.frequency_s);
    }
    s.trim_end().to_string()
}

fn configuration(d: &Device, only: Option<u32>, now: u64) -> String {
    let list = ids(d, only);
    if list.is_empty() {
        return none_text(only);
    }
    let mut s = String::new();
    for id in list {
        let Some(op) = d.sla.ops.get(&id) else { continue };
        let _ = writeln!(s, "IP SLA operation {id}");
        let _ = writeln!(s, "  Test: {}", test_text(op));
        let _ = writeln!(s, "  Frequency {} s, timeout {} ms, threshold {} ms", op.frequency_s, op.timeout_ms, op.threshold_ms);
        if !op.tag.is_empty() {
            let _ = writeln!(s, "  Tag: {}", op.tag);
        }
        let life = op.schedule.as_ref().map_or("not scheduled".to_string(), |sch| {
            format!("life {}, start {}", sch.life_s.map_or("forever".to_string(), |l| format!("{l} s")), schedule_start(sch.start))
        });
        let _ = writeln!(s, "  Schedule: {life} ({})", state_word(op, d.sla.stats.get(&id), now));
        for r in &op.reactions {
            let _ = writeln!(s, "  Reaction: {r}");
        }
        let used: Vec<String> =
            d.track.objects.iter().filter(|(_, o)| matches!(o.kind, Kind::Sla { op, .. } if op == id)).map(|(n, _)| n.to_string()).collect();
        if !used.is_empty() {
            let _ = writeln!(s, "  Followed by tracked objects: {}", used.join(", "));
        }
    }
    s.trim_end().to_string()
}

fn schedule_start(start: Start) -> String {
    match start {
        Start::Now => "now".into(),
        Start::Pending => "pending".into(),
        Start::After(s) => format!("after {}", age(u64::from(s).saturating_mul(1_000_000))),
    }
}

fn summary(d: &Device, now: u64) -> String {
    if d.sla.ops.is_empty() {
        return none_text(None);
    }
    let mut s = format!("{:<12} {:<12} {:<16} {:<10} {:<18} {}\n", "Operation", "Test", "Destination", "Frequency", "State", "Latest outcome");
    for (id, op) in &d.sla.ops {
        let st = d.sla.stats.get(id);
        let kind = op.probe.as_ref().map_or("-", Probe::name);
        let dest = op.probe.as_ref().map_or("-".to_string(), |p| p.dest().to_string());
        let _ = writeln!(
            s,
            "{:<12} {:<12} {:<16} {:<10} {:<18} {}",
            id,
            kind,
            dest,
            format!("{} s", op.frequency_s),
            state_word(op, st, now),
            outcome_text(st)
        );
    }
    s.trim_end().to_string()
}

/// The `ip sla` blocks of `show running-config`.
pub(crate) fn running_config(s: &mut String, d: &Device) {
    for (id, op) in &d.sla.ops {
        let _ = writeln!(s, "ip sla {id}");
        if let Some(p) = &op.probe {
            let _ = writeln!(s, " {}", p.line());
        }
        if op.frequency_s != 60 {
            let _ = writeln!(s, " frequency {}", op.frequency_s);
        }
        if op.timeout_ms != 5_000 {
            let _ = writeln!(s, " timeout {}", op.timeout_ms);
        }
        if op.threshold_ms != 5_000 {
            let _ = writeln!(s, " threshold {}", op.threshold_ms);
        }
        if !op.tag.is_empty() {
            let _ = writeln!(s, " tag {}", op.tag);
        }
        let _ = writeln!(s, "!");
    }
    for (id, op) in &d.sla.ops {
        if let Some(sch) = &op.schedule {
            let life = sch.life_s.map_or("forever".to_string(), |l| l.to_string());
            let _ = writeln!(s, "ip sla schedule {id} life {life} start-time {}", schedule_start(sch.start));
        }
        for r in &op.reactions {
            let _ = writeln!(s, "ip sla reaction-configuration {id} {r}");
        }
    }
    if d.sla.responder {
        let _ = writeln!(s, "ip sla responder");
    }
}
