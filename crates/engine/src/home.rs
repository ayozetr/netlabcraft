//! Home wireless routers: their factory settings, and the settings page's command.
//!
//! A home router is a multilayer switch (see `models::Model::home_router`): its LAN ports and
//! radio share VLAN 1, whose interface is the LAN gateway; the Internet port is routed. Factory
//! settings and every change go through the device's own configuration commands, so the
//! running configuration always says what the router does.

use netlabcraft_core::{DeviceId, Ipv4Net, mask_to_prefix, parse_ip};
use netlabcraft_model::{Topology, Wifi};
use netlabcraft_sim::Sim;

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
        radio.wifi = Some(Wifi { ssid: "HomeNetwork".into(), key: None, channel: 6, login: None });
    }
    // A home gateway also runs its IoT service, on its LAN address's web port.
    if let Some(d) = t.device_mut(id).filter(|d| netlabcraft_model::iot::can_serve(d)) {
        d.services.http.on = true;
        d.iot.service = true;
    }
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
