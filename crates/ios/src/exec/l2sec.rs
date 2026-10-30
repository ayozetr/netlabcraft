//! Switch edge protection and tuning: DHCP snooping, dynamic ARP inspection, BPDU guard, root
//! guard, errdisable recovery, port-security aging, spanning-tree cost and port priority,
//! EtherChannel load balancing and LACP views, and a few small switch reports (`show vlan id`,
//! `show vlan summary`, `show dtp`, `show interfaces counters`, `show sdm prefer`).

use std::fmt::Write as _;

use netlabcraft_core::VlanSet;
use netlabcraft_model::l2sec::{ArpValidate, ERRDISABLE_INTERVAL, ErrCause, LinkType, LoadBalance, PortL2, vlan_list};
use netlabcraft_model::{Device, DeviceKind, ifname};
use netlabcraft_sim::Sim;

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
