//! The shell of PCs and servers: `ipconfig`, `ping`, `tracert`, `arp`, `hostname`.

use std::fmt::Write as _;

use packetcraft_core::{DeviceId, Ipv4Net, parse_ip};
use packetcraft_model::{Device, Topology};
use packetcraft_sim::{PingOptions, Probe, Sim};

use crate::Outcome;
use crate::render::{ms, traceroute};

pub const HELP: &str = "\
Commands:
  ipconfig                         show this host's address
    ipconfig <address> <mask> [gw] [dns]  set the address (gateway, DNS server)
    ipconfig /renew                  ask a DHCP server for an address
  ipconfig /release                remove the address
    ping [-n count] <address|name>   send echo requests
  tracert <address|name>           trace the route (also: traceroute)
  nslookup <name>                  ask the DNS server for a name
  telnet <address>                 log in to a router or switch
  ssh -l <user> <address>          log in securely
  arp -a                           show the ARP cache
  arp -d                           empty the ARP cache
  hostname                         show this host's name
  cls                              clear the screen";

pub fn prompt(d: &Device) -> String {
    format!("{}> ", d.name)
}

pub fn execute(t: &mut Topology, sim: &mut Sim, dev: DeviceId, cli: &mut crate::Cli, line: &str) -> Outcome {
    let words: Vec<&str> = line.split_whitespace().collect();
    let Some(first) = words.first() else { return Outcome::default() };
    let out = |s: String| Outcome { output: s, ..Outcome::default() };
    match first.to_ascii_lowercase().as_str() {
        "help" | "?" => out(HELP.to_string()),
        "cls" | "clear" => Outcome { clear: true, ..Outcome::default() },
        "hostname" => out(t.device(dev).map(|d| d.name.clone()).unwrap_or_default()),
        "ipconfig" | "ifconfig" => ipconfig(t, sim, dev, words.get(1..).unwrap_or_default()),
        "ping" => out(host_ping(t, sim, dev, words.get(1..).unwrap_or_default())),
        "tracert" | "traceroute" => {
            let Some(target) = words.get(1) else { return out("Usage: tracert <address or name>".into()) };
            let dst = match sim.dns_resolve(t, dev, target) {
                Ok(a) => a,
                Err(e) => return out(e.to_string()),
            };
            match sim.traceroute(t, dev, dst, 30, 2_000_000) {
                Ok(h) => out(traceroute(dst, &h)),
                Err(e) => out(e.to_string()),
            }
        }
        "telnet" | "ssh" => {
            let proto = if first.eq_ignore_ascii_case("ssh") { packetcraft_model::RemoteProto::Ssh } else { packetcraft_model::RemoteProto::Telnet };
            let rest = words.get(1..).unwrap_or_default();
            let (user, target) = match rest {
                ["-l", u, a] | [a, "-l", u] => (Some(u.to_string()), a.to_string()),
                [a] if proto == packetcraft_model::RemoteProto::Telnet => (None, a.to_string()),
                _ => {
                    return out(format!(
                        "Usage: {}",
                        if proto == packetcraft_model::RemoteProto::Ssh { "ssh -l <user> <address>" } else { "telnet <address>" }
                    ));
                }
            };
            let dst = match sim.dns_resolve(t, dev, &target) {
                Ok(a) => a,
                Err(e) => return out(e.to_string()),
            };
            out(crate::exec::connect(t, sim, dev, cli, proto, dst, user))
        }
        "nslookup" => {
            let Some(name) = words.get(1) else { return out("Usage: nslookup <name>".into()) };
            let server = t.device(dev).and_then(|d| d.dns).map_or("none".to_string(), |s| s.to_string());
            match sim.dns_resolve(t, dev, name) {
                Ok(a) => out(format!("Server:  {server}\nName:    {name}\nAddress: {a}")),
                Err(e) => out(format!("Server:  {server}\n{e}")),
            }
        }
        "arp" => match words.get(1).map(|w| w.to_ascii_lowercase()) {
            Some(f) if f == "-d" => {
                sim.clear_arp(dev);
                out("ARP cache emptied.".into())
            }
            _ => {
                let mut s = format!("{:<17} {}\n", "Address", "MAC address");
                if let Some(st) = sim.state(dev) {
                    for (ip, e) in &st.arp {
                        let _ = writeln!(s, "{:<17} {}", ip.to_string(), e.mac);
                    }
                }
                if sim.state(dev).is_none_or(|st| st.arp.is_empty()) {
                    s.push_str("(no entries)");
                }
                out(s.trim_end().to_string())
            }
        },
        other => out(format!("`{other}` is not a command here. Type help for the list.")),
    }
}

fn ipconfig(t: &mut Topology, sim: &mut Sim, dev: DeviceId, args: &[&str]) -> Outcome {
    let show = |d: &Device| {
        let p = d.ports.first();
        format!(
            "Interface   {}\nMAC address {}\nIP address  {}\nSubnet mask {}\nGateway     {}\nDNS server  {}\nAssigned    {}",
            p.map_or("-", |p| p.name.as_str()),
            p.map_or("-".to_string(), |p| p.mac.to_string()),
            p.and_then(|p| p.ip).map_or("not set".to_string(), |ip| ip.addr.to_string()),
            p.and_then(|p| p.ip).map_or("not set".to_string(), |ip| ip.mask().to_string()),
            d.gateway.map_or("not set".to_string(), |g| g.to_string()),
            d.dns.map_or("not set".to_string(), |g| g.to_string()),
            if d.dhcp_client { "by DHCP" } else { "by hand (static)" },
        )
    };
    let Some(d) = t.device(dev) else { return Outcome::default() };
    let result: Result<String, String> = match args {
        [] => return Outcome { output: show(d), ..Outcome::default() },
        [flag] if flag.eq_ignore_ascii_case("/all") => return Outcome { output: show(d), ..Outcome::default() },
        [flag] if flag.eq_ignore_ascii_case("/release") => {
            if let Some(d) = t.device_mut(dev) {
                if let Some(p) = d.ports.first_mut() {
                    p.ip = None;
                }
                d.gateway = None;
                d.dns = None;
                d.dhcp_client = false;
            }
            Ok("Address released.".into())
        }
        [flag] if flag.eq_ignore_ascii_case("/renew") => match sim.dhcp_acquire(t, dev) {
            Ok(lease) => {
                if let Some(d) = t.device_mut(dev) {
                    if let Some(p) = d.ports.first_mut() {
                        p.ip = Some(lease.address);
                    }
                    d.gateway = lease.router;
                    d.dns = lease.dns;
                    d.dhcp_client = true;
                }
                Ok(format!(
                    "DHCP gave this host {}{}{}.",
                    lease.address,
                    lease.router.map(|g| format!(", gateway {g}")).unwrap_or_default(),
                    lease.dns.map(|g| format!(", DNS {g}")).unwrap_or_default()
                ))
            }
            Err(e) => Err(format!("DHCP failed: {e}.")),
        },
        [addr, mask, rest @ ..] if rest.len() <= 2 => (|| {
            let a = parse_ip(addr).map_err(|e| e.to_string())?;
            let net = Ipv4Net::with_mask(a, parse_ip(mask).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            if net.is_network_or_broadcast() {
                return Err(format!("{a} is the network or broadcast address of {}", net.network()));
            }
            let gw = match rest.first() {
                Some(g) => {
                    let g = parse_ip(g).map_err(|e| e.to_string())?;
                    if !net.contains(g) {
                        return Err(format!("the gateway {g} is not on {}", net.network()));
                    }
                    Some(g)
                }
                None => None,
            };
            let dns = match rest.get(1) {
                Some(s) => Some(parse_ip(s).map_err(|e| e.to_string())?),
                None => None,
            };
            let d = t.device_mut(dev).ok_or("the device no longer exists")?;
            d.dns = dns;
            if let Some(p) = d.ports.first_mut() {
                p.ip = Some(net);
            }
            d.gateway = gw;
            d.dhcp_client = false;
            Ok(format!("Address set to {net}{}.", gw.map(|g| format!(", gateway {g}")).unwrap_or_default()))
        })(),
        _ => Err("Usage: ipconfig [<address> <mask> [<gateway> [<dns>]]] | /renew | /release".into()),
    };
    match result {
        Ok(s) => {
            sim.topology_changed(t);
            Outcome { output: s, changed: true, clear: false }
        }
        Err(e) => Outcome { output: e, ..Outcome::default() },
    }
}

fn host_ping(t: &Topology, sim: &mut Sim, dev: DeviceId, args: &[&str]) -> String {
    let mut count: u16 = 4;
    let mut target = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a.eq_ignore_ascii_case("-n") || a.eq_ignore_ascii_case("-c") {
            match it.next().and_then(|n| n.parse::<u16>().ok()).filter(|n| (1..=1000).contains(n)) {
                Some(n) => count = n,
                None => return "The count must be 1-1000.".into(),
            }
        } else {
            target = Some(*a);
        }
    }
    let Some(target) = target else { return "Usage: ping [-n count] <address or name>".into() };
    let dst = match sim.dns_resolve(t, dev, target) {
        Ok(a) => a,
        Err(e) => return e.to_string(),
    };
    let opts = PingOptions { count, size: 32, ..PingOptions::default() };
    let probes = match sim.ping(t, dev, dst, opts) {
        Ok(p) => p,
        Err(e) => return e.to_string(),
    };
    let mut s = format!("Pinging {dst}, {count} echo requests of 32 bytes:\n");
    for (i, p) in probes.iter().enumerate() {
        let seq = i + 1;
        let _ = match p {
            Probe::Reply { from, rtt_us } => writeln!(s, "  seq {seq}: reply from {from}, {} ms", ms(*rtt_us)),
            Probe::TtlExceeded { from, .. } => writeln!(s, "  seq {seq}: {from} says the TTL ran out in transit"),
            Probe::Unreachable { from, .. } => writeln!(s, "  seq {seq}: {from} says the destination is unreachable"),
            Probe::Timeout => writeln!(s, "  seq {seq}: no answer (timed out)"),
        };
    }
    let got = probes.iter().filter(|p| matches!(p, Probe::Reply { .. })).count();
    let _ = write!(s, "{} sent, {got} answered, {}% lost", probes.len(), (probes.len() - got) * 100 / probes.len().max(1));
    s
}
