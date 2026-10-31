//! Activities: checking a lab against its expected results, and capturing those results from a
//! finished lab.

use netlabcraft_core::{DeviceId, Ipv4Net};
use netlabcraft_ios::Cli;
use netlabcraft_model::activity::{Check, CheckKind, flatten, line_hash, normalize};
use netlabcraft_model::{Device, DeviceKind, Topology};
use netlabcraft_sim::{PingOptions, Probe, Sim};

/// One check's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    pub label: String,
    pub points: u32,
    pub passed: bool,
    /// The check's hint, when it failed.
    pub feedback: String,
    pub section: String,
    /// What the answer holds, for the side-by-side view; empty when the activity hides answers.
    pub expected: String,
    /// What the student's lab holds in the same place.
    pub actual: String,
}

/// The longest output kept for the side-by-side view.
const SHOWN_OUTPUT: usize = 400;

/// The student's lines that answer the same question as `want` (`interface g0/0 | ip address
/// 10.0.0.1 …` → that block's `ip address` lines): the same block and the same first word, or
/// two words for `ip`/`ipv6`/`no`/`switchport`/`spanning-tree`.
fn same_place<'a>(have: &'a [String], want: &str) -> Vec<&'a str> {
    let (block, line) = match want.rsplit_once(" | ") {
        Some((b, l)) => (Some(b), l),
        None => (None, want),
    };
    let words: Vec<&str> = line.split_whitespace().collect();
    let n =
        if matches!(words.first(), Some(&("ip" | "ipv6" | "no" | "switchport" | "spanning-tree" | "standby" | "ppp" | "crypto"))) { 2 } else { 1 };
    let key = words.iter().take(n).copied().collect::<Vec<_>>().join(" ");
    have.iter()
        .filter(|h| {
            let (hb, hl) = match h.rsplit_once(" | ") {
                Some((b, l)) => (Some(b), l),
                None => (None, h.as_str()),
            };
            hb == block && (hl == key || hl.starts_with(&format!("{key} ")))
        })
        .map(String::as_str)
        .collect()
}

fn clip(s: &str) -> String {
    let s = s.trim();
    match s.char_indices().nth(SHOWN_OUTPUT) {
        Some((i, _)) => format!("{}…", s.get(..i).unwrap_or(s)),
        None => s.to_string(),
    }
}

/// The configuration lines a device has beyond a factory-fresh device of the same model.
pub(crate) fn answer_lines(d: &Device) -> Vec<String> {
    let mut fresh = Device::of_model(d.id, d.catalogue_model(), d.name.clone(), d.x, d.y);
    // The hostname counts as an answer: a fresh device here has none.
    fresh.name = String::new();
    let base = flatten(&netlabcraft_ios::running_config(&fresh));
    flatten(&netlabcraft_ios::running_config(d))
        .into_iter()
        .filter(|l| !base.contains(l))
        // A block header alone says nothing; its lines carry the answers.
        .filter(|l| {
            l.contains(" | ") || !(l.starts_with("interface ") || l.starts_with("router ") || l.starts_with("line ") || l.starts_with("vlan "))
        })
        .collect()
}

/// Checks for every configuration line of the routers and switches, and every host address.
pub fn capture(t: &Topology, hidden: bool, salt: &str) -> Vec<Check> {
    let mut out = Vec::new();
    for d in &t.devices {
        match d.kind {
            DeviceKind::Router | DeviceKind::Switch => {
                for (n, line) in answer_lines(d).into_iter().enumerate() {
                    let (label, kind) = if hidden {
                        (
                            format!("{}: configuration item {}", d.name, n + 1),
                            CheckKind::ConfigHashed { device: d.id, hash: line_hash(salt, d.id, &line) },
                        )
                    } else {
                        (format!("{}: {line}", d.name), CheckKind::Config { device: d.id, line })
                    };
                    out.push(Check { label, points: 1, section: d.name.clone(), feedback: String::new(), kind });
                }
            }
            k if k.is_host() => {
                if let Some(ip) = d.ports.first().and_then(|p| p.ip) {
                    let label = if hidden { format!("{}: address", d.name) } else { format!("{}: address {ip}", d.name) };
                    out.push(Check {
                        label,
                        points: 1,
                        section: d.name.clone(),
                        feedback: String::new(),
                        kind: CheckKind::HostAddress { device: d.id, address: ip.to_string(), gateway: d.gateway.map(|g| g.to_string()) },
                    });
                }
            }
            _ => {}
        }
    }
    out
}

/// Runs every check against the lab. Pings and commands run on copies, so the lab, its
/// consoles and its event list are left as they were.
/// `hidden`: the activity hides its answers, so `expected` stays empty where it would give one
/// away.
pub fn evaluate(t: &Topology, sim: &Sim, checks: &[Check], salt: &str, hidden: bool) -> Vec<CheckResult> {
    let mut configs: std::collections::BTreeMap<DeviceId, Vec<String>> = std::collections::BTreeMap::new();
    let mut lines = |d: DeviceId| -> Vec<String> {
        configs.entry(d).or_insert_with(|| t.device(d).map(|x| flatten(&netlabcraft_ios::running_config(x))).unwrap_or_default()).clone()
    };
    let mut scratch = sim.clone();
    checks
        .iter()
        .map(|c| {
            let (passed, expected, actual) = match &c.kind {
                CheckKind::Config { device, line } => {
                    let want = normalize_flat(line);
                    let have = lines(*device);
                    let near = same_place(&have, &want).join("\n");
                    (have.contains(&want), want, near)
                }
                CheckKind::ConfigHashed { device, hash } => {
                    (lines(*device).iter().any(|l| line_hash(salt, *device, l) == *hash), String::new(), String::new())
                }
                CheckKind::HostAddress { device, address, gateway } => {
                    let d = t.device(*device);
                    let ip = d.and_then(|d| d.ports.first().and_then(|p| p.ip));
                    let gw = d.and_then(|d| d.gateway);
                    let ip_ok = address.parse::<Ipv4Net>().is_ok_and(|want| ip == Some(want));
                    let gw_ok = gateway.as_ref().is_none_or(|g| gw.map(|x| x.to_string()).as_deref() == Some(g.as_str()));
                    let show = |ip: String, gw: Option<String>| match gw {
                        Some(g) => format!("{ip} via {g}"),
                        None => ip,
                    };
                    let actual = show(ip.map_or_else(|| "no address".to_string(), |i| i.to_string()), gw.map(|g| g.to_string()));
                    (ip_ok && gw_ok, show(address.clone(), gateway.clone()), actual)
                }
                CheckKind::Ping { from, to, succeed } => {
                    let opts = PingOptions { count: 3, ..PingOptions::default() };
                    let probes = match to.parse::<std::net::IpAddr>() {
                        Ok(std::net::IpAddr::V4(a)) => scratch.ping(t, *from, a, opts),
                        Ok(std::net::IpAddr::V6(a)) => scratch.ping6(t, *from, a, opts),
                        Err(_) => Ok(Vec::new()),
                    }
                    .unwrap_or_default();
                    let replied = probes.iter().any(|p| matches!(p, Probe::Reply { .. }));
                    let say = |r: bool| if r { "replies".to_string() } else { "no replies".to_string() };
                    (replied == *succeed, say(*succeed), say(replied))
                }
                CheckKind::Output { device, command, contains } => {
                    let mut tt = t.clone();
                    // Privileged mode, past any password, as the instructor means it.
                    let mut cli = Cli { mode: netlabcraft_ios::Mode::Exec, logged_in: true, ..Cli::default() };
                    let out = netlabcraft_ios::execute(&mut tt, &mut scratch, *device, &mut cli, command).output;
                    (out.to_lowercase().contains(&contains.to_lowercase()), format!("contains \"{contains}\""), clip(&out))
                }
                CheckKind::Link { .. } | CheckKind::LinkHashed { .. } => {
                    crate::instructor::eval_cable(t, &c.kind, salt).unwrap_or((false, String::new(), String::new()))
                }
            };
            // A hidden activity shows what the student has, never what the answer is (pings
            // say only whether replies are wanted).
            let expected = if hidden && !matches!(c.kind, CheckKind::Ping { .. }) { String::new() } else { expected };
            CheckResult {
                label: c.label.clone(),
                points: c.points,
                passed,
                feedback: if passed { String::new() } else { c.feedback.clone() },
                section: c.section.clone(),
                expected,
                actual,
            }
        })
        .collect()
}

/// A configuration answer typed by hand, in the flattened form (`interface g0/0 | …` keeps
/// its block; anything else is one line).
fn normalize_flat(line: &str) -> String {
    match line.split_once('|') {
        Some((block, l)) => format!("{} | {}", normalize(block), normalize(l)),
        None => normalize(line),
    }
}

/// A salt for a new activity, from what the lab holds now.
pub fn new_salt(t: &Topology, title: &str) -> String {
    line_hash(&t.devices.len().to_string(), DeviceId(t.next_id), title)
}
