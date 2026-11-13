//! What `show` commands print, and the configuration text. Layouts are NetLabCraft's own.

use std::fmt::Write as _;
use std::net::Ipv4Addr;

use netlabcraft_core::{Ipv4Net, VlanId, VlanSet};
use netlabcraft_model::{Device, DeviceKind, End, NextHop, SwitchportMode, Topology, ifname, obscure};
use netlabcraft_sim::{Hop, PingOptions, Probe, Sim, port_up};

use crate::IfRef;

/// The running configuration as re-playable command lines.
pub fn running_config(d: &Device) -> String {
    if netlabcraft_model::firewall::is_firewall(d) {
        return crate::exec::firewall::running_config(d);
    }
    let mut s = String::new();
    let a = &d.access;
    let pw = |p: &str| if a.password_encryption { format!("7 {}", obscure(p)) } else { p.to_string() };
    let _ = writeln!(s, "! NetLabCraft configuration of {}", d.name);
    if a.password_encryption {
        let _ = writeln!(s, "service password-encryption");
    }
    let _ = writeln!(s, "!\nhostname {}\n!", d.name);
    if d.ip_routing {
        let _ = writeln!(s, "ip routing\n!");
    }
    if d.cdp_off {
        let _ = writeln!(s, "no cdp run\n!");
    }
    if d.lldp_on {
        let _ = writeln!(s, "lldp run\n!");
    }
    // Servers with a key are written with it (`ntp server A key N`) among the NTP lines below.
    for a in d.ntp_servers.iter().filter(|a| !d.mgmt.ntp_server_keys.iter().any(|(x, _)| x == *a)) {
        let _ = writeln!(s, "ntp server {a}");
    }
    for a in &d.log_hosts {
        let _ = writeln!(s, "logging host {a}");
    }
    let before = s.len();
    crate::exec::mgmt::running_config(&mut s, d);
    if !d.ntp_servers.is_empty() || !d.log_hosts.is_empty() || s.len() != before {
        let _ = writeln!(s, "!");
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
        let level = if u.privilege == 1 { String::new() } else { format!(" privilege {}", u.privilege) };
        if u.hashed {
            let _ = writeln!(s, "username {}{level} secret 5 {}", u.name, u.credential);
        } else {
            let _ = writeln!(s, "username {}{level} password {}", u.name, pw(&u.credential));
        }
    }
    if let Some(b) = a.rsa_bits {
        let _ = writeln!(s, "crypto key generate rsa general-keys modulus {b}");
    }
    if let Some(v) = a.ssh_version {
        let _ = writeln!(s, "ip ssh version {v}");
    }
    if let Some(n) = a.ssh_timeout {
        let _ = writeln!(s, "ip ssh time-out {n}");
    }
    if let Some(n) = a.ssh_retries {
        let _ = writeln!(s, "ip ssh authentication-retries {n}");
    }
    if let Some(n) = a.min_password_len {
        let _ = writeln!(s, "security passwords min-length {n}");
    }
    if let Some((b, n, w)) = a.login_block {
        let _ = writeln!(s, "login block-for {b} attempts {n} within {w}");
    }
    if a.aaa {
        let _ = writeln!(s, "aaa new-model");
        if !a.login_methods.is_empty() {
            let m: Vec<&str> = a
                .login_methods
                .iter()
                .map(|m| match m {
                    netlabcraft_model::LoginMethod::Radius => "group radius",
                    netlabcraft_model::LoginMethod::Tacacs => "group tacacs+",
                    netlabcraft_model::LoginMethod::Local => "local",
                })
                .collect();
            let _ = writeln!(s, "aaa authentication login default {}", m.join(" "));
        }
    }
    for (h, k) in &a.radius_servers {
        let _ = writeln!(s, "radius-server host {h} key {}", pw(k));
    }
    for (h, k) in &a.tacacs_servers {
        let _ = writeln!(s, "tacacs-server host {h} key {}", pw(k));
    }
    for (n, rw) in &d.snmp.communities {
        let _ = writeln!(s, "snmp-server community {n} {}", if *rw { "RW" } else { "RO" });
    }
    if !d.snmp.location.is_empty() {
        let _ = writeln!(s, "snmp-server location {}", d.snmp.location);
    }
    if !d.snmp.contact.is_empty() {
        let _ = writeln!(s, "snmp-server contact {}", d.snmp.contact);
    }
    for g in &d.snmp.groups {
        let _ = writeln!(
            s,
            "snmp-server group {} v3 {}{}",
            g.name,
            if g.auth { "auth" } else { "noauth" },
            if g.write { " write v1default" } else { "" }
        );
    }
    for u in &d.snmp.users {
        match &u.auth {
            // Like real devices with password encryption on, the line isn't shown at all then.
            Some(_) if a.password_encryption => {}
            Some((h, p)) => {
                let _ = writeln!(s, "snmp-server user {} {} v3 auth {h} {p}", u.name, u.group);
            }
            None => {
                let _ = writeln!(s, "snmp-server user {} {} v3", u.name, u.group);
            }
        }
    }
    for (h, comm) in &d.snmp.trap_hosts {
        let _ = writeln!(s, "snmp-server host {h} version 2c {comm}");
    }
    if !d.snmp.traps.is_empty() {
        let _ = writeln!(s, "snmp-server enable traps snmp {}", d.snmp.traps.join(" "));
    }
    if let Some((u, p)) = &d.ftp_login {
        if !u.is_empty() {
            let _ = writeln!(s, "ip ftp username {u}");
        }
        if !p.is_empty() {
            let _ = writeln!(s, "ip ftp password {}", pw(p));
        }
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
        if let Some(dn) = &p.domain {
            let _ = writeln!(s, " domain-name {dn}");
        }
        match p.lease_secs {
            Some(u32::MAX) => {
                let _ = writeln!(s, " lease infinite");
            }
            Some(l) => {
                let (dd, h, m) = (l / 86_400, l / 3_600 % 24, l / 60 % 60);
                let _ = writeln!(
                    s,
                    " lease {dd}{}",
                    if m > 0 {
                        format!(" {h} {m}")
                    } else if h > 0 {
                        format!(" {h}")
                    } else {
                        String::new()
                    }
                );
            }
            None => {}
        }
        crate::exec::voice::pool_lines(&mut s, d, &p.name);
        let _ = writeln!(s, "!");
    }
    crate::exec::voice::running_config(&mut s, d);
    crate::exec::families::running_config(&mut s, d);
    if d.rapid_stp {
        let _ = writeln!(s, "spanning-tree mode rapid-pvst");
    }
    for (v, p) in &d.stp_priority {
        let _ = writeln!(s, "spanning-tree vlan {v} priority {p}");
    }
    if !d.stp_priority.is_empty() {
        let _ = writeln!(s, "!");
    }
    crate::exec::l2sec::global_lines(&mut s, d);
    crate::exec::l2sec::relay_global_lines(&mut s, d);
    crate::exec::fhrp::global_lines(&mut s, d);
    // VLANs live in the VLAN database (vlan.dat), not in the configuration: only a VTP
    // transparent switch writes them there.
    if d.kind == DeviceKind::Switch && d.vtp.mode == netlabcraft_model::VtpMode::Transparent {
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
    crate::exec::qos::running_config(&mut s, d);
    vpn_config(&mut s, d);
    crate::exec::security::running_config(&mut s, d);
    wlan_config(&mut s, d);
    crate::exec::cellular::running_config(&mut s, d);
    crate::exec::routing::loopback_config(&mut s, d);
    for p in &d.ports {
        let _ = writeln!(s, "interface {}", p.name);
        if !p.description.is_empty() {
            let _ = writeln!(s, " description {}", p.description);
        }
        if p.cdp_off {
            let _ = writeln!(s, " no cdp enable");
        }
        if let Some(sp) = p.speed {
            let _ = writeln!(s, " speed {sp}");
        }
        match p.duplex {
            Some(netlabcraft_model::Duplex::Full) => {
                let _ = writeln!(s, " duplex full");
            }
            Some(netlabcraft_model::Duplex::Half) => {
                let _ = writeln!(s, " duplex half");
            }
            None => {}
        }
        if p.mdix_off {
            let _ = writeln!(s, " no mdix auto");
        }
        if p.proxy_arp_off {
            let _ = writeln!(s, " no ip proxy-arp");
        }
        if p.lldp_tx_off {
            let _ = writeln!(s, " no lldp transmit");
        }
        if p.lldp_rx_off {
            let _ = writeln!(s, " no lldp receive");
        }
        let l2 = d.ports.iter().position(|x| x.name == p.name).is_some_and(|i| d.is_switchport(i));
        if d.kind == DeviceKind::Switch && p.routed {
            let _ = writeln!(s, " no switchport");
        }
        if l2 {
            let sp = &p.switchport;
            if sp.access_vlan != VlanId::DEFAULT {
                let _ = writeln!(s, " switchport access vlan {}", sp.access_vlan);
            }
            if let Some(v) = sp.voice_vlan {
                let _ = writeln!(s, " switchport voice vlan {v}");
            }
            if sp.native_vlan != VlanId::DEFAULT {
                let _ = writeln!(s, " switchport trunk native vlan {}", sp.native_vlan);
            }
            if !sp.allowed.is_all() {
                let _ = writeln!(s, " switchport trunk allowed vlan {}", sp.allowed);
            }
            if sp.mode != SwitchportMode::Access {
                let _ = writeln!(s, " switchport mode {}", sp.mode.name());
            }
            if sp.nonegotiate {
                let _ = writeln!(s, " switchport nonegotiate");
            }
            if let Some(ps) = &p.port_security {
                let _ = writeln!(s, " switchport port-security");
                if ps.max != 1 {
                    let _ = writeln!(s, " switchport port-security maximum {}", ps.max);
                }
                if ps.violation != netlabcraft_model::Violation::Shutdown {
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
            crate::exec::l2sec::port_lines(&mut s, p);
            crate::exec::families::port_lines(&mut s, p);
        }
        match p.ip {
            // The leased address isn't configuration; how it is obtained is.
            _ if p.dhcp => {
                let _ = writeln!(s, " ip address dhcp");
            }
            Some(ip) => {
                let _ = writeln!(s, " ip address {} {}", ip.addr, ip.mask());
            }
            None if d.kind == DeviceKind::Router && !l2 => {
                let _ = writeln!(s, " no ip address");
            }
            None => {}
        }
        standby_lines(&mut s, d, &p.name, &p.standby);
        crate::exec::fhrp::iface_lines(&mut s, d, &p.name);
        ospf_if_lines(&mut s, d, &p.name);
        if d.kind == DeviceKind::Router {
            if netlabcraft_model::ifname::is_serial(&p.name) {
                if p.serial.encapsulation == netlabcraft_model::Encapsulation::Ppp {
                    let _ = writeln!(s, " encapsulation ppp");
                    if !p.serial.ppp_auth.is_empty() {
                        let _ = writeln!(s, " ppp authentication {}", p.serial.ppp_auth.iter().map(|m| m.name()).collect::<Vec<_>>().join(" "));
                    }
                    if let Some((u, pw)) = &p.serial.pap_sent {
                        let _ = writeln!(s, " ppp pap sent-username {u} password {pw}");
                    }
                }
                if let Some(r) = p.serial.clock_rate {
                    let _ = writeln!(s, " clock rate {r}");
                }
            }
            ipv6_lines(&mut s, &p.ipv6);
        }
        for h in &p.helpers {
            let _ = writeln!(s, " ip helper-address {h}");
        }
        crate::exec::l2sec::relay_port_lines(&mut s, p);
        access_groups(&mut s, &p.access);
        nat_side(&mut s, p.nat);
        if let Some(b) = p.bandwidth {
            let _ = writeln!(s, " bandwidth {b}");
        }
        if let Some(dl) = p.delay {
            let _ = writeln!(s, " delay {dl}");
        }
        crypto_map_line(&mut s, d, &p.name);
        crate::exec::qos::service_policy_lines(&mut s, d, &p.name);
        crate::exec::security::iface_lines(&mut s, d, &p.name);
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
            ipv6_lines(&mut s, &sub.ipv6);
            standby_lines(&mut s, d, &format!("{}.{}", p.name, sub.number), &sub.standby);
            crate::exec::fhrp::iface_lines(&mut s, d, &format!("{}.{}", p.name, sub.number));
            ospf_if_lines(&mut s, d, &format!("{}.{}", p.name, sub.number));
            for h in &sub.helpers {
                let _ = writeln!(s, " ip helper-address {h}");
            }
            access_groups(&mut s, &sub.access);
            nat_side(&mut s, sub.nat);
            crypto_map_line(&mut s, d, &format!("{}.{}", p.name, sub.number));
            crate::exec::qos::service_policy_lines(&mut s, d, &format!("{}.{}", p.name, sub.number));
            crate::exec::security::iface_lines(&mut s, d, &format!("{}.{}", p.name, sub.number));
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
        ipv6_lines(&mut s, &svi.ipv6);
        for h in &svi.helpers {
            let _ = writeln!(s, " ip helper-address {h}");
        }
        access_groups(&mut s, &svi.access);
        nat_side(&mut s, svi.nat);
        standby_lines(&mut s, d, &format!("Vlan{}", svi.vlan), &svi.standby);
        crate::exec::fhrp::iface_lines(&mut s, d, &format!("Vlan{}", svi.vlan));
        ospf_if_lines(&mut s, d, &format!("Vlan{}", svi.vlan));
        crate::exec::qos::service_policy_lines(&mut s, d, &format!("Vlan{}", svi.vlan));
        let _ = writeln!(s, "{}", if svi.shutdown { " shutdown" } else { " no shutdown" });
        let _ = writeln!(s, "!");
    }
    for a in &d.acls {
        if netlabcraft_model::acl::numbered_kind(&a.name).is_some() {
            for r in &a.remarks {
                let _ = writeln!(s, "access-list {} remark {r}", a.name);
            }
            for e in &a.entries {
                let _ = writeln!(s, "access-list {} {}", a.name, e.text(a.extended));
            }
        } else {
            let _ = writeln!(s, "ip access-list {} {}", if a.extended { "extended" } else { "standard" }, a.name);
            for r in &a.remarks {
                let _ = writeln!(s, " remark {r}");
            }
            for e in &a.entries {
                let _ = writeln!(s, " {}", e.text(a.extended));
            }
        }
        let _ = writeln!(s, "!");
    }
    for p in &d.dhcp6_pools {
        let _ = writeln!(s, "ipv6 dhcp pool {}", p.name);
        if let Some(n) = p.prefix {
            let _ = writeln!(s, " address prefix {n}");
        }
        for a in &p.dns {
            let _ = writeln!(s, " dns-server {a}");
        }
        if let Some(dn) = &p.domain {
            let _ = writeln!(s, " domain-name {dn}");
        }
        let _ = writeln!(s, "!");
    }
    for a in &d.acls6 {
        let _ = writeln!(s, "ipv6 access-list {}", a.name);
        for e in &a.entries {
            let _ = writeln!(s, " {}", e.text());
        }
        let _ = writeln!(s, "!");
    }
    for p in &d.nat.pools {
        let _ = writeln!(s, "ip nat pool {} {} {} netmask {}", p.name, p.start, p.end, netlabcraft_core::prefix_to_mask(p.prefix));
    }
    for r in &d.nat.rules {
        let target = match &r.target {
            netlabcraft_model::NatTarget::Interface(i) => format!("interface {i}"),
            netlabcraft_model::NatTarget::Pool(p) => format!("pool {p}"),
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
        if o.passive_default {
            s.push_str(" passive-interface default\n");
            for p in &o.not_passive {
                let _ = writeln!(s, " no passive-interface {p}");
            }
        }
        for n in &o.networks {
            let _ = writeln!(s, " network {} {} area {}", n.address, n.wildcard, n.area);
        }
        if o.redistribute_static {
            s.push_str(" redistribute static subnets\n");
        }
        if o.default_originate {
            let _ = writeln!(s, " default-information originate");
        }
        if let Some(r) = o.reference_bw {
            let _ = writeln!(s, " auto-cost reference-bandwidth {r}");
        }
        if let Some(n) = o.maximum_paths {
            let _ = writeln!(s, " maximum-paths {n}");
        }
        for (n, a) in &o.areas {
            if a.stub {
                let _ = writeln!(s, " area {n} stub{}", if a.no_summary { " no-summary" } else { "" });
            }
            for r in &a.ranges {
                let _ = writeln!(s, " area {n} range {} {}", r.addr, r.mask());
            }
        }
        let _ = writeln!(s, "!");
    }
    if let Some(e) = &d.eigrp {
        let _ = writeln!(s, "router eigrp {}", e.asn);
        if let Some(id) = e.router_id {
            let _ = writeln!(s, " eigrp router-id {id}");
        }
        for p in &e.passive {
            let _ = writeln!(s, " passive-interface {p}");
        }
        for n in &e.networks {
            match n.wildcard {
                Some(w) => {
                    let _ = writeln!(s, " network {} {w}", n.address);
                }
                None => {
                    let _ = writeln!(s, " network {}", n.address);
                }
            }
        }
        if let Some(n) = e.variance {
            let _ = writeln!(s, " variance {n}");
        }
        if let Some(n) = e.maximum_paths {
            let _ = writeln!(s, " maximum-paths {n}");
        }
        let _ = writeln!(s, "{}", if e.auto_summary { " auto-summary" } else { " no auto-summary" });
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
        if let Some([u, i, h, f]) = r.timers {
            let _ = writeln!(s, " timers basic {u} {i} {h} {f}");
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
        let distance = r.distance.map_or(String::new(), |a| format!(" {a}"));
        let _ = writeln!(s, "ipv6 route {} {via}{distance}", r.net);
    }
    crate::exec::eigrp6::running_config(&mut s, d);
    crate::exec::swroute::rip6_running_config(&mut s, d);
    if let Some(o) = &d.ospf6 {
        let _ = writeln!(s, "ipv6 router ospf {}", o.process);
        if let Some(id) = o.router_id {
            let _ = writeln!(s, " router-id {id}");
        }
        for p in &o.passive {
            let _ = writeln!(s, " passive-interface {p}");
        }
        if o.default_originate {
            let _ = writeln!(s, " default-information originate");
        }
        let _ = writeln!(s, "!");
    }
    for r in &d.routes {
        let via = match &r.via {
            NextHop::Address(a) => a.to_string(),
            NextHop::Interface(n) => n.clone(),
        };
        let distance = r.distance.map_or(String::new(), |a| format!(" {a}"));
        let _ = writeln!(s, "ip route {} {} {via}{distance}", r.net.addr, r.net.mask());
    }
    if !d.routes.is_empty() {
        let _ = writeln!(s, "!");
    }
    crate::exec::automation::running_config(&mut s, d);
    if !a.banner_motd.is_empty() {
        let _ = writeln!(s, "banner motd ^{}^\n!", a.banner_motd.replace('^', ""));
    }
    // The line blocks are always written, configured or not, as on real devices.
    for (name, line) in [("line con 0", &a.console), ("line vty 0 4", &a.vty)] {
        let _ = writeln!(s, "{name}");
        if let Some(n) = d.mgmt.history_size {
            let _ = writeln!(s, " history size {n}");
        }
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
        if let Some(a) = &line.access_class {
            let _ = writeln!(s, " access-class {a} in");
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
    let up_svis: Vec<String> = netlabcraft_sim::up_ifaces(t, d).into_iter().filter(|i| i.port == netlabcraft_sim::SVI_PORT).map(|i| i.name).collect();
    for svi in &d.svis {
        let name = format!("Vlan{}", svi.vlan);
        let addr = svi.ip.map_or("unassigned".to_string(), |ip| ip.to_string());
        let _ = writeln!(s, "{:<24} {:<18} {:<7} {}", name, addr, yes(!svi.shutdown), yes(up_svis.contains(&name)));
    }
    for tun in &d.tunnels {
        let addr = tun.ip.map_or("unassigned".to_string(), |ip| ip.to_string());
        let up = netlabcraft_sim::vpn::tunnel_status(t, d, tun.number).is_ok();
        let _ = writeln!(s, "{:<24} {:<18} {:<7} {}", tun.name(), addr, yes(!tun.shutdown), yes(up));
    }
    crate::exec::routing::loopback_brief(&mut s, d);
    s.trim_end().to_string()
}

/// The routing table, from the rows the simulation has (a row per path when several share the
/// traffic).
pub fn ip_route_table(d: &Device, table: &[netlabcraft_sim::RouteEntry]) -> String {
    let mut s = String::from(
        "Codes: C connected, L local, S static, R RIP, D EIGRP, O OSPF, O IA OSPF other area, O E2 OSPF external, O*E2 OSPF external default, O*IA default into a stub area, * the default route\n\n",
    );
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
    for r in table {
        let what = match (r.code, r.via, &r.iface) {
            ('C', _, Some(i)) => format!("connected, {i}"),
            ('L', _, Some(i)) => format!("local address, {i}"),
            ('O' | 'E' | 'I', Some(v), Some(i)) => format!("via {v}, {i} (OSPF)"),
            ('D', Some(v), Some(i)) => format!("via {v}, {i} (EIGRP)"),
            ('D', None, Some(i)) => format!("summary, discarded on {i} (EIGRP)"),
            (_, Some(v), _) => format!("via {v}"),
            (_, None, Some(i)) => format!("out {i}"),
            _ => String::new(),
        };
        let code = match r.code {
            'E' if r.net.prefix == 0 => "O*E2".to_string(),
            'E' => "O E2".to_string(),
            'I' if r.net.prefix == 0 => "O*IA".to_string(),
            'I' => "O IA".to_string(),
            // The default route is starred whoever put it there (S*, R*, D*).
            c if r.net.prefix == 0 => format!("{c}*  "),
            c => format!("{c}   "),
        };
        // Distance and metric, for every route a protocol or a person put there.
        let ad = if matches!(r.code, 'C' | 'L') { String::new() } else { format!(" [{}/{}]", r.distance, r.metric) };
        let _ = writeln!(s, "{code} {:<20} {what}{ad}", r.net.to_string());
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
            format!("down (err-disabled by {})", crate::exec::l2sec::cause_words(p.l2.err_cause))
        } else if p.shutdown {
            "administratively down".to_string()
        } else if port_up(t, end) {
            "up".to_string()
        } else if t.peer(end).is_none() {
            "down (no cable)".to_string()
        } else if let Some(why) = netlabcraft_sim::serial_problem(t, end) {
            format!("up, line protocol down ({why})")
        } else if let Some(why) = netlabcraft_sim::cable_problem(t, end) {
            format!("down ({why})")
        } else if netlabcraft_sim::speed_mismatch(t, end) {
            "down (speed mismatch with the far end)".to_string()
        } else {
            "down (far end shut down)".to_string()
        };
        if only.is_none_or(|r| r.sub.is_none()) {
            let _ = writeln!(s, "{} is {link}", p.name);
            if netlabcraft_model::ifname::is_serial(&p.name) {
                let _ = writeln!(s, "  Encapsulation {}, serial", p.serial.encapsulation.name());
            } else {
                let _ = writeln!(s, "  Hardware address {}", p.mac);
            }
            if !p.description.is_empty() {
                let _ = writeln!(s, "  Description: {}", p.description);
            }
            if let Some(ip) = p.ip {
                let _ = writeln!(s, "  Internet address {ip}");
            }
            if let Some((speed, duplex)) = netlabcraft_sim::link_speed(t, end) {
                let half = duplex == netlabcraft_model::Duplex::Half;
                let _ = writeln!(s, "  {}-duplex, {speed} Mb/s", if half { "Half" } else { "Full" });
            }
            if let Some(peer) = t.peer(end) {
                let _ = writeln!(s, "  Cabled to {}{}", t.end_label(peer), cable_words(t, end));
            }
            if d.kind == DeviceKind::Switch {
                let sp = &p.switchport;
                if t.is_trunk(end) {
                    let _ = writeln!(s, "  Switchport: trunk ({}), native VLAN {}, allowed {}", sp.mode.name(), sp.native_vlan, sp.allowed);
                } else {
                    let _ = writeln!(s, "  Switchport: access ({}), VLAN {}", sp.mode.name(), sp.access_vlan);
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
    if only.is_none() {
        for tun in &d.tunnels {
            s.push_str(&tunnel_block(t, d, tun));
        }
        s.push_str(&crate::exec::routing::loopback_blocks(d));
    }
    s.trim_end().to_string()
}

/// Trunk ports and what they carry.
/// `show interfaces trunk`; with a port, only that port's row (`show interfaces gi0/1 trunk`).
pub fn trunks(t: &Topology, d: &Device, only: Option<usize>) -> String {
    let mut s = format!("{:<10} {:<10} {:<8} {}\n", "Port", "Status", "Native", "Allowed (and existing)");
    let mut any = false;
    let mut channels_shown = Vec::new();
    for (i, p) in d.ports.iter().enumerate() {
        if only.is_some_and(|o| o != i && !(p.channel.is_some() && d.ports.get(o).is_some_and(|q| q.channel == p.channel))) {
            continue;
        }
        if !t.is_trunk(End { device: d.id, port: i }) {
            continue;
        }
        any = true;
        // An EtherChannel's members trunk as one: the port-channel is listed, once.
        let (name, up) = match p.channel {
            Some(g) if channels_shown.contains(&g.number) => continue,
            Some(g) => {
                channels_shown.push(g.number);
                let up = d
                    .ports
                    .iter()
                    .enumerate()
                    .any(|(j, q)| q.channel.is_some_and(|x| x.number == g.number) && port_up(t, End { device: d.id, port: j }));
                (format!("Po{}", g.number), up)
            }
            None => (ifname::short(&p.name), port_up(t, End { device: d.id, port: i })),
        };
        let status = if up { "trunking" } else { "down" };
        let mut active = VlanSet::none();
        for v in d.vlans.keys().filter(|v| p.switchport.allowed.contains(**v)) {
            active.insert(*v);
        }
        let _ = writeln!(s, "{:<10} {:<10} {:<8} {} ({})", name, status, p.switchport.native_vlan, p.switchport.allowed, active);
    }
    if !any {
        s.push_str(if only.is_some() { "(not a trunk port)" } else { "(no trunk ports)" });
    } else if d.vtp.pruning {
        let _ = writeln!(s, "\n{:<10} {:<24} Pruned by VTP", "Port", "Pruning eligible");
        for (i, p) in d.ports.iter().enumerate().filter(|(i, _)| t.is_trunk(End { device: d.id, port: *i })) {
            let pruned = netlabcraft_sim::vtp::pruned_vlans(t, d, i);
            let eligible =
                p.l2.prune_eligible.as_ref().map_or_else(|| "2-1001".to_string(), |e| if e.is_empty() { "none".into() } else { e.to_string() });
            let _ = writeln!(
                s,
                "{:<10} {:<24} {}",
                ifname::short(&p.name),
                eligible,
                if pruned.is_empty() { "none".to_string() } else { pruned.to_string() }
            );
        }
    }
    s.trim_end().to_string()
}

/// The VLAN database with each VLAN's access ports.
pub fn vlans(t: &Topology, d: &Device) -> String {
    let mut s = format!("{:<5} {:<32} {:<9} {}\n", "VLAN", "Name", "Status", "Access ports");
    for (v, name) in &d.vlans {
        let ports: Vec<String> = d
            .ports
            .iter()
            .enumerate()
            .filter(|(i, p)| d.is_switchport(*i) && !t.is_trunk(End { device: d.id, port: *i }) && p.switchport.access_carries(*v))
            .map(|(_, p)| ifname::short(&p.name))
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
        .enumerate()
        .filter(|(i, p)| d.is_switchport(*i) && !t.is_trunk(End { device: d.id, port: *i }) && !d.vlans.contains_key(&p.switchport.access_vlan))
        .map(|(_, p)| format!("{} (VLAN {})", ifname::short(&p.name), p.switchport.access_vlan))
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
            if sim.now_us().saturating_sub(e.learned_us) >= netlabcraft_sim::MAC_AGING_US {
                continue;
            }
            n += 1;
            let port = if d.ports.get(e.port).is_some() { netlabcraft_sim::channel::port_label(t, d, e.port, true) } else { "?".to_string() };
            let _ = writeln!(s, "{:<6} {:<19} {:<9} {port}", vlan, mac.dotted(), "learned");
        }
    }
    let _ = write!(s, "{n} entr{}", if n == 1 { "y" } else { "ies" });
    s
}

pub fn arp(t: &Topology, sim: &Sim, d: &Device) -> String {
    let mut s = format!("{:<17} {:<10} {:<19} {}\n", "Address", "Age (min)", "MAC address", "Interface");
    let mut rows: Vec<(Ipv4Addr, String, String, String)> =
        netlabcraft_sim::up_ifaces(t, d).into_iter().map(|i| (i.net.addr, "-".to_string(), i.mac.to_string(), i.name)).collect();
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

pub fn version(sim: &Sim, d: &Device) -> String {
    let m = d.catalogue_model();
    let up = sim.now_us() / 1_000_000;
    let mut kinds: Vec<(String, usize)> = Vec::new();
    for p in &d.ports {
        let k = ifname::parse(&p.name).map_or("other", |n| n.kind).to_string();
        match kinds.iter_mut().find(|(n, _)| *n == k) {
            Some(e) => e.1 += 1,
            None => kinds.push((k, 1)),
        }
    }
    let ports: Vec<String> = kinds.iter().map(|(k, n)| format!("{n} {k}")).collect();
    format!(
        "NetLabCraft simulated {}, software {v}\n\
         Model {} ({})\n\
         {} has been up {} h {} min {} s (since the simulation started)\n\
         System image flash:{}\n\
         Ports: {}\n\
         Base hardware address {}\n\
         Configuration {}\n\
         Configuration register 0x{:04x}",
        d.kind.label().to_lowercase(),
        m.label,
        m.description,
        d.name,
        up / 3_600,
        up / 60 % 60,
        up % 60,
        crate::exec::mgmt::boot_image(d),
        ports.join(", "),
        d.ports.first().map_or("-".to_string(), |p| p.mac.to_string()),
        if d.startup.is_some() { "saved (startup-config present)" } else { "not saved" },
        d.nvram.register(),
        v = env!("CARGO_PKG_VERSION"),
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

/// `show ip dhcp conflict`.
pub fn dhcp_conflicts(sim: &Sim, d: &Device) -> String {
    let list = sim.state(d.id).map(|st| st.dhcp_srv.conflicts.clone()).unwrap_or_default();
    if list.is_empty() {
        return "No address conflicts: no pool address was found in use by another device.".into();
    }
    let mut s = format!("{:<17} {:<26} {}\n", "Address", "How it was found", "When (s)");
    for (ip, at) in &list {
        let _ = writeln!(s, "{:<17} {:<26} {}", ip.to_string(), "already on another device", at / 1_000_000);
    }
    let _ = write!(s, "{} conflict{} (these addresses are not handed out)", list.len(), if list.len() == 1 { "" } else { "s" });
    s
}

/// `show ip dhcp server statistics`.
pub fn dhcp_stats(sim: &Sim, d: &Device) -> String {
    let st = sim.state(d.id).map(|st| st.dhcp_srv.clone()).unwrap_or_default();
    let leases = sim.state(d.id).map_or(0, |st| st.leases.len());
    let mut s = format!("Pools              {}\nLeased addresses   {leases}\nConflicts          {}\n", d.dhcp_pools.len(), st.conflicts.len());
    s.push_str("\nReceived\n");
    for k in ["DISCOVER", "REQUEST", "RELEASE"] {
        let _ = writeln!(s, "  {k:<12} {}", st.received.get(k).copied().unwrap_or(0));
    }
    s.push_str("\nSent\n");
    for k in ["OFFER", "ACK", "NAK"] {
        let _ = writeln!(s, "  {k:<12} {}", st.sent.get(k).copied().unwrap_or(0));
    }
    s.trim_end().to_string()
}

/// `show ip dhcp pool`.
pub fn dhcp_pools(sim: &Sim, d: &Device, only: Option<&str>) -> Result<String, String> {
    if d.dhcp_pools.is_empty() {
        return Ok("No DHCP pools configured.".into());
    }
    if let Some(name) = only
        && !d.dhcp_pools.iter().any(|p| p.name == name)
    {
        return Err(format!("% No DHCP pool {name}"));
    }
    let mut s = String::new();
    for p in d.dhcp_pools.iter().filter(|p| only.is_none_or(|n| p.name == n)) {
        let leased = sim.state(d.id).map_or(0, |st| st.leases.values().filter(|l| l.pool == p.name).count());
        let _ = writeln!(s, "Pool {}", p.name);
        let _ = writeln!(s, "  Network          {}", p.network.map_or("not set (the pool hands out nothing)".to_string(), |n| n.to_string()));
        let _ = writeln!(s, "  Default router   {}", p.default_router.map_or("-".to_string(), |r| r.to_string()));
        let _ = writeln!(s, "  DNS server       {}", p.dns.map_or("-".to_string(), |r| r.to_string()));
        let _ = writeln!(s, "  Domain name      {}", p.domain.as_deref().unwrap_or("-"));
        let _ = writeln!(s, "  Lease time       {}", netlabcraft_model::mgmt::lease_text(p.lease_secs.unwrap_or(86_400)));
        let _ = writeln!(s, "  Leased addresses {leased}");
        if let Some(n) = p.network {
            let (usable, excluded) = pool_addresses(n, &d.dhcp_excluded);
            let free = usable.saturating_sub(excluded).saturating_sub(u64::try_from(leased).unwrap_or(u64::MAX));
            let _ = writeln!(s, "  Addresses        {usable} usable, {excluded} excluded, {free} free");
        }
    }
    if !d.dhcp_excluded.is_empty() {
        let ranges: Vec<String> = d.dhcp_excluded.iter().map(|(a, b)| if a == b { a.to_string() } else { format!("{a}-{b}") }).collect();
        let _ = writeln!(s, "Excluded: {}", ranges.join(", "));
    }
    Ok(s.trim_end().to_string())
}

/// How many host addresses a DHCP pool's network has, and how many of them the excluded ranges
/// take (overlapping ranges counted once).
fn pool_addresses(net: Ipv4Net, excluded: &[(std::net::Ipv4Addr, std::net::Ipv4Addr)]) -> (u64, u64) {
    let n = net.network();
    let first = u64::from(u32::from(n.addr));
    let last = u64::from(u32::from(n.broadcast()));
    // /31 and /32 have no network or broadcast address to skip.
    let (lo, hi) = if net.prefix >= 31 { (first, last) } else { (first.saturating_add(1), last.saturating_sub(1)) };
    let usable = if hi >= lo { hi - lo + 1 } else { 0 };
    let mut spans: Vec<(u64, u64)> =
        excluded.iter().map(|(a, b)| (u64::from(u32::from(*a)).max(lo), u64::from(u32::from(*b)).min(hi))).filter(|(a, b)| a <= b).collect();
    spans.sort_unstable();
    let mut taken = 0u64;
    let mut reach = 0u64;
    for (a, b) in spans {
        let a = a.max(reach);
        if a <= b {
            taken = taken.saturating_add(b - a + 1);
            reach = b.saturating_add(1);
        }
    }
    (usable, taken)
}

/// `show spanning-tree` for one VLAN.
pub fn spanning_tree(t: &Topology, sim: &Sim, d: &Device, vlan: VlanId, tree: &netlabcraft_sim::stp::VlanTree) -> String {
    let Some(me) = tree.bridges.get(&d.id) else { return format!("VLAN{:04}: this switch has no port in the tree", vlan.get()) };
    let mut s = format!("VLAN{:04}\n", vlan.get());
    let _ = writeln!(s, "  Root ID    Priority {:<6} Address {}", me.root.priority, me.root.mac.dotted());
    if me.root == me.id {
        s.push_str("             This switch is the root\n");
    } else {
        let rp = me.root_port.filter(|p| d.ports.get(*p).is_some()).map_or("-".to_string(), |p| netlabcraft_sim::channel::port_label(t, d, p, true));
        let _ = writeln!(s, "             Cost {}, reached through {rp}", me.root_cost);
    }
    let _ = writeln!(s, "  Bridge ID  Priority {:<6} Address {}\n", me.id.priority, me.id.mac.dotted());
    let _ = writeln!(s, "{:<10} {:<4} {:<5} {:<6} {:<8} Type", "Interface", "Role", "State", "Cost", "Port ID");
    for ((dev, p), info) in tree.ports.iter().filter(|((dev, _), i)| *dev == d.id && i.role != netlabcraft_sim::stp::Role::Disabled) {
        let _ = dev;
        if d.ports.get(*p).is_none() {
            continue;
        }
        let kind = if tree.inconsistent.contains(&(d.id, *p)) {
            "Root-inconsistent"
        } else if tree.loop_inconsistent.contains(&(d.id, *p)) {
            "Loop-inconsistent"
        } else if netlabcraft_sim::l2sec::edge_port(t, d, *p) {
            "Edge"
        } else if t.peer(End { device: d.id, port: *p }).and_then(|q| t.device(q.device)).is_some_and(|x| x.kind == DeviceKind::Switch) {
            "P2p"
        } else {
            "P2p (to host)"
        };
        let _ = writeln!(
            s,
            "{:<10} {:<4} {:<5} {:<6} {:<8} {kind}",
            netlabcraft_sim::channel::port_label(t, d, *p, true),
            info.role.short(),
            sim.stp_transition(d.id, *p, vlan).unwrap_or(info.role.state()),
            info.cost,
            format!("{}.{}", info.port_id / 1000, info.port_id % 1000)
        );
    }
    s.trim_end().to_string()
}

/// `show spanning-tree root`: for each VLAN the root's bridge ID, and the cost and port that lead to it.
pub fn stp_root(t: &Topology, d: &Device, trees: &[(VlanId, netlabcraft_sim::stp::VlanTree)]) -> String {
    if trees.is_empty() {
        return "No VLAN has a port in a spanning tree yet.".into();
    }
    let mut s = format!("{:<10} {:<28} {:<6} Root port\n", "VLAN", "Root bridge ID", "Cost");
    for (v, tree) in trees {
        let Some(me) = tree.bridges.get(&d.id) else { continue };
        let via = if me.root == me.id {
            "this switch is the root".to_string()
        } else {
            me.root_port.filter(|p| d.ports.get(*p).is_some()).map_or("-".to_string(), |p| netlabcraft_sim::channel::port_label(t, d, p, true))
        };
        let _ = writeln!(s, "VLAN{:04}   {:<28} {:<6} {via}", v.get(), format!("{} {}", me.root.priority, me.root.mac.dotted()), me.root_cost);
    }
    s.trim_end().to_string()
}

/// `show spanning-tree bridge`: this switch's own bridge ID in each VLAN, and whether it is the root.
pub fn stp_bridge(d: &Device, trees: &[(VlanId, netlabcraft_sim::stp::VlanTree)]) -> String {
    if trees.is_empty() {
        return "No VLAN has a port in a spanning tree yet.".into();
    }
    let mut s = format!("{:<10} {:<28} Root?\n", "VLAN", "Bridge ID");
    for (v, tree) in trees {
        let Some(me) = tree.bridges.get(&d.id) else { continue };
        let _ = writeln!(
            s,
            "VLAN{:04}   {:<28} {}",
            v.get(),
            format!("{} {}", me.id.priority, me.id.mac.dotted()),
            if me.root == me.id { "yes" } else { "no" }
        );
    }
    s.trim_end().to_string()
}

/// `show spanning-tree blockedports`: the ports that are not forwarding, per VLAN.
pub fn stp_blocked(t: &Topology, d: &Device, trees: &[(VlanId, netlabcraft_sim::stp::VlanTree)]) -> String {
    let mut s = format!("{:<10} Blocked ports\n", "VLAN");
    let mut total = 0usize;
    for (v, tree) in trees {
        let ports: Vec<String> = tree
            .ports
            .iter()
            .filter(|((dev, p), info)| {
                *dev == d.id && d.ports.get(*p).is_some() && !info.role.forwards() && info.role != netlabcraft_sim::stp::Role::Disabled
            })
            .map(|((_, p), _)| netlabcraft_sim::channel::port_label(t, d, *p, true))
            .collect();
        if !ports.is_empty() {
            total = total.saturating_add(ports.len());
            let _ = writeln!(s, "VLAN{:04}   {}", v.get(), ports.join(", "));
        }
    }
    if total == 0 {
        s.push_str("(none: every port in the tree forwards)");
    } else {
        let _ = write!(s, "{total} blocked port{}", if total == 1 { "" } else { "s" });
    }
    s.trim_end().to_string()
}

/// `show ip ospf neighbor`.
pub fn ospf_neighbors(t: &Topology, d: &Device, st: &netlabcraft_sim::ospf::OspfState) -> String {
    if d.ospf.is_none() {
        return "OSPF is not running on this router.".into();
    }
    let list = st.neighbors.get(&d.id).cloned().unwrap_or_default();
    if list.is_empty() {
        let mut why = String::new();
        crate::exec::routing::mismatch_lines(&mut why, st, d.id);
        return format!("No OSPF neighbours (check the network statements, passive interfaces and cables).\n{why}").trim_end().to_string();
    }
    let mut s = format!("{:<16} {:<10} {:<16} {:<22} Device\n", "Neighbour ID", "State", "Address", "Interface");
    for n in list {
        let name = t.device(n.device).map_or("?".to_string(), |x| x.name.clone());
        let state = match st.segments.get(&(d.id, n.iface.clone())) {
            Some(seg) if seg.dr.is_none() => "FULL/P2P",
            Some(seg) if seg.dr == Some(n.address) => "FULL/DR",
            Some(seg) if seg.bdr == Some(n.address) => "FULL/BDR",
            _ => "FULL/OTHER",
        };
        let _ = writeln!(s, "{:<16} {:<10} {:<16} {:<22} {name}", n.router_id.to_string(), state, n.address.to_string(), ifname::short(&n.iface));
    }
    for (iface, addr) in st.type_mismatch.get(&d.id).into_iter().flatten() {
        let _ = writeln!(
            s,
            "Note: the network type differs from {addr}'s on {} (point-to-point at one end, broadcast at the other): adjacent, but no route crosses this link",
            ifname::short(iface)
        );
    }
    crate::exec::routing::mismatch_lines(&mut s, st, d.id);
    s.trim_end().to_string()
}

/// `show ipv6 ospf neighbor`.
pub fn ospf6_neighbors(t: &Topology, d: &Device, st: &netlabcraft_sim::ospf6::Ospf6State) -> String {
    if d.ospf6.is_none() {
        return "OSPFv3 is not running on this router.".into();
    }
    let list = st.neighbors.get(&d.id).cloned().unwrap_or_default();
    if list.is_empty() {
        return "No OSPFv3 neighbours (check `ipv6 ospf N area A` on the interfaces, `ipv6 unicast-routing`, the router ID and cables).".into();
    }
    let mut s = format!("{:<16} {:<10} {:<26} {:<12} Device\n", "Neighbour ID", "State", "Address", "Interface");
    for n in list {
        let name = t.device(n.device).map_or("?".to_string(), |x| x.name.clone());
        let _ = writeln!(s, "{:<16} {:<10} {:<26} {:<12} {name}", n.router_id.to_string(), "FULL", n.address.to_string(), ifname::short(&n.iface));
    }
    s.trim_end().to_string()
}

/// `show ipv6 ospf`.
pub fn ospf6_summary(d: &Device, st: &netlabcraft_sim::ospf6::Ospf6State) -> String {
    let Some(o) = &d.ospf6 else { return "OSPFv3 is not running here.".into() };
    let mut s = format!("OSPFv3 process {}\n", o.process);
    let _ = writeln!(s, "  Router id {}", st.router_ids.get(&d.id).map_or("none: set one with `router-id`".to_string(), |r| r.to_string()));
    if !netlabcraft_sim::v6::routes6(d) {
        let _ = writeln!(s, "  Not running: IPv6 routing is off (`ipv6 unicast-routing`)");
    }
    for (name, area, cost, passive) in st.interfaces.get(&d.id).into_iter().flatten() {
        let _ = writeln!(s, "  {:<22} area {area}, cost {cost}{}", name, if *passive { ", passive" } else { "" });
    }
    let n = st.neighbors.get(&d.id).map_or(0, Vec::len);
    let _ = write!(s, "  {n} neighbour{}", if n == 1 { "" } else { "s" });
    s
}

/// `show ip protocols`.
pub fn ip_protocols(
    d: &Device,
    st: &netlabcraft_sim::ospf::OspfState,
    rs: &netlabcraft_sim::rip::RipState,
    es: &netlabcraft_sim::eigrp::EigrpState,
) -> String {
    let mut parts = Vec::new();
    if d.eigrp.is_some() {
        parts.push(eigrp_protocol(d, es));
    }
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
        let (u, i, h, f) = r.timers();
        let _ = writeln!(s, "  Updates every {u} s; a route is invalid after {i} s, held down {h} s, flushed after {f} s");
        if !rs.held.is_empty() {
            let silent: Vec<String> =
                rs.held.iter().filter(|x| x.router == d.id).map(|x| format!("{} on {}", x.address, ifname::short(&x.iface))).collect();
            if !silent.is_empty() {
                let _ = writeln!(s, "  Neighbours no longer heard (routes kept until invalid): {}", silent.join(", "));
            }
        }
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

fn eigrp_protocol(d: &Device, st: &netlabcraft_sim::eigrp::EigrpState) -> String {
    let Some(e) = &d.eigrp else { return String::new() };
    let mut s = format!("Routing protocol: EIGRP, autonomous system {}\n", e.asn);
    let _ = writeln!(s, "  Router id: {}", st.router_ids.get(&d.id).map_or("none (no interface address)".to_string(), |i| i.to_string()));
    s.push_str("  Metric weights: K1 1, K2 0, K3 1, K4 0, K5 0 (bandwidth and delay)\n");
    s.push_str("  Networks:\n");
    for n in &e.networks {
        match n.wildcard {
            Some(w) => {
                let _ = writeln!(s, "    {} {w}", n.address);
            }
            None => {
                let _ = writeln!(s, "    {} (classful)", n.address);
            }
        }
    }
    if !e.passive.is_empty() {
        let _ = writeln!(s, "  Passive interfaces: {}", e.passive.iter().map(|p| ifname::short(p)).collect::<Vec<_>>().join(", "));
    }
    let ifs = st.interfaces.get(&d.id).cloned().unwrap_or_default();
    let _ = writeln!(
        s,
        "  Interfaces running EIGRP: {}",
        if ifs.is_empty() { "none".to_string() } else { ifs.iter().map(|i| ifname::short(&i.name)).collect::<Vec<_>>().join(", ") }
    );
    let heard: Vec<String> = st.neighbors.get(&d.id).into_iter().flatten().map(|n| format!("{} on {}", n.address, ifname::short(&n.iface))).collect();
    let _ = writeln!(s, "  Neighbours: {}", if heard.is_empty() { "none".to_string() } else { heard.join(", ") });
    let _ = writeln!(s, "  Load balancing: up to {} paths per destination, variance {}", e.max_paths(), e.variance.unwrap_or(1).max(1));
    let _ = write!(
        s,
        "  Automatic summarisation {}; administrative distance 90 (internal)",
        if e.auto_summary { "on (not simulated: subnets are advertised)" } else { "off" }
    );
    s
}

/// `show ip eigrp neighbors`.
pub fn eigrp_neighbors(t: &Topology, d: &Device, st: &netlabcraft_sim::eigrp::EigrpState, now_us: u64) -> String {
    let Some(e) = &d.eigrp else { return "EIGRP is not running on this router.".into() };
    let list = st.neighbors.get(&d.id).cloned().unwrap_or_default();
    if list.is_empty() {
        return format!("No EIGRP neighbours in AS {} (check the network statements, the AS number, passive interfaces and cables).", e.asn);
    }
    let mut s = format!("EIGRP neighbours, AS {}\n", e.asn);
    let _ = writeln!(s, "{:<3} {:<16} {:<12} {:<9} Device", "#", "Address", "Interface", "Hold (s)");
    for (k, n) in list.iter().enumerate() {
        let name = t.device(n.device).map_or("?".to_string(), |x| x.name.clone());
        // The hold timer counts down from the last hello heard.
        let (hold, note) = match n.held_until {
            Some(until) => (until.saturating_sub(now_us) / 1_000_000, "  (no hellos heard: dropped when the hold time runs out)"),
            None => {
                let hello = u64::from(n.hello_secs.max(1)) * 1_000_000;
                (u64::from(n.hold_secs).saturating_sub((now_us % hello) / 1_000_000), "")
            }
        };
        let _ = writeln!(s, "{:<3} {:<16} {:<12} {:<9} {name}{note}", k, n.address.to_string(), ifname::short(&n.iface), hold);
    }
    s.trim_end().to_string()
}

/// `show ip eigrp topology`: per destination its feasible distance, successors and feasible
/// successors with (computed distance / reported distance).
pub fn eigrp_topology(d: &Device, st: &netlabcraft_sim::eigrp::EigrpState) -> String {
    let Some(e) = &d.eigrp else { return "EIGRP is not running on this router.".into() };
    let rid = st.router_ids.get(&d.id).map_or("none".to_string(), |r| r.to_string());
    let mut s = format!("EIGRP topology, AS {}, router id {rid}\n", e.asn);
    s.push_str("Every destination is passive (DUAL has converged). Paths show (distance / reported distance).\n");
    let entries = st.topology.get(&d.id).cloned().unwrap_or_default();
    if entries.is_empty() {
        s.push_str("\n(no destinations: no interface runs EIGRP)");
        return s;
    }
    for t in &entries {
        let succ = t.paths.iter().filter(|p| p.successor).count();
        let _ = writeln!(s, "\n{}  FD {}, {succ} successor{}", t.net, t.fd, if succ == 1 { "" } else { "s" });
        for p in &t.paths {
            match p.via {
                None => {
                    let _ = writeln!(s, "    connected, {}", ifname::short(&p.iface));
                }
                Some(v) => {
                    let role = if p.successor { "successor" } else { "feasible successor" };
                    let used = if !p.successor && p.installed { ", in use (variance)" } else { "" };
                    let _ = writeln!(s, "    {role:<19} via {v}, {} ({} / {}){used}", ifname::short(&p.iface), p.distance, p.reported);
                }
            }
        }
    }
    s.trim_end().to_string()
}

/// `show ip eigrp interfaces`.
pub fn eigrp_interfaces(d: &Device, st: &netlabcraft_sim::eigrp::EigrpState) -> String {
    let Some(e) = &d.eigrp else { return "EIGRP is not running on this router.".into() };
    let ifs = st.interfaces.get(&d.id).cloned().unwrap_or_default();
    if ifs.is_empty() {
        return format!("No interface runs EIGRP in AS {} (check the network statements and that the interfaces are up).", e.asn);
    }
    let mut s = format!("EIGRP interfaces, AS {}\n", e.asn);
    let _ = writeln!(
        s,
        "{:<12} {:<20} {:<6} {:<16} {:<11} {:<8} {:<9} Passive",
        "Interface", "Network", "Peers", "Bandwidth (kbit)", "Delay (µs)", "Hello", "Hold (s)"
    );
    for i in &ifs {
        let peers = st.neighbors.get(&d.id).into_iter().flatten().filter(|n| n.iface == i.name).count();
        let _ = writeln!(
            s,
            "{:<12} {:<20} {:<6} {:<16} {:<11} {:<8} {:<9} {}",
            ifname::short(&i.name),
            i.net.to_string(),
            peers,
            i.bandwidth,
            u64::from(i.delay) * 10,
            i.hello_secs,
            i.hold_secs,
            if i.passive { "yes" } else { "no" }
        );
    }
    s.trim_end().to_string()
}

fn ospf_protocol(d: &Device, st: &netlabcraft_sim::ospf::OspfState) -> String {
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
    let _ = writeln!(s, "  Load balancing: up to {} equal-cost paths per destination", o.maximum_paths.unwrap_or(4).max(1));
    let _ = write!(s, "  Administrative distance 110");
    s
}

pub(crate) fn ospf_if_lines(s: &mut String, d: &Device, name: &str) {
    crate::exec::swroute::eigrp_summary_lines(s, d, name);
    if let Some(e) = d.eigrp_if.get(name) {
        if let Some((asn, secs)) = e.hello {
            let _ = writeln!(s, " ip hello-interval eigrp {asn} {secs}");
        }
        if let Some((asn, secs)) = e.hold {
            let _ = writeln!(s, " ip hold-time eigrp {asn} {secs}");
        }
        if let Some((asn, secs)) = e.hello6 {
            let _ = writeln!(s, " ipv6 hello-interval eigrp {asn} {secs}");
        }
        if let Some((asn, secs)) = e.hold6 {
            let _ = writeln!(s, " ipv6 hold-time eigrp {asn} {secs}");
        }
    }
    if let Some(o) = d.ospf_if.get(name) {
        if let Some(c) = o.cost {
            let _ = writeln!(s, " ip ospf cost {c}");
        }
        if let Some(p) = o.priority {
            let _ = writeln!(s, " ip ospf priority {p}");
        }
        if let Some(h) = o.hello {
            let _ = writeln!(s, " ip ospf hello-interval {h}");
        }
        if let Some(h) = o.dead {
            let _ = writeln!(s, " ip ospf dead-interval {h}");
        }
        if o.p2p {
            s.push_str(" ip ospf network point-to-point\n");
        }
        if let Some((p, a)) = o.area {
            let _ = writeln!(s, " ip ospf {p} area {a}");
        }
    }
}

fn standby_lines(s: &mut String, d: &Device, name: &str, list: &[netlabcraft_model::Standby]) {
    if d.mgmt.hsrp_v2.iter().any(|n| n == name) {
        let _ = writeln!(s, " standby version 2");
    }
    for g in list {
        if let Some(ip) = g.ip {
            let _ = writeln!(s, " standby {} ip {ip}", g.group);
        }
        if g.priority != 100 {
            let _ = writeln!(s, " standby {} priority {}", g.group, g.priority);
        }
        if g.preempt {
            let _ = writeln!(s, " standby {} preempt", g.group);
        }
        for (i, dec) in &g.track {
            let _ = writeln!(s, " standby {} track {i}{}", g.group, if *dec == 10 { String::new() } else { format!(" decrement {dec}") });
        }
    }
}

fn nat_side(s: &mut String, side: Option<netlabcraft_model::NatSide>) {
    match side {
        Some(netlabcraft_model::NatSide::Inside) => s.push_str(" ip nat inside\n"),
        Some(netlabcraft_model::NatSide::Outside) => s.push_str(" ip nat outside\n"),
        None => {}
    }
}

fn access_groups(s: &mut String, g: &netlabcraft_model::AccessGroups) {
    if let Some(n) = &g.inbound {
        let _ = writeln!(s, " ip access-group {n} in");
    }
    if let Some(n) = &g.outbound {
        let _ = writeln!(s, " ip access-group {n} out");
    }
}

/// `show access-lists`.
pub fn access_lists(sim: &Sim, d: &Device, only: Option<&str>) -> String {
    if d.acls.is_empty() {
        return "No access lists configured.".into();
    }
    let hits = sim.state(d.id).map(|st| st.acl_hits.clone()).unwrap_or_default();
    let mut s = String::new();
    for a in d.acls.iter().filter(|a| only.is_none_or(|n| a.name.eq_ignore_ascii_case(n))) {
        let _ = writeln!(s, "{} access list {}", if a.extended { "Extended" } else { "Standard" }, a.name);
        for (i, e) in a.entries.iter().enumerate() {
            let n = hits.get(&(a.name.clone(), i)).copied().unwrap_or(0);
            let m = if n > 0 { format!(" ({n} match{})", if n == 1 { "" } else { "es" }) } else { String::new() };
            let _ = writeln!(s, "    {} {}{m}", a.seq_of(i), e.text(a.extended));
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
    let secured: Vec<(usize, &netlabcraft_model::Port)> = d.ports.iter().enumerate().filter(|(_, p)| p.port_security.is_some()).collect();
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
    let chans = netlabcraft_sim::channel::channels(t, d);
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
    if v.pruning {
        let _ = writeln!(s, "Pruning             enabled");
    }
    let _ = write!(s, "VLANs               {}", d.vlans.len());
    s
}

/// An interface's IPv6 lines in the running configuration.
pub(crate) fn ipv6_lines(s: &mut String, c: &netlabcraft_model::Ipv6Conf) {
    if let Some(ll) = c.link_local {
        let _ = writeln!(s, " ipv6 address {ll} link-local");
    }
    for a in &c.addresses {
        let _ = writeln!(s, " ipv6 address {a}");
    }
    crate::exec::routing::ipv6_client_lines(s, c);
    if c.enabled {
        let _ = writeln!(s, " ipv6 enable");
    }
    if let Some(pool) = &c.dhcp_server {
        let _ = writeln!(s, " ipv6 dhcp server {pool}");
    }
    for a in &c.dhcp_relay {
        let _ = writeln!(s, " ipv6 dhcp relay destination {a}");
    }
    if let Some(asn) = c.eigrp {
        let _ = writeln!(s, " ipv6 eigrp {asn}");
    }
    if let Some(name) = &c.rip {
        let _ = writeln!(s, " ipv6 rip {name} enable");
    }
    if let Some((p, a)) = c.ospf {
        let _ = writeln!(s, " ipv6 ospf {p} area {a}");
    }
    if c.nd_managed {
        let _ = writeln!(s, " ipv6 nd managed-config-flag");
    }
    if c.nd_other {
        let _ = writeln!(s, " ipv6 nd other-config-flag");
    }
    if let Some(f) = &c.filter_in {
        let _ = writeln!(s, " ipv6 traffic-filter {f} in");
    }
    if let Some(f) = &c.filter_out {
        let _ = writeln!(s, " ipv6 traffic-filter {f} out");
    }
}

/// `show ipv6 interface brief`.
pub fn ipv6_brief(t: &Topology, d: &Device) -> String {
    let mut s = format!("{:<24} {:<22} {}\n", "Interface", "Status", "IPv6 addresses");
    let ifs = netlabcraft_sim::v6::l3_ifaces6(d);
    let up6 = netlabcraft_sim::v6::up_ifaces6(t, d);
    let row = |s: &mut String, name: &str, status: &str| {
        let _ = write!(s, "{name:<24} {status:<22} ");
        match ifs.iter().find(|x| x.name == name) {
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
    };
    for (i, p) in d.ports.iter().enumerate() {
        let status = if p.shutdown {
            "administratively down"
        } else if port_up(t, End { device: d.id, port: i }) {
            "up"
        } else {
            "down"
        };
        // A switch's own ports carry no IPv6; only its routed ones are listed.
        if !d.is_switchport(i) {
            row(&mut s, &p.name, status);
        }
        for sub in &p.subifs {
            let name = format!("{}.{}", p.name, sub.number);
            let status = if sub.shutdown || p.shutdown { "administratively down" } else { status };
            row(&mut s, &name, status);
        }
    }
    for svi in &d.svis {
        let name = format!("Vlan{}", svi.vlan);
        let status = if svi.shutdown {
            "administratively down"
        } else if up6.iter().any(|x| x.name == name) || netlabcraft_sim::up_ifaces(t, d).iter().any(|x| x.name == name) {
            "up"
        } else {
            "down"
        };
        row(&mut s, &name, status);
    }
    for l in &d.loopbacks {
        row(&mut s, &l.name(), if l.shutdown { "administratively down" } else { "up" });
    }
    s.trim_end().to_string()
}

/// `show ipv6 route`.
pub fn ipv6_route(t: &Topology, sim: &mut Sim, d: &Device) -> String {
    let dynamic = sim.dynamic_routes6(t, d);
    let table = netlabcraft_sim::v6::routing_table6(t, d, &dynamic);
    let mut s = String::from(
        "Codes: C connected, L local, S static, O OSPFv3, OI OSPFv3 other area, OE2 OSPFv3 external, D EIGRP, R RIPng, ND default from a router advertisement\n",
    );
    if !d.ipv6_routing {
        s.push_str("IPv6 forwarding is off (no `ipv6 unicast-routing`)\n");
    }
    for r in &table {
        let via = match (&r.next_hop, &r.iface) {
            (Some(nh), Some(i)) => format!("via {nh}, {i}"),
            (Some(nh), None) => format!("via {nh}"),
            (None, Some(i)) if r.code == 'S' => format!("out of {i}"),
            (None, Some(i)) if i == netlabcraft_sim::eigrp::NULL0 => format!("summary, discarded on {i}"),
            (None, Some(i)) => format!("directly on {i}"),
            (None, None) => String::new(),
        };
        let code = match r.code {
            'I' => "OI".to_string(),
            'E' => "OE2".to_string(),
            'N' => "ND".to_string(),
            c => c.to_string(),
        };
        let ad = if matches!(r.code, 'C' | 'L') { String::new() } else { format!(" [{}]", r.distance) };
        let _ = writeln!(s, "{code:<2} {:<28} {via}{ad}", r.net.to_string());
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
pub fn rip_database(t: &Topology, d: &Device, st: &netlabcraft_sim::rip::RipState) -> String {
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
    for x in st.down.get(&d.id).into_iter().flatten() {
        let _ = writeln!(s, "{:<20} {:<6} {:<16} {}  possibly down", x.net.to_string(), 16, x.via.to_string(), ifname::short(&x.iface));
    }
    let _ = t;
    s.trim_end().to_string()
}

/// `show running-config interface X`: that interface's block.
pub fn running_config_interface(d: &Device, name: &str) -> String {
    let run = running_config(d);
    let head = format!("interface {name}");
    let mut out = Vec::new();
    let mut inside = false;
    for line in run.lines() {
        if line == head {
            inside = true;
        } else if inside && !line.starts_with(' ') {
            break;
        }
        if inside {
            out.push(line);
        }
    }
    if out.is_empty() { format!("% {name} has no configuration block") } else { out.join("\n") }
}

/// `show ip interface [X]`.
pub fn ip_interface(t: &Topology, d: &Device, only: Option<IfRef>) -> String {
    let mut blocks = Vec::new();
    for (i, p) in d.ports.iter().enumerate() {
        if only.is_some_and(|r| r.port != i) {
            continue;
        }
        let end = End { device: d.id, port: i };
        let mut rows: Vec<(
            String,
            Option<Ipv4Net>,
            &netlabcraft_model::AccessGroups,
            &[std::net::Ipv4Addr],
            Option<netlabcraft_model::NatSide>,
            bool,
        )> = Vec::new();
        if only.is_none_or(|r| r.sub.is_none()) {
            rows.push((p.name.clone(), p.ip, &p.access, &p.helpers, p.nat, p.shutdown));
        }
        for s in &p.subifs {
            if only.is_none_or(|r| r.sub == Some(s.number)) {
                rows.push((format!("{}.{}", p.name, s.number), s.ip, &s.access, &s.helpers, s.nat, s.shutdown || p.shutdown));
            }
        }
        for (name, ip, acl, helpers, nat, down) in rows {
            let state = if down {
                "administratively down"
            } else if port_up(t, end) {
                "up"
            } else {
                "down"
            };
            let mut s = format!("{name} is {state}\n");
            match ip {
                Some(ip) => {
                    let _ = writeln!(s, "  Address {ip}, broadcast {}", ip.broadcast());
                }
                None => s.push_str("  No IPv4 address\n"),
            }
            let _ = writeln!(s, "  Access list in: {}", acl.inbound.as_deref().unwrap_or("none"));
            let _ = writeln!(s, "  Access list out: {}", acl.outbound.as_deref().unwrap_or("none"));
            if !helpers.is_empty() {
                let _ = writeln!(s, "  DHCP relay to: {}", helpers.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "));
            }
            match nat {
                Some(netlabcraft_model::NatSide::Inside) => s.push_str("  NAT: inside\n"),
                Some(netlabcraft_model::NatSide::Outside) => s.push_str("  NAT: outside\n"),
                None => {}
            }
            if d.kind == DeviceKind::Router {
                s.push_str(if p.proxy_arp_off { "  Proxy ARP: off\n" } else { "  Proxy ARP: on\n" });
            }
            blocks.push(s.trim_end().to_string());
        }
    }
    for svi in d.svis.iter().filter(|_| only.is_none()) {
        let mut s = format!("Vlan{} is {}\n", svi.vlan, if svi.shutdown { "administratively down" } else { "up" });
        match svi.ip {
            Some(ip) => {
                let _ = write!(s, "  Address {ip}, broadcast {}", ip.broadcast());
            }
            None => s.push_str("  No IPv4 address"),
        }
        blocks.push(s);
    }
    blocks.join("\n")
}

/// `show interfaces status` (switches).
pub fn interface_status(t: &Topology, d: &Device) -> String {
    let mut s = format!("{:<10} {:<18} {:<13} {:<8} {}\n", "Port", "Description", "State", "VLAN", "Speed");
    for (i, p) in d.ports.iter().enumerate() {
        let end = End { device: d.id, port: i };
        let state = if p.err_disabled {
            "err-disabled"
        } else if p.shutdown {
            "disabled"
        } else if port_up(t, end) {
            "connected"
        } else {
            "no link"
        };
        let vlan = if p.routed {
            "routed".to_string()
        } else if t.is_trunk(End { device: d.id, port: i }) {
            "trunk".to_string()
        } else {
            p.switchport.access_vlan.to_string()
        };
        let speed = if p.name.starts_with("GigabitEthernet") { "1000" } else { "100" };
        let desc: String = p.description.chars().take(18).collect();
        let _ = writeln!(s, "{:<10} {:<18} {:<13} {:<8} {speed}", ifname::short(&p.name), desc, state, vlan);
    }
    s.trim_end().to_string()
}

/// `show interfaces X switchport`.
pub fn switchport(t: &Topology, d: &Device, port: usize) -> Result<String, String> {
    let p = d.ports.get(port).ok_or("% No such port")?;
    if !d.switches_frames() {
        return Err("% Only switch ports have switchport settings".into());
    }
    if !d.is_switchport(port) {
        return Ok(format!("{}: routed port (no switchport)", p.name));
    }
    let sp = &p.switchport;
    let vname = |v: VlanId| d.vlans.get(&v).map_or("not in the VLAN database".to_string(), Clone::clone);
    let oper = if t.is_trunk(End { device: d.id, port }) { "trunk" } else { "access" };
    let mut s = format!("{}\n", p.name);
    let _ = writeln!(s, "  Mode: {} (working as {oper}){}", sp.mode.name(), if sp.nonegotiate { ", no negotiation" } else { "" });
    let _ = writeln!(s, "  Access VLAN: {} ({})", sp.access_vlan, vname(sp.access_vlan));
    if let Some(v) = sp.voice_vlan {
        let _ = writeln!(s, "  Voice VLAN: {v} ({})", vname(v));
    }
    let _ = writeln!(s, "  Trunk native VLAN: {} ({})", sp.native_vlan, vname(sp.native_vlan));
    let _ = writeln!(s, "  Trunk allowed VLANs: {}", if sp.allowed.is_all() { "all".to_string() } else { sp.allowed.to_string() });
    let _ = writeln!(s, "  Port security: {}", if p.port_security.is_some() { "on" } else { "off" });
    let ch = netlabcraft_sim::channel::port_label(t, d, port, false);
    let _ = write!(s, "  EtherChannel: {}", if ch == p.name { "none".to_string() } else { ch });
    Ok(s)
}

/// `show flash`: our own file listing.
pub fn flash(d: &Device) -> String {
    let files = crate::exec::mgmt::flash_files(d);
    let mut s = format!("{:<4} {:>10}  {}\n", "#", "Bytes", "Name");
    for (i, (name, size)) in files.iter().enumerate() {
        let _ = writeln!(s, "{:<4} {:>10}  {name}", i + 1, size);
    }
    let used: u64 = files.iter().map(|f| f.1).sum();
    let _ = write!(s, "{} bytes free of 268435456", 268_435_456u64.saturating_sub(used));
    s
}

/// `show interfaces vlan N`.
pub fn svi_interface(t: &Topology, d: &Device, vlan: u16) -> Result<String, String> {
    let svi = d.svis.iter().find(|s| s.vlan.get() == vlan).ok_or(format!("% No interface Vlan{vlan} (create it with `interface vlan {vlan}`)"))?;
    let up = netlabcraft_sim::up_ifaces(t, d).iter().any(|i| i.name == format!("Vlan{vlan}"));
    let state = if svi.shutdown {
        "administratively down"
    } else if up {
        "up"
    } else {
        "down (no port of the VLAN is up)"
    };
    let mut s = format!("Vlan{vlan} is {state}\n  Hardware address {}\n", netlabcraft_sim::svi_mac(d));
    if !svi.description.is_empty() {
        let _ = writeln!(s, "  Description: {}", svi.description);
    }
    let _ = write!(s, "  Internet address {}", svi.ip.map_or("not set".to_string(), |n| n.to_string()));
    Ok(s)
}

/// ` with a cross-over cable` (` with an automatic cable, working as straight-through`).
pub fn cable_words(t: &Topology, end: End) -> String {
    let Some(l) = t.link_at(end) else { return String::new() };
    let kind = |c: netlabcraft_model::cable::Cable| c.label().trim_start_matches("Copper ").to_lowercase();
    match l.cable {
        netlabcraft_model::cable::Cable::Auto => format!(" with an automatic cable, working as {}", kind(t.resolved_cable(l))),
        c => format!(" with a {} cable", kind(c)),
    }
}

/// `show controllers` on an Ethernet port.
pub fn ethernet_controller(t: &Topology, d: &Device, port: usize) -> String {
    let Some(p) = d.ports.get(port) else { return String::new() };
    let end = End { device: d.id, port };
    let link = match netlabcraft_sim::link_speed(t, end) {
        Some((sp, du)) => format!("link up, {sp} Mb/s, {} duplex", if du == netlabcraft_model::Duplex::Half { "half" } else { "full" }),
        None => "no link".into(),
    };
    let set = |o: Option<String>| o.unwrap_or_else(|| "auto".into());
    let medium = d.port_medium(port).map_or("copper", |m| m.words());
    let cable = if t.link_at(end).is_some() { format!("\n  Cable:{}", cable_words(t, end).trim_start_matches(" with")) } else { String::new() };
    format!(
        "{}: Ethernet ({medium}), {link}\n  Speed set to {}, duplex set to {}\n  Auto-MDIX {}{cable}",
        p.name,
        set(p.speed.map(|x| format!("{x} Mb/s"))),
        set(p.duplex.map(|x| if x == netlabcraft_model::Duplex::Half { "half".into() } else { "full".into() })),
        if p.mdix_off { "off" } else { "on" }
    )
}

/// `show ssh`.
pub fn ssh_sessions(cli: &crate::Cli) -> String {
    if cli.vty {
        "Connection Version State\n0          2.0     Session started (this session, if over SSH)".into()
    } else {
        "No SSH sessions are open to this console.".into()
    }
}

/// `show protocols`.
pub fn protocols(t: &Topology, d: &Device) -> String {
    let mut s = format!("Global values:\n  Internet Protocol routing is {}\n", if d.routes_ip() { "enabled" } else { "disabled" });
    for (i, p) in d.ports.iter().enumerate() {
        let end = End { device: d.id, port: i };
        let state = if p.shutdown {
            "administratively down"
        } else if port_up(t, end) {
            "up"
        } else {
            "down"
        };
        let _ = writeln!(s, "{} is {state}, line protocol is {}", p.name, if port_up(t, end) { "up" } else { "down" });
        if let Some(ip) = p.ip {
            let _ = writeln!(s, "  Internet address is {ip}");
        }
    }
    for svi in &d.svis {
        let _ = writeln!(s, "Vlan{} is {}", svi.vlan, if svi.shutdown { "administratively down" } else { "up" });
        if let Some(ip) = svi.ip {
            let _ = writeln!(s, "  Internet address is {ip}");
        }
    }
    s.trim_end().to_string()
}

/// `show interfaces description`.
pub fn int_descriptions(t: &Topology, d: &Device) -> String {
    let mut s = format!("{:<12} {:<22} {:<9} {}\n", "Interface", "Status", "Protocol", "Description");
    for (i, p) in d.ports.iter().enumerate() {
        let end = End { device: d.id, port: i };
        let up = port_up(t, end);
        let status = if p.shutdown {
            "admin down"
        } else if up {
            "up"
        } else {
            "down"
        };
        let _ = writeln!(s, "{:<12} {:<22} {:<9} {}", ifname::short(&p.name), status, if up { "up" } else { "down" }, p.description);
        for sub in &p.subifs {
            let _ = writeln!(
                s,
                "{:<12} {:<22} {:<9} {}",
                format!("{}.{}", ifname::short(&p.name), sub.number),
                status,
                if up { "up" } else { "down" },
                sub.description
            );
        }
    }
    for svi in &d.svis {
        let _ = writeln!(s, "{:<12} {:<22} {:<9} {}", format!("Vl{}", svi.vlan), if svi.shutdown { "admin down" } else { "up" }, "", svi.description);
    }
    s.trim_end().to_string()
}

/// `show login`.
pub fn login_state(d: &Device, blocked: Option<u64>) -> String {
    match (d.access.login_block, blocked) {
        (None, _) => "No login blocking is configured (`login block-for`).".into(),
        (Some((b, n, w)), None) => {
            format!("Logins are blocked for {b} s after {n} failures within {w} s.\nRouter presently in Normal mode: logins accepted.")
        }
        (Some((b, n, w)), Some(left)) => format!(
            "Logins are blocked for {b} s after {n} failures within {w} s.\nRouter presently in Quiet mode: logins refused for {left} more seconds."
        ),
    }
}

/// `show ipv6 interface X`.
pub fn ipv6_interface(t: &Topology, d: &Device, r: IfRef) -> Result<String, String> {
    let p = d.ports.get(r.port).ok_or("% No such port")?;
    let name = r.sub.map_or(p.name.clone(), |n| format!("{}.{n}", p.name));
    let up = port_up(t, End { device: d.id, port: r.port });
    let Some(ifc) = netlabcraft_sim::v6::l3_ifaces6(d).into_iter().find(|i| i.name == name) else {
        return Ok(format!("{name} is {}: IPv6 is not enabled on it", if up { "up" } else { "down" }));
    };
    let mut s = format!(
        "{name} is {}, line protocol is {}\n",
        if p.shutdown {
            "administratively down"
        } else if up {
            "up"
        } else {
            "down"
        },
        if up { "up" } else { "down" }
    );
    let _ = writeln!(s, "  IPv6 is enabled, link-local address is {}", ifc.link_local);
    let _ = writeln!(s, "  Global unicast address(es):");
    for a in &ifc.addresses {
        let _ = writeln!(s, "    {}, subnet is {}", a.addr, a.network());
    }
    let _ = writeln!(s, "  Joined group address(es):\n    FF02::1");
    if netlabcraft_sim::v6::routes6(d) {
        let _ = writeln!(s, "    FF02::2");
    }
    // Addresses sharing their last 24 bits share one group, listed once.
    let mut groups: Vec<String> = Vec::new();
    for a in ifc.addresses.iter().map(|n| n.addr).chain([ifc.link_local]) {
        let g = netlabcraft_wire_solicited(a);
        if !groups.contains(&g) {
            let _ = writeln!(s, "    {g}");
            groups.push(g);
        }
    }
    if netlabcraft_sim::v6::routes6(d) {
        let _ = write!(s, "  Router advertisements are sent; hosts use them to autoconfigure");
    } else {
        let _ = write!(s, "  Not routing IPv6 (no `ipv6 unicast-routing`)");
    }
    Ok(s)
}

/// The solicited-node group of an address (`FF02::1:FFxx:xxxx`), as `show ipv6 interface` lists it.
fn netlabcraft_wire_solicited(a: std::net::Ipv6Addr) -> String {
    let o = a.octets();
    format!("FF02::1:FF{:02X}:{:X}", o[13], u16::from_be_bytes([o[14], o[15]]))
}

/// `show ip traffic`.
pub fn ip_traffic(sim: &Sim, dev: netlabcraft_core::DeviceId) -> String {
    let c = sim.state(dev).map(|st| st.ip_traffic.clone()).unwrap_or_default();
    let n = |k: &str| c.get(k).copied().unwrap_or(0);
    let delivered: u64 = c.iter().filter(|(k, _)| k.starts_with("delivered")).map(|(_, v)| *v).sum();
    let mut s = String::from("IPv4 packets\n");
    let _ = writeln!(s, "  Received       {}", n("received"));
    let _ = writeln!(s, "  For this device {delivered}");
    let _ = writeln!(s, "  Forwarded      {}", n("forwarded"));
    let _ = writeln!(s, "  Sent (own)     {}", n("sent"));
    let _ = writeln!(s, "  Dropped: no route {}, TTL expired {}", n("no route"), n("TTL expired"));
    s.push_str("Delivered by protocol\n");
    for (k, label) in [
        ("delivered: ICMP echo request", "ICMP echo requests"),
        ("delivered: ICMP echo reply", "ICMP echo replies"),
        ("delivered: ICMP other", "ICMP other"),
        ("delivered: TCP", "TCP"),
        ("delivered: UDP", "UDP"),
        ("delivered: other", "Other"),
    ] {
        let _ = writeln!(s, "  {label:<19} {}", n(k));
    }
    s.trim_end().to_string()
}

/// `show tcp brief`: connections set up to or from this device.
pub fn tcp_brief(sim: &Sim, dev: netlabcraft_core::DeviceId) -> String {
    let list = sim.state(dev).map(|st| st.tcp_recent.clone()).unwrap_or_default();
    if list.is_empty() {
        return "No TCP connections.".into();
    }
    let mut s = format!("{:<22} {:<22} {}\n", "Local address", "Remote address", "Opened (s)");
    for r in &list {
        let _ =
            writeln!(s, "{:<22} {:<22} {}", format!("{}:{}", r.local.0, r.local.1), format!("{}:{}", r.remote.0, r.remote.1), r.since_us / 1_000_000);
    }
    s.trim_end().to_string()
}

/// `show users`.
pub fn users(cli: &crate::Cli) -> String {
    let line = if cli.vty { "vty 0 (remote)" } else { "con 0" };
    let user = cli.user.as_deref().unwrap_or("");
    format!("{:<3}{:<18} {:<10} {}\n*  {:<18} {:<10} {}", "", "Line", "User", "State", line, user, "active (this session)")
}

/// `show cdp`.
pub fn cdp(d: &Device) -> String {
    if d.cdp_off {
        return "Neighbour discovery is off on this device (`cdp run` turns it on).".into();
    }
    let off: Vec<String> = d.ports.iter().filter(|p| p.cdp_off).map(|p| ifname::short(&p.name)).collect();
    let mut s = String::from("Neighbour discovery is on: advertisements every 60 s, kept for 180 s.");
    if !off.is_empty() {
        let _ = write!(s, "\nOff on: {}", off.join(", "));
    }
    s
}

/// `show cdp neighbors [detail]`.
pub fn cdp_neighbors(t: &Topology, sim: &Sim, d: &Device, detail: bool) -> String {
    if d.cdp_off {
        return "Neighbour discovery is off on this device.".into();
    }
    let list = netlabcraft_sim::cdp::neighbors(t, d);
    // Time left before an entry would expire, counting down between advertisements.
    let hold = 180 - (sim.now_us() / 1_000_000 % 60);
    let kind = |k: DeviceKind| if k == DeviceKind::Router { "router" } else { "switch" };
    let mut s = String::new();
    if detail {
        for n in &list {
            let _ = writeln!(s, "Neighbour {} ({}, {})", n.name, kind(n.kind), n.platform);
            let _ = writeln!(s, "  Seen on   {}, its port {}", n.local, n.remote);
            let _ = writeln!(s, "  Address   {}", n.address.map_or("none".to_string(), |a| a.addr.to_string()));
            let _ = writeln!(s, "  Software  NetLabCraft {}", env!("CARGO_PKG_VERSION"));
            let _ = writeln!(s, "  Expires   in {hold} s\n");
        }
    } else {
        let _ = writeln!(s, "{:<14} {:<14} {:<7} {:<9} {:<9} Expires", "Neighbour", "Platform", "Type", "Here", "There");
        for n in &list {
            let _ = writeln!(
                s,
                "{:<14} {:<14} {:<7} {:<9} {:<9} {hold} s",
                n.name,
                n.platform,
                kind(n.kind),
                ifname::short(&n.local),
                ifname::short(&n.remote)
            );
        }
    }
    let _ = write!(s, "{} neighbour{}", list.len(), if list.len() == 1 { "" } else { "s" });
    s
}

/// `show lldp`.
pub fn lldp(d: &Device) -> String {
    if !d.lldp_on {
        return "LLDP is off on this device (`lldp run` turns it on).".into();
    }
    let mut s = format!(
        "LLDP is on: advertisements every {} s, kept for {} s.",
        netlabcraft_sim::lldp::LLDP_EVERY_US / 1_000_000,
        netlabcraft_sim::lldp::HOLD_SECS
    );
    let names = |f: fn(&netlabcraft_model::Port) -> bool| d.ports.iter().filter(|p| f(p)).map(|p| ifname::short(&p.name)).collect::<Vec<_>>();
    for (what, off) in [("Not sending on", names(|p| p.lldp_tx_off)), ("Not hearing on", names(|p| p.lldp_rx_off))] {
        if !off.is_empty() {
            let _ = write!(s, "\n{what}: {}", off.join(", "));
        }
    }
    s
}

/// `show lldp neighbors [detail]`.
pub fn lldp_neighbors(t: &Topology, sim: &Sim, d: &Device, detail: bool) -> String {
    if !d.lldp_on {
        return "LLDP is off on this device.".into();
    }
    let list = netlabcraft_sim::lldp::neighbors(t, d);
    let every = netlabcraft_sim::lldp::LLDP_EVERY_US / 1_000_000;
    let hold = u64::from(netlabcraft_sim::lldp::HOLD_SECS) - (sim.now_us() / 1_000_000 % every);
    let caps = |c: u16| {
        let mut v = Vec::new();
        if c & netlabcraft_sim::lldp::CAP_ROUTER != 0 {
            v.push("router");
        }
        if c & netlabcraft_sim::lldp::CAP_BRIDGE != 0 {
            v.push("bridge");
        }
        if v.is_empty() { "-".to_string() } else { v.join(", ") }
    };
    let mut s = String::new();
    if detail {
        for n in &list {
            let i = &n.info;
            let _ = writeln!(s, "Neighbour {}", i.system_name);
            let _ = writeln!(s, "  Heard on     {}, their port {}", n.local, i.port);
            let _ = writeln!(s, "  Chassis      {}", i.chassis);
            let _ = writeln!(s, "  Description  {}", i.description);
            let _ = writeln!(s, "  Can act as   {}", caps(i.caps.0));
            let _ = writeln!(s, "  Address      {}", i.address.map_or("none".to_string(), |a| a.to_string()));
            let _ = writeln!(s, "  Expires      in {hold} s\n");
        }
    } else {
        let _ = writeln!(s, "{:<14} {:<9} {:<9} {:<15} Expires", "Neighbour", "Here", "There", "Can act as");
        for n in &list {
            let _ = writeln!(
                s,
                "{:<14} {:<9} {:<9} {:<15} {hold} s",
                n.info.system_name,
                ifname::short(&n.local),
                ifname::short(&n.info.port),
                caps(n.info.caps.0)
            );
        }
    }
    let _ = write!(s, "{} neighbour{}", list.len(), if list.len() == 1 { "" } else { "s" });
    s
}

/// `show processes [cpu]`: the protocol tasks this device is running. A simulated device has no
/// real load; the figures are small and steady so the command answers what students look for.
pub fn processes(t: &Topology, sim: &Sim, d: &Device) -> String {
    let mut tasks: Vec<&str> = vec!["Console", "IP input", "ARP"];
    if d.kind == DeviceKind::Switch {
        tasks.extend(["Spanning tree", "MAC address table"]);
    }
    if !d.cdp_off && d.kind != DeviceKind::Pc {
        tasks.push("CDP");
    }
    if d.lldp_on {
        tasks.push("LLDP");
    }
    if d.ospf.is_some() {
        tasks.push("OSPF");
    }
    if d.rip.is_some() {
        tasks.push("RIP");
    }
    if d.eigrp.is_some() {
        tasks.push("EIGRP");
    }
    if !d.dhcp_pools.is_empty() {
        tasks.push("DHCP server");
    }
    if !d.ntp_servers.is_empty() {
        tasks.push("NTP client");
    }
    let busy = 1
        + u64::from(d.ospf.is_some())
        + u64::from(d.rip.is_some())
        + u64::from(d.eigrp.is_some())
        + u64::try_from((0..d.ports.len()).filter(|i| t.peer(netlabcraft_model::End { device: d.id, port: *i }).is_some()).count() / 8).unwrap_or(0);
    let up = sim.now_us() / 1_000;
    let mut s = format!("CPU use: {busy}% over the last 5 s, {busy}% over 1 min, {busy}% over 5 min\n\n");
    let _ = writeln!(s, "{:>4}  {:<20} {:>12}", "Task", "Name", "Run time (ms)");
    for (n, name) in tasks.iter().enumerate() {
        let share = up / 1_000 / (n as u64 + 2);
        let _ = writeln!(s, "{:>4}  {:<20} {:>12}", n + 1, name, share);
    }
    s.trim_end().to_string()
}

/// `show memory [statistics]`: what the configuration and tables would take.
pub fn memory(d: &Device) -> String {
    let total: u64 = match d.kind {
        DeviceKind::Router => 512 * 1024,
        DeviceKind::Switch => 128 * 1024,
        _ => 64 * 1024,
    };
    let config = u64::try_from(running_config(d).len()).unwrap_or(0) / 1024 + 1;
    let used = 24 * 1024 + config * 4 + u64::try_from(d.ports.len()).unwrap_or(0) * 32;
    format!(
        "{:<12} {:>10} {:>10} {:>10}\n{:<12} {:>10} {:>10} {:>10}\n(kilobytes)",
        "",
        "Total",
        "Used",
        "Free",
        "Main memory",
        total,
        used,
        total.saturating_sub(used)
    )
}

/// `show standby`.
pub fn standby(d: &Device, roles: &[netlabcraft_sim::hsrp::Role]) -> String {
    let configured = d.ports.iter().any(|p| !p.standby.is_empty() || p.subifs.iter().any(|s| !s.standby.is_empty()))
        || d.svis.iter().any(|s| !s.standby.is_empty());
    if !configured {
        return "No HSRP group is configured here.".into();
    }
    if roles.is_empty() {
        return "HSRP groups are configured, but none is running (interface down, or no virtual address in the group).".into();
    }
    let mut s = String::new();
    for r in roles {
        let me = |a: Option<std::net::Ipv4Addr>, state| {
            if r.state == state { "this router".to_string() } else { a.map_or("none".to_string(), |x| x.to_string()) }
        };
        let _ =
            writeln!(s, "Group {} on {}: {} (priority {}{})", r.group, r.iface, r.state.name(), r.priority, if r.preempt { ", preempt" } else { "" });
        let _ = writeln!(s, "  Version {}, virtual address {}, virtual MAC {}", r.version, r.virtual_ip, r.virtual_mac().dotted());
        if !r.tracked_down.is_empty() {
            let _ = writeln!(s, "  Priority lowered from {}: tracked interface down ({})", r.configured_priority, r.tracked_down.join(", "));
        }
        let _ = writeln!(
            s,
            "  Active: {}; standby: {}",
            me(r.active, netlabcraft_sim::hsrp::State::Active),
            me(r.standby, netlabcraft_sim::hsrp::State::Standby)
        );
    }
    s.trim_end().to_string()
}

/// `show ipv6 access-list`.
pub fn ipv6_access_lists(sim: &Sim, d: &Device) -> String {
    if d.acls6.is_empty() {
        return "No IPv6 access lists configured.".into();
    }
    let hits = sim.state(d.id).map(|st| st.acl_hits.clone()).unwrap_or_default();
    let mut s = String::new();
    for a in &d.acls6 {
        let key = format!("ipv6 {}", a.name);
        let _ = writeln!(s, "IPv6 access list {}", a.name);
        for (i, e) in a.entries.iter().enumerate() {
            let n = hits.get(&(key.clone(), i)).copied().unwrap_or(0);
            let m = if n > 0 { format!(" ({n} match{})", if n == 1 { "" } else { "es" }) } else { String::new() };
            let _ = writeln!(s, "    {} {}{m}", (i + 1) * 10, e.text());
        }
        let implicit = hits.get(&(key.clone(), usize::MAX)).copied().unwrap_or(0);
        let _ = writeln!(
            s,
            "    (implicit) neighbour discovery passes, everything else is denied{}",
            if implicit > 0 { format!(" ({implicit} matches)") } else { String::new() }
        );
    }
    s.trim_end().to_string()
}

/// `show ip ospf`.
pub fn ospf_summary(d: &Device, st: &netlabcraft_sim::ospf::OspfState) -> String {
    let Some(o) = &d.ospf else { return "OSPF is not running here.".into() };
    let areas: std::collections::BTreeSet<u32> = o.networks.iter().map(|n| n.area).collect();
    let ifs = st.interfaces.get(&d.id).map_or(0, Vec::len);
    let mut s = format!("OSPF process {}\n", o.process);
    let _ = writeln!(s, "  Router id {}", st.router_ids.get(&d.id).map_or("none (no interface address)".to_string(), |r| r.to_string()));
    let _ = writeln!(s, "  Reference bandwidth {} Mbit/s", o.reference_bw.unwrap_or(100));
    let _ = writeln!(s, "  Areas: {}", areas.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "));
    let _ = writeln!(
        s,
        "  {ifs} interface{} running OSPF, {} neighbour{}",
        if ifs == 1 { "" } else { "s" },
        st.neighbors.get(&d.id).map_or(0, Vec::len),
        if st.neighbors.get(&d.id).map_or(0, Vec::len) == 1 { "" } else { "s" }
    );
    if areas.len() > 1 {
        s.push_str("  Joins several areas: an area border router\n");
    }
    let _ = write!(s, "  Routes are computed at once (no SPF timers)");
    s
}

/// `show ip ospf interface [brief]`.
pub fn ospf_interfaces(d: &Device, st: &netlabcraft_sim::ospf::OspfState) -> String {
    if d.ospf.is_none() {
        return "OSPF is not running here.".into();
    }
    let list = st.interfaces.get(&d.id).cloned().unwrap_or_default();
    if list.is_empty() {
        return "No interface runs OSPF (check the network statements and that interfaces are up).".into();
    }
    let mut s = format!("{:<10} {:<6} {:<20} {:<6} {:<9} {}\n", "Interface", "Area", "Network", "Cost", "Role", "Neighbours");
    for (name, net, cost, passive) in list {
        let seg = st.segments.get(&(d.id, name.clone()));
        let me = netlabcraft_sim::l3_ifaces(d).into_iter().find(|i| i.name == name).map(|i| i.net.addr);
        let role = if passive {
            "passive"
        } else {
            match seg {
                Some(g) if g.dr.is_none() => "p2p",
                Some(g) if g.dr == me => "DR",
                Some(g) if g.bdr == me => "BDR",
                Some(g) if g.priority == 0 => "never-DR",
                _ => "other",
            }
        };
        let n = st.neighbors.get(&d.id).map_or(0, |l| l.iter().filter(|x| x.iface == name).count());
        let _ = writeln!(s, "{:<10} {:<6} {:<20} {:<6} {:<9} {n}", ifname::short(&name), seg.map_or(0, |g| g.area), net.to_string(), cost, role);
    }
    s.trim_end().to_string()
}

/// `show ip ospf interface g0/0`: one interface's area, cost, timers, priority and the
/// segment's DR and BDR. `typed` is any accepted spelling of the name (`g0/0`, `vlan 10`).
pub fn ospf_interface(d: &Device, st: &netlabcraft_sim::ospf::OspfState, typed: &str) -> Result<String, String> {
    let Some(ospf) = d.ospf.as_ref() else { return Ok("OSPF is not running here.".into()) };
    let compact: String = typed.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    let split = compact.find(|c: char| c.is_ascii_digit()).unwrap_or(compact.len());
    let (word, rest) = compact.split_at(split);
    let list = st.interfaces.get(&d.id).cloned().unwrap_or_default();
    let found = list.into_iter().find(|(name, ..)| {
        let lower = name.to_ascii_lowercase();
        let at = lower.find(|c: char| c.is_ascii_digit()).unwrap_or(lower.len());
        !word.is_empty() && lower.get(..at).is_some_and(|w| w.starts_with(word)) && lower.get(at..) == Some(rest)
    });
    let Some((name, net, cost, passive)) = found else {
        return Err(format!("% OSPF doesn't run on {typed} (show ip ospf interface brief lists where it does)"));
    };
    let seg = st.segments.get(&(d.id, name.clone()));
    let me = st.addresses.get(&(d.id, name.clone())).copied();
    let rid = st.router_ids.get(&d.id).map_or("none".to_string(), |r| r.to_string());
    let (hello, dead) = netlabcraft_sim::ospf::timers(d, &name);
    let neighbours = st.neighbors.get(&d.id).map_or(0, |l| l.iter().filter(|x| x.iface == name).count());
    let mut s = String::new();
    let _ = writeln!(s, "{name}: OSPF process {}, area {}", ospf.process, seg.map_or(0, |g| g.area));
    let _ = writeln!(s, "  Address {}/{}, router ID {rid}, cost {cost}", me.map_or(net.addr.to_string(), |a| a.to_string()), net.prefix);
    match seg {
        _ if passive => s.push_str("  Passive: advertised, but no hellos are sent and no neighbours form\n"),
        Some(g) if g.dr.is_none() => s.push_str("  Point-to-point: no DR or BDR is elected\n"),
        Some(g) => {
            let role = if g.dr == me {
                "this router is the DR"
            } else if g.bdr == me {
                "this router is the BDR"
            } else if g.priority == 0 {
                "this router never becomes DR or BDR"
            } else {
                "this router is neither DR nor BDR (DROTHER)"
            };
            let addr = |a: Option<std::net::Ipv4Addr>| a.map_or("none".to_string(), |a| a.to_string());
            let _ = writeln!(s, "  Broadcast segment, priority {}: {role}", g.priority);
            let _ = writeln!(s, "  DR {}, BDR {}", addr(g.dr), addr(g.bdr));
        }
        None => {}
    }
    let _ = writeln!(s, "  Hello every {hello} s, dead after {dead} s");
    let _ = write!(s, "  Neighbours on this interface: {neighbours}");
    Ok(s)
}

/// `show ipv6 dhcp pool`.
pub fn dhcp6_pools(sim: &Sim, d: &Device) -> String {
    if d.dhcp6_pools.is_empty() {
        return "No DHCPv6 pools configured.".into();
    }
    let leases = sim.state(d.id).map(|s| s.dhcp6_leases.clone()).unwrap_or_default();
    let mut s = String::new();
    for p in &d.dhcp6_pools {
        let used = leases.values().filter(|(_, pool)| *pool == p.name).count();
        let _ = writeln!(s, "DHCPv6 pool {}", p.name);
        let _ = writeln!(s, "  Addresses: {}", p.prefix.map_or("none (stateless: settings only)".to_string(), |n| format!("{n}, {used} handed out")));
        let _ = writeln!(
            s,
            "  DNS server: {}",
            if p.dns.is_empty() { "none".to_string() } else { p.dns.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ") }
        );
        let _ = writeln!(s, "  Domain: {}", p.domain.as_deref().unwrap_or("none"));
        let users: Vec<String> =
            d.ports.iter().filter(|x| x.ipv6.dhcp_server.as_deref() == Some(p.name.as_str())).map(|x| ifname::short(&x.name)).collect();
        let _ = writeln!(s, "  Served on: {}", if users.is_empty() { "no interface yet".to_string() } else { users.join(", ") });
    }
    s.trim_end().to_string()
}

/// `show ipv6 dhcp binding`.
pub fn dhcp6_bindings(sim: &Sim, d: &Device) -> String {
    let leases = sim.state(d.id).map(|s| s.dhcp6_leases.clone()).unwrap_or_default();
    if leases.is_empty() {
        return "No DHCPv6 addresses handed out.".into();
    }
    let mut s = format!("{:<28} {:<19} {}\n", "Address", "Client MAC", "Pool");
    for (a, (m, pool)) in &leases {
        let _ = writeln!(s, "{:<28} {:<19} {pool}", a.to_string(), m.dotted());
    }
    s.trim_end().to_string()
}

// ---- site-to-site VPN: GRE tunnels and IPsec ----

/// The VPN part of the running configuration: IPsec settings, then the tunnel interfaces.
fn vpn_config(s: &mut String, d: &Device) {
    let c = &d.crypto;
    for p in &c.policies {
        let _ = writeln!(s, "crypto isakmp policy {}", p.priority);
        let _ = writeln!(s, " encryption {}", p.encryption);
        let _ = writeln!(s, " hash {}", p.hash);
        let _ = writeln!(s, " authentication {}", p.authentication);
        let _ = writeln!(s, " group {}", p.group);
        if let Some(l) = p.lifetime {
            let _ = writeln!(s, " lifetime {l}");
        }
        let _ = writeln!(s, "!");
    }
    for (a, k) in &c.keys {
        let _ = writeln!(s, "crypto isakmp key {k} address {a}");
    }
    if !c.keys.is_empty() {
        let _ = writeln!(s, "!");
    }
    for t in &c.transform_sets {
        let _ = writeln!(s, "crypto ipsec transform-set {} {}", t.name, t.transforms.join(" "));
        if t.transport {
            let _ = writeln!(s, " mode transport");
        }
    }
    if let Some(n) = c.sa_lifetime.seconds {
        let _ = writeln!(s, "crypto ipsec security-association lifetime seconds {n}");
    }
    if let Some(n) = c.sa_lifetime.kilobytes {
        let _ = writeln!(s, "crypto ipsec security-association lifetime kilobytes {n}");
    }
    if !c.transform_sets.is_empty() || !c.sa_lifetime.is_unset() {
        let _ = writeln!(s, "!");
    }
    crate::exec::ikev2::running_config(s, d);
    for e in &c.maps {
        let _ = writeln!(s, "crypto map {} {} ipsec-isakmp", e.map, e.seq);
        if let Some(p) = e.peer {
            let _ = writeln!(s, " set peer {p}");
        }
        if let Some(t) = &e.transform_set {
            let _ = writeln!(s, " set transform-set {t}");
        }
        if let Some(a) = &e.acl {
            let _ = writeln!(s, " match address {a}");
        }
        if let Some(p) = &e.ikev2_profile {
            let _ = writeln!(s, " set ikev2-profile {p}");
        }
        if let Some(n) = e.lifetime.seconds {
            let _ = writeln!(s, " set security-association lifetime seconds {n}");
        }
        if let Some(n) = e.lifetime.kilobytes {
            let _ = writeln!(s, " set security-association lifetime kilobytes {n}");
        }
        let _ = writeln!(s, "!");
    }
    for t in &d.tunnels {
        let _ = writeln!(s, "interface {}", t.name());
        if !t.description.is_empty() {
            let _ = writeln!(s, " description {}", t.description);
        }
        match t.ip {
            Some(ip) => {
                let _ = writeln!(s, " ip address {} {}", ip.addr, ip.mask());
            }
            None => {
                let _ = writeln!(s, " no ip address");
            }
        }
        ospf_if_lines(s, d, &t.name());
        match &t.source {
            Some(netlabcraft_model::vpn::TunnelSource::Interface(n)) => {
                let _ = writeln!(s, " tunnel source {n}");
            }
            Some(netlabcraft_model::vpn::TunnelSource::Address(a)) => {
                let _ = writeln!(s, " tunnel source {a}");
            }
            None => {}
        }
        if let Some(a) = t.destination {
            let _ = writeln!(s, " tunnel destination {a}");
        }
        crate::exec::qos::service_policy_lines(s, d, &t.name());
        crate::exec::security::iface_lines(s, d, &t.name());
        if t.shutdown {
            let _ = writeln!(s, " shutdown");
        }
        let _ = writeln!(s, "!");
    }
}

/// ` crypto map NAME` under an interface that has one.
fn crypto_map_line(s: &mut String, d: &Device, name: &str) {
    if let Some(m) = d.crypto.applied.get(name) {
        let _ = writeln!(s, " crypto map {m}");
    }
}

/// One tunnel's `show interfaces` block.
fn tunnel_block(t: &Topology, d: &Device, tun: &netlabcraft_model::vpn::Tunnel) -> String {
    let mut s = String::new();
    let state = match netlabcraft_sim::vpn::tunnel_status(t, d, tun.number) {
        Ok(_) => "up, line protocol up".to_string(),
        Err(_) if tun.shutdown => "administratively down".to_string(),
        Err(why) => format!("up, line protocol down ({why})"),
    };
    let _ = writeln!(s, "{} is {state}", tun.name());
    if !tun.description.is_empty() {
        let _ = writeln!(s, "  Description: {}", tun.description);
    }
    match tun.ip {
        Some(ip) => {
            let _ = writeln!(s, "  Internet address {ip}");
        }
        None => s.push_str("  No address\n"),
    }
    let src = match &tun.source {
        Some(netlabcraft_model::vpn::TunnelSource::Interface(n)) => {
            let addr = netlabcraft_sim::up_ifaces(t, d).into_iter().find(|i| i.name == *n).map(|i| format!(" ({})", i.net.addr)).unwrap_or_default();
            format!("{n}{addr}")
        }
        Some(netlabcraft_model::vpn::TunnelSource::Address(a)) => a.to_string(),
        None => "not set".to_string(),
    };
    let dst = tun.destination.map_or("not set".to_string(), |a| a.to_string());
    let _ = writeln!(s, "  Tunnel source {src}, destination {dst}");
    s.push_str("  Encapsulation GRE over IPv4 (IP protocol 47), nominal bandwidth 100 kbit/s\n");
    s
}

/// `show interfaces tunnel N`.
pub fn tunnel_interface(t: &Topology, d: &Device, n: u32) -> Result<String, String> {
    let tun = d.tunnels.iter().find(|x| x.number == n).ok_or_else(|| format!("% There is no interface Tunnel{n}"))?;
    Ok(tunnel_block(t, d, tun).trim_end().to_string())
}

/// `show crypto isakmp sa`: one row per IPsec peer traffic has tried to reach.
pub fn crypto_isakmp_sa(sim: &Sim, d: &Device) -> String {
    let sas = sim.ipsec_sas(d.id);
    if sas.is_empty() {
        return "No IKE peers yet (an association is set up when traffic matches a crypto map)".into();
    }
    let mut s = format!("{:<16} {:<16} {}\n", "Peer", "Local", "State");
    for sa in &sas {
        let state = match (&sa.failed, sa.phase) {
            (_, netlabcraft_sim::vpn::IkePhase::Established) => match sa.v1.as_deref().and_then(|v| v.suite) {
                Some(suite) => format!("established (pre-shared key; {})", suite.describe()),
                None => "established (pre-shared key)".to_string(),
            },
            (Some(why), _) => format!("failed: {why}"),
            (None, phase) => format!("negotiating: {}", phase.describe()),
        };
        let _ = writeln!(s, "{:<16} {:<16} {state}", sa.peer, sa.local);
        if sa.phase == netlabcraft_sim::vpn::IkePhase::Established && sa.v1.is_some() {
            let (i, r) = sa.icookie_or_spi();
            let _ = writeln!(s, "{:34}cookies {}/{}; {}", "", hex8(&i), hex8(&r), ike_sa_life(sim, sa));
            if let Some(next) = sa.next_phase() {
                let _ = writeln!(s, "{:34}replacement under way: {next}", "");
            }
        }
    }
    s.trim_end().to_string()
}

/// The AH half of an AH-and-ESP bundle, for the `show crypto` reports: the algorithm, the SPIs
/// of both directions and the one before the last rekey.
pub(crate) fn ah_half_line(sa: &netlabcraft_sim::vpn::IpsecSa) -> Option<String> {
    let ah = sa.ah.as_ref()?;
    let mut line = format!("AH: {} outside ESP (real); outbound AH SPI 0x{:08x}, inbound AH SPI 0x{:08x}", ah.integ.name(), ah.spi_out, ah.spi_in);
    if let Some(prev) = &ah.prev_in {
        let _ = write!(line, "; previous inbound AH SPI 0x{:08x} (accepted until the peer deletes it)", prev.0);
    }
    Some(line)
}

fn hex8(b: &[u8; 8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The age, lifetime and history of an ISAKMP or IKE SA, for `show crypto isakmp sa` and
/// `show crypto ikev2 sa`.
pub(crate) fn ike_sa_life(sim: &Sim, sa: &netlabcraft_sim::vpn::IpsecSa) -> String {
    let now = sim.now_us();
    let age = now.saturating_sub(sa.ike_up_us) / 1_000_000;
    let mut line = String::new();
    if sa.ike_life != 0 {
        let state = match sa.ike_life_state(now) {
            netlabcraft_sim::vpn::Life::Expired => ", expired",
            netlabcraft_sim::vpn::Life::Soft if sa.ike_rekey_us != 0 => ", replacement being negotiated",
            _ => "",
        };
        let _ = write!(line, "age {age} s, {} of {} s left{state}; ", sa.ike_remaining(now), sa.ike_life);
    }
    let _ = write!(line, "replaced {} time{}", sa.ike_rekeys, if sa.ike_rekeys == 1 { "" } else { "s" });
    if let Some((i, r)) = &sa.ike_prev {
        let _ = write!(line, " (previous {}/{})", hex8(i), hex8(r));
    }
    line
}

/// `show crypto ipsec sa`: per peer, the SPIs and packet counts.
pub fn crypto_ipsec_sa(sim: &Sim, d: &Device) -> String {
    let sas = sim.ipsec_sas(d.id);
    if sas.is_empty() {
        return "No IPsec security associations".into();
    }
    let mut s = String::new();
    for sa in &sas {
        let _ = writeln!(
            s,
            "Peer {} (local {}){}",
            sa.peer,
            sa.local,
            if sa.iface.is_empty() { String::new() } else { format!(", interface {}", sa.iface) }
        );
        if !sa.map.is_empty() {
            let _ = writeln!(s, "  Crypto map {} entry {}", sa.map, sa.seq);
        }
        match &sa.failed {
            Some(why) => {
                let _ = writeln!(s, "  No association: {why}");
            }
            None if sa.spi_out != 0 => {
                let _ = writeln!(s, "  Outbound ESP SPI 0x{:08x}, inbound ESP SPI 0x{:08x}", sa.spi_out, sa.spi_in);
            }
            None => {}
        }
        if sa.phase == netlabcraft_sim::vpn::IkePhase::Established && sa.life != (0, 0) {
            let (secs, kb) = sa.remaining(sim.now_us());
            let state = match sa.life_state(sim.now_us()) {
                netlabcraft_sim::vpn::Life::Expired => ", expired",
                netlabcraft_sim::vpn::Life::Soft if sa.rekey_us != 0 => ", new keys being negotiated",
                _ => "",
            };
            let _ = writeln!(
                s,
                "  Lifetime left: {secs} of {} seconds, {kb} of {} kilobytes{state}; keys renewed {} time{}",
                sa.life.0,
                sa.life.1,
                sa.rekeys,
                if sa.rekeys == 1 { "" } else { "s" }
            );
            if let Some(prev) = &sa.prev_in {
                let _ = writeln!(s, "  Previous inbound ESP SPI 0x{:08x} (accepted until the peer deletes it)", prev.0);
            }
        }
        if sa.phase == netlabcraft_sim::vpn::IkePhase::Established {
            let suite = sa.v1.as_deref().map(|v| v.esp_suite).or_else(|| sa.v2.as_deref().map(|v| v.esp_suite)).flatten();
            let esp = match suite {
                Some(suite) => format!("{} (real ciphers)", suite.describe()),
                None => "the simulator's stand-in cipher (the transform set has no cipher)".to_string(),
            };
            let _ = writeln!(s, "  ESP: {esp}");
            if let Some(line) = ah_half_line(sa) {
                let _ = writeln!(s, "  {line}");
            }
        }
        let _ = writeln!(s, "  Packets encapsulated {}, decapsulated {}, dropped {}", sa.encaps, sa.decaps, sa.drops);
        let _ = writeln!(s);
    }
    s.trim_end().to_string()
}

/// `show crypto isakmp policy`.
pub fn isakmp_policies(d: &Device) -> String {
    if d.crypto.policies.is_empty() {
        return "No ISAKMP policies configured".into();
    }
    let mut s = String::new();
    for p in &d.crypto.policies {
        let _ = writeln!(
            s,
            "Policy {}: encryption {}, hash {}, authentication {}, DH group {}, lifetime {} s",
            p.priority,
            p.encryption,
            p.hash,
            p.authentication,
            p.group,
            p.lifetime.unwrap_or(86_400)
        );
    }
    s.trim_end().to_string()
}

/// `show crypto map`.
pub fn crypto_maps(d: &Device) -> String {
    let c = &d.crypto;
    if c.maps.is_empty() {
        return "No crypto maps configured".into();
    }
    let mut s = String::new();
    for e in &c.maps {
        let _ = writeln!(s, "Crypto map {} entry {} (IPsec with IKE)", e.map, e.seq);
        let _ = writeln!(s, "  Peer: {}", e.peer.map_or("not set".to_string(), |p| p.to_string()));
        let ts = e.transform_set.as_deref().map_or("not set".to_string(), |n| match c.transform_set(n) {
            Some(t) => format!("{n} ({}{})", t.transforms.join(" "), if t.transport { ", transport mode" } else { "" }),
            None => format!("{n} (missing)"),
        });
        let _ = writeln!(s, "  Transform set: {ts}");
        let _ = writeln!(s, "  Protects traffic matching: {}", e.acl.as_deref().unwrap_or("nothing (no `match address`)"));
        let on: Vec<&str> = c.applied.iter().filter(|(_, m)| **m == e.map).map(|(i, _)| i.as_str()).collect();
        let _ = writeln!(s, "  Applied to: {}", if on.is_empty() { "no interface".to_string() } else { on.join(", ") });
    }
    s.trim_end().to_string()
}

/// Wireless-controller lines: a controller's WLANs, an access point's controller.
fn wlan_config(s: &mut String, d: &Device) {
    use netlabcraft_model::wlan::WlanSecurity;
    for w in &d.wlan.wlans {
        let sec = match &w.security {
            WlanSecurity::Open => "open".to_string(),
            WlanSecurity::Psk(k) => format!("psk {k}"),
            WlanSecurity::Enterprise => "enterprise".to_string(),
        };
        let vlan = w.vlan.map(|v| format!(" vlan {v}")).unwrap_or_default();
        let central = if w.central { " central" } else { "" };
        let _ = writeln!(s, "wlan {} {} {sec}{vlan}{central}", w.id, w.ssid);
    }
    if let Some(a) = d.wlan.controller {
        let _ = writeln!(s, "capwap controller {a}");
    }
    if !d.wlan.is_default() {
        let _ = writeln!(s, "!");
    }
}
