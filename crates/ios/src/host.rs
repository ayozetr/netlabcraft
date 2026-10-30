//! The shell of PCs and servers: `ipconfig`, `ping`, `tracert`, `arp`, `hostname`, `ftp`.

use std::fmt::Write as _;

use netlabcraft_core::{DeviceId, Ipv4Net, parse_ip};
use netlabcraft_model::{Device, Topology};
use netlabcraft_sim::{PingOptions, Probe, Sim};

use crate::Outcome;
use crate::render::{ms, traceroute};

pub const HELP: &str = "\
Commands:
  ipconfig                               show this host's address
  ipconfig /all                          also the DHCP lease, domain and server
  ipconfig <address> <mask> [gw] [dns]   set the address (gateway, DNS server)
  ipconfig /renew                        ask a DHCP server for an address
  ipconfig /release                      remove the address
  ipv6config                             show the IPv6 settings
  ipv6config <address/len> [gw]          set an IPv6 address (and gateway)
  ipv6config auto                        take an address from the router (SLAAC)
  ipv6config off                         turn IPv6 off
  ping [-n count] [-l size] [-t] <host>  send echo requests (-t: keep going, up to 100)
  tracert <address|name>                 trace the route (also: traceroute)
  nslookup <name>                        ask the DNS server for a name
  telnet <address>                       log in to a router or switch
  ssh -l <user> <address>                log in securely
  ftp <address>                          send and fetch files on an FTP server
  netstat                                TCP connections this host set up
  dir                                    list this host's files
  arp -a                                 show the ARP cache
  arp -d [address]                       empty the ARP cache (or forget one entry)
  hostname                               show this host's name
  cls                                    clear the screen";

/// The most echo requests `ping -t` sends.
const PING_T_CAP: u16 = 100;

/// A host's FTP session: the server, and where the login stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtpClient {
    server: std::net::Ipv4Addr,
    stage: FtpStage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FtpStage {
    User,
    Password(String),
    Ready(String, String),
}

impl FtpClient {
    pub fn prompt(&self) -> &'static str {
        match self.stage {
            FtpStage::User => "Username: ",
            FtpStage::Password(_) => crate::PASSWORD_PROMPT,
            FtpStage::Ready(..) => "ftp> ",
        }
    }
}

const FTP_HELP: &str = "\
  put <file>      send one of this host's files
  get <file>      fetch a file from the server
  dir             list the server's files
  delete <file>   delete a file on the server
  quit            leave ftp";

/// Applies to the lab the server files FTP changed.
pub fn apply_ftp_changes(t: &mut Topology, sim: &mut Sim) {
    for (srv, name, content) in sim.take_ftp_changes() {
        if let Some(d) = t.device_mut(srv) {
            netlabcraft_sim::ftp::apply_change(&mut d.services.ftp.files, &name, content);
        }
    }
}

/// A line typed inside `ftp`.
pub fn ftp_line(t: &mut Topology, sim: &mut Sim, dev: DeviceId, cli: &mut crate::Cli, line: &str) -> Outcome {
    use netlabcraft_sim::ftp::FtpOp;
    let out = |s: String| Outcome { output: s, ..Outcome::default() };
    let Some(mut f) = cli.ftp.take() else { return Outcome::default() };
    match f.stage.clone() {
        FtpStage::User => {
            f.stage = FtpStage::Password(line.trim().to_string());
            cli.ftp = Some(f);
            Outcome::default()
        }
        FtpStage::Password(user) => match sim.ftp(t, dev, f.server, &user, line, &FtpOp::Login) {
            Ok(_) => {
                f.stage = FtpStage::Ready(user, line.to_string());
                cli.ftp = Some(f);
                out("230 Logged in".into())
            }
            Err(e) => out(format!("{e}\nftp session closed")),
        },
        FtpStage::Ready(user, pass) => {
            let words: Vec<&str> = line.split_whitespace().collect();
            let cmd = words.first().map(|w| w.to_ascii_lowercase()).unwrap_or_default();
            let name = words.get(1).map(|s| s.to_string());
            let op = match (cmd.as_str(), name) {
                ("", _) => {
                    cli.ftp = Some(f);
                    return Outcome::default();
                }
                ("quit" | "bye" | "exit", _) => return out("221 Goodbye".into()),
                ("help" | "?", _) => {
                    cli.ftp = Some(f);
                    return out(FTP_HELP.into());
                }
                ("put", Some(n)) => match t.device(dev).and_then(|d| d.files.iter().find(|x| x.name == n)) {
                    Some(file) => FtpOp::Put(n, file.content.clone().into_bytes()),
                    None => {
                        cli.ftp = Some(f);
                        return out(format!("{n}: no such file on this host (dir lists them)"));
                    }
                },
                ("get", Some(n)) => FtpOp::Get(n),
                ("delete", Some(n)) => FtpOp::Delete(n),
                ("dir" | "ls", _) => FtpOp::List,
                _ => {
                    cli.ftp = Some(f);
                    return out(format!("`{line}`: not an ftp command\n{FTP_HELP}"));
                }
            };
            let r = sim.ftp(t, dev, f.server, &user, &pass, &op);
            apply_ftp_changes(t, sim);
            cli.ftp = Some(f);
            out(match (r, op) {
                (Ok((done, _)), FtpOp::Put(n, b)) => format!("Sending {n} ({} bytes)\n{done}", b.len()),
                (Ok((done, bytes)), FtpOp::Get(n)) => {
                    let content: String = String::from_utf8_lossy(&bytes).chars().take(netlabcraft_model::services::MAX_FILE).collect();
                    let len = bytes.len();
                    if let Some(d) = t.device_mut(dev) {
                        netlabcraft_sim::ftp::apply_change(&mut d.files, &n, Some(content));
                    }
                    format!("Receiving {n} ({len} bytes)\n{done}")
                }
                (Ok((done, bytes)), FtpOp::List) => format!("{}\n{done}", String::from_utf8_lossy(&bytes).trim_end()),
                (Ok((done, _)), _) => done,
                (Err(e), _) => e.to_string(),
            })
        }
    }
}

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
        "ipv6config" => ipv6config(t, sim, dev, words.get(1..).unwrap_or_default()),
        "tracert" | "traceroute" => {
            let Some(target) = words.get(1) else { return out("Usage: tracert <address or name>".into()) };
            if let Ok(dst6) = target.parse::<std::net::Ipv6Addr>() {
                return match sim.traceroute6(t, dev, dst6, 30, 2_000_000) {
                    Ok(h) => out(traceroute(dst6.into(), &h)),
                    Err(e) => out(e.to_string()),
                };
            }
            let dst = match sim.dns_resolve(t, dev, target) {
                Ok(a) => a,
                Err(e) => return out(e.to_string()),
            };
            match sim.traceroute(t, dev, dst, 30, 2_000_000) {
                Ok(h) => out(traceroute(dst.into(), &h)),
                Err(e) => out(e.to_string()),
            }
        }
        "telnet" | "ssh" => {
            let proto = if first.eq_ignore_ascii_case("ssh") { netlabcraft_model::RemoteProto::Ssh } else { netlabcraft_model::RemoteProto::Telnet };
            let rest = words.get(1..).unwrap_or_default();
            let (user, target) = match rest {
                ["-l", u, a] | [a, "-l", u] => (Some(u.to_string()), a.to_string()),
                [a] if proto == netlabcraft_model::RemoteProto::Telnet => (None, a.to_string()),
                _ => {
                    return out(format!(
                        "Usage: {}",
                        if proto == netlabcraft_model::RemoteProto::Ssh { "ssh -l <user> <address>" } else { "telnet <address>" }
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
        "ftp" => {
            let Some(addr) = words.get(1) else { return out("Usage: ftp <address>".into()) };
            match sim.dns_resolve(t, dev, addr) {
                Ok(server) => {
                    cli.ftp = Some(FtpClient { server, stage: FtpStage::User });
                    out(format!("Trying to connect...{server}"))
                }
                Err(e) => out(e.to_string()),
            }
        }
        "dir" => {
            let files = t.device(dev).map(|d| d.files.clone()).unwrap_or_default();
            let mut s = String::from("Directory of this host\n\n");
            for f in &files {
                let _ = writeln!(s, "{:>8}  {}", f.content.len(), f.name);
            }
            let _ = write!(s, "{} file{}", files.len(), if files.len() == 1 { "" } else { "s" });
            out(s)
        }
        "netstat" => out(netstat(sim, dev)),
        "arp" => match words.get(1).map(|w| w.to_ascii_lowercase()) {
            Some(f) if f == "-d" => match words.get(2) {
                None | Some(&"*") => {
                    sim.clear_arp(dev);
                    out("ARP cache emptied.".into())
                }
                Some(a) => match parse_ip(a) {
                    Ok(ip) if sim.forget_arp(dev, ip) => out(format!("{ip} removed from the ARP cache.")),
                    Ok(ip) => out(format!("{ip} is not in the ARP cache.")),
                    Err(e) => out(e.to_string()),
                },
            },
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
    let show_basic = |d: &Device| {
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
    let show_all = |d: &Device| {
        let l = d.dhcp_lease.clone().unwrap_or_default();
        let mut s = show_basic(d);
        if d.dhcp_client {
            let _ = write!(
                s,
                "\nDomain name {}\nLease time  {}\nDHCP server {}",
                l.domain.as_deref().unwrap_or("not set"),
                l.lease_secs.map_or("unknown".to_string(), netlabcraft_model::mgmt::lease_text),
                l.server.map_or("unknown".to_string(), |a| a.to_string())
            );
        }
        s + &ipv6_lines(d)
    };
    let show = |d: &Device| show_basic(d) + &ipv6_lines(d);
    let Some(d) = t.device(dev) else { return Outcome::default() };
    let result: Result<String, String> = match args {
        [] => return Outcome { output: show(d), ..Outcome::default() },
        [flag] if flag.eq_ignore_ascii_case("/all") => return Outcome { output: show_all(d), ..Outcome::default() },
        [flag] if flag.eq_ignore_ascii_case("/release") => {
            if let Some(d) = t.device_mut(dev) {
                if let Some(p) = d.ports.first_mut() {
                    p.ip = None;
                }
                d.gateway = None;
                d.dns = None;
                d.dhcp_client = false;
                d.dhcp_lease = None;
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
                    d.voice.tftp = lease.tftp;
                    d.dhcp_lease =
                        Some(netlabcraft_model::mgmt::HostLease { domain: lease.domain.clone(), lease_secs: lease.lease_secs, server: lease.server });
                }
                Ok(format!(
                    "DHCP gave this host {}{}{}{}{}.",
                    lease.address,
                    lease.router.map(|g| format!(", gateway {g}")).unwrap_or_default(),
                    lease.dns.map(|g| format!(", DNS {g}")).unwrap_or_default(),
                    lease.domain.as_ref().map(|g| format!(", domain {g}")).unwrap_or_default(),
                    lease.tftp.map(|g| format!(", TFTP server {g}")).unwrap_or_default()
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
            d.dhcp_lease = None;
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

/// `netstat`: the TCP connections this host set up or answered.
fn netstat(sim: &Sim, dev: DeviceId) -> String {
    let list = sim.state(dev).map(|st| st.tcp_recent.clone()).unwrap_or_default();
    if list.is_empty() {
        return "No TCP connections yet.".into();
    }
    let mut s = format!("{:<6} {:<22} {:<22} {}\n", "Proto", "Local address", "Remote address", "Opened (s)");
    for r in &list {
        let _ = writeln!(
            s,
            "{:<6} {:<22} {:<22} {}",
            "TCP",
            format!("{}:{}", r.local.0, r.local.1),
            format!("{}:{}", r.remote.0, r.remote.1),
            r.since_us / 1_000_000
        );
    }
    s.trim_end().to_string()
}

fn host_ping(t: &Topology, sim: &mut Sim, dev: DeviceId, args: &[&str]) -> String {
    let mut count: u16 = 4;
    let mut size: usize = 32;
    let mut forever = false;
    let mut df = false;
    let mut target = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a.eq_ignore_ascii_case("-n") || a.eq_ignore_ascii_case("-c") {
            match it.next().and_then(|n| n.parse::<u16>().ok()).filter(|n| (1..=1000).contains(n)) {
                Some(n) => count = n,
                None => return "The count must be 1-1000.".into(),
            }
        } else if a.eq_ignore_ascii_case("-l") {
            match it.next().and_then(|n| n.parse::<usize>().ok()).filter(|n| *n <= 1_472) {
                Some(n) => size = n,
                None => return "The size must be 0-1472 bytes.".into(),
            }
        } else if a.eq_ignore_ascii_case("-t") {
            forever = true;
        } else if a.eq_ignore_ascii_case("-f") {
            df = true;
        } else if a.starts_with('-') {
            return format!("`{a}` is not a ping option (-n, -l, -t, -f).");
        } else {
            target = Some(*a);
        }
    }
    let Some(target) = target else { return "Usage: ping [-n count] [-l size] [-t] <address or name>".into() };
    if forever {
        count = PING_T_CAP;
    }
    let opts = PingOptions { count, size, df, ..PingOptions::default() };
    let (dst, probes): (std::net::IpAddr, _) = if let Ok(dst6) = target.parse::<std::net::Ipv6Addr>() {
        (dst6.into(), sim.ping6(t, dev, dst6, opts))
    } else {
        let dst = match sim.dns_resolve(t, dev, target) {
            Ok(a) => a,
            Err(e) => return e.to_string(),
        };
        (dst.into(), sim.ping(t, dev, dst, opts))
    };
    let probes = match probes {
        Ok(p) => p,
        Err(e) => return e.to_string(),
    };
    let mut s = if forever {
        format!("Pinging {dst} until stopped ({PING_T_CAP} echo requests at most here), {size} bytes each:\n")
    } else {
        format!("Pinging {dst}, {count} echo requests of {size} bytes:\n")
    };
    for (i, p) in probes.iter().enumerate() {
        let seq = i + 1;
        let _ = match p {
            Probe::Reply { from, rtt_us } => writeln!(s, "  seq {seq}: reply from {from}, {} ms", ms(*rtt_us)),
            Probe::TtlExceeded { from, .. } => writeln!(s, "  seq {seq}: {from} says the TTL ran out in transit"),
            Probe::Unreachable { from, code: 4 } => {
                writeln!(s, "  seq {seq}: {from} says the packet needs fragmenting but \"don't fragment\" is set")
            }
            Probe::Unreachable { from, .. } => writeln!(s, "  seq {seq}: {from} says the destination is unreachable"),
            Probe::Timeout => writeln!(s, "  seq {seq}: no answer (timed out)"),
        };
    }
    let got = probes.iter().filter(|p| matches!(p, Probe::Reply { .. })).count();
    let _ = write!(s, "{} sent, {got} answered, {}% lost", probes.len(), (probes.len() - got) * 100 / probes.len().max(1));
    s
}

/// The IPv6 part of `ipconfig` and `ipv6config`.
fn ipv6_lines(d: &Device) -> String {
    let Some(i) = netlabcraft_sim::v6::l3_ifaces6(d).into_iter().next() else { return "\nIPv6        off".into() };
    let mut s = format!("\nLink-local  {}", i.link_local);
    for a in &i.addresses {
        let _ = write!(s, "\nIPv6        {a}");
    }
    let how = if d.ports.first().is_some_and(|p| p.ipv6.autoconfig) { " (from the router)" } else { "" };
    let _ = write!(s, "\nIPv6 gw     {}{how}", d.gateway6.map_or("not set".to_string(), |g| g.to_string()));
    if let Some(dns) = d.dns6 {
        let _ = write!(s, "\nIPv6 DNS    {dns}");
    }
    s
}

fn ipv6config(t: &mut Topology, sim: &mut Sim, dev: DeviceId, args: &[&str]) -> Outcome {
    let Some(d) = t.device(dev) else { return Outcome::default() };
    let result: Result<String, String> = match args {
        [] => return Outcome { output: ipv6_lines(d).trim_start().to_string(), ..Outcome::default() },
        [w] if w.eq_ignore_ascii_case("/all") => {
            return Outcome {
                output: format!("Interface   {}{}", d.ports.first().map_or("-", |p| p.name.as_str()), ipv6_lines(d)),
                ..Outcome::default()
            };
        }
        [w] if w.eq_ignore_ascii_case("off") => {
            if let Some(d) = t.device_mut(dev) {
                if let Some(p) = d.ports.first_mut() {
                    p.ipv6 = netlabcraft_model::Ipv6Conf::default();
                }
                d.gateway6 = None;
            }
            Ok("IPv6 turned off.".into())
        }
        [w] if w.eq_ignore_ascii_case("auto") => {
            // Link-local first, so the router solicitation has a source.
            if let Some(p) = t.device_mut(dev).and_then(|d| d.ports.first_mut()) {
                p.ipv6.enabled = true;
            }
            sim.topology_changed(t);
            match sim.autoconfig6_full(t, dev) {
                Ok(auto) => {
                    let (addr, router) = (auto.address, auto.router);
                    if let Some(d) = t.device_mut(dev) {
                        if let Some(p) = d.ports.first_mut() {
                            p.ipv6 = netlabcraft_model::Ipv6Conf {
                                enabled: true,
                                addresses: vec![addr],
                                link_local: None,
                                autoconfig: true,
                                ..Default::default()
                            };
                        }
                        d.gateway6 = Some(router);
                        d.dns6 = auto.dns;
                    }
                    Ok(format!(
                        "Autoconfigured {addr}, gateway {router}{} ({}).",
                        auto.dns.map(|a| format!(", DNS {a}")).unwrap_or_default(),
                        auto.how
                    ))
                }
                Err(e) => Err(format!("Autoconfiguration failed: {e}.")),
            }
        }
        [addr, rest @ ..] if rest.len() <= 1 => (|| {
            let net: netlabcraft_core::Ipv6Net = addr.parse().map_err(|e: netlabcraft_core::CoreError| e.to_string())?;
            if net.addr.is_multicast() || net.addr.is_unspecified() || netlabcraft_core::Ipv6Net::is_link_local(net.addr) {
                return Err(format!("{} can't be this host's address", net.addr));
            }
            let gw = match rest.first() {
                Some(g) => Some(g.parse::<std::net::Ipv6Addr>().map_err(|_| format!("`{g}` is not an IPv6 address"))?),
                None => None,
            };
            if let Some(g) = gw
                && !netlabcraft_core::Ipv6Net::is_link_local(g)
                && !net.contains(g)
            {
                return Err(format!("the gateway {g} is not on {}", net.network()));
            }
            let d = t.device_mut(dev).ok_or("the device no longer exists")?;
            if let Some(p) = d.ports.first_mut() {
                p.ipv6 =
                    netlabcraft_model::Ipv6Conf { enabled: true, addresses: vec![net], link_local: None, autoconfig: false, ..Default::default() };
            }
            d.gateway6 = gw;
            Ok(format!("IPv6 address set to {net}{}.", gw.map(|g| format!(", gateway {g}")).unwrap_or_default()))
        })(),
        _ => Err("Usage: ipv6config [<address/len> [<gateway>] | auto | off]".into()),
    };
    match result {
        Ok(s) => {
            sim.topology_changed(t);
            Outcome { output: s, changed: true, clear: false }
        }
        Err(e) => {
            sim.topology_changed(t);
            Outcome { output: e, changed: true, clear: false }
        }
    }
}
