//! Controller-based Wi-Fi: a wireless LAN controller (model `WLC`) holds the WLANs, and the
//! lightweight access points (model `LAP`) that join it over the wired network offer them.
//! WPA2-Enterprise WLANs check each host's login with the controller's RADIUS servers.
//!
//! Who joins whom and who may associate are worked out from the lab, as the routing protocols
//! are; the simulator shows the CAPWAP, 802.11 and EAPOL exchanges that would say so.

use std::collections::BTreeSet;
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, Ipv4Net, VlanId};
use serde::{Deserialize, Serialize};

use crate::{Device, DeviceKind, End, Topology, Wifi, ifname};

/// Most WLANs a controller holds, and most access points that may join one controller.
pub const MAX_WLANS: usize = 16;
pub const MAX_JOINED: usize = 64;
/// The model ids.
pub const WLC_MODEL: &str = "WLC";
pub const LAP_MODEL: &str = "LAP";

/// How a WLAN admits hosts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WlanSecurity {
    Open,
    /// WPA2-Personal with this passphrase.
    Psk(String),
    /// WPA2-Enterprise: IEEE 802.1X, each host's login checked by the controller's RADIUS
    /// servers (`radius-server host`).
    Enterprise,
}

impl WlanSecurity {
    pub fn name(&self) -> &'static str {
        match self {
            WlanSecurity::Open => "open",
            WlanSecurity::Psk(_) => "WPA2-PSK",
            WlanSecurity::Enterprise => "WPA2-Enterprise",
        }
    }
}

/// A WLAN a controller pushes to its access points.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Wlan {
    /// 1-16.
    pub id: u8,
    pub ssid: String,
    pub security: WlanSecurity,
    /// The VLAN the access points bridge this WLAN's hosts into, tagged on their uplink
    /// (`None`: the AP's own VLAN, untagged).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vlan: Option<VlanId>,
    /// Central switching: the access points tunnel the hosts' traffic to the controller over
    /// CAPWAP data, and the controller bridges it into the VLAN (`false`: local switching, the
    /// APs bridge it onto their own uplink).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub central: bool,
}

/// Wireless-controller settings of a device.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WlanConf {
    /// Controllers: the WLANs, by id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wlans: Vec<Wlan>,
    /// Lightweight APs: the controller to join (`None`: one in the AP's own subnet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller: Option<Ipv4Addr>,
}

impl WlanConf {
    pub fn is_default(&self) -> bool {
        *self == WlanConf::default()
    }
}

/// A host's WPA2-Enterprise login.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WifiLogin {
    pub user: String,
    pub password: String,
}

/// How wide an access point's radio reaches unless told otherwise, in metres.
pub const DEFAULT_COVERAGE_M: f32 = 250.0;
/// The coverage range an access point accepts, in centimetres (1 m to 2 km).
pub const MIN_COVERAGE_CM: u32 = 100;
pub const MAX_COVERAGE_CM: u32 = 200_000;

/// How a standalone access point (or a home router) secures its network, and what a host must
/// be set to in order to join it. WEP and TKIP are broken and are here because the courses
/// still show them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WifiMode {
    Open,
    Wep,
    WpaPsk,
    Wpa2Psk,
    WpaEnterprise,
    Wpa2Enterprise,
}

impl WifiMode {
    pub const ALL: [WifiMode; 6] =
        [WifiMode::Open, WifiMode::Wep, WifiMode::WpaPsk, WifiMode::Wpa2Psk, WifiMode::WpaEnterprise, WifiMode::Wpa2Enterprise];

    /// The name `show` commands and messages use.
    pub fn name(self) -> &'static str {
        match self {
            WifiMode::Open => "open",
            WifiMode::Wep => "WEP",
            WifiMode::WpaPsk => "WPA-PSK",
            WifiMode::Wpa2Psk => "WPA2-PSK",
            WifiMode::WpaEnterprise => "WPA-Enterprise",
            WifiMode::Wpa2Enterprise => "WPA2-Enterprise",
        }
    }

    /// The mode a word names: the file spelling (`wpa2-psk`), the screen name (`WPA2`) or `none`.
    pub fn parse(word: &str) -> Option<WifiMode> {
        match word.trim().to_ascii_lowercase().as_str() {
            "open" | "none" | "disabled" => Some(WifiMode::Open),
            "wep" => Some(WifiMode::Wep),
            "wpa-psk" | "wpa_psk" => Some(WifiMode::WpaPsk),
            "wpa2-psk" | "wpa2_psk" => Some(WifiMode::Wpa2Psk),
            "wpa" | "wpa-enterprise" | "wpa_enterprise" => Some(WifiMode::WpaEnterprise),
            "wpa2" | "wpa2-enterprise" | "wpa2_enterprise" => Some(WifiMode::Wpa2Enterprise),
            _ => None,
        }
    }

    pub fn is_psk(self) -> bool {
        matches!(self, WifiMode::WpaPsk | WifiMode::Wpa2Psk)
    }

    pub fn is_enterprise(self) -> bool {
        matches!(self, WifiMode::WpaEnterprise | WifiMode::Wpa2Enterprise)
    }

    /// WPA or WPA2 of either kind: the modes that have an encryption type and a handshake.
    pub fn is_wpa(self) -> bool {
        self.is_psk() || self.is_enterprise()
    }
}

/// The encryption type of a WPA network.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cipher {
    #[default]
    Aes,
    Tkip,
}

impl Cipher {
    pub fn name(self) -> &'static str {
        match self {
            Cipher::Aes => "AES",
            Cipher::Tkip => "TKIP",
        }
    }

    pub fn parse(word: &str) -> Option<Cipher> {
        match word.trim().to_ascii_lowercase().as_str() {
            "aes" | "ccmp" => Some(Cipher::Aes),
            "tkip" => Some(Cipher::Tkip),
            _ => None,
        }
    }
}

/// The security settings of a Wi-Fi radio beyond the passphrase (`Wifi::key`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WifiSecurity {
    /// `None`: from the passphrase and login (see `Wifi::mode`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<WifiMode>,
    #[serde(default)]
    pub encryption: Cipher,
    /// WEP: 5 or 13 ASCII characters, or 10 or 26 hexadecimal digits.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub wep_key: String,
    /// Access points in an enterprise mode: the RADIUS server that checks the logins, and the
    /// secret the access point shares with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius_ip: Option<Ipv4Addr>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub radius_secret: String,
}

impl WifiSecurity {
    pub fn is_default(&self) -> bool {
        *self == WifiSecurity::default()
    }
}

/// Whether a WEP key is well formed: 5 or 13 ASCII characters, or 10 or 26 hexadecimal digits.
pub fn check_wep_key(key: &str) -> Result<(), String> {
    wep_bytes(key).map(|_| ()).ok_or_else(|| "a WEP key is 5 or 13 ASCII characters, or 10 or 26 hexadecimal digits".to_string())
}

/// The key bytes a WEP key stands for, so `abcde` and `6162636465` are the same key.
pub fn wep_bytes(key: &str) -> Option<Vec<u8>> {
    let n = key.chars().count();
    if (n == 10 || n == 26) && key.chars().all(|c| c.is_ascii_hexdigit()) {
        return (0..n).step_by(2).map(|i| u8::from_str_radix(key.get(i..i + 2)?, 16).ok()).collect();
    }
    if (n == 5 || n == 13) && key.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) {
        return Some(key.as_bytes().to_vec());
    }
    None
}

/// A network a radio offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub ssid: String,
    /// What the 802.11 exchange shows: open (also for WEP), pre-shared key or 802.1X.
    pub security: WlanSecurity,
    /// The mode and encryption type a host must be set to.
    pub mode: WifiMode,
    pub cipher: Cipher,
    pub wep_key: String,
    /// An autonomous access point in an enterprise mode: its RADIUS server and shared secret.
    pub radius: Option<(Ipv4Addr, String)>,
    /// The device whose radio it is.
    pub ap: DeviceId,
    /// The controller whose WLAN it is (`None`: an autonomous access point's own setting).
    pub controller: Option<DeviceId>,
    pub channel: u8,
    /// The VLAN the WLAN's hosts land in (`None`: the radio port's own access VLAN).
    pub vlan: Option<VlanId>,
    /// Tunnelled to the controller (central switching) rather than bridged by the AP.
    pub central: bool,
}

impl Offer {
    /// The mode a host counts as being set to for this network: its own, except that a host
    /// from a lab file older than the modes, with a login, is taken to be set for an enterprise
    /// network.
    pub fn host_mode(&self, want: &Wifi) -> WifiMode {
        if want.security.mode.is_none() && self.mode.is_enterprise() && want.login.is_some() { self.mode } else { want.mode() }
    }
}

/// What became of a host's attempt to join the nearest radio offering its network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub ap: End,
    pub offer: Offer,
    /// `Ok` when the host's settings get it in, else why not.
    pub outcome: Result<(), String>,
}

pub fn is_wlc(d: &Device) -> bool {
    d.kind == DeviceKind::Switch && d.model == WLC_MODEL
}

pub fn is_lap(d: &Device) -> bool {
    d.kind == DeviceKind::Switch && d.model == LAP_MODEL
}

/// A switch's management address: its first VLAN interface with an address, up.
pub fn mgmt_addr(d: &Device) -> Option<(VlanId, Ipv4Net)> {
    d.svis.iter().filter(|s| !s.shutdown).find_map(|s| s.ip.map(|ip| (s.vlan, ip)))
}

/// The radio channel a lightweight AP picks: 1, 6 or 11, so neighbours rarely overlap.
pub fn lap_channel(id: DeviceId) -> u8 {
    [1, 6, 11].get((id.0 % 3) as usize).copied().unwrap_or(6)
}

impl Topology {
    /// Whether device `to` is reachable from `from` over the switched network of `vlan`.
    pub fn l2_reaches(&self, from: DeviceId, vlan: VlanId, to: DeviceId) -> bool {
        let carries = |d: &Device, p: usize| -> bool {
            let Some(port) = d.ports.get(p) else { return false };
            if port.shutdown || port.err_disabled || ifname::is_wireless(&port.name) {
                return false;
            }
            match d.kind {
                DeviceKind::Switch if port.routed => false,
                DeviceKind::Switch if self.is_trunk(End { device: d.id, port: p }) => port.switchport.allowed.contains(vlan),
                DeviceKind::Switch => port.switchport.access_vlan == vlan,
                DeviceKind::Hub => true,
                _ => false,
            }
        };
        let mut seen = BTreeSet::from([from]);
        let mut todo = vec![from];
        while let Some(id) = todo.pop() {
            let Some(d) = self.device(id) else { continue };
            for p in 0..d.ports.len() {
                if !carries(d, p) {
                    continue;
                }
                let Some(link) = self.link_at(End { device: id, port: p }) else { continue };
                let far = if link.a.device == id && link.a.port == p { link.b } else { link.a };
                let Some(fd) = self.device(far.device) else { continue };
                if fd.ports.get(far.port).is_none_or(|x| x.shutdown) {
                    continue;
                }
                if far.device == to && (fd.kind != DeviceKind::Switch || carries(fd, far.port)) {
                    return true;
                }
                if matches!(fd.kind, DeviceKind::Switch | DeviceKind::Hub) && carries(fd, far.port) && seen.insert(far.device) {
                    todo.push(far.device);
                }
            }
        }
        false
    }

    /// Whether a switch's default gateway sits in its own management subnet and answers there.
    fn gateway_reachable(&self, d: &Device) -> bool {
        let (Some(gw), Some((vlan, net))) = (d.gateway, mgmt_addr(d)) else { return false };
        if !net.contains(gw) {
            return false;
        }
        self.devices.iter().any(|g| {
            let owns = g.ports.iter().any(|p| p.ip.is_some_and(|ip| ip.addr == gw) || p.subifs.iter().any(|s| s.ip.is_some_and(|ip| ip.addr == gw)))
                || g.svis.iter().any(|s| s.ip.is_some_and(|ip| ip.addr == gw));
            owns && self.l2_reaches(d.id, vlan, g.id)
        })
    }

    /// The controller a lightweight AP is joined to: the one at its configured address, or the
    /// first in its own subnet; reachable over the switched network (through both default
    /// gateways when in another subnet). At most [`MAX_JOINED`] APs (the first by id) join one
    /// controller.
    pub fn lap_controller(&self, lap: DeviceId) -> Option<DeviceId> {
        let wlc = self.lap_controller_unbounded(lap)?;
        if self.devices.iter().filter(|d| is_lap(d)).count() <= MAX_JOINED {
            return Some(wlc);
        }
        let ahead = self.devices.iter().filter(|o| is_lap(o) && o.id < lap && self.lap_controller_unbounded(o.id) == Some(wlc)).count();
        (ahead < MAX_JOINED).then_some(wlc)
    }

    /// The lightweight APs joined to a controller, by id.
    pub fn joined_aps(&self, wlc: DeviceId) -> Vec<DeviceId> {
        self.devices
            .iter()
            .filter(|d| is_lap(d))
            .filter(|d| self.lap_controller_unbounded(d.id) == Some(wlc))
            .map(|d| d.id)
            .take(MAX_JOINED)
            .collect()
    }

    fn lap_controller_unbounded(&self, lap: DeviceId) -> Option<DeviceId> {
        let d = self.device(lap).filter(|d| is_lap(d))?;
        let (vlan, net) = mgmt_addr(d)?;
        let wlc = self.devices.iter().filter(|w| is_wlc(w)).find(|w| {
            mgmt_addr(w).is_some_and(|(_, wnet)| match d.wlan.controller {
                Some(a) => wnet.addr == a,
                None => net.contains(wnet.addr) && wnet.prefix == net.prefix,
            })
        })?;
        let (_, wnet) = mgmt_addr(wlc)?;
        let reach =
            if net.contains(wnet.addr) { self.l2_reaches(lap, vlan, wlc.id) } else { self.gateway_reachable(d) && self.gateway_reachable(wlc) };
        reach.then_some(wlc.id)
    }

    /// The networks a radio port offers: an access point's own setting, or the WLANs of a
    /// lightweight AP's controller.
    pub fn offers(&self, ap: End) -> Vec<Offer> {
        let Some(d) = self.device(ap.device).filter(|d| d.kind == DeviceKind::Switch) else { return Vec::new() };
        let Some(p) = d.ports.get(ap.port).filter(|p| ifname::is_wireless(&p.name) && !p.shutdown) else { return Vec::new() };
        if is_lap(d) {
            let Some(wlc) = self.lap_controller(d.id).and_then(|w| self.device(w)) else { return Vec::new() };
            return wlc
                .wlan
                .wlans
                .iter()
                .map(|w| Offer {
                    ssid: w.ssid.clone(),
                    mode: match w.security {
                        WlanSecurity::Open => WifiMode::Open,
                        WlanSecurity::Psk(_) => WifiMode::Wpa2Psk,
                        WlanSecurity::Enterprise => WifiMode::Wpa2Enterprise,
                    },
                    cipher: Cipher::Aes,
                    wep_key: String::new(),
                    radius: None,
                    ap: d.id,
                    security: w.security.clone(),
                    controller: Some(wlc.id),
                    channel: lap_channel(d.id),
                    vlan: w.vlan,
                    central: w.central,
                })
                .collect();
        }
        p.wifi
            .iter()
            .filter(|w| !w.ssid.is_empty())
            .map(|w| {
                let mode = w.mode();
                Offer {
                    ssid: w.ssid.clone(),
                    security: match mode {
                        WifiMode::Open | WifiMode::Wep => WlanSecurity::Open,
                        WifiMode::WpaPsk | WifiMode::Wpa2Psk => WlanSecurity::Psk(w.key.clone().unwrap_or_default()),
                        WifiMode::WpaEnterprise | WifiMode::Wpa2Enterprise => WlanSecurity::Enterprise,
                    },
                    mode,
                    cipher: w.security.encryption,
                    wep_key: w.security.wep_key.clone(),
                    radius: w.security.radius_ip.map(|ip| (ip, w.security.radius_secret.clone())),
                    ap: d.id,
                    controller: None,
                    channel: w.channel,
                    vlan: None,
                    central: false,
                }
            })
            .collect()
    }

    /// The VLANs a lightweight AP bridges WLANs into: those its controller's locally switched
    /// WLANs name (centrally switched ones go to the controller instead).
    pub fn lap_vlans(&self, lap: DeviceId) -> Vec<VlanId> {
        let Some(wlc) = self.device(lap).filter(|d| is_lap(d)).and_then(|_| self.lap_controller(lap)).and_then(|w| self.device(w)) else {
            return Vec::new();
        };
        let mut v: Vec<VlanId> = wlc.wlan.wlans.iter().filter(|w| !w.central).filter_map(|w| w.vlan).collect();
        v.sort();
        v.dedup();
        v
    }

    /// Whether a switch knows a VLAN: one it has, (a lightweight AP) one of its WLANs', or (a
    /// controller) one its centrally switched WLANs bridge into.
    pub fn switch_has_vlan(&self, d: &Device, vlan: VlanId) -> bool {
        d.vlans.contains_key(&vlan)
            || (is_lap(d) && self.lap_vlans(d.id).contains(&vlan))
            || (is_wlc(d) && d.wlan.wlans.iter().any(|w| w.central && w.vlan == Some(vlan)))
    }

    /// A centrally switched Wi-Fi host: its controller, its access point radio and the VLAN
    /// the controller bridges it into (the WLAN's, else the controller's management VLAN).
    pub fn central_client(&self, client: End) -> Option<(DeviceId, End, VlanId)> {
        let a = self.wifi_attempt(client).filter(|a| a.outcome.is_ok() && a.offer.central)?;
        let wlc = self.device(a.offer.controller?)?;
        let vlan = a.offer.vlan.or_else(|| mgmt_addr(wlc).map(|(v, _)| v)).unwrap_or(VlanId::DEFAULT);
        Some((wlc.id, a.ap, vlan))
    }

    /// Whether an access point's radio carries a VLAN besides its access VLAN: a lightweight
    /// AP's radio carries every VLAN its WLANs map to.
    pub fn radio_carries(&self, d: &Device, port: usize, vlan: VlanId) -> bool {
        d.ports.get(port).is_some_and(|p| ifname::is_wireless(&p.name)) && is_lap(d) && self.lap_vlans(d.id).contains(&vlan)
    }

    /// The VLAN a Wi-Fi host's frames land in at its access point: its WLAN's, else the radio
    /// port's access VLAN. `None` when the host isn't associated.
    pub fn wifi_vlan(&self, client: End) -> Option<VlanId> {
        let a = self.wifi_attempt(client).filter(|a| a.outcome.is_ok())?;
        let radio = self.device(a.ap.device)?.ports.get(a.ap.port)?;
        Some(a.offer.vlan.unwrap_or(radio.switchport.access_vlan))
    }

    /// Whether a controller's RADIUS servers accept a login: the first server it names that is
    /// a server running AAA, with the controller as a client sharing the same secret, decides.
    pub fn radius_accepts(&self, wlc: DeviceId, user: &str, password: &str) -> Result<(), String> {
        let d = self.device(wlc).ok_or("no controller")?;
        let (_, me) = mgmt_addr(d).ok_or("the controller has no management address")?;
        if d.access.radius_servers.is_empty() {
            return Err("the controller has no RADIUS server (radius-server host)".into());
        }
        for (addr, key) in &d.access.radius_servers {
            if let Some(answer) = self.radius_answer(me.addr, *addr, key, user, password) {
                return answer;
            }
        }
        Err("no RADIUS server answers the controller (check the address, the AAA service and its client entry)".into())
    }

    /// What the RADIUS server at `addr` says to a login from the client at `me` sharing `key`:
    /// `None` when no AAA server there knows that client.
    fn radius_answer(&self, me: Ipv4Addr, addr: Ipv4Addr, key: &str, user: &str, password: &str) -> Option<Result<(), String>> {
        let server = self.devices.iter().find(|s| s.kind == DeviceKind::Server && s.ports.iter().any(|p| p.ip.is_some_and(|ip| ip.addr == addr)))?;
        let aaa = &server.services.aaa;
        if !aaa.on || !aaa.clients.iter().any(|c| c.address == me && c.secret == key) {
            return None;
        }
        Some(if aaa.users.iter().any(|u| u.name == user && u.password == password) {
            Ok(())
        } else {
            Err(format!("the RADIUS server refused the login of `{user}`"))
        })
    }

    /// An access point's own RADIUS check (WPA or WPA2 enterprise on a standalone access point
    /// or home router): its RADIUS server must be reachable from the AP and know it as a client
    /// with the shared secret, and then decides about the login. Another subnet is reached when
    /// the AP's default gateway answers.
    pub fn ap_radius_accepts(&self, ap: DeviceId, radius: Option<&(Ipv4Addr, String)>, user: &str, password: &str) -> Result<(), String> {
        let d = self.device(ap).ok_or("no access point")?;
        let Some((addr, key)) = radius else { return Err("the access point has no RADIUS server set".into()) };
        let (vlan, me) = mgmt_addr(d).ok_or("the access point has no management address (give its VLAN interface an address)")?;
        let server = self
            .devices
            .iter()
            .find(|s| s.kind == DeviceKind::Server && s.ports.iter().any(|p| p.ip.is_some_and(|ip| ip.addr == *addr)))
            .ok_or("no server answers at the access point's RADIUS address")?;
        let reach = if me.contains(*addr) { self.l2_reaches(ap, vlan, server.id) } else { self.gateway_reachable(d) };
        if !reach {
            return Err("the access point can't reach its RADIUS server".into());
        }
        self.radius_answer(me.addr, *addr, key, user, password)
            .unwrap_or_else(|| Err("the RADIUS server doesn't know the access point (check the AAA service and its client entry and secret)".into()))
    }

    fn radio(&self, end: End) -> Option<(&Device, &crate::Port, &Wifi)> {
        let d = self.device(end.device)?;
        let p = d.ports.get(end.port).filter(|p| ifname::is_wireless(&p.name) && !p.shutdown)?;
        Some((d, p, p.wifi.as_ref()?))
    }

    /// Whether an access point (or wireless router) reaches a Wi-Fi host. Once the lab has a
    /// physical layout of its own, the walls decide: a host hears the access points of its own
    /// building and none of another building's; a host or access point standing outside any
    /// building (in a city) falls back to the distance on the logical map. Without a layout
    /// (every device in the one implicit building) the logical distance decides, up to
    /// [`crate::WIFI_RANGE`].
    pub fn wifi_in_range(&self, host: &Device, ap: &Device) -> bool {
        if !self.implicit_layout() {
            let building = |d: &Device| self.home_of(d.id).and_then(|h| self.building_of(h.place)).map(|b| b.id);
            if let (Some(a), Some(b)) = (building(host), building(ap)) {
                // Two radios in one place whose map has a scale of its own (its size was set): the
                // coverage range, in metres, is measured on that map.
                if let (Some(ha), Some(hb)) = (self.home_of(host.id), self.home_of(ap.id))
                    && ha.place == hb.place
                    && self.map_scale_is_custom(ha.place)
                {
                    let metres = ((ha.x - hb.x).powi(2) + (ha.y - hb.y).powi(2)).sqrt() * self.map_scale(ha.place);
                    let range_m = ap
                        .ports
                        .iter()
                        .find(|p| ifname::is_wireless(&p.name))
                        .and_then(|p| p.wifi.as_ref())
                        .map_or(DEFAULT_COVERAGE_M, Wifi::coverage_m);
                    return a == b && metres <= range_m;
                }
                return a == b;
            }
        }
        let reach = ap.ports.iter().find(|p| ifname::is_wireless(&p.name)).and_then(|p| p.wifi.as_ref()).map_or(crate::WIFI_RANGE, Wifi::range_units);
        ((ap.x - host.x).powi(2) + (ap.y - host.y).powi(2)).sqrt() <= reach
    }

    /// The nearest radio in range offering the network a Wi-Fi host is set to join, and
    /// whether the host's key or login gets it in. A radio that admits the host wins over a
    /// nearer one that would refuse it.
    pub fn wifi_attempt(&self, client: End) -> Option<Attempt> {
        let (cd, _, want) = self.radio(client)?;
        if !cd.kind.is_host() || want.ssid.is_empty() {
            return None;
        }
        let mut best: Option<(bool, f32, Attempt)> = None;
        for d in self.devices.iter().filter(|d| d.kind == DeviceKind::Switch) {
            let dist = ((d.x - cd.x).powi(2) + (d.y - cd.y).powi(2)).sqrt();
            if !self.wifi_in_range(cd, d) {
                continue;
            }
            for p in 0..d.ports.len() {
                let ap = End { device: d.id, port: p };
                for offer in self.offers(ap).into_iter().filter(|o| o.ssid == want.ssid) {
                    let outcome = self.admits(&offer, want);
                    let ok = outcome.is_ok();
                    let better = match &best {
                        None => true,
                        Some((bok, bdist, _)) => (ok && !bok) || (ok == *bok && dist < *bdist),
                    };
                    if better {
                        best = Some((ok, dist, Attempt { ap, offer, outcome }));
                    }
                }
            }
        }
        best.map(|(_, _, a)| a)
    }

    /// Whether a host's Wi-Fi settings get it into a network: its mode (and, for WPA, its
    /// encryption type) must be the network's, and then the key, passphrase or login decides.
    /// A host from a lab file older than the modes is set by its passphrase or login alone.
    pub fn admits(&self, offer: &Offer, want: &Wifi) -> Result<(), String> {
        let mine = offer.host_mode(want);
        let uses = |m: WifiMode| format!("the network uses {} but the host is set to {}", m.name(), mine.name());
        match offer.mode {
            WifiMode::Open if mine == WifiMode::Open && want.login.is_none() => Ok(()),
            WifiMode::Open => Err("the network is open but the host has a key or login set".into()),
            WifiMode::Wep => {
                if mine == WifiMode::Open {
                    return Err("the network needs a WEP key".into());
                }
                if mine != WifiMode::Wep {
                    return Err(uses(offer.mode));
                }
                match (wep_bytes(&want.security.wep_key), wep_bytes(&offer.wep_key)) {
                    (Some(a), Some(b)) if a == b => Ok(()),
                    _ => Err("wrong WEP key".into()),
                }
            }
            WifiMode::WpaPsk | WifiMode::Wpa2Psk => {
                let WlanSecurity::Psk(pass) = &offer.security else { return Err("the network has no passphrase".into()) };
                let (needs, wrong) = if offer.mode == WifiMode::WpaPsk {
                    ("the network needs a WPA passphrase", "wrong WPA passphrase")
                } else {
                    ("the network needs a WPA2 passphrase", "wrong WPA2 passphrase")
                };
                if mine == WifiMode::Open {
                    return Err(needs.into());
                }
                if mine != offer.mode {
                    return Err(uses(offer.mode));
                }
                if want.security.encryption != offer.cipher {
                    return Err(format!(
                        "the network encrypts with {} but the host is set to {}",
                        offer.cipher.name(),
                        want.security.encryption.name()
                    ));
                }
                match &want.key {
                    Some(k) if k == pass => Ok(()),
                    Some(_) => Err(wrong.into()),
                    None => Err(needs.into()),
                }
            }
            WifiMode::WpaEnterprise | WifiMode::Wpa2Enterprise => {
                let needs = if offer.mode == WifiMode::WpaEnterprise {
                    "the network needs a username and password (WPA-Enterprise)"
                } else {
                    "the network needs a username and password (WPA2-Enterprise)"
                };
                if mine == WifiMode::Open {
                    return Err(needs.into());
                }
                if mine != offer.mode {
                    return Err(uses(offer.mode));
                }
                if want.security.encryption != offer.cipher {
                    return Err(format!(
                        "the network encrypts with {} but the host is set to {}",
                        offer.cipher.name(),
                        want.security.encryption.name()
                    ));
                }
                let Some(l) = &want.login else { return Err(needs.into()) };
                match offer.controller {
                    Some(w) => self.radius_accepts(w, &l.user, &l.password),
                    None => self.ap_radius_accepts(offer.ap, offer.radius.as_ref(), &l.user, &l.password),
                }
            }
        }
    }

    /// The access point a Wi-Fi host is associated with: the nearest one in range offering
    /// its network that admits it.
    pub fn wifi_ap_of(&self, client: End) -> Option<End> {
        self.wifi_attempt(client).filter(|a| a.outcome.is_ok()).map(|a| a.ap)
    }

    /// The hosts associated with an access point's radio.
    pub fn wifi_clients(&self, ap: End) -> Vec<End> {
        let Some(d) = self.device(ap.device).filter(|d| d.kind == DeviceKind::Switch) else { return Vec::new() };
        if d.ports.get(ap.port).is_none_or(|p| !ifname::is_wireless(&p.name) || p.shutdown) {
            return Vec::new();
        }
        self.devices
            .iter()
            .filter(|d| d.kind.is_host())
            .flat_map(|d| (0..d.ports.len()).map(move |p| End { device: d.id, port: p }))
            .filter(|e| self.wifi_ap_of(*e) == Some(ap))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wep_keys_are_five_or_thirteen_characters_or_ten_or_twenty_six_hex_digits() {
        for ok in ["abcde", "abcdefghijklm", "6162636465", "00112233445566778899aabbcc"] {
            assert!(check_wep_key(ok).is_ok(), "{ok}");
        }
        for bad in ["", "abcd", "abcdef", "abcdefghijkl", "61626364zz", "0011223344556677889900"] {
            assert!(check_wep_key(bad).is_err(), "{bad}");
        }
        // The same key in ASCII and in hex.
        assert_eq!(wep_bytes("abcde"), wep_bytes("6162636465"));
    }

    #[test]
    fn a_wifi_from_an_older_file_reads_as_wpa2_psk_with_aes_and_the_default_range() {
        let w: Wifi = serde_json::from_str(r#"{"ssid":"Aula","key":"clase1234","channel":6}"#).unwrap();
        assert_eq!((w.mode(), w.security.encryption), (WifiMode::Wpa2Psk, Cipher::Aes));
        assert_eq!(w.range_units(), crate::WIFI_RANGE);
        assert_eq!(w.coverage_m(), DEFAULT_COVERAGE_M);
        let open: Wifi = serde_json::from_str(r#"{"ssid":"Casa"}"#).unwrap();
        assert_eq!(open.mode(), WifiMode::Open);
        // The new settings are not written while they are the defaults.
        let json = serde_json::to_string(&w).unwrap();
        assert!(!json.contains("security") && !json.contains("coverage"), "{json}");
        let set = Wifi {
            ssid: "x".into(),
            security: WifiSecurity { mode: Some(WifiMode::Wpa2Enterprise), encryption: Cipher::Tkip, ..WifiSecurity::default() },
            coverage_cm: Some(5000),
            ..Wifi::default()
        };
        let back: Wifi = serde_json::from_str(&serde_json::to_string(&set).unwrap()).unwrap();
        assert_eq!(back, set);
        assert!(set.range_units() < crate::WIFI_RANGE);
    }

    #[test]
    fn mode_and_cipher_names_parse() {
        for m in WifiMode::ALL {
            assert_eq!(WifiMode::parse(&serde_json::to_string(&m).unwrap().replace('"', "")), Some(m));
        }
        assert_eq!(WifiMode::parse("WPA2"), Some(WifiMode::Wpa2Enterprise));
        assert_eq!(Cipher::parse("TKIP"), Some(Cipher::Tkip));
        assert_eq!(Cipher::parse("rc4"), None);
    }
}
