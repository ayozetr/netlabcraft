//! Wireless-controller commands: `wlan` on a controller, `capwap controller` on a lightweight
//! access point, and `show wlan` / `show capwap` on both.

use std::fmt::Write as _;

use netlabcraft_core::parse_ip;
use netlabcraft_model::wlan::{MAX_WLANS, Wlan, WlanSecurity, is_lap, is_wlc, lap_channel, mgmt_addr};
use netlabcraft_model::{End, Topology, ifname};

use super::{Ctx, R};
use crate::grammar::Id;

/// The hosts associated through a lightweight AP's radio, with the SSID each joined.
fn ap_hosts(t: &Topology, ap: netlabcraft_core::DeviceId) -> Vec<(String, String)> {
    let Some(d) = t.device(ap) else { return Vec::new() };
    let Some(radio) = d.ports.iter().position(|p| ifname::is_wireless(&p.name)) else { return Vec::new() };
    t.wifi_clients(End { device: ap, port: radio })
        .into_iter()
        .filter_map(|c| {
            let h = t.device(c.device)?;
            Some((h.name.clone(), h.ports.get(c.port)?.wifi.as_ref()?.ssid.clone()))
        })
        .collect()
}

fn show_wlan(t: &Topology, dev: netlabcraft_core::DeviceId) -> R {
    let d = t.device(dev).ok_or("% That device no longer exists")?;
    let (wlans, aps) = if is_wlc(d) {
        (d.wlan.wlans.clone(), t.joined_aps(d.id))
    } else if is_lap(d) {
        match t.lap_controller(d.id).and_then(|w| t.device(w)) {
            Some(w) => (w.wlan.wlans.clone(), vec![d.id]),
            None => return Ok("Not joined to a controller: no WLANs to offer.".into()),
        }
    } else {
        return Err("% WLANs live on wireless LAN controllers (model WLC)".into());
    };
    if wlans.is_empty() {
        return Ok("No WLANs configured.".into());
    }
    let hosts: Vec<(String, String, String)> = aps
        .iter()
        .flat_map(|ap| ap_hosts(t, *ap).into_iter().map(move |(h, ssid)| (h, ssid, t.device(*ap).map(|x| x.name.clone()).unwrap_or_default())))
        .collect();
    let mut s = format!("{:<5} {:<24} {:<16} {:<6} Hosts\n", "WLAN", "SSID", "Security", "VLAN");
    for w in &wlans {
        let n = hosts.iter().filter(|(_, ssid, _)| *ssid == w.ssid).count();
        let vlan = w.vlan.map_or("-".to_string(), |v| v.to_string());
        let _ = writeln!(s, "{:<5} {:<24} {:<16} {vlan:<6} {n}", w.id, w.ssid, w.security.name());
    }
    if !hosts.is_empty() {
        let _ = writeln!(s, "\n{:<20} {:<24} Access point", "Host", "SSID");
        for (h, ssid, ap) in &hosts {
            let _ = writeln!(s, "{h:<20} {ssid:<24} {ap}");
        }
    }
    Ok(s.trim_end().to_string())
}

fn show_capwap(t: &Topology, dev: netlabcraft_core::DeviceId) -> R {
    let d = t.device(dev).ok_or("% That device no longer exists")?;
    if is_wlc(d) {
        let me = mgmt_addr(d).map_or("none (give interface Vlan1 an address)".to_string(), |(_, n)| n.addr.to_string());
        let aps = t.joined_aps(d.id);
        let mut s = format!("Controller {}, management address {me}\nAccess points joined: {}\n", d.name, aps.len());
        if !aps.is_empty() {
            let _ = writeln!(s, "\n{:<20} {:<16} {:<8} Hosts", "AP name", "Address", "Channel");
            for ap in aps {
                let Some(a) = t.device(ap) else { continue };
                let addr = mgmt_addr(a).map(|(_, n)| n.addr.to_string()).unwrap_or_default();
                let _ = writeln!(s, "{:<20} {addr:<16} {:<8} {}", a.name, lap_channel(ap), ap_hosts(t, ap).len());
            }
        }
        return Ok(s.trim_end().to_string());
    }
    if !is_lap(d) {
        return Err("% CAPWAP runs on wireless LAN controllers and lightweight access points".into());
    }
    let mut s = String::new();
    let mine = mgmt_addr(d);
    let _ = writeln!(
        s,
        "Access point {}, address {}",
        d.name,
        mine.map_or("none (give interface Vlan1 an address)".to_string(), |(_, n)| n.addr.to_string())
    );
    let _ = writeln!(s, "Controller wanted: {}", d.wlan.controller.map_or("any in this subnet".to_string(), |a| a.to_string()));
    match t.lap_controller(d.id).and_then(|w| t.device(w)) {
        Some(w) => {
            let addr = mgmt_addr(w).map(|(_, n)| n.addr.to_string()).unwrap_or_default();
            let _ = writeln!(s, "Joined to {} ({addr}), radio on channel {}, {} WLAN(s)", w.name, lap_channel(d.id), w.wlan.wlans.len());
            let vlans = t.lap_vlans(d.id);
            if !vlans.is_empty() {
                let list: Vec<String> = vlans.iter().map(ToString::to_string).collect();
                let trunk = (0..d.ports.len())
                    .any(|p| d.ports.get(p).is_some_and(|x| !ifname::is_wireless(&x.name)) && t.is_trunk(End { device: d.id, port: p }));
                let _ = writeln!(
                    s,
                    "WLAN VLANs {}: {}",
                    list.join(","),
                    if trunk { "tagged on the trunk uplink" } else { "no trunk uplink to carry them (switchport mode trunk)" }
                );
            }
        }
        None => {
            let _ = writeln!(s, "Not joined: no controller answers (check both addresses, the cabling and the default gateways)");
        }
    }
    Ok(s.trim_end().to_string())
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("").trim();
    match id {
        Id::ShowWlan => show_wlan(c.t, c.dev),
        Id::ShowCapwap => show_capwap(c.t, c.dev),
        Id::WlanAdd | Id::NoWlan => {
            if !is_wlc(c.d()?) {
                return Err("% WLANs are set on a wireless LAN controller (model WLC)".into());
            }
            let n: u8 = arg(0).parse().ok().filter(|n| (1..=MAX_WLANS as u8).contains(n)).ok_or(format!("% A WLAN number is 1-{MAX_WLANS}"))?;
            if id == Id::NoWlan {
                let d = c.dm()?;
                let before = d.wlan.wlans.len();
                d.wlan.wlans.retain(|w| w.id != n);
                return if d.wlan.wlans.len() == before { Err(format!("% There is no WLAN {n}")) } else { Ok(String::new()) };
            }
            let ssid = arg(1);
            if ssid.is_empty() || ssid.chars().count() > 32 || !ssid.chars().all(|ch| ch.is_ascii_graphic()) {
                return Err("% Give an SSID of 1-32 printable characters, without spaces".into());
            }
            let mut words: Vec<&str> = arg(2).split_whitespace().collect();
            // An optional `central` last: the APs tunnel the hosts' traffic to the controller.
            let central = words.last().is_some_and(|w| w.eq_ignore_ascii_case("central"));
            if central {
                words.pop();
            }
            // An optional `vlan V` at the end: the VLAN the APs bridge this WLAN into.
            let mut vlan = None;
            if let [.., kw, v] = words.as_slice()
                && kw.eq_ignore_ascii_case("vlan")
            {
                let n = v.parse::<u16>().ok().and_then(netlabcraft_core::VlanId::new).ok_or("% A VLAN is 1-4094")?;
                vlan = Some(n);
                words.truncate(words.len().saturating_sub(2));
            }
            let security = match words.as_slice() {
                [] | ["open"] => WlanSecurity::Open,
                ["psk", pass] if (8..=63).contains(&pass.chars().count()) => WlanSecurity::Psk((*pass).to_string()),
                ["psk", ..] => return Err("% A WPA2 passphrase has 8-63 characters, without spaces".into()),
                ["enterprise"] => WlanSecurity::Enterprise,
                _ => return Err("% Security is open, psk PASSPHRASE or enterprise".into()),
            };
            let d = c.dm()?;
            if d.wlan.wlans.iter().any(|w| w.ssid == ssid && w.id != n) {
                return Err(format!("% Another WLAN already offers {ssid}"));
            }
            let w = Wlan { id: n, ssid: ssid.to_string(), security, vlan, central };
            match d.wlan.wlans.iter_mut().find(|x| x.id == n) {
                Some(x) => *x = w,
                None => {
                    d.wlan.wlans.push(w);
                    d.wlan.wlans.sort_by_key(|x| x.id);
                }
            }
            Ok(String::new())
        }
        Id::CapwapController | Id::NoCapwapController => {
            if !is_lap(c.d()?) {
                return Err("% Only a lightweight access point (model LAP) joins a controller".into());
            }
            let a = if id == Id::CapwapController { Some(parse_ip(arg(0)).map_err(|e| format!("% {e}"))?) } else { None };
            c.dm()?.wlan.controller = a;
            Ok(String::new())
        }
        _ => Err("% Not a wireless-controller command".into()),
    }
}
