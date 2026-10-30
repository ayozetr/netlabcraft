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

/// A network a radio offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub ssid: String,
    pub security: WlanSecurity,
    /// The controller whose WLAN it is (`None`: an autonomous access point's own setting).
    pub controller: Option<DeviceId>,
    pub channel: u8,
    /// The VLAN the WLAN's hosts land in (`None`: the radio port's own access VLAN).
    pub vlan: Option<VlanId>,
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
                    security: w.security.clone(),
                    controller: Some(wlc.id),
                    channel: lap_channel(d.id),
                    vlan: w.vlan,
                })
                .collect();
        }
        p.wifi
            .iter()
            .filter(|w| !w.ssid.is_empty())
            .map(|w| Offer {
                ssid: w.ssid.clone(),
                security: w.key.clone().map_or(WlanSecurity::Open, WlanSecurity::Psk),
                controller: None,
                channel: w.channel,
                vlan: None,
            })
            .collect()
    }

    /// The VLANs a lightweight AP bridges WLANs into: those its controller's WLANs name.
    pub fn lap_vlans(&self, lap: DeviceId) -> Vec<VlanId> {
        let Some(wlc) = self.device(lap).filter(|d| is_lap(d)).and_then(|_| self.lap_controller(lap)).and_then(|w| self.device(w)) else {
            return Vec::new();
        };
        let mut v: Vec<VlanId> = wlc.wlan.wlans.iter().filter_map(|w| w.vlan).collect();
        v.sort();
        v.dedup();
        v
    }

    /// Whether a switch knows a VLAN: one it has, or (a lightweight AP) one of its WLANs'.
    pub fn switch_has_vlan(&self, d: &Device, vlan: VlanId) -> bool {
        d.vlans.contains_key(&vlan) || (is_lap(d) && self.lap_vlans(d.id).contains(&vlan))
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
            let Some(server) =
                self.devices.iter().find(|s| s.kind == DeviceKind::Server && s.ports.iter().any(|p| p.ip.is_some_and(|ip| ip.addr == *addr)))
            else {
                continue;
            };
            let aaa = &server.services.aaa;
            if !aaa.on || !aaa.clients.iter().any(|c| c.address == me.addr && c.secret == *key) {
                continue;
            }
            return if aaa.users.iter().any(|u| u.name == user && u.password == password) {
                Ok(())
            } else {
                Err(format!("the RADIUS server refused the login of `{user}`"))
            };
        }
        Err("no RADIUS server answers the controller (check the address, the AAA service and its client entry)".into())
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
                return a == b;
            }
        }
        ((ap.x - host.x).powi(2) + (ap.y - host.y).powi(2)).sqrt() <= crate::WIFI_RANGE
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

    /// Whether a host's Wi-Fi settings get it into a network.
    pub fn admits(&self, offer: &Offer, want: &Wifi) -> Result<(), String> {
        match (&offer.security, &want.key, &want.login) {
            (WlanSecurity::Open, None, None) => Ok(()),
            (WlanSecurity::Open, ..) => Err("the network is open but the host has a key or login set".into()),
            (WlanSecurity::Psk(k), Some(mine), _) if k == mine => Ok(()),
            (WlanSecurity::Psk(_), Some(_), _) => Err("wrong WPA2 passphrase".into()),
            (WlanSecurity::Psk(_), None, _) => Err("the network needs a WPA2 passphrase".into()),
            (WlanSecurity::Enterprise, _, Some(l)) => match offer.controller {
                Some(w) => self.radius_accepts(w, &l.user, &l.password),
                None => Err("no controller to check the login".into()),
            },
            (WlanSecurity::Enterprise, _, None) => Err("the network needs a username and password (WPA2-Enterprise)".into()),
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
