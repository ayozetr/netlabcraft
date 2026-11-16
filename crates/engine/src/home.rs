//! Home wireless routers: their factory settings, and the settings page's command.
//!
//! A home router is a multilayer switch (see `models::Model::home_router`): its LAN ports and
//! radio share VLAN 1, whose interface is the LAN gateway; the Internet port is routed. Factory
//! settings and every change go through the device's own configuration commands, so the
//! running configuration always says what the router does.

use netlabcraft_core::{DeviceId, Ipv4Net, mask_to_prefix, parse_ip};
use netlabcraft_model::wlan::{Cipher, MAX_COVERAGE_CM, MIN_COVERAGE_CM, WifiLogin, WifiMode, WifiSecurity, check_wep_key};
use netlabcraft_model::{Topology, Wifi};
use netlabcraft_sim::Sim;
use serde_json::Value;

/// The LAN a new home router hands out (`.100` up).
pub const FACTORY_CONFIG: &str = "\
ip routing
interface Ethernet1
 spanning-tree portfast
interface Ethernet2
 spanning-tree portfast
interface Ethernet3
 spanning-tree portfast
interface Ethernet4
 spanning-tree portfast
interface Wireless0
 spanning-tree portfast
interface Internet0
 no switchport
 ip nat outside
 no shutdown
interface Vlan1
 ip address 192.168.0.1 255.255.255.0
 ip nat inside
 no shutdown
ip dhcp excluded-address 192.168.0.1 192.168.0.99
ip dhcp pool LAN
 network 192.168.0.0 255.255.255.0
 default-router 192.168.0.1
access-list 1 permit 192.168.0.0 0.0.0.255
ip nat inside source list 1 interface Internet0 overload
";

/// Applies the factory settings to a home router just added; anything else is left alone.
pub fn factory(t: &mut Topology, id: DeviceId) {
    // A central office server leaves the factory with its interfaces up: it is the provider's
    // equipment, and what students set is the pool.
    if let Some(d) = t.device_mut(id).filter(|d| d.is_central_office()) {
        for p in &mut d.ports {
            p.shutdown = false;
        }
        return;
    }
    if !t.device(id).is_some_and(|d| d.catalogue_model().home_router()) {
        return;
    }
    let mut scratch = Sim::new();
    let _ = netlabcraft_ios::apply_config(t, &mut scratch, id, FACTORY_CONFIG);
    if let Some(radio) = t.device_mut(id).and_then(|d| d.ports.iter_mut().find(|p| netlabcraft_model::ifname::is_wireless(&p.name))) {
        radio.wifi = Some(Wifi { ssid: "HomeNetwork".into(), key: None, channel: 6, login: None, ..Default::default() });
    }
    // A home gateway also runs its IoT service, on its LAN address's web port.
    if let Some(d) = t.device_mut(id).filter(|d| netlabcraft_model::iot::can_serve(d)) {
        d.services.http.on = true;
        d.iot.service = true;
    }
}

/// The Wi-Fi settings a `device.wireless` call asks for, checked; `None` for an empty SSID (the
/// radio is off). `old` are the radio's present settings: a channel or coverage range not given
/// stays as it is. Without `mode` the settings read as they did before the modes existed: WPA2-PSK
/// when there is a passphrase, an open network when there is not.
pub(crate) fn wifi_from_params(is_host: bool, p: &Value, old: Option<&Wifi>) -> Result<Option<Wifi>, String> {
    let text = |k: &str| p.get(k).and_then(Value::as_str);
    let ssid: String = text("ssid").ok_or("`ssid` (text) is required")?.trim().chars().take(32).collect();
    if ssid.is_empty() {
        return Ok(None);
    }
    let key = text("key").map(str::to_string).filter(|k| !k.is_empty());
    let wep_key = text("wep_key").unwrap_or("").to_string();
    let mode = match text("mode").filter(|m| !m.trim().is_empty()) {
        Some(m) => {
            Some(WifiMode::parse(m).ok_or_else(|| format!("`mode` is open, wep, wpa-psk, wpa2-psk, wpa-enterprise or wpa2-enterprise, not `{m}`"))?)
        }
        None => None,
    };
    let encryption = match text("encryption").filter(|m| !m.trim().is_empty()) {
        Some(e) => Some(Cipher::parse(e).ok_or_else(|| format!("`encryption` is aes or tkip, not `{e}`"))?),
        None => None,
    };
    let channel = p.get("channel").and_then(Value::as_u64).map_or(old.map_or(6, |w| w.channel), |c| c.clamp(1, 13) as u8);
    let user: String = text("user").unwrap_or("").trim().chars().take(64).collect();
    let password: String = text("password").unwrap_or("").chars().take(64).collect();
    if !user.is_empty() && !is_host {
        return Err("only hosts log in to an enterprise network; the access point is set to a RADIUS server (`radius_ip`), a controller's WLANs with device.wlan".into());
    }
    let login = (!user.is_empty()).then_some(WifiLogin { user, password });
    let radius_ip = match text("radius_ip").map(str::trim).filter(|a| !a.is_empty()) {
        Some(a) => Some(parse_ip(a).map_err(|e| format!("`radius_ip`: {e}"))?),
        None => None,
    };
    let radius_secret: String = text("radius_secret").unwrap_or("").chars().take(64).collect();
    if is_host && (radius_ip.is_some() || !radius_secret.is_empty()) {
        return Err("a host has no RADIUS server: it gives a `user` and `password`; the access point has the server".into());
    }
    if let Some(k) = &key
        && !(8..=63).contains(&k.chars().count())
    {
        return Err("a WPA passphrase has 8-63 characters".into());
    }
    let mut security = WifiSecurity { mode, ..WifiSecurity::default() };
    match mode {
        // Before the modes: the passphrase decides.
        None => {
            if !wep_key.is_empty() {
                return Err("a WEP key needs `mode`: wep".into());
            }
            if radius_ip.is_some() {
                return Err("a RADIUS server needs `mode`: wpa-enterprise or wpa2-enterprise".into());
            }
            security.encryption = encryption.unwrap_or_default();
        }
        Some(WifiMode::Open) => {
            if key.is_some() || !wep_key.is_empty() || login.is_some() {
                return Err("an open network has no key, passphrase or login".into());
            }
        }
        Some(WifiMode::Wep) => {
            if key.is_some() || login.is_some() {
                return Err("a WEP network has a WEP key (`wep_key`), not a passphrase or login".into());
            }
            check_wep_key(&wep_key)?;
            security.wep_key = wep_key;
        }
        Some(m) if m.is_psk() => {
            if key.is_none() {
                return Err("a WPA-PSK or WPA2-PSK network needs its passphrase (`key`, 8-63 characters)".into());
            }
            if !wep_key.is_empty() || login.is_some() {
                return Err("a pre-shared-key network has a passphrase, not a WEP key or login".into());
            }
            security.encryption = encryption.unwrap_or_default();
        }
        Some(_) => {
            if key.is_some() || !wep_key.is_empty() {
                return Err("an enterprise network has a login (hosts) or a RADIUS server (access points), not a passphrase or WEP key".into());
            }
            if is_host {
                if login.is_none() {
                    return Err("an enterprise network needs the host's `user` and `password`".into());
                }
            } else if radius_ip.is_none() || radius_secret.is_empty() {
                return Err("an enterprise access point needs its RADIUS server (`radius_ip`) and shared secret (`radius_secret`)".into());
            }
            security.encryption = encryption.unwrap_or_default();
            security.radius_ip = radius_ip;
            security.radius_secret = radius_secret;
        }
    }
    if encryption.is_some_and(|e| e != Cipher::Aes) && mode.is_some_and(|m| !m.is_wpa()) {
        return Err("only WPA and WPA2 networks have an encryption type".into());
    }
    let coverage_cm = match p.get("coverage_m").and_then(Value::as_f64) {
        None => old.and_then(|w| w.coverage_cm),
        Some(_) if is_host => return Err("only an access point has a coverage range".into()),
        Some(m) if !m.is_finite() => return Err("`coverage_m` is a number of metres".into()),
        Some(m) => {
            let cm = (m * 100.0).round().clamp(0.0, 1.0e9) as u32;
            if !(MIN_COVERAGE_CM..=MAX_COVERAGE_CM).contains(&cm) {
                return Err(format!("the coverage range is {} to {} metres", MIN_COVERAGE_CM / 100, MAX_COVERAGE_CM / 100));
            }
            (cm != (netlabcraft_model::wlan::DEFAULT_COVERAGE_M * 100.0) as u32).then_some(cm)
        }
    };
    Ok(Some(Wifi { ssid, key, channel, login, security, coverage_cm }))
}

/// What the settings page sets.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub lan: Option<(String, String)>,
    pub dhcp: Option<bool>,
    pub dhcp_start: Option<String>,
    /// Static Internet address, mask and gateway (`None` leaves the Internet port as it is).
    pub internet: Option<(String, String, String)>,
    /// The Internet port takes its address from the provider's DHCP server.
    pub internet_dhcp: bool,
}

/// The configuration lines that make a home router match `s`.
pub fn settings_config(t: &Topology, id: DeviceId, s: &Settings) -> Result<String, String> {
    let d = t.device(id).ok_or("no such device")?;
    if !d.catalogue_model().home_router() {
        return Err(format!("{} is not a home router", d.name));
    }
    let lan_now = d.svis.iter().find(|v| v.vlan.get() == 1).and_then(|v| v.ip).unwrap_or(Ipv4Net::new(std::net::Ipv4Addr::new(192, 168, 0, 1), 24));
    let mut lan = lan_now;
    let mut out = String::new();
    if let Some((addr, mask)) = &s.lan {
        let a = parse_ip(addr).map_err(|e| e.to_string())?;
        let m = parse_ip(mask).map_err(|e| e.to_string())?;
        let prefix = mask_to_prefix(m).ok_or(format!("{mask} is not a subnet mask"))?;
        lan = Ipv4Net::new(a, prefix);
        out.push_str(&format!("interface Vlan1\n ip address {a} {m}\nexit\n"));
    }
    let net = lan.network();
    let dhcp_on = s.dhcp.unwrap_or(!d.dhcp_pools.is_empty());
    // The pool, its excluded range and NAT's list follow the LAN.
    out.push_str("no ip dhcp pool LAN\n");
    for (x, y) in &d.dhcp_excluded {
        out.push_str(&format!("no ip dhcp excluded-address {x} {y}\n"));
    }
    if dhcp_on {
        let start = match &s.dhcp_start {
            Some(a) => parse_ip(a).map_err(|e| e.to_string())?,
            None => std::net::Ipv4Addr::from(u32::from(net.addr) + 100),
        };
        if !lan.contains(start) || start == lan.addr {
            return Err(format!("the first address {start} must be in {net} and not the router's"));
        }
        let first = std::net::Ipv4Addr::from(u32::from(net.addr) + 1);
        let before = std::net::Ipv4Addr::from(u32::from(start).saturating_sub(1));
        if u32::from(before) >= u32::from(first) {
            out.push_str(&format!("ip dhcp excluded-address {first} {before}\n"));
        }
        out.push_str(&format!("ip dhcp pool LAN\n network {} {}\n default-router {}\nexit\n", net.addr, net.mask(), lan.addr));
    }
    out.push_str(&format!("no access-list 1\naccess-list 1 permit {} {}\n", net.addr, std::net::Ipv4Addr::from(!u32::from(net.mask()))));
    if s.internet_dhcp {
        for r in d.routes.iter().filter(|r| r.net.prefix == 0) {
            if let netlabcraft_model::NextHop::Address(old) = r.via {
                out.push_str(&format!("no ip route 0.0.0.0 0.0.0.0 {old}\n"));
            }
        }
        out.push_str("interface Internet0\n ip address dhcp\nexit\n");
    } else if let Some((addr, mask, gw)) = &s.internet {
        let a = parse_ip(addr).map_err(|e| e.to_string())?;
        let m = parse_ip(mask).map_err(|e| e.to_string())?;
        let g = parse_ip(gw).map_err(|e| e.to_string())?;
        for r in d.routes.iter().filter(|r| r.net.prefix == 0) {
            if let netlabcraft_model::NextHop::Address(old) = r.via {
                out.push_str(&format!("no ip route 0.0.0.0 0.0.0.0 {old}\n"));
            }
        }
        out.push_str(&format!("interface Internet0\n ip address {a} {m}\nexit\nip route 0.0.0.0 0.0.0.0 {g}\n"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_factory_line_is_understood() {
        let mut t = Topology::new();
        let m = netlabcraft_model::models::find("Home-Router").unwrap();
        let id = t.add_model(m, None, 0.0, 0.0).unwrap();
        let failed = netlabcraft_ios::apply_config(&mut t, &mut Sim::new(), id, FACTORY_CONFIG);
        assert!(failed.is_empty(), "{failed:?}");
    }
}
