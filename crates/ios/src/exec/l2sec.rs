//! Switch edge protection and tuning: DHCP snooping, dynamic ARP inspection, BPDU guard, root
//! guard, errdisable recovery, port-security aging, spanning-tree cost and port priority,
//! EtherChannel load balancing and LACP views, and a few small switch reports (`show vlan id`,
//! `show vlan summary`, `show dtp`, `show interfaces counters`, `show sdm prefer`).

use std::fmt::Write as _;

use netlabcraft_core::{VlanId, VlanSet};
use netlabcraft_model::l2sec::{ArpValidate, ERRDISABLE_INTERVAL, ErrCause, LinkType, LoadBalance, PortL2, vlan_list};
use netlabcraft_model::{Device, DeviceKind, End, Topology, ifname};
use netlabcraft_sim::Sim;
use netlabcraft_sim::stp::Role;

use super::{Ctx, R};
use crate::grammar::Id;

fn number(text: &str, lo: u32, hi: u32, what: &str) -> Result<u32, String> {
    text.trim().parse::<u32>().ok().filter(|n| (lo..=hi).contains(n)).ok_or_else(|| format!("% {what} is a number from {lo} to {hi}"))
}

fn vlans(text: &str) -> Result<VlanSet, String> {
    VlanSet::parse_list(text).map_err(|e| format!("% {e}"))
}

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

fn short(d: &Device, port: usize) -> String {
    d.ports.get(port).map_or_else(|| "?".to_string(), |p| ifname::short(&p.name))
}

/// Runs one of this module's commands.
pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    use Id::*;
    match id {
        // ---- DHCP snooping ----
        L2Snooping | L2NoSnooping => {
            let on = id == L2Snooping;
            let d = c.dm()?;
            d.l2.snooping = on;
            Ok(if on && d.l2.snooping_vlans.is_empty() {
                "DHCP snooping is on; name the VLANs it watches with `ip dhcp snooping vlan N`.".into()
            } else {
                String::new()
            })
        }
        L2SnoopingVlan | L2NoSnoopingVlan => {
            let set = vlans(arg(0))?;
            let d = c.dm()?;
            for v in set.iter() {
                if id == L2SnoopingVlan {
                    d.l2.snooping_vlans.insert(v);
                } else {
                    d.l2.snooping_vlans.remove(&v);
                }
            }
            Ok(if id == L2SnoopingVlan && !d.l2.snooping {
                "Saved; snooping acts once `ip dhcp snooping` turns it on.".into()
            } else {
                String::new()
            })
        }
        L2SnoopingOpt82 | L2NoSnoopingOpt82 => {
            c.dm()?.l2.option82 = id == L2SnoopingOpt82;
            Ok(String::new())
        }
        L2SnoopingTrust | L2NoSnoopingTrust => {
            let on = id == L2SnoopingTrust;
            c.each_if(|p, _| {
                p.l2.snoop_trust = on;
                Ok(())
            })
        }
        L2SnoopingRate | L2NoSnoopingRate => {
            let rate = if id == L2SnoopingRate { Some(number(arg(0), 1, 2048, "The rate")?) } else { None };
            c.each_if(|p, _| {
                p.l2.snoop_rate = rate;
                Ok(())
            })
        }
        L2ShowSnooping => Ok(show_snooping(c.sim, c.d()?)),
        L2ShowSnoopingBinding => Ok(show_bindings(c.sim, c.d()?)),
        L2ClearSnoopingBinding => {
            c.sim.clear_snooping(c.dev);
            Ok(String::new())
        }
        // ---- ARP inspection ----
        L2DaiVlan | L2NoDaiVlan => {
            let set = vlans(arg(0))?;
            let d = c.dm()?;
            for v in set.iter() {
                if id == L2DaiVlan {
                    d.l2.dai_vlans.insert(v);
                } else {
                    d.l2.dai_vlans.remove(&v);
                }
            }
            Ok(String::new())
        }
        L2DaiValidate => {
            let mut v = ArpValidate::default();
            for w in arg(0).split_whitespace() {
                match w.to_ascii_lowercase().as_str() {
                    "src-mac" => v.src_mac = true,
                    "dst-mac" => v.dst_mac = true,
                    "ip" => v.ip = true,
                    other => return Err(format!("% `{other}` is not a check: src-mac, dst-mac or ip")),
                }
            }
            if !v.any() {
                return Err("% Name at least one check: src-mac, dst-mac or ip".into());
            }
            c.dm()?.l2.dai_validate = v;
            Ok(String::new())
        }
        L2NoDaiValidate => {
            c.dm()?.l2.dai_validate = ArpValidate::default();
            Ok(String::new())
        }
        L2DaiTrust | L2NoDaiTrust => {
            let on = id == L2DaiTrust;
            c.each_if(|p, _| {
                p.l2.dai_trust = on;
                Ok(())
            })
        }
        L2DaiRate | L2NoDaiRate => {
            let rate = if id == L2DaiRate { Some(number(arg(0), 1, 2048, "The rate")?) } else { None };
            c.each_if(|p, _| {
                p.l2.dai_rate = rate;
                Ok(())
            })
        }
        L2ShowDai => Ok(show_dai(c.sim, c.d()?)),
        // ---- spanning-tree edge protection ----
        L2PortfastDefault | L2NoPortfastDefault => {
            let on = id == L2PortfastDefault;
            c.dm()?.l2.portfast_default = on;
            Ok(if on {
                "Every access port is now an edge port; trunks are not. Keep switches off access ports, or add BPDU guard.".into()
            } else {
                String::new()
            })
        }
        L2BpduguardDefault | L2NoBpduguardDefault => {
            c.dm()?.l2.bpduguard_default = id == L2BpduguardDefault;
            Ok(String::new())
        }
        L2Bpduguard => {
            let on = match arg(0).to_ascii_lowercase().as_str() {
                "enable" => true,
                "disable" => false,
                _ => return Err("% `enable` or `disable`".into()),
            };
            c.each_if(|p, _| {
                p.l2.bpduguard = Some(on);
                Ok(())
            })
        }
        L2NoBpduguard => c.each_if(|p, _| {
            p.l2.bpduguard = None;
            Ok(())
        }),
        L2Guard | L2NoGuard => {
            let on = id == L2Guard
                && match arg(0).to_ascii_lowercase().as_str() {
                    "root" => true,
                    "none" => false,
                    _ => return Err("% `root` or `none` (loop guard is not simulated)".into()),
                };
            c.each_if(|p, _| {
                p.l2.root_guard = on;
                Ok(())
            })
        }
        L2LinkType | L2NoLinkType => {
            let lt = if id == L2LinkType {
                Some(match arg(0).to_ascii_lowercase().as_str() {
                    "point-to-point" => LinkType::PointToPoint,
                    "shared" => LinkType::Shared,
                    _ => return Err("% `point-to-point` or `shared`".into()),
                })
            } else {
                None
            };
            c.each_if(|p, _| {
                p.l2.link_type = lt;
                Ok(())
            })
        }
        // ---- errdisable ----
        L2RecoveryCause | L2NoRecoveryCause => {
            let causes: Vec<ErrCause> =
                if arg(0).eq_ignore_ascii_case("all") {
                    ErrCause::ALL.to_vec()
                } else {
                    vec![ErrCause::parse(arg(0)).ok_or_else(|| {
                        format!("% `{}` is not a cause: bpduguard, psecure-violation, dhcp-rate-limit, arp-inspection or all", arg(0))
                    })?]
                };
            let d = c.dm()?;
            for cause in causes {
                if id == L2RecoveryCause {
                    d.l2.recovery.insert(cause);
                } else {
                    d.l2.recovery.remove(&cause);
                }
            }
            Ok(String::new())
        }
        L2RecoveryInterval | L2NoRecoveryInterval => {
            let n = if id == L2RecoveryInterval { number(arg(0), 30, 86_400, "The interval (seconds)")? } else { ERRDISABLE_INTERVAL };
            c.dm()?.l2.recovery_interval = n;
            Ok(String::new())
        }
        L2ShowErrDisabled => Ok(show_err_disabled(c.d()?)),
        L2ShowRecovery => Ok(show_recovery(c.sim, c.d()?)),
        // ---- port security aging ----
        L2PsecAgingTime | L2NoPsecAgingTime => {
            let n = if id == L2PsecAgingTime { number(arg(0), 0, 1440, "The aging time (minutes)")? } else { 0 };
            c.each_if(|p, _| {
                let name = p.name.clone();
                let ps = p.port_security.as_mut().ok_or_else(|| format!("% {name} has no port security: `switchport port-security` first"))?;
                ps.aging_time = n;
                Ok(())
            })
        }
        L2PsecAgingType | L2NoPsecAgingType => {
            let kind = if id == L2PsecAgingType {
                match arg(0).to_ascii_lowercase().as_str() {
                    "absolute" => netlabcraft_model::l2sec::AgingType::Absolute,
                    "inactivity" => netlabcraft_model::l2sec::AgingType::Inactivity,
                    _ => return Err("% `absolute` or `inactivity`".into()),
                }
            } else {
                netlabcraft_model::l2sec::AgingType::Absolute
            };
            c.each_if(|p, _| {
                let name = p.name.clone();
                let ps = p.port_security.as_mut().ok_or_else(|| format!("% {name} has no port security: `switchport port-security` first"))?;
                ps.aging_type = kind;
                Ok(())
            })
        }
        // ---- spanning-tree cost and priority ----
        L2StpCost
        | L2NoStpCost
        | L2StpVlanCost
        | L2NoStpVlanCost
        | L2StpPortPriority
        | L2NoStpPortPriority
        | L2StpVlanPortPriority
        | L2NoStpVlanPortPriority => {
            let per_vlan = matches!(id, L2StpVlanCost | L2NoStpVlanCost | L2StpVlanPortPriority | L2NoStpVlanPortPriority);
            let keys: Vec<u16> = if per_vlan { vlans(arg(0))?.iter().map(VlanId::get).collect() } else { vec![0] };
            let value_at = usize::from(per_vlan);
            let cost = matches!(id, L2StpCost | L2NoStpCost | L2StpVlanCost | L2NoStpVlanCost);
            let set = matches!(id, L2StpCost | L2StpVlanCost | L2StpPortPriority | L2StpVlanPortPriority);
            let value = if !set {
                None
            } else if cost {
                Some(number(arg(value_at), 1, 200_000_000, "The cost")?)
            } else {
                let p = number(arg(value_at), 0, 240, "The port priority")?;
                if p % 16 != 0 {
                    return Err("% The port priority is a multiple of 16 from 0 to 240".into());
                }
                Some(p)
            };
            c.each_if(|p, _| {
                let map = if cost { &mut p.l2.stp_cost } else { &mut p.l2.stp_priority };
                for k in &keys {
                    match value {
                        Some(v) => {
                            map.insert(*k, v);
                        }
                        None => {
                            map.remove(k);
                        }
                    }
                }
                Ok(())
            })
        }
        L2ShowStpSummary => {
            let d = c.d()?.clone();
            let mut trees = Vec::new();
            for v in d.vlans.keys().copied() {
                let tree = c.sim.spanning_tree(c.t, v).clone();
                if tree.bridges.contains_key(&d.id) {
                    trees.push((v, tree));
                }
            }
            Ok(show_stp_summary(c.t, c.sim, &d, &trees))
        }
        L2ShowStpDetail => {
            let v: VlanId = arg(0).parse().map_err(|e| format!("% {e}"))?;
            let d = c.d()?.clone();
            if !d.vlans.contains_key(&v) {
                return Err(format!("% VLAN {v} does not exist on this switch"));
            }
            let tree = c.sim.spanning_tree(c.t, v).clone();
            Ok(show_stp_detail(c.t, c.sim, &d, v, &tree))
        }
        // ---- EtherChannel ----
        L2LoadBalance | L2NoLoadBalance => {
            let lb = if id == L2LoadBalance {
                LoadBalance::parse(arg(0))
                    .ok_or_else(|| format!("% `{}` is not a method: src-mac, dst-mac, src-dst-mac, src-ip, dst-ip or src-dst-ip", arg(0)))?
            } else {
                LoadBalance::default()
            };
            c.dm()?.l2.load_balance = lb;
            Ok(String::new())
        }
        L2ShowLoadBalance => Ok(show_load_balance(c.d()?)),
        L2ShowPortChannel => Ok(show_port_channels(c.t, c.d()?)),
        L2LacpPriority | L2NoLacpPriority => {
            let n = if id == L2LacpPriority {
                Some(u16::try_from(number(arg(0), 1, 65_535, "The LACP port priority")?).unwrap_or(u16::MAX))
            } else {
                None
            };
            c.each_if(|p, _| {
                p.l2.lacp_priority = n;
                Ok(())
            })
        }
        L2ShowLacpNeighbor => Ok(show_lacp_neighbors(c.t, c.d()?)),
        _ => Err("% Not a switch-protection command".into()),
    }
}

/// Global configuration lines, for `show running-config`.
pub(crate) fn global_lines(s: &mut String, d: &Device) {
    if d.kind != DeviceKind::Switch {
        return;
    }
    let l = &d.l2;
    let start = s.len();
    if l.snooping {
        let _ = writeln!(s, "ip dhcp snooping");
    }
    if !l.snooping_vlans.is_empty() {
        let _ = writeln!(s, "ip dhcp snooping vlan {}", vlan_list(&l.snooping_vlans));
    }
    if !l.option82 {
        let _ = writeln!(s, "no ip dhcp snooping information option");
    }
    if !l.dai_vlans.is_empty() {
        let _ = writeln!(s, "ip arp inspection vlan {}", vlan_list(&l.dai_vlans));
    }
    if l.dai_validate.any() {
        let v = l.dai_validate;
        let words: Vec<&str> = [(v.src_mac, "src-mac"), (v.dst_mac, "dst-mac"), (v.ip, "ip")].into_iter().filter(|x| x.0).map(|x| x.1).collect();
        let _ = writeln!(s, "ip arp inspection validate {}", words.join(" "));
    }
    if l.portfast_default {
        let _ = writeln!(s, "spanning-tree portfast default");
    }
    if l.bpduguard_default {
        let _ = writeln!(s, "spanning-tree portfast bpduguard default");
    }
    for cause in &l.recovery {
        let _ = writeln!(s, "errdisable recovery cause {}", cause.name());
    }
    if l.recovery_interval != ERRDISABLE_INTERVAL {
        let _ = writeln!(s, "errdisable recovery interval {}", l.recovery_interval);
    }
    if l.load_balance != LoadBalance::default() {
        let _ = writeln!(s, "port-channel load-balance {}", l.load_balance.name());
    }
    if s.len() > start {
        let _ = writeln!(s, "!");
    }
}

/// Interface lines of a switch port, for `show running-config`.
pub(crate) fn port_lines(s: &mut String, p: &netlabcraft_model::Port) {
    let l: &PortL2 = &p.l2;
    if l.encap_dot1q {
        let _ = writeln!(s, " switchport trunk encapsulation dot1q");
    }
    if let Some(ps) = &p.port_security {
        if ps.aging_time > 0 {
            let _ = writeln!(s, " switchport port-security aging time {}", ps.aging_time);
        }
        if ps.aging_type != netlabcraft_model::l2sec::AgingType::Absolute {
            let _ = writeln!(s, " switchport port-security aging type {}", ps.aging_type.name());
        }
    }
    if let Some(on) = l.bpduguard {
        let _ = writeln!(s, " spanning-tree bpduguard {}", if on { "enable" } else { "disable" });
    }
    if l.root_guard {
        let _ = writeln!(s, " spanning-tree guard root");
    }
    if let Some(lt) = l.link_type {
        let _ = writeln!(s, " spanning-tree link-type {}", lt.name());
    }
    for (k, v) in &l.stp_cost {
        match k {
            0 => writeln!(s, " spanning-tree cost {v}"),
            _ => writeln!(s, " spanning-tree vlan {k} cost {v}"),
        }
        .unwrap_or_default();
    }
    for (k, v) in &l.stp_priority {
        match k {
            0 => writeln!(s, " spanning-tree port-priority {v}"),
            _ => writeln!(s, " spanning-tree vlan {k} port-priority {v}"),
        }
        .unwrap_or_default();
    }
    if let Some(n) = l.lacp_priority {
        let _ = writeln!(s, " lacp port-priority {n}");
    }
    if l.snoop_trust {
        let _ = writeln!(s, " ip dhcp snooping trust");
    }
    if let Some(n) = l.snoop_rate {
        let _ = writeln!(s, " ip dhcp snooping limit rate {n}");
    }
    if l.dai_trust {
        let _ = writeln!(s, " ip arp inspection trust");
    }
    if let Some(n) = l.dai_rate {
        let _ = writeln!(s, " ip arp inspection limit rate {n}");
    }
}

fn show_snooping(sim: &Sim, d: &Device) -> String {
    let l = &d.l2;
    let mut s = String::new();
    let _ = writeln!(s, "DHCP snooping:            {}", on_off(l.snooping));
    let watched = if l.snooping_vlans.is_empty() { "none".to_string() } else { vlan_list(&l.snooping_vlans) };
    let _ = writeln!(s, "VLANs watched:            {watched}{}", if l.snooping { "" } else { " (inactive until snooping is on)" });
    let _ = writeln!(s, "Option 82 insertion:      {}", on_off(l.option82));
    let n = sim.snooping_counts(d.id);
    let _ = writeln!(s, "DHCP messages passed:     {}", n.dhcp_forwarded);
    let _ = writeln!(s, "DHCP messages dropped:    {}", n.dhcp_dropped);
    let ports: Vec<(usize, &netlabcraft_model::Port)> =
        d.ports.iter().enumerate().filter(|(_, p)| p.l2.snoop_trust || p.l2.snoop_rate.is_some()).collect();
    let _ = writeln!(s, "\n{:<10} {:<8} Rate limit (messages/s)", "Port", "Trusted");
    for (i, p) in &ports {
        let rate = p.l2.snoop_rate.map_or("none".to_string(), |r| r.to_string());
        let _ = writeln!(s, "{:<10} {:<8} {rate}", short(d, *i), if p.l2.snoop_trust { "yes" } else { "no" });
    }
    if ports.is_empty() {
        let _ = writeln!(s, "(every port is untrusted, with no rate limit)");
    }
    s.trim_end().to_string()
}

fn show_bindings(sim: &Sim, d: &Device) -> String {
    let list = sim.snooping_bindings(d.id);
    let now = sim.now_us();
    let mut s = format!("{:<15} {:<16} {:<10} {:<5} Port\n", "MAC address", "IP address", "Lease (s)", "VLAN");
    for b in &list {
        let left = u64::from(b.lease_s).saturating_sub(now.saturating_sub(b.at_us) / 1_000_000);
        let _ = writeln!(s, "{:<15} {:<16} {:<10} {:<5} {}", b.mac.dotted(), b.ip.to_string(), left, b.vlan.to_string(), short(d, b.port));
    }
    let _ = write!(s, "{} binding{}", list.len(), if list.len() == 1 { "" } else { "s" });
    s
}

fn show_dai(sim: &Sim, d: &Device) -> String {
    let l = &d.l2;
    let mut s = String::new();
    let vl = if l.dai_vlans.is_empty() { "none".to_string() } else { vlan_list(&l.dai_vlans) };
    let _ = writeln!(s, "VLANs inspected:  {vl}");
    let v = l.dai_validate;
    let checks: Vec<&str> = [(v.src_mac, "src-mac"), (v.dst_mac, "dst-mac"), (v.ip, "ip")].into_iter().filter(|x| x.0).map(|x| x.1).collect();
    let _ = writeln!(s, "Extra checks:     {}", if checks.is_empty() { "none".to_string() } else { checks.join(", ") });
    if !l.snooping && !l.dai_vlans.is_empty() {
        let _ = writeln!(s, "Note: DHCP snooping is off, so there are no bindings: ARP from untrusted ports is dropped.");
    }
    if !l.dai_vlans.is_empty() {
        let _ = writeln!(s, "\n{:<6} {:<10} {:<8} Failed checks", "VLAN", "Forwarded", "Dropped");
        for vlan in &l.dai_vlans {
            let n = sim.arp_inspection_counts(d.id, *vlan);
            let _ = writeln!(s, "{:<6} {:<10} {:<8} {}", vlan.to_string(), n.forwarded, n.dropped, n.dropped_validation);
        }
    }
    let ports: Vec<usize> = d.ports.iter().enumerate().filter(|(_, p)| p.l2.dai_trust || p.l2.dai_rate.is_some()).map(|(i, _)| i).collect();
    let _ = writeln!(s, "\n{:<10} {:<8} Rate limit (messages/s)", "Port", "Trusted");
    for i in &ports {
        let Some(p) = d.ports.get(*i) else { continue };
        let rate = p.l2.dai_rate.map_or("none".to_string(), |r| r.to_string());
        let _ = writeln!(s, "{:<10} {:<8} {rate}", short(d, *i), if p.l2.dai_trust { "yes" } else { "no" });
    }
    if ports.is_empty() {
        let _ = writeln!(s, "(every port is untrusted, with no rate limit)");
    }
    s.trim_end().to_string()
}

/// What shut a port, in words (`show interfaces`).
pub(crate) fn cause_words(cause: Option<ErrCause>) -> &'static str {
    match cause.unwrap_or(ErrCause::PsecureViolation) {
        ErrCause::PsecureViolation => "port security",
        ErrCause::Bpduguard => "BPDU guard",
        ErrCause::DhcpRateLimit => "the DHCP snooping rate limit",
        ErrCause::ArpInspection => "the ARP inspection rate limit",
    }
}

fn cause_of(p: &netlabcraft_model::Port) -> ErrCause {
    p.l2.err_cause.unwrap_or(ErrCause::PsecureViolation)
}

fn show_err_disabled(d: &Device) -> String {
    let down: Vec<(usize, &netlabcraft_model::Port)> = d.ports.iter().enumerate().filter(|(_, p)| p.err_disabled).collect();
    if down.is_empty() {
        return "No port is err-disabled.".into();
    }
    let mut s = format!("{:<10} {:<18} {:<13} Cause\n", "Port", "Description", "State");
    for (i, p) in down {
        let desc: String = p.description.chars().take(18).collect();
        let _ = writeln!(s, "{:<10} {:<18} {:<13} {}", short(d, i), desc, "err-disabled", cause_of(p).name());
    }
    s.trim_end().to_string()
}

fn show_recovery(sim: &Sim, d: &Device) -> String {
    let l = &d.l2;
    let mut s = format!("{:<20} Recovers by itself\n", "Cause");
    for cause in ErrCause::ALL {
        let _ = writeln!(s, "{:<20} {}", cause.name(), if l.recovery.contains(&cause) { "yes" } else { "no" });
    }
    let _ = writeln!(s, "\nRecovery interval: {} s", l.recovery_interval);
    let down: Vec<(usize, &netlabcraft_model::Port)> = d.ports.iter().enumerate().filter(|(_, p)| p.err_disabled).collect();
    if !down.is_empty() {
        let _ = writeln!(s, "\n{:<10} {:<20} Back up in", "Port", "Cause");
        for (i, p) in down {
            let cause = cause_of(p);
            let when = if !l.recovery.contains(&cause) {
                "never (shutdown / no shutdown)".to_string()
            } else {
                let at = sim.err_disabled_since(d.id, i).unwrap_or(sim.now_us());
                let left = (at.saturating_add(u64::from(l.recovery_interval).saturating_mul(1_000_000))).saturating_sub(sim.now_us());
                format!("{} s", left.div_ceil(1_000_000))
            };
            let _ = writeln!(s, "{:<10} {:<20} {when}", short(d, i), cause.name());
        }
    }
    s.trim_end().to_string()
}

fn port_state(sim: &Sim, d: &Device, p: usize, vlan: VlanId, role: Role) -> &'static str {
    sim.stp_transition(d.id, p, vlan).unwrap_or(role.state())
}

fn show_stp_summary(t: &Topology, sim: &Sim, d: &Device, trees: &[(VlanId, netlabcraft_sim::stp::VlanTree)]) -> String {
    let l = &d.l2;
    let mut s = String::new();
    let _ = writeln!(s, "Mode:                 {}", if d.rapid_stp { "rapid-pvst" } else { "pvst" });
    let roots: Vec<String> =
        trees.iter().filter(|(_, tr)| tr.bridges.get(&d.id).is_some_and(|b| b.root == b.id)).map(|(v, _)| format!("VLAN{:04}", v.get())).collect();
    let _ = writeln!(s, "Root bridge of:       {}", if roots.is_empty() { "none".to_string() } else { roots.join(", ") });
    let _ = writeln!(s, "Portfast default:     {}", on_off(l.portfast_default));
    let _ = writeln!(s, "BPDU guard default:   {}", on_off(l.bpduguard_default));
    let guarded: Vec<String> = d.ports.iter().enumerate().filter(|(_, p)| p.l2.root_guard).map(|(i, _)| short(d, i)).collect();
    let _ = writeln!(s, "Root guard on:        {}", if guarded.is_empty() { "none".to_string() } else { guarded.join(", ") });
    let _ = writeln!(s, "\n{:<10} {:<9} {:<10} {:<9} {:<11} Total", "VLAN", "Blocking", "Listening", "Learning", "Forwarding");
    let mut total = [0usize; 5];
    for (v, tree) in trees {
        let mut n = [0usize; 4];
        for ((dev, p), info) in tree.ports.iter().filter(|((dev, _), i)| *dev == d.id && i.role != Role::Disabled) {
            let _ = dev;
            let i = match port_state(sim, d, *p, *v, info.role) {
                "BLK" => 0,
                "LIS" => 1,
                "LRN" => 2,
                _ => 3,
            };
            if let Some(x) = n.get_mut(i) {
                *x += 1;
            }
        }
        let sum: usize = n.iter().sum();
        for (a, b) in total.iter_mut().zip(n.iter().chain(std::iter::once(&sum))) {
            *a += b;
        }
        let _ = writeln!(s, "{:<10} {:<9} {:<10} {:<9} {:<11} {sum}", format!("VLAN{:04}", v.get()), n[0], n[1], n[2], n[3]);
    }
    let label = format!("{} VLAN{}", trees.len(), if trees.len() == 1 { "" } else { "s" });
    let _ = writeln!(s, "{:<10} {:<9} {:<10} {:<9} {:<11} {}", label, total[0], total[1], total[2], total[3], total[4]);
    let _ = t;
    s.trim_end().to_string()
}

fn show_stp_detail(t: &Topology, sim: &Sim, d: &Device, vlan: VlanId, tree: &netlabcraft_sim::stp::VlanTree) -> String {
    let Some(me) = tree.bridges.get(&d.id) else { return format!("VLAN{:04}: this switch has no port in the tree", vlan.get()) };
    let mut s = format!("VLAN{:04} ({})\n", vlan.get(), if d.rapid_stp { "rapid spanning tree" } else { "IEEE 802.1D spanning tree" });
    let _ = writeln!(s, "  This switch:  priority {}, address {}", me.id.priority, me.id.mac.dotted());
    if me.root == me.id {
        let _ = writeln!(s, "  Root:         this switch");
    } else {
        let rp = me.root_port.map_or("-".to_string(), |p| netlabcraft_sim::channel::port_label(t, d, p, true));
        let _ =
            writeln!(s, "  Root:         priority {}, address {}; path cost {} through {rp}", me.root.priority, me.root.mac.dotted(), me.root_cost);
    }
    let _ = writeln!(s, "  Timers:       hello 2 s, max age 20 s, forward delay 15 s");
    for ((dev, p), info) in tree.ports.iter().filter(|((dev, _), i)| *dev == d.id && i.role != Role::Disabled) {
        let _ = dev;
        let Some(port) = d.ports.get(*p) else { continue };
        let role = match info.role {
            Role::Root => "root",
            Role::Designated => "designated",
            Role::Alternate => "alternate",
            Role::Disabled => "disabled",
        };
        let state = match port_state(sim, d, *p, vlan, info.role) {
            "LIS" => "listening",
            "LRN" => "learning",
            "BLK" => "blocking",
            _ => "forwarding",
        };
        let inconsistent = tree.inconsistent.contains(&(d.id, *p));
        let _ = writeln!(
            s,
            "\n  {} (port id {}.{}): {role}, {}",
            netlabcraft_sim::channel::port_label(t, d, *p, false),
            info.port_id / 1000,
            info.port_id % 1000,
            if inconsistent { "blocked by root guard (root-inconsistent)" } else { state }
        );
        let lt = port.l2.link_type.map_or_else(
            || {
                let to_switch = t.peer(End { device: d.id, port: *p }).and_then(|q| t.device(q.device)).is_some_and(|x| x.kind == DeviceKind::Switch);
                if to_switch { "point-to-point (from duplex)" } else { "point-to-point (to a host)" }
            },
            |x| x.name(),
        );
        let edge = netlabcraft_sim::l2sec::edge_port(t, d, *p);
        let _ = writeln!(s, "    Path cost {}{}; link type {lt}", info.cost, if port.l2.cost(vlan).is_some() { " (set)" } else { " (from speed)" });
        let _ = writeln!(
            s,
            "    Edge port {}; BPDU guard {}; root guard {}",
            if edge { "yes" } else { "no" },
            on_off(netlabcraft_sim::l2sec::bpduguard_on(t, d, *p)),
            on_off(port.l2.root_guard)
        );
    }
    s.trim_end().to_string()
}

fn show_load_balance(d: &Device) -> String {
    let lb = d.l2.load_balance;
    let (ip, other) = match lb {
        LoadBalance::SrcMac => ("source MAC address", "source MAC address"),
        LoadBalance::DstMac => ("destination MAC address", "destination MAC address"),
        LoadBalance::SrcDstMac => ("source and destination MAC addresses", "source and destination MAC addresses"),
        LoadBalance::SrcIp => ("source IP address", "source MAC address"),
        LoadBalance::DstIp => ("destination IP address", "destination MAC address"),
        LoadBalance::SrcDstIp => ("source and destination IP addresses", "source and destination MAC addresses"),
    };
    format!("EtherChannel load balancing: {}\n  IPv4 frames pick a member by their {ip}\n  Other frames pick a member by their {other}", lb.name())
}

fn show_port_channels(t: &Topology, d: &Device) -> String {
    let chans = netlabcraft_sim::channel::channels(t, d);
    if chans.is_empty() {
        return "No EtherChannel on this switch.".into();
    }
    let mut s = String::new();
    for c in &chans {
        let _ = writeln!(
            s,
            "Port-channel{}: {}, protocol {}, {} of {} ports bundled, load balancing {}",
            c.number,
            if c.is_up() { "up" } else { "down" },
            c.protocol(d),
            c.bundled().len(),
            c.members.len(),
            d.l2.load_balance.name()
        );
        let _ = writeln!(s, "  {:<9} {:<10} {:<11} LACP priority", "Port", "Mode", "State");
        for (p, st) in &c.members {
            let Some(port) = d.ports.get(*p) else { continue };
            let mode = port.channel.map_or("", |g| g.mode.name());
            let pri = port.l2.lacp_priority.unwrap_or(32_768);
            let _ = writeln!(s, "  {:<9} {:<10} {:<11} {pri}", ifname::short(&port.name), mode, st.name());
        }
        let _ = writeln!(s);
    }
    s.trim_end().to_string()
}

fn lacp_state(state: u8) -> String {
    use netlabcraft_sim::l2sec::lacp;
    let mut w = vec![if state & lacp::ACTIVITY != 0 { "active" } else { "passive" }];
    if state & lacp::SYNC != 0 {
        w.push("in sync");
    }
    if state & lacp::COLLECTING != 0 {
        w.push("collecting");
    }
    if state & lacp::DISTRIBUTING != 0 {
        w.push("distributing");
    }
    if state & lacp::DEFAULTED != 0 {
        w.push("nothing heard");
    }
    w.join(", ")
}

fn show_lacp_neighbors(t: &Topology, d: &Device) -> String {
    let chans: Vec<_> = netlabcraft_sim::channel::channels(t, d).into_iter().filter(|c| c.protocol(d) == "LACP").collect();
    if chans.is_empty() {
        return "No LACP channel on this switch.".into();
    }
    let mut s = String::new();
    for c in &chans {
        let _ = writeln!(s, "Port-channel{}", c.number);
        let _ = writeln!(s, "  {:<9} {:<22} {:<6} {:<9} {:<5} Partner state", "Port", "Partner system", "Port", "Priority", "Key");
        for (p, _) in &c.members {
            let name = short(d, *p);
            match netlabcraft_sim::channel::lacpdu(t, d, *p).map(|x| x.partner).filter(|x| x.state & netlabcraft_sim::l2sec::lacp::DEFAULTED == 0) {
                Some(x) => {
                    let _ = writeln!(
                        s,
                        "  {:<9} {:<22} {:<6} {:<9} {:<5} {}",
                        name,
                        format!("{},{}", x.system_priority, x.system.dotted()),
                        x.port,
                        x.port_priority,
                        x.key,
                        lacp_state(x.state)
                    );
                }
                None => {
                    let _ = writeln!(s, "  {name:<9} (no LACP partner heard)");
                }
            }
        }
    }
    s.trim_end().to_string()
}
