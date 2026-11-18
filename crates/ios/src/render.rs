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
    crate::exec::netflow::running_config(&mut s, d);
    crate::exec::keychain::config_lines(&mut s, d);
    let before = s.len();
    crate::exec::mgmt::running_config(&mut s, d);
    if !d.ntp_servers.is_empty() || !d.log_hosts.is_empty() || s.len() != before {
        let _ = writeln!(s, "!");
    }
    crate::exec::knobs::global_config(&mut s, d);
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
    for (n, ip) in &a.hosts {
        let _ = writeln!(s, "ip host {n} {ip}");
    }
    if !a.name_servers.is_empty() {
        let list: Vec<String> = a.name_servers.iter().map(ToString::to_string).collect();
        let _ = writeln!(s, "ip name-server {}", list.join(" "));
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
    if a.enable_secret.is_some()
        || a.enable_password.is_some()
        || a.no_domain_lookup
        || a.domain_name.is_some()
        || !a.hosts.is_empty()
        || !a.name_servers.is_empty()
        || !a.users.is_empty()
    {
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
    if d.mst.on {
        let _ = writeln!(s, "spanning-tree mode mst");
        crate::exec::mst::running_config(&mut s, d);
    } else if d.rapid_stp {
        let _ = writeln!(s, "spanning-tree mode rapid-pvst");
    }
    for (v, p) in &d.stp_priority {
        let _ = writeln!(s, "spanning-tree vlan {v} priority {p}");
    }
    if !d.stp_priority.is_empty() {
        let _ = writeln!(s, "!");
    }
    crate::exec::l2sec::global_lines(&mut s, d);
    crate::exec::igmp::global_lines(&mut s, d);
    crate::exec::archive::running_config(&mut s, d);
    crate::exec::srcguard::global_lines(&mut s, d);
    crate::exec::l2sec::relay_global_lines(&mut s, d);
    crate::exec::span::global_lines(&mut s, d);
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
    crate::exec::pppoe::running_config(&mut s, d);
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
        if p.single_mode {
            let _ = writeln!(s, " transceiver type single-mode");
        }
        match p.keepalive {
            Some(0) => {
                let _ = writeln!(s, " no keepalive");
            }
            Some(n) => {
                let _ = writeln!(s, " keepalive {n}");
            }
            None => {}
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
        crate::exec::pppoe::port_lines(&mut s, d, &p.name);
        standby_lines(&mut s, d, &p.name, &p.standby);
        crate::exec::fhrp::iface_lines(&mut s, d, &p.name);
        crate::exec::pbr::iface_line(&mut s, d, &p.name);
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
        crate::exec::netflow::iface_lines(&mut s, d, &p.name);
        crate::exec::knobs::iface_lines(&mut s, d, &p.name);
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
            crate::exec::pbr::iface_line(&mut s, d, &format!("{}.{}", p.name, sub.number));
            ospf_if_lines(&mut s, d, &format!("{}.{}", p.name, sub.number));
            for h in &sub.helpers {
                let _ = writeln!(s, " ip helper-address {h}");
            }
            access_groups(&mut s, &sub.access);
            nat_side(&mut s, sub.nat);
            crypto_map_line(&mut s, d, &format!("{}.{}", p.name, sub.number));
            crate::exec::qos::service_policy_lines(&mut s, d, &format!("{}.{}", p.name, sub.number));
            crate::exec::security::iface_lines(&mut s, d, &format!("{}.{}", p.name, sub.number));
            crate::exec::netflow::iface_lines(&mut s, d, &format!("{}.{}", p.name, sub.number));
            crate::exec::knobs::iface_lines(&mut s, d, &format!("{}.{}", p.name, sub.number));
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
        crate::exec::knobs::iface_lines(&mut s, d, &format!("Vlan{}", svi.vlan));
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
    let to = &d.nat.timeouts;
    for (word, v) in [("timeout", to.all), ("udp-timeout", to.udp), ("icmp-timeout", to.icmp), ("tcp-timeout", to.tcp)] {
        if let Some(v) = v {
            let _ = writeln!(s, "ip nat translation {word} {v}");
        }
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
        crate::exec::redist::config_lines(&mut s, &o.redistribute, o.default_metric.map(|m| m.to_string()), &o.distribute, true);
        if o.default_originate {
            let _ = writeln!(s, " default-information originate");
        }
        if let Some(r) = o.reference_bw {
            let _ = writeln!(s, " auto-cost reference-bandwidth {r}");
        }
        if let Some(n) = o.maximum_paths {
            let _ = writeln!(s, " maximum-paths {n}");
        }
        if let Some((a, b, c)) = o.spf_throttle {
            let _ = writeln!(s, " timers throttle spf {a} {b} {c}");
        }
        for (n, a) in &o.areas {
            if a.stub {
                let _ = writeln!(s, " area {n} stub{}", if a.no_summary { " no-summary" } else { "" });
            }
            if a.nssa {
                let _ = writeln!(
                    s,
                    " area {n} nssa{}{}{}{}{}",
                    if a.no_summary { " no-summary" } else { "" },
                    if a.nssa_no_redist { " no-redistribution" } else { "" },
                    if a.nssa_default { " default-information-originate" } else { "" },
                    if a.nssa_default && a.nssa_default_only { " nssa-only" } else { "" },
                    if a.nssa_translate_always { " translate type7 always" } else { "" }
                );
            }
            if let Some(cost) = a.default_cost {
                let _ = writeln!(s, " area {n} default-cost {cost}");
            }
            for peer in &a.virtual_links {
                let mut line = format!(" area {n} virtual-link {peer}");
                if let Some(v) = a.vlink_opts.iter().find(|v| v.peer == *peer) {
                    if let Some(x) = v.hello {
                        let _ = write!(line, " hello-interval {x}");
                    }
                    if let Some(x) = v.retransmit {
                        let _ = write!(line, " retransmit-interval {x}");
                    }
                    if let Some(x) = v.transmit_delay {
                        let _ = write!(line, " transmit-delay {x}");
                    }
                    if let Some(x) = v.dead {
                        let _ = write!(line, " dead-interval {x}");
                    }
                    match v.auth {
                        Some(netlabcraft_model::OspfAuthKind::Simple) => line.push_str(" authentication"),
                        Some(netlabcraft_model::OspfAuthKind::Md5) => line.push_str(" authentication message-digest"),
                        Some(netlabcraft_model::OspfAuthKind::Null) => line.push_str(" authentication null"),
                        None => {}
                    }
                    if let Some(k) = &v.auth_key {
                        let _ = write!(line, " authentication-key {k}");
                    }
                    for (id, k) in &v.md5_keys {
                        let _ = write!(line, " message-digest-key {id} md5 {k}");
                    }
                }
                let _ = writeln!(s, "{line}");
            }
            for r in &a.ranges {
                let _ = writeln!(s, " area {n} range {} {}", r.addr, r.mask());
            }
            match a.auth {
                Some(netlabcraft_model::OspfAuthKind::Md5) => {
                    let _ = writeln!(s, " area {n} authentication message-digest");
                }
                Some(_) => {
                    let _ = writeln!(s, " area {n} authentication");
                }
                None => {}
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
        crate::exec::redist::config_lines(&mut s, &e.redistribute, e.default_metric.map(|m| m.words()), &e.distribute, false);
        if let Some(n) = e.variance {
            let _ = writeln!(s, " variance {n}");
        }
        if let Some(n) = e.maximum_paths {
            let _ = writeln!(s, " maximum-paths {n}");
        }
        let _ = writeln!(s, "{}", if e.auto_summary { " auto-summary" } else { " no auto-summary" });
        crate::exec::knobs::proto_lines(&mut s, d, "eigrp");
        let _ = writeln!(s, "!");
    }
    crate::exec::bgp::running_config(&mut s, d);
    crate::exec::policy::running_config(&mut s, d);
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
        crate::exec::redist::config_lines(&mut s, &r.redistribute, r.default_metric.map(|m| m.to_string()), &r.distribute, false);
        if !r.auto_summary {
            let _ = writeln!(s, " no auto-summary");
        }
        if let Some([u, i, h, f]) = r.timers {
            let _ = writeln!(s, " timers basic {u} {i} {h} {f}");
        }
        crate::exec::knobs::proto_lines(&mut s, d, "rip");
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
        let tag = if r.tag == 0 { String::new() } else { format!(" tag {}", r.tag) };
        let _ = writeln!(s, "ipv6 route {} {via}{distance}{tag}", r.net);
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
        for (area, a) in &o.area_auth {
            match a {
                netlabcraft_model::Ospf6Auth::Null => {
                    let _ = writeln!(s, " area {area} authentication null");
                }
                a => {
                    let _ = writeln!(s, " area {area} authentication {}", ospf6_auth_words(a));
                }
            }
        }
        let _ = writeln!(s, "!");
    }
    crate::exec::sla::running_config(&mut s, d);
    crate::exec::pbr::local_line(&mut s, d);
    for dn in &d.default_networks {
        let _ = writeln!(s, "ip default-network {dn}");
    }
    crate::exec::track::running_config(&mut s, d);
    for r in &d.routes {
        let via = match &r.via {
            NextHop::Address(a) => a.to_string(),
            NextHop::Interface(n) => n.clone(),
        };
        let distance = r.distance.map_or(String::new(), |a| format!(" {a}"));
        let tag = if r.tag == 0 { String::new() } else { format!(" tag {}", r.tag) };
        let permanent = if d.track.is_permanent(&r.net, &r.via) { " permanent" } else { "" };
        let track = d.track.follows(&r.net, &r.via).map_or(String::new(), |n| format!(" track {n}"));
        let _ = writeln!(s, "ip route {} {} {via}{distance}{tag}{permanent}{track}", r.net.addr, r.net.mask());
    }
    if !d.routes.is_empty() {
        let _ = writeln!(s, "!");
    }
    crate::exec::automation::running_config(&mut s, d);
    crate::exec::pki::running_config(&mut s, d);
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
        crate::exec::knobs::line_lines(&mut s, d, if name == "line con 0" { "con" } else { "vty" });
        let _ = writeln!(s, "!");
    }
    // The asynchronous lines of an octal card that were set.
    for n in 1..=d.octal_lines().min(16) {
        let group = format!("tty{n}");
        let transport = a.async_transport.get(&n);
        let mut knob = String::new();
        crate::exec::knobs::line_lines(&mut knob, d, &group);
        if transport.is_none() && knob.is_empty() {
            continue;
        }
        let _ = writeln!(s, "line {n}");
        if let Some(tr) = transport {
            let names: Vec<&str> = tr.iter().map(|p| p.name()).collect();
            let _ = writeln!(s, " transport input {}", if names.is_empty() { "none".to_string() } else { names.join(" ") });
        }
        s.push_str(&knob);
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
    crate::exec::pppoe::brief(&mut s, d);
    s.trim_end().to_string()
}

/// The routing table, from the rows the simulation has (a row per path when several share the
/// traffic).
/// Bytes `show ip route summary` counts for one route in its memory estimate.
const ROUTE_ENTRY_BYTES: usize = 160;

/// `show ip route summary`: how many routes each source gave, how many there are in all and per mask length, and an estimate of the memory they take.
pub fn ip_route_summary(table: &[netlabcraft_sim::RouteEntry]) -> String {
    let count = |codes: &[char]| table.iter().filter(|r| codes.contains(&r.code)).count();
    let rows = [
        ("connected", count(&['C']), String::new()),
        ("local", count(&['L']), String::new()),
        ("static", count(&['S']), String::new()),
        ("RIP", count(&['R']), String::new()),
        (
            "EIGRP",
            count(&['D', 'X']),
            if count(&['X']) > 0 { format!("internal {}, external {}", count(&['D']), count(&['X'])) } else { String::new() },
        ),
        ("BGP", count(&['B']), String::new()),
        (
            "OSPF",
            count(&['O', 'I', 'E', 'F', 'N', 'M']),
            format!("intra-area {}, inter-area {}, external {}", count(&['O']), count(&['I']), count(&['E', 'F', 'N', 'M'])),
        ),
    ];
    let other = table.len().saturating_sub(rows.iter().map(|r| r.1).sum::<usize>());
    let mut s = format!("{:<10} {:<7} Detail\n", "Source", "Routes");
    for (name, n, detail) in rows.iter().filter(|r| r.1 > 0) {
        let _ = writeln!(s, "{name:<10} {n:<7} {detail}");
    }
    if other > 0 {
        let _ = writeln!(s, "{:<10} {other:<7}", "other");
    }
    let prefixes: std::collections::BTreeSet<_> = table.iter().map(|r| r.net.to_string()).collect();
    let _ = writeln!(s, "{:<10} {:<7} {} distinct prefix{}", "total", table.len(), prefixes.len(), if prefixes.len() == 1 { "" } else { "es" });
    // How many routes each mask length has.
    let mut by_mask: std::collections::BTreeMap<u8, usize> = std::collections::BTreeMap::new();
    for r in table {
        *by_mask.entry(r.net.prefix).or_insert(0) += 1;
    }
    s.push_str("Routes by mask:\n");
    for (prefix, n) in &by_mask {
        let _ = writeln!(s, "  /{prefix:<3} {n}");
    }
    // Not a measurement: the simulator keeps no memory pool, so the figure is a count times a fixed size.
    let _ = writeln!(
        s,
        "Memory: {} routes x {ROUTE_ENTRY_BYTES} bytes = {} bytes (simulated estimate, not a measurement)",
        table.len(),
        table.len().saturating_mul(ROUTE_ENTRY_BYTES)
    );
    match table.iter().find(|r| r.net.prefix == 0) {
        Some(r) => {
            let via = r.via.map(|v| format!("via {v}")).or_else(|| r.iface.as_ref().map(|i| format!("out {i}"))).unwrap_or_default();
            let _ = write!(s, "Default route: {via}");
        }
        None => s.push_str("No default route"),
    }
    s
}

pub fn ip_route_table(d: &Device, table: &[netlabcraft_sim::RouteEntry]) -> String {
    let mut s = String::from(
        "Codes: C connected, L local, S static, R RIP, D EIGRP, B BGP, D EX EIGRP external, O OSPF, O IA OSPF other area, O E1 and O E2 OSPF external, O N1 and O N2 OSPF external from a not-so-stubby area, O*E2 OSPF external default, O*N2 default into a not-so-stubby area, O*IA default into a stub area, * the default route\n\n",
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
        let _ = writeln!(s, "{}", route_line(r));
    }
    s.trim_end().to_string()
}

/// One row of `show ip route`.
fn route_line(r: &netlabcraft_sim::RouteEntry) -> String {
    let what = match (r.code, r.via, &r.iface) {
        ('C', _, Some(i)) => format!("connected, {i}"),
        ('L', _, Some(i)) => format!("local address, {i}"),
        ('O' | 'E' | 'F' | 'I' | 'N' | 'M', Some(v), Some(i)) => format!("via {v}, {i} (OSPF)"),
        ('D' | 'X', Some(v), Some(i)) => format!("via {v}, {i} (EIGRP)"),
        ('D', None, Some(i)) => format!("summary, discarded on {i} (EIGRP)"),
        ('B', Some(v), Some(i)) => format!("via {v}, {i} (BGP)"),
        ('B', None, Some(i)) => format!("summary, discarded on {i} (BGP)"),
        (_, Some(v), _) => format!("via {v}"),
        (_, None, Some(i)) => format!("out {i}"),
        _ => String::new(),
    };
    let code = match r.code {
        'E' if r.net.prefix == 0 => "O*E2".to_string(),
        'E' => "O E2".to_string(),
        'F' => "O E1".to_string(),
        'N' if r.net.prefix == 0 => "O*N2".to_string(),
        'N' => "O N2".to_string(),
        'M' => "O N1".to_string(),
        'X' if r.net.prefix == 0 => "D*EX".to_string(),
        'X' => "D EX".to_string(),
        'I' if r.net.prefix == 0 => "O*IA".to_string(),
        'I' => "O IA".to_string(),
        // The default route is starred whoever put it there (S*, R*, D*).
        c if r.net.prefix == 0 => format!("{c}*  "),
        c => format!("{c}   "),
    };
    // Distance and metric, for every route a protocol or a person put there.
    let ad = if matches!(r.code, 'C' | 'L') { String::new() } else { format!(" [{}/{}]", r.distance, r.metric) };
    let tag = if r.tag == 0 { String::new() } else { format!(" tag {}", r.tag) };
    format!("{code} {:<20} {what}{ad}{tag}", r.net.to_string())
}

/// `show ip route tag N`: the rows of the table that carry this tag.
pub fn ip_route_tagged(table: &[netlabcraft_sim::RouteEntry], tag: u32) -> String {
    let rows: Vec<String> = table.iter().filter(|r| r.tag == tag).map(route_line).collect();
    if rows.is_empty() {
        return format!("No route carries tag {tag}");
    }
    format!("Routes with tag {tag}:\n{}", rows.join("\n"))
}

/// A port's bandwidth (kbit/s) and delay (µs): what the port was told, else what its type usually has.
fn port_metrics(p: &netlabcraft_model::Port) -> (u32, u64) {
    let (bw, delay) = netlabcraft_sim::eigrp::default_metrics(&p.name);
    (p.bandwidth.unwrap_or(bw).max(1), u64::from(p.delay.unwrap_or(delay)).saturating_mul(10))
}

/// Load out of 255 (1 when idle): the average bit rate since the counters began against the bandwidth.
fn load_of(octets: u64, elapsed_us: u64, bw_kbit: u32) -> u64 {
    if elapsed_us == 0 {
        return 1;
    }
    let num = u128::from(octets).saturating_mul(8).saturating_mul(1_000_000).saturating_mul(255);
    let den = u128::from(elapsed_us).saturating_mul(u128::from(bw_kbit.max(1))).saturating_mul(1_000);
    u64::try_from(num / den.max(1)).unwrap_or(255).clamp(1, 255)
}

/// Simulated microseconds as `h:mm:ss`.
pub(crate) fn hms(us: u64) -> String {
    let s = us / 1_000_000;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// Interface details: all of them, or one.
pub fn interfaces(t: &Topology, sim: &Sim, d: &Device, only: Option<IfRef>) -> String {
    let mut s = String::new();
    for (i, p) in d.ports.iter().enumerate() {
        if only.is_some_and(|r| r.port != i) {
            continue;
        }
        let end = End { device: d.id, port: i };
        // The port's status (what the cable and the administrator say) and its line protocol
        // (whether frames can be exchanged): the two words students filter on.
        let (status, proto) = if p.err_disabled {
            (format!("down (err-disabled by {})", crate::exec::l2sec::cause_words(p.l2.err_cause)), "down (err-disabled)".to_string())
        } else if p.shutdown {
            ("administratively down".to_string(), "down".to_string())
        } else if port_up(t, end) {
            ("up".to_string(), "up".to_string())
        } else if t.peer(end).is_none() {
            ("down (no cable)".to_string(), "down".to_string())
        } else if let Some(why) = netlabcraft_sim::serial_problem(t, end) {
            ("up".to_string(), format!("down ({why})"))
        } else if let Some(why) = netlabcraft_sim::cable_problem(t, end) {
            (format!("down ({why})"), "down".to_string())
        } else if netlabcraft_sim::speed_mismatch(t, end) {
            ("down (speed mismatch with the far end)".to_string(), "down".to_string())
        } else {
            ("down (far end shut down)".to_string(), "down".to_string())
        };
        if only.is_none_or(|r| r.sub.is_none()) {
            let _ = writeln!(s, "{} is {status}, line protocol is {proto}", p.name);
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
            let (bw, delay_us) = port_metrics(p);
            let cnt = sim.port_counters(d.id, i);
            let cleared = sim.port_counters_cleared_at(d.id);
            let elapsed = sim.now_us().saturating_sub(cleared.unwrap_or(0));
            s.push_str("  MTU 1500 bytes");
            if let Some(m) = d.security.ip_mtu.get(&p.name) {
                let _ = write!(s, " (IP MTU {m})");
            }
            let _ = writeln!(s, ", BW {bw} Kbit/sec, DLY {delay_us} usec");
            let _ = writeln!(
                s,
                "  Reliability 255/255, txload {}/255, rxload {}/255 (averaged since the counters began)",
                load_of(cnt.out_octets, elapsed, bw),
                load_of(cnt.in_octets, elapsed, bw)
            );
            if netlabcraft_sim::port_max_speed(&p.name).is_some() {
                let _ = writeln!(s, "  Encapsulation ARPA");
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
            match cleared {
                None => s.push_str("  Counters: never cleared\n"),
                Some(at) => {
                    let _ = writeln!(s, "  Counters: cleared {} ago", hms(sim.now_us().saturating_sub(at)));
                }
            }
            let _ = writeln!(
                s,
                "  {} packets input, {} bytes, {} broadcasts, {} multicasts",
                cnt.in_packets(),
                cnt.in_octets,
                cnt.in_bcast,
                cnt.in_mcast
            );
            let _ = writeln!(
                s,
                "  {} packets output, {} bytes, {} broadcasts, {} multicasts",
                cnt.out_packets(),
                cnt.out_octets,
                cnt.out_bcast,
                cnt.out_mcast
            );
            let _ = writeln!(s, "  {} input errors, {} input drops, {} output drops", cnt.in_errors, cnt.in_drops, cnt.out_drops);
            let _ = writeln!(s, "  {}", keepalive_words(p.keepalive));
        }
        for sub in &p.subifs {
            if only.is_some_and(|r| r.sub.is_some_and(|n| n != sub.number)) {
                continue;
            }
            let (state, proto) = if sub.shutdown || p.shutdown {
                ("administratively down", "down")
            } else if port_up(t, end) {
                ("up", "up")
            } else {
                ("down", "down")
            };
            let _ = writeln!(s, "{}.{} is {state}, line protocol is {proto}", p.name, sub.number);
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

/// The keepalive line of `show interfaces`: the setting (`None`: the default of 10 seconds).
fn keepalive_words(setting: Option<u32>) -> String {
    match setting {
        Some(0) => "Keepalive not set".to_string(),
        Some(n) => format!("Keepalive set ({n} sec)"),
        None => "Keepalive set (10 sec)".to_string(),
    }
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
            if sim.now_us().saturating_sub(e.learned_us) >= netlabcraft_sim::mac_aging_us(d) {
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
pub fn ospf_neighbors(t: &Topology, d: &Device, st: &netlabcraft_sim::ospf::OspfState, sim: &netlabcraft_sim::Sim) -> String {
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
        let role = match st.segments.get(&(d.id, n.iface.clone())) {
            Some(seg) if seg.dr.is_none() => "P2P",
            Some(seg) if seg.dr == Some(n.address) => "DR",
            Some(seg) if seg.bdr == Some(n.address) => "BDR",
            _ => "OTHER",
        };
        // Before any packet has gone over the adjacency it is shown as converged.
        let word = sim.ospf_neighbor_state(d.id, &n.iface, n.router_id).map_or("FULL", netlabcraft_sim::ospf_flood::NbrState::word);
        let state = format!("{word}/{role}");
        let _ = writeln!(s, "{:<16} {:<10} {:<16} {:<22} {name}", n.router_id.to_string(), state, n.address.to_string(), ifname::short(&n.iface));
    }
    // A virtual link that is up is a neighbour too, across its transit area.
    for (k, v) in st.virtual_links.get(&d.id).into_iter().flatten().filter(|v| v.up).enumerate() {
        let name = v.peer.and_then(|p| t.device(p)).map_or("?".to_string(), |x| x.name.clone());
        let _ = writeln!(s, "{:<16} {:<10} {:<16} {:<22} {name}", v.peer_id.to_string(), "FULL/ -", "-", format!("OSPF_VL{k} (area {})", v.area));
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

/// `ipsec spi 256 md5 KEY`: how the running configuration writes an OSPFv3 AH setting.
fn ospf6_auth_words(a: &netlabcraft_model::Ospf6Auth) -> String {
    match a {
        netlabcraft_model::Ospf6Auth::Null => "null".to_string(),
        netlabcraft_model::Ospf6Auth::Ipsec { spi, alg, key } => {
            let alg = if *alg == netlabcraft_model::Ospf6AuthAlg::Md5 { "md5" } else { "sha1" };
            format!("ipsec spi {spi} {alg} {key}")
        }
    }
}

/// `md5 with SPI 256`: an OSPFv3 AH setting without its key, for the views.
fn ospf6_auth_summary(a: &netlabcraft_model::Ospf6Auth) -> String {
    match a {
        netlabcraft_model::Ospf6Auth::Null => "none".to_string(),
        netlabcraft_model::Ospf6Auth::Ipsec { spi, alg, .. } => {
            format!("{} with SPI {spi}", if *alg == netlabcraft_model::Ospf6AuthAlg::Md5 { "md5" } else { "sha1" })
        }
    }
}

/// `show ipv6 ospf neighbor`.
pub fn ospf6_neighbors(t: &Topology, d: &Device, st: &netlabcraft_sim::ospf6::Ospf6State) -> String {
    if d.ospf6.is_none() {
        return "OSPFv3 is not running on this router.".into();
    }
    let list = st.neighbors.get(&d.id).cloned().unwrap_or_default();
    let refused = st.refused.get(&d.id).cloned().unwrap_or_default();
    if list.is_empty() && refused.is_empty() {
        return "No OSPFv3 neighbours (check `ipv6 ospf N area A` on the interfaces, `ipv6 unicast-routing`, the router ID and cables).".into();
    }
    let mut s = String::new();
    if !list.is_empty() {
        s = format!("{:<16} {:<10} {:<26} {:<12} Device\n", "Neighbour ID", "State", "Address", "Interface");
    }
    for n in list {
        let name = t.device(n.device).map_or("?".to_string(), |x| x.name.clone());
        let _ = writeln!(s, "{:<16} {:<10} {:<26} {:<12} {name}", n.router_id.to_string(), "FULL", n.address.to_string(), ifname::short(&n.iface));
    }
    for (iface, rid, why) in refused {
        let _ = writeln!(s, "Not a neighbour: {rid} on {}: {why}", ifname::short(&iface));
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
    let up = netlabcraft_sim::v6::l3_ifaces6(d);
    for (name, area, cost, passive) in st.interfaces.get(&d.id).into_iter().flatten() {
        let auth = up
            .iter()
            .find(|i| i.name == *name)
            .and_then(|i| netlabcraft_sim::ospf6::auth_of(d, i, *area))
            .map_or(String::new(), |a| format!(", authentication: AH {}", ospf6_auth_summary(a)));
        let _ = writeln!(s, "  {:<22} area {area}, cost {cost}{}{auth}", name, if *passive { ", passive" } else { "" });
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
        crate::exec::redist::protocol_lines(&mut s, &r.redistribute, r.default_metric.map(|m| m.to_string()), &r.distribute);
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

/// `show ip protocols summary`: one line for each routing protocol the router runs.
pub fn ip_protocols_summary(
    d: &Device,
    st: &netlabcraft_sim::ospf::OspfState,
    rs: &netlabcraft_sim::rip::RipState,
    es: &netlabcraft_sim::eigrp::EigrpState,
) -> String {
    let mut rows: Vec<String> = Vec::new();
    let row = |name: &str, id: String, rid: String, ifs: usize, passive: usize, nbrs: usize, routes: usize| {
        format!("{name:<8} {id:<8} {rid:<16} {ifs:<11} {passive:<8} {nbrs:<11} {routes}")
    };
    if let Some(o) = &d.ospf {
        let ifs = st.interfaces.get(&d.id).map_or(&[][..], Vec::as_slice);
        rows.push(row(
            "OSPF",
            o.process.to_string(),
            st.router_ids.get(&d.id).map_or("-".to_string(), ToString::to_string),
            ifs.len(),
            ifs.iter().filter(|i| i.3).count(),
            st.neighbors.get(&d.id).map_or(0, Vec::len),
            st.routes.get(&d.id).map_or(0, Vec::len),
        ));
    }
    if let Some(e) = &d.eigrp {
        let ifs = es.interfaces.get(&d.id).map_or(&[][..], Vec::as_slice);
        rows.push(row(
            "EIGRP",
            format!("AS {}", e.asn),
            es.router_ids.get(&d.id).map_or("-".to_string(), ToString::to_string),
            ifs.len(),
            ifs.iter().filter(|i| i.passive).count(),
            es.neighbors.get(&d.id).map_or(0, Vec::len),
            es.routes.get(&d.id).map_or(0, Vec::len),
        ));
    }
    if let Some(r) = &d.rip {
        let ifs = rs.interfaces.get(&d.id).map_or(&[][..], Vec::as_slice);
        rows.push(row(
            "RIP",
            format!("v{}", r.version),
            "-".to_string(),
            ifs.len(),
            ifs.iter().filter(|i| i.2).count(),
            rs.neighbors.get(&d.id).map_or(0, Vec::len),
            rs.routes.get(&d.id).map_or(0, Vec::len),
        ));
    }
    if rows.is_empty() {
        return "No routing protocol is running (static routes only).".into();
    }
    let mut s = format!("{:<8} {:<8} {:<16} {:<11} {:<8} {:<11} Routes\n", "Protocol", "ID", "Router ID", "Interfaces", "Passive", "Neighbours");
    s.push_str(&rows.join("\n"));
    s
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
    crate::exec::redist::protocol_lines(&mut s, &e.redistribute, e.default_metric.map(|m| m.words()), &e.distribute);
    let _ = writeln!(s, "  Load balancing: up to {} paths per destination, variance {}", e.max_paths(), e.variance.unwrap_or(1).max(1));
    let _ = write!(
        s,
        "  Automatic summarisation {}; administrative distance 90 (internal), 170 (external)",
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
    crate::exec::redist::protocol_lines(&mut s, &o.redistribute, o.default_metric.map(|m| m.to_string()), &o.distribute);
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
        if let Some(h) = o.retransmit {
            let _ = writeln!(s, " ip ospf retransmit-interval {h}");
        }
        if let Some(h) = o.transmit_delay {
            let _ = writeln!(s, " ip ospf transmit-delay {h}");
        }
        if o.p2p {
            s.push_str(" ip ospf network point-to-point\n");
        }
        if let Some((p, a)) = o.area {
            let _ = writeln!(s, " ip ospf {p} area {a}");
        }
        match o.auth {
            Some(netlabcraft_model::OspfAuthKind::Simple) => s.push_str(" ip ospf authentication\n"),
            Some(netlabcraft_model::OspfAuthKind::Md5) => s.push_str(" ip ospf authentication message-digest\n"),
            Some(netlabcraft_model::OspfAuthKind::Null) => s.push_str(" ip ospf authentication null\n"),
            None => {}
        }
        if let Some(k) = &o.auth_key {
            let _ = writeln!(s, " ip ospf authentication-key {k}");
        }
        for (id, k) in &o.md5_keys {
            let _ = writeln!(s, " ip ospf message-digest-key {id} md5 {k}");
        }
        if let Some(chain) = &o.key_chain {
            let _ = writeln!(s, " ip ospf authentication key-chain {chain}");
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
            let n = hits.get(&(a.name.clone(), a.hit_key(Some(i)))).copied().unwrap_or(0);
            let m = if n > 0 { format!(" ({n} match{})", if n == 1 { "" } else { "es" }) } else { String::new() };
            let _ = writeln!(s, "    {} {}{m}", a.seq_of(i), e.text(a.extended));
        }
        let implicit = hits.get(&(a.name.clone(), a.hit_key(None))).copied().unwrap_or(0);
        let _ = writeln!(s, "    (implicit) deny everything else{}", if implicit > 0 { format!(" ({implicit} matches)") } else { String::new() });
    }
    s.trim_end().to_string()
}

/// `show ip nat translations`.
pub fn nat_translations(sim: &Sim, d: &Device, verbose: bool) -> String {
    let fmt = |a: Ipv4Addr, id: Option<u16>| id.map_or(a.to_string(), |i| format!("{a}:{i}"));
    let now = sim.now_us();
    let ago = |t: u64| format!("{} s ago", now.saturating_sub(t) / 1_000_000);
    let mut rows: Vec<String> = Vec::new();
    for (l, g) in &d.nat.statics {
        let mut row = format!("{:<6} {:<22} {:<22} {}", "---", g.to_string(), l.to_string(), "---");
        if verbose {
            row.push_str("\n    static entry from the configuration, never ages out");
        }
        rows.push(row);
    }
    if let Some(st) = sim.state(d.id) {
        for e in st.nat.iter().filter(|e| (!e.fixed || e.local_id.is_some()) && e.alive(d, now)) {
            let proto = match e.protocol {
                1 => "icmp",
                17 => "udp",
                6 => "tcp",
                _ => "ip",
            };
            let mut row = format!("{proto:<6} {:<22} {:<22} {}", fmt(e.inside_global, e.global_id), fmt(e.inside_local, e.local_id), e.outside);
            if verbose {
                let kind = match (e.fixed, e.local_id.is_some()) {
                    (true, _) => "static, with a flow of its own",
                    (false, true) => "dynamic, translated per flow (port or identifier)",
                    (false, false) => "dynamic, address only",
                };
                let expiry = match e.lifetime_us(d) {
                    Some(life) => format!("expires in {} s", life.saturating_sub(now.saturating_sub(e.last_us)) / 1_000_000),
                    None => "does not expire".to_string(),
                };
                let _ = write!(
                    row,
                    "\n    {kind}; created {}, last used {}, {} packet{} translated; {expiry}",
                    ago(e.created_us),
                    ago(e.last_us),
                    e.uses,
                    if e.uses == 1 { "" } else { "s" }
                );
            }
            rows.push(row);
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
    match &c.ospf_auth {
        Some(netlabcraft_model::Ospf6Auth::Null) => s.push_str(" ospfv3 authentication null\n"),
        Some(a @ netlabcraft_model::Ospf6Auth::Ipsec { .. }) => {
            let _ = writeln!(s, " ospfv3 authentication {}", ospf6_auth_words(a));
        }
        None => {}
    }
    if c.nd_managed {
        let _ = writeln!(s, " ipv6 nd managed-config-flag");
    }
    if c.nd_other {
        let _ = writeln!(s, " ipv6 nd other-config-flag");
    }
    if c.nd_suppress {
        let _ = writeln!(s, " ipv6 nd ra suppress all");
    }
    if let Some(n) = c.nd_ra_lifetime {
        let _ = writeln!(s, " ipv6 nd ra lifetime {n}");
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
        let tag = if r.tag == 0 { String::new() } else { format!(" tag {}", r.tag) };
        let _ = writeln!(s, "{code:<2} {:<28} {via}{ad}{tag}", r.net.to_string());
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

/// Whether an interface setting kept by the console is in force: a setting that is on unless a
/// `no` line was typed (`default_on`), or one that is off unless its line is present.
fn if_knob(d: &Device, iface: &str, key: &str, default_on: bool) -> bool {
    use netlabcraft_model::knobs::Scope;
    match d.knobs.get(Scope::Iface(iface), key) {
        Some(line) => !line.starts_with("no "),
        None => default_on,
    }
}

/// The lines of `show ip interface` that every layer-3 interface has, after its address.
fn ip_interface_details(
    d: &Device,
    name: &str,
    acl: &netlabcraft_model::AccessGroups,
    helpers: &[Ipv4Addr],
    nat: Option<netlabcraft_model::NatSide>,
    proxy: Option<bool>,
) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "  Access list in: {}", acl.inbound.as_deref().unwrap_or("none"));
    let _ = writeln!(s, "  Access list out: {}", acl.outbound.as_deref().unwrap_or("none"));
    if helpers.is_empty() {
        s.push_str("  Helper addresses: none\n");
    } else {
        let _ = writeln!(s, "  DHCP relay to: {}", helpers.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "));
    }
    match nat {
        Some(netlabcraft_model::NatSide::Inside) => s.push_str("  NAT: inside\n"),
        Some(netlabcraft_model::NatSide::Outside) => s.push_str("  NAT: outside\n"),
        None => s.push_str("  NAT: not used on this interface\n"),
    }
    match proxy {
        Some(true) => s.push_str("  Proxy ARP: off\n"),
        Some(false) => s.push_str("  Proxy ARP: on\n"),
        None => {}
    }
    let on = |b: bool| if b { "on" } else { "off" };
    s.push_str("  MTU is 1500 bytes\n");
    let _ = writeln!(s, "  Local proxy ARP: {}", on(if_knob(d, name, "ip local-proxy-arp", false)));
    let _ = writeln!(s, "  Directed broadcast forwarding: {}", on(if_knob(d, name, "ip directed-broadcast", false)));
    let _ = writeln!(s, "  Destination-unreachable replies: {}", on(if_knob(d, name, "ip unreachables", true)));
    let _ = writeln!(s, "  Redirect messages: {}", on(if_knob(d, name, "ip redirects", true)));
    let _ = writeln!(s, "  Mask replies: {}", on(if_knob(d, name, "ip mask-reply", false)));
    let _ = writeln!(s, "  Route cache fast path: {}", on(if_knob(d, name, "ip route-cache", true)));
    let _ = writeln!(s, "  Packet reassembly check: {}", on(if_knob(d, name, "ip virtual-reassembly", false)));
    let multicast = d.knobs.get(netlabcraft_model::knobs::Scope::Global, "ip multicast-routing").is_some();
    let _ = writeln!(s, "  Multicast routing: {}", if multicast { "enabled on the device" } else { "not enabled" });
    s
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
            let (state, proto) = if down {
                ("administratively down", "down")
            } else if port_up(t, end) {
                ("up", "up")
            } else {
                ("down", "down")
            };
            let mut s = format!("{name} is {state}, line protocol is {proto}\n");
            match ip {
                Some(ip) => {
                    let _ = writeln!(s, "  Address {ip}, broadcast {}", ip.broadcast());
                    let _ = writeln!(s, "  Mask {}", ip.mask());
                }
                None => s.push_str("  No IPv4 address\n"),
            }
            let proxy = (d.kind == DeviceKind::Router).then_some(p.proxy_arp_off);
            s.push_str(&ip_interface_details(d, &name, acl, helpers, nat, proxy));
            blocks.push(s.trim_end().to_string());
        }
    }
    for svi in d.svis.iter().filter(|_| only.is_none()) {
        let name = format!("Vlan{}", svi.vlan);
        let mut s = if svi.shutdown {
            format!("{name} is administratively down, line protocol is down\n")
        } else {
            format!("{name} is up, line protocol is up\n")
        };
        match svi.ip {
            Some(ip) => {
                let _ = writeln!(s, "  Address {ip}, broadcast {}", ip.broadcast());
                let _ = writeln!(s, "  Mask {}", ip.mask());
            }
            None => s.push_str("  No IPv4 address\n"),
        }
        s.push_str(&ip_interface_details(d, &name, &svi.access, &svi.helpers, svi.nat, None));
        blocks.push(s.trim_end().to_string());
    }
    blocks.join("\n")
}

/// `show interfaces status` (switches).
pub fn interface_transceivers(t: &Topology, d: &Device) -> String {
    use netlabcraft_model::cable::Medium;
    use netlabcraft_model::physical::BudgetState;
    let mut s = format!("{:<10} {:<20} {:<10} {:<8} {:<8} {:<8} {}\n", "Port", "Transceiver", "Length", "Loss", "Budget", "Margin", "State");
    let mut any = false;
    for (i, p) in d.ports.iter().enumerate() {
        if !matches!(d.port_medium(i), Some(Medium::Fiber | Medium::Combo)) {
            continue;
        }
        any = true;
        let end = End { device: d.id, port: i };
        let kind = if p.single_mode { "single-mode 1310 nm" } else { "multi-mode 850 nm" };
        let cols = match t.link_at(end).and_then(|l| t.fiber_budget(l)) {
            Some(b) => (
                format!("{} m", b.meters),
                format!("{:.1} dB", b.loss_db),
                format!("{:.1} dB", b.budget_db),
                format!("{:.1} dB", b.margin_db()),
                match b.state() {
                    BudgetState::Ok => "ok",
                    BudgetState::Degraded => "degraded",
                    BudgetState::Over => "over budget",
                },
            ),
            None => ("-".to_string(), "-".to_string(), "-".to_string(), "-".to_string(), "no fiber cable"),
        };
        let _ = writeln!(s, "{:<10} {:<20} {:<10} {:<8} {:<8} {:<8} {}", ifname::short(&p.name), kind, cols.0, cols.1, cols.2, cols.3, cols.4);
    }
    if !any {
        s.push_str("No fiber or combo ports on this device\n");
    }
    s
}

pub fn interface_status(t: &Topology, d: &Device, only: Option<usize>) -> String {
    let mut s = format!("{:<10} {:<18} {:<13} {:<8} {:<7} {:<6} {}\n", "Port", "Description", "State", "VLAN", "Duplex", "Speed", "Type");
    for (i, p) in d.ports.iter().enumerate() {
        if only.is_some_and(|o| o != i) {
            continue;
        }
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
        // What the link negotiated while it is up; the setting (or auto) while it is not.
        let (speed, duplex) = match netlabcraft_sim::link_speed(t, end) {
            Some((sp, du)) => (sp.to_string(), (if du == netlabcraft_model::Duplex::Half { "half" } else { "full" }).to_string()),
            None => (
                p.speed.map_or(speed.to_string(), |x| x.to_string()),
                p.duplex.map_or("auto", |x| if x == netlabcraft_model::Duplex::Half { "half" } else { "full" }).to_string(),
            ),
        };
        let kind = match d.port_medium(i) {
            Some(netlabcraft_model::cable::Medium::Copper) if netlabcraft_sim::port_max_speed(&p.name) == Some(1000) => "10/100/1000BaseTX",
            Some(netlabcraft_model::cable::Medium::Copper) => "10/100BaseTX",
            Some(netlabcraft_model::cable::Medium::Fiber) => "1000BaseX (SFP)",
            Some(netlabcraft_model::cable::Medium::Combo) => "combo (copper or SFP)",
            _ => "-",
        };
        let desc: String = p.description.chars().take(18).collect();
        let _ = writeln!(s, "{:<10} {:<18} {:<13} {:<8} {:<7} {:<6} {kind}", ifname::short(&p.name), desc, state, vlan, duplex, speed);
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
    let (state, proto) = if svi.shutdown {
        ("administratively down", "down")
    } else if up {
        ("up", "up")
    } else {
        ("down (no port of the VLAN is up)", "down")
    };
    let mut s = format!("Vlan{vlan} is {state}, line protocol is {proto}\n  Hardware address {}\n", netlabcraft_sim::svi_mac(d));
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
    let model = d.catalogue_model();
    format!(
        "{}: Ethernet ({medium}), {link}\n  Hardware: {} ({}); only the speed, duplex and auto-MDIX are simulated\n  Speed set to {}, duplex set to {}\n  Auto-MDIX {}{cable}",
        p.name,
        model.label,
        model.id,
        set(p.speed.map(|x| format!("{x} Mb/s"))),
        set(p.duplex.map(|x| if x == netlabcraft_model::Duplex::Half { "half".into() } else { "full".into() })),
        if p.mdix_off { "off" } else { "on" }
    )
}

/// `show controllers` for a serial port: what the model knows about the cable and the line.
pub fn serial_controller(t: &Topology, d: &Device, port: usize) -> String {
    let Some(p) = d.ports.get(port) else { return String::new() };
    let end = End { device: d.id, port };
    let model = d.catalogue_model();
    let mut s = format!("{}: serial interface of the {} ({})\n", p.name, model.label, model.id);
    match t.dce_of(end) {
        None => s.push_str("  Cable: none attached\n"),
        Some(dce) if dce == end => {
            let rate = p.serial.clock_rate.map_or("not set (the link stays down)".to_string(), |r| format!("{r} bit/s"));
            let _ = writeln!(s, "  Cable: DCE end of a serial cable, clock rate {rate}");
        }
        Some(_) => {
            let far = t.peer(end).and_then(|e| t.device(e.device).and_then(|x| x.ports.get(e.port))).and_then(|x| x.serial.clock_rate);
            let _ =
                writeln!(s, "  Cable: DTE end of a serial cable, clocked by the other end{}", far.map_or(String::new(), |r| format!(" ({r} bit/s)")));
        }
    }
    let state = if p.shutdown {
        "administratively down".to_string()
    } else if port_up(t, end) {
        "up".to_string()
    } else if t.peer(end).is_none() {
        "down (no cable)".to_string()
    } else if let Some(why) = netlabcraft_sim::serial_problem(t, end) {
        format!("down ({why})")
    } else {
        "down".to_string()
    };
    let _ = writeln!(s, "  Line: {state}");
    let _ = writeln!(s, "  Encapsulation: {}", p.serial.encapsulation.name());
    s.push_str("  Simulated: the cable ends, the clock and the encapsulation; there are no modem signal lines or line errors");
    s
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
pub fn int_descriptions(t: &Topology, d: &Device, only: Option<IfRef>) -> String {
    let mut s = format!("{:<12} {:<22} {:<9} {}\n", "Interface", "Status", "Protocol", "Description");
    for (i, p) in d.ports.iter().enumerate() {
        if only.is_some_and(|r| r.port != i) {
            continue;
        }
        let end = End { device: d.id, port: i };
        let up = port_up(t, end);
        let status = if p.shutdown {
            "admin down"
        } else if up {
            "up"
        } else {
            "down"
        };
        if only.is_none_or(|r| r.sub.is_none()) {
            let _ = writeln!(s, "{:<12} {:<22} {:<9} {}", ifname::short(&p.name), status, if up { "up" } else { "down" }, p.description);
        }
        for sub in p.subifs.iter().filter(|x| only.is_none_or(|r| r.sub == Some(x.number))) {
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
    for svi in d.svis.iter().filter(|_| only.is_none()) {
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
    let _ = writeln!(s, "  Dropped: no route {}, TTL expired {}, access list {}", n("no route"), n("TTL expired"), n("dropped by an access list"));
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
/// Seconds between the device's CDP advertisements (`cdp timer`).
fn cdp_timer(d: &Device) -> u64 {
    netlabcraft_sim::knob_number(d, "cdp timer").filter(|n| *n > 0).unwrap_or(60)
}

/// Seconds a neighbour keeps the device's CDP advertisement (`cdp holdtime`).
fn cdp_hold(d: &Device) -> u64 {
    netlabcraft_sim::knob_number(d, "cdp holdtime").filter(|n| *n > 0).unwrap_or(180)
}

pub fn cdp(d: &Device) -> String {
    if d.cdp_off {
        return "Neighbour discovery is off on this device (`cdp run` turns it on).".into();
    }
    let off: Vec<String> = d.ports.iter().filter(|p| p.cdp_off).map(|p| ifname::short(&p.name)).collect();
    let mut s = format!("Neighbour discovery is on: advertisements every {} s, kept for {} s.", cdp_timer(d), cdp_hold(d));
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
    let hold = |name: &str| {
        let sender = t.devices.iter().find(|x| x.name == name);
        let (every, ttl) = sender.map_or((60, 180), |x| (cdp_timer(x), cdp_hold(x)));
        ttl.saturating_sub(sim.now_us() / 1_000_000 % every.max(1))
    };
    let kind = |k: DeviceKind| if k == DeviceKind::Router { "router" } else { "switch" };
    let mut s = String::new();
    if detail {
        for n in &list {
            let _ = writeln!(s, "Neighbour {} ({}, {})", n.name, kind(n.kind), n.platform);
            let _ = writeln!(s, "  Seen on   {}, its port {}", n.local, n.remote);
            let _ = writeln!(s, "  Address   {}", n.address.map_or("none".to_string(), |a| a.addr.to_string()));
            let _ = writeln!(s, "  Software  NetLabCraft {}", env!("CARGO_PKG_VERSION"));
            let _ = writeln!(s, "  Expires   in {} s\n", hold(&n.name));
        }
    } else {
        let _ = writeln!(s, "{:<14} {:<14} {:<7} {:<9} {:<9} Expires", "Neighbour", "Platform", "Type", "Here", "There");
        for n in &list {
            let _ = writeln!(
                s,
                "{:<14} {:<14} {:<7} {:<9} {:<9} {} s",
                n.name,
                n.platform,
                kind(n.kind),
                ifname::short(&n.local),
                ifname::short(&n.remote),
                hold(&n.name)
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
    let mut s =
        format!("LLDP is on: advertisements every {} s, kept for {} s.", netlabcraft_sim::lldp::timer_secs(d), netlabcraft_sim::lldp::hold_secs(d));
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
    // Time left on a neighbour's advertisement: its own holdtime less the time since its last one.
    let hold = |name: &str| {
        let sender = t.devices.iter().find(|x| x.name == name);
        let every = sender.map_or(netlabcraft_sim::lldp::DEFAULT_TIMER_SECS, netlabcraft_sim::lldp::timer_secs).max(1);
        let ttl = sender.map_or(netlabcraft_sim::lldp::HOLD_SECS, netlabcraft_sim::lldp::hold_secs);
        u64::from(ttl).saturating_sub(sim.now_us() / 1_000_000 % every)
    };
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
            let _ = writeln!(s, "  Expires      in {} s\n", hold(&i.system_name));
        }
    } else {
        let _ = writeln!(s, "{:<14} {:<9} {:<9} {:<15} Expires", "Neighbour", "Here", "There", "Can act as");
        for n in &list {
            let _ = writeln!(
                s,
                "{:<14} {:<9} {:<9} {:<15} {} s",
                n.info.system_name,
                ifname::short(&n.local),
                ifname::short(&n.info.port),
                caps(n.info.caps.0),
                hold(&n.info.system_name)
            );
        }
    }
    let _ = write!(s, "{} neighbour{}", list.len(), if list.len() == 1 { "" } else { "s" });
    s
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
pub fn ospf_summary(d: &Device, st: &netlabcraft_sim::ospf::OspfState, spf: Option<&netlabcraft_sim::DevState>, now_us: u64) -> String {
    let Some(o) = &d.ospf else { return "OSPF is not running here.".into() };
    let mut areas: std::collections::BTreeSet<u32> = o.networks.iter().map(|n| n.area).collect();
    areas.extend(st.segments.iter().filter(|((dev, _), _)| *dev == d.id).map(|(_, seg)| seg.area));
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
    for a in &areas {
        let kind = netlabcraft_sim::ospf::kind_of(o, *a);
        let conf = o.area(*a);
        let mut line = format!("  Area {a}: {}", if *a == 0 { "backbone" } else { kind.word() });
        if kind != netlabcraft_sim::ospf::AreaKind::Normal {
            if conf.no_summary {
                line.push_str(", totally stubby (no summary routes from other areas, a default instead)");
            }
            if conf.nssa_default {
                line.push_str(if conf.nssa_default_only {
                    ", sends a type 7 default route into the area (nssa-only: P bit clear)"
                } else {
                    ", sends a type 7 default route into the area"
                });
            }
            if conf.nssa_no_redist {
                line.push_str(", keeps its own redistributed routes out of the area (no-redistribution)");
            }
            if conf.nssa_translate_always {
                line.push_str(", translates type 7 LSAs into type 5 whether or not it is elected (translate type7 always)");
            }
            if let Some(c) = conf.default_cost {
                line.push_str(&format!(", default route cost {c}"));
            }
        }
        if let Some(tr) = st.translators.get(a).filter(|_| kind == netlabcraft_sim::ospf::AreaKind::Nssa) {
            let who = if *tr == d.id {
                "this router".to_string()
            } else {
                st.router_ids.get(tr).map_or("another router".to_string(), |r| format!("router {r}"))
            };
            line.push_str(&format!(", type 7 to type 5 translator: {who}"));
        }
        let _ = writeln!(s, "{line}");
    }
    let links = st.virtual_links.get(&d.id).map_or(0, Vec::len);
    if links > 0 {
        let up = st.virtual_links.get(&d.id).map_or(0, |l| l.iter().filter(|v| v.up).count());
        let _ = writeln!(s, "  {links} virtual link{} configured, {up} up (show ip ospf virtual-links)", if links == 1 { "" } else { "s" });
    }
    let (start, hold, max) = o.spf_throttle.unwrap_or((
        netlabcraft_sim::ospf_lsdb::SPF_START_MS,
        netlabcraft_sim::ospf_lsdb::SPF_HOLD_MS,
        netlabcraft_sim::ospf_lsdb::SPF_MAX_MS,
    ));
    let _ = writeln!(s, "  SPF throttle: wait {start} ms after a change, hold {hold} ms between runs, at most {max} ms");
    let runs = spf.map_or(0, |x| x.spf_runs);
    match spf.and_then(|x| x.spf_last_us) {
        Some(last) => {
            let ago = now_us.saturating_sub(last) / 1_000_000;
            let hold_now = spf.map_or(hold, |x| x.spf_hold_ms.max(hold));
            let _ = write!(s, "  SPF ran {runs} time{}, the last {ago} s ago (hold interval now {hold_now} ms)", if runs == 1 { "" } else { "s" });
        }
        None => {
            let _ = write!(s, "  SPF has not run yet");
        }
    }
    s
}

/// The timers and authentication of a virtual link, on one line.
fn vlink_detail(v: &netlabcraft_sim::ospf::VirtualLink) -> String {
    let auth = match v.auth {
        netlabcraft_model::OspfAuthKind::Null => "no authentication",
        netlabcraft_model::OspfAuthKind::Simple => "simple password authentication",
        netlabcraft_model::OspfAuthKind::Md5 => "message-digest authentication",
    };
    format!("Hello {} s, dead {} s, retransmit {} s, transmit delay {} s, {auth}", v.hello, v.dead, v.retransmit, v.delay)
}

/// `show ip ospf virtual-links`: each virtual link the router is configured with, and whether it works.
pub fn ospf_virtual_links(t: &Topology, d: &Device, st: &netlabcraft_sim::ospf::OspfState) -> String {
    if d.ospf.is_none() {
        return "OSPF is not running here.".into();
    }
    let list = st.virtual_links.get(&d.id).map(Vec::as_slice).unwrap_or_default();
    if list.is_empty() {
        return "No virtual links are configured here (area N virtual-link ROUTER-ID, under router ospf).".into();
    }
    let mut s = String::new();
    for (n, v) in list.iter().enumerate() {
        let name = v.peer.and_then(|p| t.device(p)).map_or(String::new(), |p| format!(" ({})", p.name));
        if v.up {
            let _ = writeln!(s, "Virtual link VL{n} to router {}{name} is up", v.peer_id);
            let _ = writeln!(s, "  Transit area {}, cost {} across it; the link joins this router to the backbone", v.area, v.cost);
            let _ = writeln!(s, "  {}", vlink_detail(v));
        } else {
            let _ = writeln!(s, "Virtual link VL{n} to router {}{name} is down", v.peer_id);
            let _ = writeln!(s, "  Transit area {}: {}", v.area, v.why);
            let _ = writeln!(s, "  {}", vlink_detail(v));
        }
    }
    s.trim_end().to_string()
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
    if let Some(ifc) = netlabcraft_sim::l3_ifaces(d).into_iter().find(|i| i.name == name) {
        let auth = match netlabcraft_sim::ospf::auth_kind(d, &ifc) {
            netlabcraft_model::OspfAuthKind::Null => "none".to_string(),
            netlabcraft_model::OspfAuthKind::Simple => "clear-text password".to_string(),
            netlabcraft_model::OspfAuthKind::Md5 => {
                let keys = netlabcraft_sim::ospf::md5_sign_keys(d, &name, netlabcraft_sim::ospf::analysis_clock(d));
                let chain = d.ospf_if.get(&name).and_then(|o| o.key_chain.as_deref());
                match (keys.first(), chain) {
                    (Some((k, _)), Some(c)) => format!("message digest, key chain {c}, signing with key {k}"),
                    (Some((k, _)), None) if keys.len() > 1 => {
                        format!("message digest, signing with key {k} (and {} older key{})", keys.len() - 1, if keys.len() == 2 { "" } else { "s" })
                    }
                    (Some((k, _)), None) => format!("message digest, signing with key {k}"),
                    (None, Some(c)) => format!("message digest, key chain {c} has no key that signs now"),
                    (None, None) => "message digest, no key configured".to_string(),
                }
            }
        };
        let _ = writeln!(s, "  Authentication: {auth}");
    }
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
    remote_access_config(s, d);
    for e in &c.maps {
        if let Some(dynamic) = &e.dynamic_ref {
            let _ = writeln!(s, "crypto map {} {} ipsec-isakmp dynamic {dynamic}", e.map, e.seq);
            continue;
        }
        if e.template {
            let _ = writeln!(s, "crypto dynamic-map {} {}", e.map, e.seq);
            if e.reverse_route {
                let _ = writeln!(s, " reverse-route");
            }
        } else {
            let _ = writeln!(s, "crypto map {} {} ipsec-isakmp", e.map, e.seq);
        }
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

/// The remote-access server's part of the running configuration: pools, client groups, the AAA
/// lists and the crypto map settings that name them.
fn remote_access_config(s: &mut String, d: &Device) {
    let ra = &d.crypto.ra;
    for l in &ra.login_lists {
        let _ = writeln!(s, "aaa authentication login {l} local");
    }
    for l in &ra.authz_lists {
        let _ = writeln!(s, "aaa authorization network {l} local");
    }
    for p in &ra.pools {
        let _ = writeln!(s, "ip local pool {} {} {}", p.name, p.start, p.end);
    }
    for g in &ra.groups {
        let _ = writeln!(s, "crypto isakmp client configuration group {}", g.name);
        if !g.key.is_empty() {
            let _ = writeln!(s, " key {}", g.key);
        }
        if let Some(p) = &g.pool {
            let _ = writeln!(s, " pool {p}");
        }
        let _ = writeln!(s, "!");
    }
    for (m, l) in &ra.client_auth {
        let _ = writeln!(s, "crypto map {m} client authentication list {l}");
    }
    for (m, l) in &ra.isakmp_authz {
        let _ = writeln!(s, "crypto map {m} isakmp authorization list {l}");
    }
    for (m, w) in &ra.client_address {
        let _ = writeln!(s, "crypto map {m} client configuration address {w}");
    }
}

/// `show ip local pool`: each pool and the clients that hold its addresses.
pub fn local_pools(sim: &Sim, d: &Device) -> String {
    let ra = &d.crypto.ra;
    if ra.pools.is_empty() {
        return "No local address pools configured".into();
    }
    let held = sim.ra_sessions(d.id);
    let mut s = format!("{:<16} {:<16} {:<16} {:<6} {}\n", "Pool", "First", "Last", "Free", "In use");
    for p in &ra.pools {
        let users: Vec<String> =
            held.iter().filter(|x| p.addresses().any(|a| a == x.assigned)).map(|x| format!("{} ({})", x.assigned, x.peer)).collect();
        let free = usize::try_from(p.size().min(netlabcraft_model::vpn::MAX_POOL_ADDRESSES)).unwrap_or(0).saturating_sub(users.len());
        let _ = writeln!(s, "{:<16} {:<16} {:<16} {:<6} {}", p.name, p.start.to_string(), p.end.to_string(), free, users.join(", "));
    }
    s.trim_end().to_string()
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
        Ok(_) => "up, line protocol is up".to_string(),
        Err(_) if tun.shutdown => "administratively down, line protocol is down".to_string(),
        Err(why) => format!("up, line protocol is down ({why})"),
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
    let ra = sim.ra_sessions(d.id);
    if sas.is_empty() && ra.is_empty() {
        return "No IKE peers yet (an association is set up when traffic matches a crypto map)".into();
    }
    let mut s = format!("{:<16} {:<16} {}\n", "Peer", "Local", "State");
    for x in &ra {
        let _ = writeln!(s, "{:<16} {:<16} remote-access client: group {}, user {}, address {}", x.peer, x.local, x.group, x.user, x.assigned);
    }
    for sa in &sas {
        let state = match (&sa.failed, sa.phase) {
            (_, netlabcraft_sim::vpn::IkePhase::Established) => match sa.v1.as_deref().and_then(|v| v.suite.map(|s| (s, &v.peer_cert))) {
                Some((suite, cert)) if suite.rsa => format!("established (RSA signature, peer certificate {cert}; {})", suite.describe()),
                Some((suite, _)) => format!("established (pre-shared key; {})", suite.describe()),
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
    let ra = sim.ra_sessions(d.id);
    if sas.is_empty() && ra.is_empty() {
        return "No IPsec security associations".into();
    }
    let mut s = String::new();
    for x in &ra {
        let _ = writeln!(s, "Remote-access client {} (local {}), interface {}", x.peer, x.local, x.iface);
        let _ = writeln!(s, "  Group {}, user {}, VPN address {}", x.group, x.user, x.assigned);
        let _ = writeln!(s, "  Outbound ESP SPI 0x{:08x}, inbound ESP SPI 0x{:08x}", x.spi_out, x.spi_in);
        let esp = match x.suite {
            Some(suite) => format!("{} (real ciphers)", suite.describe()),
            None => "the simulator's stand-in cipher (the transform set has no cipher)".to_string(),
        };
        let _ = writeln!(s, "  ESP: {esp}");
        let _ = writeln!(s, "  Packets encapsulated {}, decapsulated {}, dropped {}", x.encaps, x.decaps, x.drops);
        let _ = writeln!(s);
    }
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
            let mode = if netlabcraft_sim::vpn::peer_uses_transport(d, sa.peer) {
                "transport (the packet's own IP header stays and only its payload is protected: traffic between this router and its peer)"
            } else {
                "tunnel (whole packets travel inside a new IP header)"
            };
            let _ = writeln!(s, "  Mode: {mode}");
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
        if let Some(dynamic) = &e.dynamic_ref {
            let _ = writeln!(s, "Crypto map {} entry {} (remote-access clients, dynamic map {dynamic})", e.map, e.seq);
            let _ = writeln!(s, "  Client authentication list: {}", c.ra.client_auth.get(&e.map).map_or("none", String::as_str));
            let _ = writeln!(s, "  ISAKMP authorization list: {}", c.ra.isakmp_authz.get(&e.map).map_or("none", String::as_str));
            let on: Vec<&str> = c.applied.iter().filter(|(_, m)| **m == e.map).map(|(i, _)| i.as_str()).collect();
            let _ = writeln!(s, "  Applied to: {}", if on.is_empty() { "no interface".to_string() } else { on.join(", ") });
            continue;
        }
        if e.template {
            let ts = e.transform_set.as_deref().unwrap_or("not set");
            let _ = writeln!(s, "Dynamic map {} entry {}: transform set {ts}{}", e.map, e.seq, if e.reverse_route { ", reverse-route" } else { "" });
            continue;
        }
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
