//! What `show` commands print, and the configuration text. Layouts are PacketCraft's own.

use std::fmt::Write as _;
use std::net::Ipv4Addr;

use packetcraft_core::{VlanId, VlanSet};
use packetcraft_model::{Device, DeviceKind, End, NextHop, SwitchportMode, Topology, ifname, obscure};
use packetcraft_sim::{Hop, PingOptions, Probe, Sim, port_up, routing_table};

use crate::IfRef;

/// The running configuration as re-playable command lines.
pub fn running_config(d: &Device) -> String {
    let mut s = String::new();
    let a = &d.access;
    let pw = |p: &str| if a.password_encryption { format!("7 {}", obscure(p)) } else { p.to_string() };
    let _ = writeln!(s, "! PacketCraft configuration of {}", d.name);
    if a.password_encryption {
        let _ = writeln!(s, "service password-encryption");
    }
    let _ = writeln!(s, "!\nhostname {}\n!", d.name);
    if d.ip_routing {
        let _ = writeln!(s, "ip routing\n!");
    }
    if d.ipv6_routing {
        let _ = writeln!(s, "ipv6 unicast-routing\n!");
    }
    if let Some(h) = &a.enable_secret {
        let _ = writeln!(s, "enable secret 5 {h}");
    }
    if let Some(p) = &a.enable_password {
        let _ = writeln!(s, "enable password {}", pw(p));
    }
    if a.no_domain_lookup {
        let _ = writeln!(s, "no ip domain-lookup");
    }
    if let Some(dn) = &a.domain_name {
        let _ = writeln!(s, "ip domain-name {dn}");
    }
    for u in &a.users {
        if u.hashed {
            let _ = writeln!(s, "username {} secret 5 {}", u.name, u.credential);
        } else {
            let _ = writeln!(s, "username {} password {}", u.name, pw(&u.credential));
        }
    }
    if let Some(b) = a.rsa_bits {
        let _ = writeln!(s, "crypto key generate rsa general-keys modulus {b}");
    }
    if let Some(v) = a.ssh_version {
        let _ = writeln!(s, "ip ssh version {v}");
    }
    if a.enable_secret.is_some() || a.enable_password.is_some() || a.no_domain_lookup || a.domain_name.is_some() || !a.users.is_empty() {
        let _ = writeln!(s, "!");
    }
    for (x, y) in &d.dhcp_excluded {
        if x == y {
            let _ = writeln!(s, "ip dhcp excluded-address {x}");
        } else {
            let _ = writeln!(s, "ip dhcp excluded-address {x} {y}");
        }
    }
    if !d.dhcp_excluded.is_empty() {
        let _ = writeln!(s, "!");
    }
    for p in &d.dhcp_pools {
        let _ = writeln!(s, "ip dhcp pool {}", p.name);
        if let Some(n) = p.network {
            let _ = writeln!(s, " network {} {}", n.addr, n.mask());
        }
        if let Some(r) = p.default_router {
            let _ = writeln!(s, " default-router {r}");
        }
        if let Some(r) = p.dns {
            let _ = writeln!(s, " dns-server {r}");
        }
        let _ = writeln!(s, "!");
    }
    for (v, p) in &d.stp_priority {
        let _ = writeln!(s, "spanning-tree vlan {v} priority {p}");
    }
    if !d.stp_priority.is_empty() {
        let _ = writeln!(s, "!");
    }
    // A VTP client's VLANs come from its domain, not from its configuration.
    if d.kind == DeviceKind::Switch && d.vtp.mode != packetcraft_model::VtpMode::Client {
        for (v, name) in &d.vlans {
            if *v == VlanId::DEFAULT {
                continue;
            }
            let _ = writeln!(s, "vlan {v}");
            if *name != Device::default_vlan_name(*v) {
                let _ = writeln!(s, " name {name}");
            }
            let _ = writeln!(s, "!");
        }
    }
    for p in &d.ports {
        let _ = writeln!(s, "interface {}", p.name);
        if !p.description.is_empty() {
            let _ = writeln!(s, " description {}", p.description);
        }
        if d.kind == DeviceKind::Switch && p.routed {
            let _ = writeln!(s, " no switchport");
        }
        if d.kind == DeviceKind::Switch && !p.routed {
            let sp = &p.switchport;
            if sp.access_vlan != VlanId::DEFAULT {
                let _ = writeln!(s, " switchport access vlan {}", sp.access_vlan);
            }
            if sp.native_vlan != VlanId::DEFAULT {
                let _ = writeln!(s, " switchport trunk native vlan {}", sp.native_vlan);
            }
            if !sp.allowed.is_all() {
                let _ = writeln!(s, " switchport trunk allowed vlan {}", sp.allowed);
            }
            if sp.mode == SwitchportMode::Trunk {
                let _ = writeln!(s, " switchport mode trunk");
            }
            if let Some(ps) = &p.port_security {
                let _ = writeln!(s, " switchport port-security");
                if ps.max != 1 {
                    let _ = writeln!(s, " switchport port-security maximum {}", ps.max);
                }
                if ps.violation != packetcraft_model::Violation::Shutdown {
                    let _ = writeln!(s, " switchport port-security violation {}", ps.violation.name());
                }
                if ps.sticky {
                    let _ = writeln!(s, " switchport port-security mac-address sticky");
                }
                for m in &ps.macs {
                    let _ = writeln!(s, " switchport port-security mac-address {}{}", if ps.sticky { "sticky " } else { "" }, m.dotted());
                }
            }
            if let Some(g) = p.channel {
                let _ = writeln!(s, " channel-group {} mode {}", g.number, g.mode.name());
            }
            if p.portfast {
                let _ = writeln!(s, " spanning-tree portfast");
            }
        }
        match p.ip {
            Some(ip) => {
                let _ = writeln!(s, " ip address {} {}", ip.addr, ip.mask());
            }
            None if d.kind == DeviceKind::Router => {
                let _ = writeln!(s, " no ip address");
            }
            None => {}
        }
        if d.kind == DeviceKind::Router {
            if let Some(ll) = p.ipv6.link_local {
                let _ = writeln!(s, " ipv6 address {ll} link-local");
            }
            for a in &p.ipv6.addresses {
                let _ = writeln!(s, " ipv6 address {a}");
            }
            if p.ipv6.enabled {
                let _ = writeln!(s, " ipv6 enable");
            }
        }
        for h in &p.helpers {
            let _ = writeln!(s, " ip helper-address {h}");
        }
        access_groups(&mut s, &p.access);
        nat_side(&mut s, p.nat);
        // Routers write both states: their ports start shut, so a replay must say which are up.
        if p.shutdown {
            let _ = writeln!(s, " shutdown");
        } else if d.kind.ports_start_shut() {
            let _ = writeln!(s, " no shutdown");
        }
        let _ = writeln!(s, "!");
        for sub in &p.subifs {
            let _ = writeln!(s, "interface {}.{}", p.name, sub.number);
            if !sub.description.is_empty() {
                let _ = writeln!(s, " description {}", sub.description);
            }
            if let Some(v) = sub.vlan {
                let _ = writeln!(s, " encapsulation dot1Q {v}");
            }
            if let Some(ip) = sub.ip {
                let _ = writeln!(s, " ip address {} {}", ip.addr, ip.mask());
            }
            for h in &sub.helpers {
                let _ = writeln!(s, " ip helper-address {h}");
            }
            access_groups(&mut s, &sub.access);
            nat_side(&mut s, sub.nat);
            if sub.shutdown {
                let _ = writeln!(s, " shutdown");
            }
            let _ = writeln!(s, "!");
        }
    }
    for svi in &d.svis {
        let _ = writeln!(s, "interface Vlan{}", svi.vlan);
        if !svi.description.is_empty() {
            let _ = writeln!(s, " description {}", svi.description);
        }
        match svi.ip {
            Some(ip) => {
                let _ = writeln!(s, " ip address {} {}", ip.addr, ip.mask());
            }
            None => {
                let _ = writeln!(s, " no ip address");
            }
        }
        let _ = writeln!(s, "{}", if svi.shutdown { " shutdown" } else { " no shutdown" });
        let _ = writeln!(s, "!");
    }
    for a in &d.acls {
        if packetcraft_model::acl::numbered_kind(&a.name).is_some() {
            for e in &a.entries {
                let _ = writeln!(s, "access-list {} {}", a.name, e.text(a.extended));
            }
        } else {
            let _ = writeln!(s, "ip access-list {} {}", if a.extended { "extended" } else { "standard" }, a.name);
            for e in &a.entries {
                let _ = writeln!(s, " {}", e.text(a.extended));
            }
        }
        let _ = writeln!(s, "!");
    }
    for p in &d.nat.pools {
        let _ = writeln!(s, "ip nat pool {} {} {} netmask {}", p.name, p.start, p.end, packetcraft_core::prefix_to_mask(p.prefix));
    }
    for r in &d.nat.rules {
        let target = match &r.target {
            packetcraft_model::NatTarget::Interface(i) => format!("interface {i}"),
            packetcraft_model::NatTarget::Pool(p) => format!("pool {p}"),
        };
        let _ = writeln!(s, "ip nat inside source list {} {target}{}", r.acl, if r.overload { " overload" } else { "" });
    }
    for (l, g) in &d.nat.statics {
        let _ = writeln!(s, "ip nat inside source static {l} {g}");
    }
    if !d.nat.is_empty() {
        let _ = writeln!(s, "!");
    }
    if let Some(o) = &d.ospf {
        let _ = writeln!(s, "router ospf {}", o.process);
        if let Some(id) = o.router_id {
            let _ = writeln!(s, " router-id {id}");
        }
        for p in &o.passive {
            let _ = writeln!(s, " passive-interface {p}");
        }
        for n in &o.networks {
            let _ = writeln!(s, " network {} {} area {}", n.address, n.wildcard, n.area);
        }
        if o.default_originate {
            let _ = writeln!(s, " default-information originate");
        }
        let _ = writeln!(s, "!");
    }
    if let Some(r) = &d.rip {
        let _ = writeln!(s, "router rip");
        if r.version != 1 {
            let _ = writeln!(s, " version {}", r.version);
        }
        for p in &r.passive {
            let _ = writeln!(s, " passive-interface {p}");
        }
        for n in &r.networks {
            let _ = writeln!(s, " network {n}");
        }
        if r.default_originate {
            let _ = writeln!(s, " default-information originate");
        }
        if !r.auto_summary {
            let _ = writeln!(s, " no auto-summary");
        }
        let _ = writeln!(s, "!");
    }
    if d.kind == DeviceKind::Switch
        && let Some(g) = d.gateway
    {
        let _ = writeln!(s, "ip default-gateway {g}\n!");
    }
    for r in &d.routes6 {
        let via = [r.iface.clone(), r.next_hop.map(|a| a.to_string())].into_iter().flatten().collect::<Vec<_>>().join(" ");
        let _ = writeln!(s, "ipv6 route {} {via}", r.net);
    }
    for r in &d.routes {
        let via = match &r.via {
            NextHop::Address(a) => a.to_string(),
            NextHop::Interface(n) => n.clone(),
        };
        let _ = writeln!(s, "ip route {} {} {via}", r.net.addr, r.net.mask());
    }
    if !d.routes.is_empty() {
        let _ = writeln!(s, "!");
    }
    if !a.banner_motd.is_empty() {
        let _ = writeln!(s, "banner motd ^{}^\n!", a.banner_motd.replace('^', ""));
    }
    for (name, line) in [("line con 0", &a.console), ("line vty 0 4", &a.vty)] {
        if line.password.is_none() && !line.login && !line.login_local && line.transport.is_none() {
            continue;
        }
        let _ = writeln!(s, "{name}");
        if let Some(p) = &line.password {
            let _ = writeln!(s, " password {}", pw(p));
        }
        if line.login {
            let _ = writeln!(s, " login");
        }
        if line.login_local {
            let _ = writeln!(s, " login local");
        }
        if let Some(tr) = &line.transport {
            let names: Vec<&str> = tr.iter().map(|p| p.name()).collect();
            let _ = writeln!(s, " transport input {}", if names.is_empty() { "none".to_string() } else { names.join(" ") });
        }
        let _ = writeln!(s, "!");
    }
    s.push_str("end");
    s
}

fn yes(b: bool) -> &'static str {
    if b { "up" } else { "down" }
}

/// One row per interface: address, administrative state, link state.
pub fn ip_int_brief(t: &Topology, d: &Device) -> String {
    let mut s = format!("{:<24} {:<18} {:<7} {}\n", "Interface", "IP address", "Admin", "Link");
    for (i, p) in d.ports.iter().enumerate() {
        let up = port_up(t, End { device: d.id, port: i });
        let addr = p.ip.map_or("unassigned".to_string(), |ip| ip.to_string());
        let _ = writeln!(s, "{:<24} {:<18} {:<7} {}", p.name, addr, yes(!p.shutdown), yes(up));
        for sub in &p.subifs {
            let addr = sub.ip.map_or("unassigned".to_string(), |ip| ip.to_string());
            let _ = writeln!(
                s,
                "{:<24} {:<18} {:<7} {}",
                format!("{}.{}", p.name, sub.number),
                addr,
                yes(!sub.shutdown && !p.shutdown),
                yes(up && !sub.shutdown && sub.vlan.is_some())
            );
        }
    }
    let up_svis: Vec<String> = packetcraft_sim::up_ifaces(t, d).into_iter().filter(|i| i.port == packetcraft_sim::SVI_PORT).map(|i| i.name).collect();
    for svi in &d.svis {
        let name = format!("Vlan{}", svi.vlan);
        let addr = svi.ip.map_or("unassigned".to_string(), |ip| ip.to_string());
        let _ = writeln!(s, "{:<24} {:<18} {:<7} {}", name, addr, yes(!svi.shutdown), yes(up_svis.contains(&name)));
    }
    s.trim_end().to_string()
}

/// The routing table.
pub fn ip_route(t: &Topology, d: &Device) -> String {
    let table = routing_table(t, d);
    let mut s = String::from("Codes: C connected, L local, S static, R RIP, O OSPF, O*E2 OSPF external default\n\n");
    if d.kind == DeviceKind::Switch && !d.ip_routing {
        let _ = writeln!(
            s,
            "IP routing is off: this switch only reaches other networks through its default gateway ({})\n",
            d.gateway.map_or("not set".to_string(), |g| g.to_string())
        );
    }
    let default = table.iter().find(|r| r.net.prefix == 0);
    match default {
        Some(r) => {
            let via = r.via.map(|v| format!("via {v}")).or_else(|| r.iface.as_ref().map(|i| format!("out {i}"))).unwrap_or_default();
            let _ = writeln!(s, "Default route: {via}\n");
        }
        None => s.push_str("No default route\n\n"),
    }
    if table.is_empty() {
        s.push_str("(no routes: no interface is up with an address)");
        return s;
    }
    for r in &table {
        let what = match (r.code, r.via, &r.iface) {
            ('C', _, Some(i)) => format!("connected, {i}"),
            ('L', _, Some(i)) => format!("local address, {i}"),
            ('O' | 'E', Some(v), Some(i)) => format!("via {v}, {i} (OSPF)"),
            (_, Some(v), _) => format!("via {v}"),
            (_, None, Some(i)) => format!("out {i}"),
            _ => String::new(),
        };
        let code = match r.code {
            'E' => "O*E2".to_string(),
            c => format!("{c}   "),
        };
        let _ = writeln!(s, "{code} {:<20} {what}", r.net.to_string());
    }
    s.trim_end().to_string()
}

/// Interface details: all of them, or one.
pub fn interfaces(t: &Topology, d: &Device, only: Option<IfRef>) -> String {
    let mut s = String::new();
    for (i, p) in d.ports.iter().enumerate() {
        if only.is_some_and(|r| r.port != i) {
            continue;
        }
        let end = End { device: d.id, port: i };
        let link = if p.err_disabled {
            "down (err-disabled by port security)".to_string()
        } else if p.shutdown {
            "administratively down".to_string()
        } else if port_up(t, end) {
            "up".to_string()
        } else if t.peer(end).is_none() {
            "down (no cable)".to_string()
        } else {
            "down (far end shut down)".to_string()
        };
        if only.is_none_or(|r| r.sub.is_none()) {
            let _ = writeln!(s, "{} is {link}", p.name);
            let _ = writeln!(s, "  Hardware address {}", p.mac);
            if !p.description.is_empty() {
                let _ = writeln!(s, "  Description: {}", p.description);
            }
            if let Some(ip) = p.ip {
                let _ = writeln!(s, "  Internet address {ip}");
            }
            if let Some(peer) = t.peer(end) {
                let _ = writeln!(s, "  Cabled to {}", t.end_label(peer));
            }
            if d.kind == DeviceKind::Switch {
                let sp = &p.switchport;
                match sp.mode {
                    SwitchportMode::Access => {
                        let _ = writeln!(s, "  Switchport: access, VLAN {}", sp.access_vlan);
                    }
                    SwitchportMode::Trunk => {
                        let _ = writeln!(s, "  Switchport: trunk, native VLAN {}, allowed {}", sp.native_vlan, sp.allowed);
                    }
                }
            }
        }
        for sub in &p.subifs {
            if only.is_some_and(|r| r.sub.is_some_and(|n| n != sub.number)) {
                continue;
            }
            let state = if sub.shutdown || p.shutdown {
                "administratively down"
            } else if port_up(t, end) {
                "up"
            } else {
                "down"
            };
            let _ = writeln!(s, "{}.{} is {state}", p.name, sub.number);
            match sub.vlan {
                Some(v) => {
                    let _ = writeln!(s, "  802.1Q VLAN {v}");
                }
                None => s.push_str("  No encapsulation (carries nothing)\n"),
            }
            if let Some(ip) = sub.ip {
                let _ = writeln!(s, "  Internet address {ip}");
            }
        }
    }
    s.trim_end().to_string()
}

/// Trunk ports and what they carry.
pub fn trunks(t: &Topology, d: &Device) -> String {
    let mut s = format!("{:<10} {:<10} {:<8} {}\n", "Port", "Status", "Native", "Allowed (and existing)");
    let mut any = false;
    for (i, p) in d.ports.iter().enumerate() {
        if p.switchport.mode != SwitchportMode::Trunk {
            continue;
        }
        any = true;
        let status = if port_up(t, End { device: d.id, port: i }) { "trunking" } else { "down" };
        let mut active = VlanSet::none();
        for v in d.vlans.keys().filter(|v| p.switchport.allowed.contains(**v)) {
            active.insert(*v);
        }
        let _ = writeln!(s, "{:<10} {:<10} {:<8} {} ({})", ifname::short(&p.name), status, p.switchport.native_vlan, p.switchport.allowed, active);
    }
    if !any {
        s.push_str("(no trunk ports)");
    }
    s.trim_end().to_string()
}

/// The VLAN database with each VLAN's access ports.
pub fn vlans(d: &Device) -> String {
    let mut s = format!("{:<5} {:<32} {:<9} {}\n", "VLAN", "Name", "Status", "Access ports");
    for (v, name) in &d.vlans {
        let ports: Vec<String> = d
            .ports
            .iter()
            .filter(|p| p.switchport.mode == SwitchportMode::Access && p.switchport.access_vlan == *v)
            .map(|p| ifname::short(&p.name))
            .collect();
        let mut lines = ports.chunks(6).map(|c| c.join(", "));
        let _ = writeln!(s, "{:<5} {:<32} {:<9} {}", v.to_string(), name, "active", lines.next().unwrap_or_default());
        for l in lines {
            let _ = writeln!(s, "{:<48} {l}", "");
        }
    }
    // Ports left in a VLAN that was deleted carry nothing; say so.
    let orphans: Vec<String> = d
        .ports
        .iter()
        .filter(|p| p.switchport.mode == SwitchportMode::Access && !d.vlans.contains_key(&p.switchport.access_vlan))
        .map(|p| format!("{} (VLAN {})", ifname::short(&p.name), p.switchport.access_vlan))
        .collect();
    if !orphans.is_empty() {
        let _ = writeln!(s, "\nInactive (VLAN missing): {}", orphans.join(", "));
    }
    s.trim_end().to_string()
}

pub fn mac_table(t: &Topology, sim: &Sim, d: &Device) -> String {
    let mut s = format!("{:<6} {:<19} {:<9} {}\n", "VLAN", "MAC address", "Type", "Port");
    let mut n = 0;
    if let Some(st) = sim.state(d.id) {
        for ((vlan, mac), e) in &st.macs {
            if sim.now_us().saturating_sub(e.learned_us) >= packetcraft_sim::MAC_AGING_US {
                continue;
            }
            n += 1;
            let port = if d.ports.get(e.port).is_some() { packetcraft_sim::channel::port_label(t, d, e.port, true) } else { "?".to_string() };
            let _ = writeln!(s, "{:<6} {:<19} {:<9} {port}", vlan, mac.dotted(), "learned");
        }
    }
    let _ = write!(s, "{n} entr{}", if n == 1 { "y" } else { "ies" });
    s
}

pub fn arp(t: &Topology, sim: &Sim, d: &Device) -> String {
    let mut s = format!("{:<17} {:<10} {:<19} {}\n", "Address", "Age (min)", "MAC address", "Interface");
    let mut rows: Vec<(Ipv4Addr, String, String, String)> =
        packetcraft_sim::up_ifaces(t, d).into_iter().map(|i| (i.net.addr, "-".to_string(), i.mac.to_string(), i.name)).collect();
    if let Some(st) = sim.state(d.id) {
        for (ip, e) in &st.arp {
            let age = sim.now_us().saturating_sub(e.learned_us) / 60_000_000;
            rows.push((*ip, age.to_string(), e.mac.to_string(), e.iface.clone()));
        }
    }
    rows.sort();
    for (ip, age, mac, iface) in rows {
        let _ = writeln!(s, "{:<17} {:<10} {:<19} {iface}", ip.to_string(), age, mac);
    }
    s.trim_end().to_string()
}

pub fn version(d: &Device) -> String {
    format!(
        "PacketCraft simulated {}, software {}\n{} ports, base hardware address {}\nConfiguration {}",
        d.kind.label().to_lowercase(),
        env!("CARGO_PKG_VERSION"),
        d.ports.len(),
        d.ports.first().map_or("-".to_string(), |p| p.mac.to_string()),
        if d.startup.is_some() { "saved (startup-config present)" } else { "not saved" },
    )
}

pub fn ms(us: u64) -> String {
    if us < 1_000 { "<1".into() } else { format!("{}", us.div_ceil(1_000)) }
}

/// The router/switch ping report.
pub fn ping(dst: std::net::IpAddr, probes: &[Probe], opts: PingOptions) -> String {
    let marks: String = probes.iter().map(|p| p.mark()).collect();
    let ok: Vec<u64> = probes
        .iter()
        .filter_map(|p| match p {
            Probe::Reply { rtt_us, .. } => Some(*rtt_us),
            _ => None,
        })
        .collect();
    let icmp = if dst.is_ipv6() { "ICMPv6" } else { "ICMP" };
    let mut s = format!("Sending {} {icmp} echo requests to {dst}, {} s timeout:\n{marks}\n", probes.len(), opts.timeout_us / 1_000_000);
    let pct = ok.len() * 100 / probes.len().max(1);
    let _ = write!(s, "{pct}% answered ({}/{})", ok.len(), probes.len());
    if let (Some(min), Some(max)) = (ok.iter().min(), ok.iter().max()) {
        let avg = ok.iter().sum::<u64>() / ok.len() as u64;
        let _ = write!(s, ", round trip min/avg/max {}/{}/{} ms", ms(*min), ms(avg), ms(*max));
    }
    s.push_str("\nKey: ! reply  . timeout  U unreachable  T TTL exceeded");
    s
}

/// The router/switch traceroute report.
pub fn traceroute(dst: std::net::IpAddr, hops: &[Hop]) -> String {
    let mut s = format!("Tracing the route to {dst} (30 hops at most):\n");
    for h in hops {
        let from = h.probes.iter().find_map(|p| match p {
            Probe::Reply { from, .. } | Probe::TtlExceeded { from, .. } | Probe::Unreachable { from, .. } => Some(*from),
            Probe::Timeout => None,
        });
        let times: Vec<String> = h
            .probes
            .iter()
            .map(|p| match p {
                Probe::Reply { rtt_us, .. } | Probe::TtlExceeded { rtt_us, .. } => format!("{} ms", ms(*rtt_us)),
                Probe::Unreachable { .. } => "!U".into(),
                Probe::Timeout => "*".into(),
            })
            .collect();
        let _ = writeln!(s, "{:>3}  {:<16} {}", h.ttl, from.map_or("*".to_string(), |f| f.to_string()), times.join("  "));
    }
    s.trim_end().to_string()
}

/// `show ip dhcp binding`.
pub fn dhcp_bindings(sim: &Sim, d: &Device) -> String {
    let mut s = format!("{:<17} {:<19} {:<14} {}\n", "Address", "Client MAC", "Pool", "Since (s)");
    let mut n = 0;
    if let Some(st) = sim.state(d.id) {
        for (ip, l) in &st.leases {
            n += 1;
            let _ = writeln!(s, "{:<17} {:<19} {:<14} {}", ip.to_string(), l.mac.to_string(), l.pool, l.since_us / 1_000_000);
        }
    }
    let _ = write!(s, "{n} binding{}", if n == 1 { "" } else { "s" });
    s
}

/// `show ip dhcp pool`.
pub fn dhcp_pools(sim: &Sim, d: &Device) -> String {
    if d.dhcp_pools.is_empty() {
        return "No DHCP pools configured.".into();
    }
    let mut s = String::new();
    for p in &d.dhcp_pools {
        let leased = sim.state(d.id).map_or(0, |st| st.leases.values().filter(|l| l.pool == p.name).count());
        let _ = writeln!(s, "Pool {}", p.name);
        let _ = writeln!(s, "  Network          {}", p.network.map_or("not set (the pool hands out nothing)".to_string(), |n| n.to_string()));
        let _ = writeln!(s, "  Default router   {}", p.default_router.map_or("-".to_string(), |r| r.to_string()));
        let _ = writeln!(s, "  DNS server       {}", p.dns.map_or("-".to_string(), |r| r.to_string()));
        let _ = writeln!(s, "  Leased addresses {leased}");
    }
    if !d.dhcp_excluded.is_empty() {
        let ranges: Vec<String> = d.dhcp_excluded.iter().map(|(a, b)| if a == b { a.to_string() } else { format!("{a}-{b}") }).collect();
        let _ = writeln!(s, "Excluded: {}", ranges.join(", "));
    }
    s.trim_end().to_string()
}

/// `show spanning-tree` for one VLAN.
pub fn spanning_tree(t: &Topology, d: &Device, vlan: VlanId, tree: &packetcraft_sim::stp::VlanTree) -> String {
    let Some(me) = tree.bridges.get(&d.id) else { return format!("VLAN{:04}: this switch has no port in the tree", vlan.get()) };
    let mut s = format!("VLAN{:04}\n", vlan.get());
    let _ = writeln!(s, "  Root ID    Priority {:<6} Address {}", me.root.priority, me.root.mac.dotted());
    if me.root == me.id {
        s.push_str("             This switch is the root\n");
    } else {
        let rp = me.root_port.filter(|p| d.ports.get(*p).is_some()).map_or("-".to_string(), |p| packetcraft_sim::channel::port_label(t, d, p, true));
        let _ = writeln!(s, "             Cost {}, reached through {rp}", me.root_cost);
    }
    let _ = writeln!(s, "  Bridge ID  Priority {:<6} Address {}\n", me.id.priority, me.id.mac.dotted());
    let _ = writeln!(s, "{:<10} {:<4} {:<5} {:<6} {:<8} Type", "Interface", "Role", "State", "Cost", "Port ID");
    for ((dev, p), info) in tree.ports.iter().filter(|((dev, _), i)| *dev == d.id && i.role != packetcraft_sim::stp::Role::Disabled) {
        let _ = dev;
        let Some(port) = d.ports.get(*p) else { continue };
        let kind = if port.portfast {
            "Edge"
        } else if t.peer(End { device: d.id, port: *p }).and_then(|q| t.device(q.device)).is_some_and(|x| x.kind == DeviceKind::Switch) {
            "P2p"
        } else {
            "P2p (to host)"
        };
        let _ = writeln!(
            s,
            "{:<10} {:<4} {:<5} {:<6} {:<8} {kind}",
            packetcraft_sim::channel::port_label(t, d, *p, true),
            info.role.short(),
            info.role.state(),
            info.cost,
            format!("128.{}", p + 1)
        );
    }
    s.trim_end().to_string()
}

/// `show ip ospf neighbor`.
pub fn ospf_neighbors(t: &Topology, d: &Device, st: &packetcraft_sim::ospf::OspfState) -> String {
    if d.ospf.is_none() {
        return "OSPF is not running on this router.".into();
    }
    let list = st.neighbors.get(&d.id).cloned().unwrap_or_default();
    if list.is_empty() {
        return "No OSPF neighbours (check the network statements, passive interfaces and cables).".into();
    }
    let mut s = format!("{:<16} {:<10} {:<16} {:<22} Device\n", "Neighbour ID", "State", "Address", "Interface");
    for n in list {
        let name = t.device(n.device).map_or("?".to_string(), |x| x.name.clone());
        let _ = writeln!(s, "{:<16} {:<10} {:<16} {:<22} {name}", n.router_id.to_string(), "FULL", n.address.to_string(), ifname::short(&n.iface));
    }
    s.trim_end().to_string()
}

/// `show ip protocols`.
pub fn ip_protocols(d: &Device, st: &packetcraft_sim::ospf::OspfState, rs: &packetcraft_sim::rip::RipState) -> String {
    let mut parts = Vec::new();
    if d.ospf.is_some() {
        parts.push(ospf_protocol(d, st));
    }
    if let Some(r) = &d.rip {
        let mut s = format!("Routing protocol: RIP version {}\n", r.version);
        s.push_str("  Networks:\n");
        for n in &r.networks {
            let _ = writeln!(s, "    {n}");
        }
        if !r.passive.is_empty() {
            let _ = writeln!(s, "  Passive interfaces: {}", r.passive.iter().map(|p| ifname::short(p)).collect::<Vec<_>>().join(", "));
        }
        let ifs = rs.interfaces.get(&d.id).cloned().unwrap_or_default();
        let _ = writeln!(
            s,
            "  Interfaces running RIP: {}",
            if ifs.is_empty() { "none".to_string() } else { ifs.iter().map(|(n, _, _)| ifname::short(n)).collect::<Vec<_>>().join(", ") }
        );
        let heard: Vec<String> = rs.neighbors.get(&d.id).into_iter().flatten().map(|(_, a, i)| format!("{a} on {}", ifname::short(i))).collect();
        let _ = writeln!(s, "  Neighbours heard: {}", if heard.is_empty() { "none".to_string() } else { heard.join(", ") });
        if r.default_originate {
            s.push_str("  Advertises its default route\n");
        }
        let _ = write!(
            s,
            "  Automatic summarisation {}; administrative distance 120",
            if r.auto_summary { "on (not simulated: subnets are advertised)" } else { "off" }
        );
        parts.push(s);
    }
    if parts.is_empty() {
        return "No routing protocol is running (static routes only).".into();
    }
    parts.join("\n\n")
}

fn ospf_protocol(d: &Device, st: &packetcraft_sim::ospf::OspfState) -> String {
    let Some(o) = &d.ospf else { return String::new() };
    let mut s = format!("Routing protocol: OSPF, process {}\n", o.process);
    let _ = writeln!(s, "  Router id: {}", st.router_ids.get(&d.id).map_or("none (no interface address)".to_string(), |i| i.to_string()));
    s.push_str("  Networks:\n");
    for n in &o.networks {
        let _ = writeln!(s, "    {} {} area {}", n.address, n.wildcard, n.area);
    }
    if !o.passive.is_empty() {
        let _ = writeln!(s, "  Passive interfaces: {}", o.passive.iter().map(|p| ifname::short(p)).collect::<Vec<_>>().join(", "));
    }
    let ifs = st.interfaces.get(&d.id).cloned().unwrap_or_default();
    let _ = writeln!(
        s,
        "  Interfaces running OSPF: {}",
        if ifs.is_empty() { "none".to_string() } else { ifs.iter().map(|(n, _, _, _)| ifname::short(n)).collect::<Vec<_>>().join(", ") }
    );
    if o.default_originate {
        s.push_str("  Advertises its default route\n");
    }
    let _ = write!(s, "  Administrative distance 110");
    s
}

fn nat_side(s: &mut String, side: Option<packetcraft_model::NatSide>) {
    match side {
        Some(packetcraft_model::NatSide::Inside) => s.push_str(" ip nat inside\n"),
        Some(packetcraft_model::NatSide::Outside) => s.push_str(" ip nat outside\n"),
        None => {}
    }
}

fn access_groups(s: &mut String, g: &packetcraft_model::AccessGroups) {
    if let Some(n) = &g.inbound {
        let _ = writeln!(s, " ip access-group {n} in");
    }
    if let Some(n) = &g.outbound {
        let _ = writeln!(s, " ip access-group {n} out");
    }
}

/// `show access-lists`.
pub fn access_lists(sim: &Sim, d: &Device) -> String {
    if d.acls.is_empty() {
        return "No access lists configured.".into();
    }
    let hits = sim.state(d.id).map(|st| st.acl_hits.clone()).unwrap_or_default();
    let mut s = String::new();
    for a in &d.acls {
        let _ = writeln!(s, "{} access list {}", if a.extended { "Extended" } else { "Standard" }, a.name);
        for (i, e) in a.entries.iter().enumerate() {
            let n = hits.get(&(a.name.clone(), i)).copied().unwrap_or(0);
            let m = if n > 0 { format!(" ({n} match{})", if n == 1 { "" } else { "es" }) } else { String::new() };
            let _ = writeln!(s, "    {} {}{m}", (i + 1) * 10, e.text(a.extended));
        }
        let implicit = hits.get(&(a.name.clone(), usize::MAX)).copied().unwrap_or(0);
        let _ = writeln!(s, "    (implicit) deny everything else{}", if implicit > 0 { format!(" ({implicit} matches)") } else { String::new() });
    }
    s.trim_end().to_string()
}

/// `show ip nat translations`.
pub fn nat_translations(sim: &Sim, d: &Device) -> String {
    let fmt = |a: Ipv4Addr, id: Option<u16>| id.map_or(a.to_string(), |i| format!("{a}:{i}"));
    let mut rows: Vec<String> = Vec::new();
    for (l, g) in &d.nat.statics {
        rows.push(format!("{:<6} {:<22} {:<22} {}", "---", g.to_string(), l.to_string(), "---"));
    }
    if let Some(st) = sim.state(d.id) {
        for e in st.nat.iter().filter(|e| !e.fixed || e.local_id.is_some()) {
            let proto = match e.protocol {
                1 => "icmp",
                17 => "udp",
                6 => "tcp",
                _ => "ip",
            };
            rows.push(format!("{proto:<6} {:<22} {:<22} {}", fmt(e.inside_global, e.global_id), fmt(e.inside_local, e.local_id), e.outside));
        }
    }
    if rows.is_empty() {
        return "No translations.".into();
    }
    let mut s = format!("{:<6} {:<22} {:<22} {}\n", "Proto", "Inside global", "Inside local", "Outside");
    s.push_str(&rows.join("\n"));
    s
}

/// `show port-security`.
pub fn port_security(sim: &Sim, d: &Device) -> String {
    let secured: Vec<(usize, &packetcraft_model::Port)> = d.ports.iter().enumerate().filter(|(_, p)| p.port_security.is_some()).collect();
    if secured.is_empty() {
        return "No port has port security.".into();
    }
    let st = sim.state(d.id);
    let mut s = format!("{:<10} {:<8} {:<9} {:<11} {:<10} {}\n", "Port", "Max", "Current", "Violations", "Action", "State");
    for (i, p) in secured {
        let Some(ps) = &p.port_security else { continue };
        let learned = st.and_then(|x| x.secure.get(&i)).map_or(0, Vec::len);
        let v = st.and_then(|x| x.violations.get(&i)).copied().unwrap_or(0);
        let state = if p.err_disabled {
            "err-disabled"
        } else if p.shutdown {
            "shut down"
        } else {
            "secure-up"
        };
        let _ =
            writeln!(s, "{:<10} {:<8} {:<9} {:<11} {:<10} {state}", ifname::short(&p.name), ps.max, ps.macs.len() + learned, v, ps.violation.name());
        for m in &ps.macs {
            let _ = writeln!(s, "{:<10} {} ({})", "", m.dotted(), if ps.sticky { "sticky" } else { "configured" });
        }
    }
    s.trim_end().to_string()
}

/// `show etherchannel summary`.
pub fn etherchannel(t: &Topology, d: &Device) -> String {
    let chans = packetcraft_sim::channel::channels(t, d);
    if chans.is_empty() {
        return "No EtherChannel on this switch.".into();
    }
    let mut s = String::new();
    for c in &chans {
        let state = if c.is_up() { "up" } else { "down" };
        let _ = writeln!(s, "Port-channel{} ({}), {state}, {} of {} ports bundled", c.number, c.protocol(d), c.bundled().len(), c.members.len());
        for (p, st) in &c.members {
            let Some(port) = d.ports.get(*p) else { continue };
            let mode = port.channel.map_or("", |g| g.mode.name());
            let _ = writeln!(s, "  {:<9} mode {:<10} {}", ifname::short(&port.name), mode, st.name());
        }
    }
    s.trim_end().to_string()
}

/// `show vtp status`.
pub fn vtp(d: &Device) -> String {
    let v = &d.vtp;
    let domain = if v.domain.is_empty() { "(none yet: joins the first domain it hears on a trunk)" } else { v.domain.as_str() };
    let mut s = String::new();
    let _ = writeln!(s, "Mode                {}", v.mode.name());
    let _ = writeln!(s, "Domain              {domain}");
    let _ = writeln!(s, "Version             {}", v.version);
    let _ = writeln!(s, "Revision            {}", v.revision);
    let _ = writeln!(s, "Password            {}", if v.password.is_some() { "set" } else { "not set" });
    let _ = write!(s, "VLANs               {}", d.vlans.len());
    s
}

/// `show ipv6 interface brief`.
pub fn ipv6_brief(t: &Topology, d: &Device) -> String {
    let mut s = format!("{:<24} {:<22} {}\n", "Interface", "Status", "IPv6 addresses");
    let ifs = packetcraft_sim::v6::l3_ifaces6(d);
    for (i, p) in d.ports.iter().enumerate() {
        let status = if p.shutdown {
            "administratively down"
        } else if port_up(t, End { device: d.id, port: i }) {
            "up"
        } else {
            "down"
        };
        let _ = write!(s, "{:<24} {:<22} ", p.name, status);
        match ifs.iter().find(|x| x.port == i) {
            Some(x) => {
                let _ = writeln!(s, "{} (link-local)", x.link_local);
                for a in &x.addresses {
                    let _ = writeln!(s, "{:<47} {a}", "");
                }
            }
            None => {
                let _ = writeln!(s, "IPv6 off");
            }
        }
    }
    s.trim_end().to_string()
}

/// `show ipv6 route`.
pub fn ipv6_route(t: &Topology, d: &Device) -> String {
    let table = packetcraft_sim::v6::routing_table6(t, d);
    let mut s = String::from("Codes: C connected, L local, S static\n");
    if !d.ipv6_routing {
        s.push_str("IPv6 forwarding is off (no `ipv6 unicast-routing`)\n");
    }
    for r in &table {
        let via = match (&r.next_hop, &r.iface) {
            (Some(nh), Some(i)) => format!("via {nh}, {i}"),
            (Some(nh), None) => format!("via {nh}"),
            (None, Some(i)) if r.code == 'S' => format!("out of {i}"),
            (None, Some(i)) => format!("directly on {i}"),
            (None, None) => String::new(),
        };
        let _ = writeln!(s, "{}  {:<28} {via}", r.code, r.net.to_string());
    }
    if table.is_empty() {
        s.push_str("(no IPv6 routes)");
    }
    s.trim_end().to_string()
}

/// `show ipv6 neighbors`.
pub fn ipv6_neighbors(sim: &Sim, d: &Device) -> String {
    let mut s = format!("{:<28} {:<19} {}\n", "IPv6 address", "MAC address", "Interface");
    let entries = sim.state(d.id).map(|st| st.nd.iter().collect::<Vec<_>>()).unwrap_or_default();
    for (ip, e) in &entries {
        let _ = writeln!(s, "{:<28} {:<19} {}", ip.to_string(), e.mac.dotted(), ifname::short(&e.iface));
    }
    if entries.is_empty() {
        s.push_str("(no neighbours yet)");
    }
    s.trim_end().to_string()
}

/// `show ip rip database`.
pub fn rip_database(t: &Topology, d: &Device, st: &packetcraft_sim::rip::RipState) -> String {
    if d.rip.is_none() {
        return "RIP is not running on this router.".into();
    }
    let mut s = format!("{:<20} {:<6} {:<16} {}\n", "Network", "Hops", "Next hop", "Interface");
    for (name, net, _) in st.interfaces.get(&d.id).into_iter().flatten() {
        let _ = writeln!(s, "{:<20} {:<6} {:<16} {}", net.to_string(), 0, "directly", ifname::short(name));
    }
    for r in st.routes.get(&d.id).into_iter().flatten() {
        let _ = writeln!(s, "{:<20} {:<6} {:<16} {}", r.net.to_string(), r.hops, r.via.to_string(), ifname::short(&r.iface));
    }
    let _ = t;
    s.trim_end().to_string()
}
