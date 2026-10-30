//! Activities: checking a lab against its expected results, and capturing those results from a
//! finished lab.

use packetcraft_core::{DeviceId, Ipv4Net};
use packetcraft_ios::Cli;
use packetcraft_model::activity::{Check, CheckKind, flatten, line_hash, normalize};
use packetcraft_model::{Device, DeviceKind, Topology};
use packetcraft_sim::{PingOptions, Probe, Sim};

/// One check's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    pub label: String,
    pub points: u32,
    pub passed: bool,
}

/// The configuration lines a device has beyond a factory-fresh device of the same model.
fn answer_lines(d: &Device) -> Vec<String> {
    let mut fresh = Device::of_model(d.id, d.catalogue_model(), d.name.clone(), d.x, d.y);
    // The hostname counts as an answer: a fresh device here has none.
    fresh.name = String::new();
    let base = flatten(&packetcraft_ios::running_config(&fresh));
    flatten(&packetcraft_ios::running_config(d))
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
                    out.push(Check { label, points: 1, kind });
                }
            }
            k if k.is_host() => {
                if let Some(ip) = d.ports.first().and_then(|p| p.ip) {
                    let label = if hidden { format!("{}: address", d.name) } else { format!("{}: address {ip}", d.name) };
                    out.push(Check {
                        label,
                        points: 1,
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
pub fn evaluate(t: &Topology, sim: &Sim, checks: &[Check], salt: &str) -> Vec<CheckResult> {
    let mut configs: std::collections::BTreeMap<DeviceId, Vec<String>> = std::collections::BTreeMap::new();
    let mut lines = |d: DeviceId| -> Vec<String> {
        configs.entry(d).or_insert_with(|| t.device(d).map(|x| flatten(&packetcraft_ios::running_config(x))).unwrap_or_default()).clone()
    };
    let mut scratch = sim.clone();
    checks
        .iter()
        .map(|c| {
            let passed = match &c.kind {
                CheckKind::Config { device, line } => lines(*device).contains(&normalize_flat(line)),
                CheckKind::ConfigHashed { device, hash } => lines(*device).iter().any(|l| line_hash(salt, *device, l) == *hash),
                CheckKind::HostAddress { device, address, gateway } => t.device(*device).is_some_and(|d| {
                    let ip_ok = address.parse::<Ipv4Net>().is_ok_and(|want| d.ports.first().and_then(|p| p.ip) == Some(want));
                    let gw_ok = gateway.as_ref().is_none_or(|g| d.gateway.map(|x| x.to_string()).as_deref() == Some(g.as_str()));
                    ip_ok && gw_ok
                }),
                CheckKind::Ping { from, to, succeed } => {
                    let opts = PingOptions { count: 3, ..PingOptions::default() };
                    let probes = match to.parse::<std::net::IpAddr>() {
                        Ok(std::net::IpAddr::V4(a)) => scratch.ping(t, *from, a, opts),
                        Ok(std::net::IpAddr::V6(a)) => scratch.ping6(t, *from, a, opts),
                        Err(_) => Ok(Vec::new()),
                    }
                    .unwrap_or_default();
                    probes.iter().any(|p| matches!(p, Probe::Reply { .. })) == *succeed
                }
                CheckKind::Output { device, command, contains } => {
                    let mut tt = t.clone();
                    // Privileged mode, past any password, as the instructor means it.
                    let mut cli = Cli { mode: packetcraft_ios::Mode::Exec, logged_in: true, ..Cli::default() };
                    let out = packetcraft_ios::execute(&mut tt, &mut scratch, *device, &mut cli, command).output;
                    out.to_lowercase().contains(&contains.to_lowercase())
                }
            };
            CheckResult { label: c.label.clone(), points: c.points, passed }
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
