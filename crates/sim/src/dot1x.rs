//! IEEE 802.1X port-based access control on the wired ports of a switch.
//!
//! A switch with `dot1x system-auth-control` and a port set to `authentication port-control
//! auto` lets nothing through that port but EAPOL until the host on it has logged in. The switch
//! is a pass-through authenticator (IEEE 802.1X-2020 §8): the host's supplicant (a PC or server
//! with a login on its port, `dot1x.supplicant`) answers the switch's EAP identity request, and
//! every EAP message the host sends goes on to the authentication server's RADIUS service inside
//! an Access-Request (RFC 3579), exactly as the controller does for a WPA2-Enterprise network;
//! the Server's side is [`crate::peap`], so the login is the same PEAP tunnel with its optional
//! certificate. The server's answer decides the port: an accept opens it, a reject keeps it shut.
//!
//! A host that never answers (no supplicant) leaves the port in the guest VLAN once the switch
//! has asked three times (`authentication event no-response action authorize vlan N`); a host
//! the server refuses can be given the auth-fail VLAN (`authentication event fail action
//! authorize vlan N`). A VLAN given this way is not the port's configured access VLAN, and
//! lasts as long as the state does (a link change or `dot1x re-authenticate` starts over).
//!
//! The wireless side of 802.1X lives in [`crate::wlan`]; the frames are the same
//! ([`netlabcraft_wire::wlan::Eapol`]), here carried in Ethernet frames to the PAE group address.

use netlabcraft_core::{DeviceId, MacAddr, VlanId};
use netlabcraft_model::knobs::Scope;
use netlabcraft_model::wlan::mgmt_addr;
use netlabcraft_model::{Device, DeviceKind, End, SwitchportMode, Topology};
use netlabcraft_wire::radius::{ACCESS_ACCEPT, ACCESS_CHALLENGE, RADIUS_PORT, Radius};
use netlabcraft_wire::wlan::{EAP_FAILURE, EAP_IDENTITY, EAP_PEAP, EAP_REQUEST, EAP_RESPONSE, EAP_SUCCESS, ETHERTYPE_EAPOL, Eap, Eapol, PEAP_START};
use netlabcraft_wire::{Frame, Udp};

use crate::{DebugKind, Sim, port_up};

/// The group address EAPOL frames from a supplicant go to (IEEE 802.1X-2020 §11.1.1).
pub const PAE_GROUP: MacAddr = MacAddr([0x01, 0x80, 0xc2, 0x00, 0x00, 0x03]);

/// How long, in simulated seconds, the switch waits between its identity requests unless
/// `dot1x timeout tx-period` says otherwise.
pub const DEFAULT_TX_PERIOD: u64 = 30;
/// Requests the switch sends a host before it decides nobody is there (for the guest VLAN).
const REQUESTS: u64 = 3;
/// Most logins a switch keeps waiting for a RADIUS answer.
const MAX_WAITS: usize = 64;

/// What `authentication port-control` says about a port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// The host has to log in.
    Auto,
    /// Always open, no login asked.
    ForceAuthorized,
    /// Always shut.
    ForceUnauthorized,
}

impl Control {
    pub fn name(self) -> &'static str {
        match self {
            Control::Auto => "auto",
            Control::ForceAuthorized => "force-authorized",
            Control::ForceUnauthorized => "force-unauthorized",
        }
    }
}

/// How a port came to be open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    /// `force-authorized`.
    Forced,
    /// The server accepted the host's login.
    Authenticated,
    /// Nobody answered, and the guest VLAN took the port.
    GuestVlan(VlanId),
    /// The server refused the host, and the auth-fail VLAN took the port.
    AuthFailVlan(VlanId),
}

/// Whether a controlled port carries traffic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Authorized(Via),
    /// Shut, and why.
    Unauthorized(&'static str),
}

/// Where a login has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Phase {
    /// Asked for an identity; nothing answered yet.
    #[default]
    Waiting,
    /// The host answered and its login is going back and forth with the server.
    Running,
    Success,
    Failed,
}

/// One controlled port of a switch.
#[derive(Debug, Clone, Default)]
pub struct PortAuth {
    pub(crate) since: u64,
    pub(crate) phase: Phase,
    pub(crate) peer: Option<End>,
    pub(crate) mac: Option<MacAddr>,
    pub(crate) identity: String,
    pub(crate) radius_state: Option<Vec<u8>>,
    pub(crate) rx: u32,
    pub(crate) tx: u32,
}

/// A host's supplicant on one port.
#[derive(Debug, Clone, Default)]
pub struct Supp {
    pub(crate) peer: Option<End>,
    pub(crate) tunnel: Option<crate::peap::ClientTunnel>,
    pub(crate) result: Option<bool>,
    pub(crate) rx: u32,
    pub(crate) tx: u32,
}

/// A login the switch asked a RADIUS server about, by the switch's source port.
#[derive(Debug, Clone)]
pub(crate) struct WiredWait {
    switch: DeviceId,
    port: usize,
    authenticator: [u8; 16],
    secret: String,
}

/// What a person sees of a controlled port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    pub control: Control,
    pub verdict: Verdict,
    pub phase: &'static str,
    pub identity: String,
    pub mac: Option<MacAddr>,
    pub rx: u32,
    pub tx: u32,
}

/// Whether the switch has 802.1X turned on (`dot1x system-auth-control`).
pub fn system_on(d: &Device) -> bool {
    d.kind == DeviceKind::Switch && d.knobs.get(Scope::Global, "dot1x system-auth-control").is_some()
}

/// What `authentication port-control` says about a switch port; `None` when 802.1X does not
/// control it (not set, a trunk, a routed port, or the switch has it off).
pub fn control(d: &Device, port: usize) -> Option<Control> {
    if !system_on(d) {
        return None;
    }
    let p = d.ports.get(port)?;
    if p.routed || !d.is_switchport(port) || p.switchport.mode == SwitchportMode::Trunk {
        return None;
    }
    let line = d.knobs.get(Scope::Iface(&p.name), "authentication port-control")?;
    match line.split_whitespace().last()? {
        "auto" => Some(Control::Auto),
        "force-authorized" => Some(Control::ForceAuthorized),
        "force-unauthorized" => Some(Control::ForceUnauthorized),
        _ => None,
    }
}

fn vlan_setting(d: &Device, port: usize, key: &str) -> Option<VlanId> {
    let p = d.ports.get(port)?;
    let n: u16 = d.knobs.get(Scope::Iface(&p.name), key)?.split_whitespace().last()?.parse().ok()?;
    VlanId::new(n)
}

/// The guest VLAN of a port (`authentication event no-response action authorize vlan N`).
pub fn guest_vlan(d: &Device, port: usize) -> Option<VlanId> {
    vlan_setting(d, port, "authentication event no-response action authorize vlan")
}

/// The auth-fail VLAN of a port (`authentication event fail action authorize vlan N`).
pub fn auth_fail_vlan(d: &Device, port: usize) -> Option<VlanId> {
    vlan_setting(d, port, "authentication event fail action authorize vlan")
}

/// Seconds between the switch's identity requests on a port.
pub fn tx_period(d: &Device, port: usize) -> u64 {
    d.ports
        .get(port)
        .and_then(|p| d.knobs.get(Scope::Iface(&p.name), "dot1x timeout tx-period"))
        .and_then(|l| l.split_whitespace().last())
        .and_then(|w| w.parse::<u64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_TX_PERIOD)
}

/// Whether the switch can ask a RADIUS server about a login: `aaa new-model`, a RADIUS server,
/// and `aaa authentication dot1x default group radius`.
pub fn method_ready(d: &Device) -> bool {
    d.access.aaa
        && !d.access.radius_servers.is_empty()
        && d.knobs.get(Scope::Global, "aaa authentication dot1x default").is_some_and(|l| l.contains("radius"))
}

fn eapol_frame(src: MacAddr, dst: MacAddr, e: &Eapol) -> Frame {
    Frame { dst, src, vlan: None, ethertype: ETHERTYPE_EAPOL, payload: e.encode() }
}

impl Sim {
    fn dot1x_verdict(&self, d: &Device, port: usize) -> Option<(Control, Verdict)> {
        let ctl = control(d, port)?;
        let a = self.state.get(&d.id).and_then(|s| s.dot1x.get(&port));
        let v = match ctl {
            Control::ForceAuthorized => Verdict::Authorized(Via::Forced),
            Control::ForceUnauthorized => Verdict::Unauthorized("port-control is force-unauthorized"),
            Control::Auto => match a {
                None => Verdict::Unauthorized("the login has not started"),
                Some(a) => match a.phase {
                    Phase::Success => Verdict::Authorized(Via::Authenticated),
                    Phase::Failed => match auth_fail_vlan(d, port) {
                        Some(v) => Verdict::Authorized(Via::AuthFailVlan(v)),
                        None => Verdict::Unauthorized("the server refused the login"),
                    },
                    Phase::Running => Verdict::Unauthorized("the login is under way"),
                    Phase::Waiting => {
                        let wait = tx_period(d, port).saturating_mul(REQUESTS).saturating_mul(1_000_000);
                        if self.now.saturating_sub(a.since) >= wait {
                            match guest_vlan(d, port) {
                                Some(v) => Verdict::Authorized(Via::GuestVlan(v)),
                                None => Verdict::Unauthorized("no supplicant answered"),
                            }
                        } else {
                            Verdict::Unauthorized("waiting for the host to answer")
                        }
                    }
                },
            },
        };
        Some((ctl, v))
    }

    /// What the console shows of a controlled port.
    pub fn dot1x_port(&self, d: &Device, port: usize) -> Option<PortInfo> {
        let (control, verdict) = self.dot1x_verdict(d, port)?;
        let a = self.state.get(&d.id).and_then(|s| s.dot1x.get(&port));
        Some(PortInfo {
            control,
            verdict,
            phase: match a.map(|a| a.phase) {
                None => "not started",
                Some(Phase::Waiting) => "waiting for the host",
                Some(Phase::Running) => "authenticating",
                Some(Phase::Success) => "authenticated",
                Some(Phase::Failed) => "failed",
            },
            identity: a.map(|a| a.identity.clone()).unwrap_or_default(),
            mac: a.and_then(|a| a.mac),
            rx: a.map_or(0, |a| a.rx),
            tx: a.map_or(0, |a| a.tx),
        })
    }

    /// The VLAN 802.1X gave a port in place of its access VLAN (guest or auth-fail), if it did.
    pub fn dot1x_vlan(&self, d: &Device, port: usize) -> Option<VlanId> {
        match self.dot1x_verdict(d, port)?.1 {
            Verdict::Authorized(Via::GuestVlan(v) | Via::AuthFailVlan(v)) => Some(v),
            _ => None,
        }
    }

    /// Whether `vlan` is the one 802.1X gave the port; `None` when it gave it none.
    pub(crate) fn dot1x_carries(&self, d: &Device, port: usize, vlan: VlanId) -> Option<bool> {
        self.dot1x_vlan(d, port).map(|v| v == vlan)
    }

    /// Why a frame arriving on a controlled, unauthorized port is dropped; `None` when the port
    /// is open (or not controlled).
    pub(crate) fn dot1x_ingress(&self, d: &Device, port: usize) -> Option<String> {
        if !system_on(d) {
            return None;
        }
        match self.dot1x_verdict(d, port) {
            Some((_, Verdict::Unauthorized(why))) => Some(format!("802.1X: the port is not authorized ({why}); only EAPOL is accepted")),
            _ => None,
        }
    }

    /// Whether a frame leaving `from` is stopped because the port is controlled and not yet
    /// authorized (EAPOL always gets out).
    pub(crate) fn dot1x_egress_blocked(&self, t: &Topology, from: End, frame: &[u8]) -> bool {
        let Some(d) = t.device(from.device) else { return false };
        if !system_on(d) || !matches!(self.dot1x_verdict(d, from.port), Some((_, Verdict::Unauthorized(_)))) {
            return false;
        }
        !Frame::decode(frame).is_ok_and(|f| f.ethertype == ETHERTYPE_EAPOL)
    }

    /// Forgets every login and supplicant state: each port starts over the next time the clock
    /// runs.
    pub fn forget_dot1x(&mut self) {
        for st in self.state.values_mut() {
            st.dot1x.clear();
            st.supp.clear();
        }
        self.dot1x_wired.clear();
    }

    /// `dot1x re-authenticate`: the port's login starts again. Whether the port was controlled.
    pub fn dot1x_reauthenticate(&mut self, d: DeviceId, port: usize) -> bool {
        let peer = self.state.get_mut(&d).and_then(|s| s.dot1x.remove(&port)).and_then(|a| a.peer);
        if let Some(h) = peer {
            if let Some(s) = self.state.get_mut(&h.device) {
                s.supp.remove(&h.port);
            }
            true
        } else {
            false
        }
    }

    /// A host's supplicant on a port: whether its login worked (`None`: not decided yet) and the
    /// EAPOL frames it received and sent.
    pub fn dot1x_supplicant(&self, d: DeviceId, port: usize) -> Option<(Option<bool>, u32, u32)> {
        self.state.get(&d).and_then(|s| s.supp.get(&port)).map(|s| (s.result, s.rx, s.tx))
    }

    /// Drops what no longer holds: a port that is not controlled, down, or cabled to something
    /// else, and a supplicant whose port went the same way.
    pub(crate) fn dot1x_sync(&mut self, t: &Topology) {
        for (id, st) in &mut self.state {
            let Some(d) = t.device(*id) else { continue };
            st.dot1x.retain(|port, a| {
                let end = End { device: *id, port: *port };
                control(d, *port) == Some(Control::Auto) && port_up(t, end) && a.peer == t.peer(end)
            });
            st.supp.retain(|port, s| {
                let end = End { device: *id, port: *port };
                d.ports.get(*port).is_some_and(|p| p.supplicant.is_some()) && port_up(t, end) && s.peer == t.peer(end)
            });
        }
        self.dot1x_wired.retain(|_, w| t.device(w.switch).is_some_and(|d| control(d, w.port) == Some(Control::Auto)));
    }

    /// Starts the login on every controlled port that has a cable and has not started one: the
    /// host's supplicant says it is there (EAPOL-Start), or the switch asks for an identity.
    pub(crate) fn dot1x_kick(&mut self, t: &Topology) {
        let now = self.now;
        for d in t.devices.iter().filter(|d| system_on(d)) {
            for port in 0..d.ports.len() {
                if control(d, port) != Some(Control::Auto) {
                    continue;
                }
                let end = End { device: d.id, port };
                let Some(peer) = t.peer(end) else { continue };
                if !port_up(t, end) {
                    continue;
                }
                if self.state.get(&d.id).and_then(|s| s.dot1x.get(&port)).is_some_and(|a| a.peer == Some(peer)) {
                    continue;
                }
                self.st(d.id).dot1x.insert(port, PortAuth { since: now, peer: Some(peer), ..PortAuth::default() });
                let name = d.ports.get(port).map(|p| p.name.clone()).unwrap_or_default();
                let host = t.device(peer.device).filter(|h| h.kind.is_host() && h.ports.get(peer.port).is_some_and(|p| p.supplicant.is_some()));
                match host {
                    Some(h) => {
                        self.debug_log(d.id, DebugKind::Dot1x, &format!("{name}: link up, waiting for the EAPOL-Start of {}", h.name));
                        self.supplicant_start(t, h, peer.port);
                    }
                    None => {
                        self.debug_log(d.id, DebugKind::Dot1x, &format!("{name}: link up, asking the host for its identity"));
                        self.dot1x_identity_request(t, d, port);
                    }
                }
            }
        }
    }

    fn dot1x_identity_request(&mut self, t: &Topology, d: &Device, port: usize) {
        self.seq += 1;
        let id = (self.seq % 255) as u8 + 1;
        self.dot1x_to_supplicant(t, d, port, Eapol::Eap(Eap { code: EAP_REQUEST, id, kind: Some(EAP_IDENTITY), data: Vec::new() }));
    }

    /// An EAPOL frame from the switch out of a port, to the supplicant once it has been heard
    /// (else to the group address).
    fn dot1x_to_supplicant(&mut self, t: &Topology, d: &Device, port: usize, e: Eapol) {
        let Some(p) = d.ports.get(port) else { return };
        let dst = self.state.get(&d.id).and_then(|s| s.dot1x.get(&port)).and_then(|a| a.mac).unwrap_or(PAE_GROUP);
        if let Some(a) = self.st(d.id).dot1x.get_mut(&port) {
            a.tx = a.tx.saturating_add(1);
        }
        self.emit_frame(t, d, port, &eapol_frame(p.mac, dst, &e));
    }

    /// The host's side of "I am here": an EAPOL-Start out of the port.
    fn supplicant_start(&mut self, t: &Topology, h: &Device, port: usize) {
        let peer = t.peer(End { device: h.id, port });
        let Some(p) = h.ports.get(port) else { return };
        let s = self.st(h.id).supp.entry(port).or_default();
        *s = Supp { peer, tx: 1, ..Supp::default() };
        self.emit_frame(t, h, port, &eapol_frame(p.mac, PAE_GROUP, &Eapol::Start));
    }

    fn supplicant_send(&mut self, t: &Topology, h: &Device, port: usize, to: MacAddr, e: Eap) {
        let Some(p) = h.ports.get(port) else { return };
        if let Some(s) = self.st(h.id).supp.get_mut(&port) {
            s.tx = s.tx.saturating_add(1);
        }
        self.emit_frame(t, h, port, &eapol_frame(p.mac, to, &Eapol::Eap(e)));
    }

    /// An EAPOL frame at a switch port: the start of a login, a logoff, or an EAP response to
    /// pass on to the authentication server.
    pub(crate) fn dot1x_switch_in(&mut self, t: &Topology, d: &Device, port: usize, f: &Frame) -> Result<(), String> {
        let Some(ctl) = control(d, port) else {
            return Err("802.1X is not controlling this port: an EAPOL frame is a link-local frame and stops at the first bridge".into());
        };
        let e = Eapol::decode(&f.payload).map_err(|e| e.to_string())?;
        let name = d.ports.get(port).map(|p| p.name.clone()).unwrap_or_default();
        if ctl != Control::Auto {
            self.debug_log(d.id, DebugKind::Dot1x, &format!("{name}: EAPOL ignored, port-control is {}", ctl.name()));
            return Err(format!("802.1X: port-control is {}; the port does not ask for a login", ctl.name()));
        }
        let end = End { device: d.id, port };
        // A frame that arrives before the switch's own kick has started the port.
        if self.state.get(&d.id).is_none_or(|s| !s.dot1x.contains_key(&port)) {
            let since = self.now;
            self.st(d.id).dot1x.insert(port, PortAuth { since, peer: t.peer(end), ..PortAuth::default() });
        }
        if let Some(a) = self.st(d.id).dot1x.get_mut(&port) {
            a.rx = a.rx.saturating_add(1);
        }
        match e {
            Eapol::Start => {
                self.debug_log(d.id, DebugKind::Dot1x, &format!("{name}: EAPOL-Start from {}, asking for its identity", f.src));
                if let Some(a) = self.st(d.id).dot1x.get_mut(&port) {
                    a.phase = Phase::Waiting;
                    a.mac = Some(f.src);
                    a.radius_state = None;
                }
                self.dot1x_identity_request(t, d, port);
                Ok(())
            }
            Eapol::Logoff => {
                self.debug_log(d.id, DebugKind::Dot1x, &format!("{name}: EAPOL-Logoff from {}: the port closes", f.src));
                let now = self.now;
                if let Some(a) = self.st(d.id).dot1x.get_mut(&port) {
                    a.phase = Phase::Waiting;
                    a.since = now;
                    a.radius_state = None;
                }
                Ok(())
            }
            Eapol::Eap(r) if r.code == EAP_RESPONSE => self.dot1x_relay(t, d, port, f.src, &r),
            _ => Err("an EAPOL frame the authenticator has no use for".into()),
        }
    }

    /// A pass-through authenticator: the host's EAP response goes to the RADIUS server inside
    /// an Access-Request, with the State the server gave last time.
    fn dot1x_relay(&mut self, t: &Topology, d: &Device, port: usize, from: MacAddr, r: &Eap) -> Result<(), String> {
        let name = d.ports.get(port).map(|p| p.name.clone()).unwrap_or_default();
        if let Some(a) = self.st(d.id).dot1x.get_mut(&port) {
            a.mac = Some(from);
            if r.kind == Some(EAP_IDENTITY) {
                a.identity = String::from_utf8_lossy(&r.data).chars().take(64).collect();
                a.radius_state = None;
            }
            a.phase = Phase::Running;
        }
        let (user, state) = self
            .state
            .get(&d.id)
            .and_then(|s| s.dot1x.get(&port))
            .map(|a| (a.identity.clone(), a.radius_state.clone()))
            .unwrap_or_default();
        self.debug_log(d.id, DebugKind::Dot1x, &format!("{name}: EAP response from {from} (identity `{user}`) goes to the authentication server"));
        let refuse = |me: &mut Sim, why: &str| {
            me.debug_log(d.id, DebugKind::Dot1x, &format!("{name}: {why}: the login of `{user}` is refused"));
            me.dot1x_finish(t, d, port, r.id, false);
        };
        if !method_ready(d) {
            refuse(self, "no authentication method is configured (aaa new-model, a radius-server host and aaa authentication dot1x default group radius)");
            return Ok(());
        }
        let Some((server, secret)) = d.access.radius_servers.first().cloned() else {
            refuse(self, "no RADIUS server");
            return Ok(());
        };
        let Some((_, src)) = mgmt_addr(d) else {
            refuse(self, "the switch has no address (an interface vlan with an IP address) to ask the server from");
            return Ok(());
        };
        let sport = self.next_port();
        self.seq += 1;
        let id = (self.seq % 256) as u8;
        let authenticator = netlabcraft_wire::md5::md5(format!("{}:{}:{user}:{port}", self.now, self.seq).as_bytes());
        let req = Radius::eap_request(id, authenticator, &user, &r.encode(), state.as_deref(), &secret, src.addr);
        if self.dot1x_wired.len() >= MAX_WAITS {
            self.dot1x_wired.clear();
        }
        self.dot1x_wired.insert(sport, WiredWait { switch: d.id, port, authenticator, secret });
        self.udp_out(t, d, src.addr, server, sport, RADIUS_PORT, req.encode())
    }

    /// The server's answer to a login: the next EAP request for the host, or the verdict.
    pub(crate) fn dot1x_radius_answer(&mut self, t: &Topology, d: &Device, u: &Udp) -> Result<(), String> {
        let w = self.dot1x_wired.remove(&u.dst_port).ok_or("no 802.1X login waiting")?;
        if w.switch != d.id {
            return Err("RADIUS answer for another switch".into());
        }
        let r = Radius::decode(&u.payload).map_err(|e| e.to_string())?;
        if !r.verifies(&w.authenticator, &w.secret) {
            return Err("RADIUS answer signed with another secret".into());
        }
        if !r.checks_message(&w.authenticator, &w.secret) {
            return Err("RADIUS answer without a valid Message-Authenticator: discarded".into());
        }
        let eap = r.eap_message().and_then(|m| Eap::decode(&m).ok());
        let name = d.ports.get(w.port).map(|p| p.name.clone()).unwrap_or_default();
        match r.code {
            ACCESS_CHALLENGE => {
                let eap = eap.ok_or("an Access-Challenge without EAP")?;
                if let Some(a) = self.st(d.id).dot1x.get_mut(&w.port) {
                    a.radius_state = r.state().map(<[u8]>::to_vec);
                }
                self.debug_log(d.id, DebugKind::Dot1x, &format!("{name}: the server asks for more, the EAP request goes on to the host"));
                self.dot1x_to_supplicant(t, d, w.port, Eapol::Eap(eap));
                Ok(())
            }
            ACCESS_ACCEPT => {
                self.dot1x_finish(t, d, w.port, eap.map_or(1, |e| e.id), true);
                Ok(())
            }
            _ => {
                self.dot1x_finish(t, d, w.port, eap.map_or(1, |e| e.id), false);
                Ok(())
            }
        }
    }

    /// The verdict on a login: EAP Success or Failure to the host, and the port's new state.
    fn dot1x_finish(&mut self, t: &Topology, d: &Device, port: usize, id: u8, ok: bool) {
        let name = d.ports.get(port).map(|p| p.name.clone()).unwrap_or_default();
        let who = self.state.get(&d.id).and_then(|s| s.dot1x.get(&port)).map(|a| a.identity.clone()).unwrap_or_default();
        if let Some(a) = self.st(d.id).dot1x.get_mut(&port) {
            a.phase = if ok { Phase::Success } else { Phase::Failed };
            a.radius_state = None;
        }
        let verdict = if ok { "accepted: the port is authorized" } else { "refused: the port stays shut" };
        self.debug_log(d.id, DebugKind::Dot1x, &format!("{name}: the server's answer for `{who}` is {verdict}"));
        let code = if ok { EAP_SUCCESS } else { EAP_FAILURE };
        self.dot1x_to_supplicant(t, d, port, Eapol::Eap(Eap { code, id, kind: None, data: Vec::new() }));
        if ok {
            self.log(d.id, &format!("{name}: 802.1X login of {who} accepted"));
        } else {
            self.log(d.id, &format!("{name}: 802.1X login of {who} refused"));
        }
    }

    /// EAPOL at a host: the supplicant answers the switch, or learns how the login ended.
    pub(crate) fn dot1x_supplicant_in(&mut self, t: &Topology, d: &Device, port: usize, f: &Frame) -> Result<(), String> {
        let Some(login) = d.ports.get(port).and_then(|p| p.supplicant.clone()) else {
            return Err("no 802.1X login is set on this port: EAPOL ignored".into());
        };
        let e = Eapol::decode(&f.payload).map_err(|e| e.to_string())?;
        let peer = t.peer(End { device: d.id, port });
        {
            let s = self.st(d.id).supp.entry(port).or_default();
            s.peer = peer;
            s.rx = s.rx.saturating_add(1);
        }
        match e {
            Eapol::Eap(q) if q.code == EAP_REQUEST && q.kind == Some(EAP_IDENTITY) => {
                if let Some(s) = self.st(d.id).supp.get_mut(&port) {
                    s.tunnel = None;
                    s.result = None;
                }
                self.debug_log(d.id, DebugKind::Dot1x, &format!("the switch asks for an identity: answering `{}`", login.user));
                self.supplicant_send(t, d, port, f.src, Eap { code: EAP_RESPONSE, id: q.id, kind: Some(EAP_IDENTITY), data: login.user.clone().into_bytes() });
                Ok(())
            }
            Eapol::Eap(q) if q.code == EAP_REQUEST && q.kind == Some(EAP_PEAP) => {
                let start = q.data.first().is_some_and(|b| b & PEAP_START != 0);
                let seed = format!("peap-wired{}{}{}{port}", self.now, self.seq, d.id.0);
                let trust = crate::pki::login_trust(&login, self.clock(d.id).0);
                let s = self.st(d.id).supp.entry(port).or_default();
                if start || s.tunnel.is_none() {
                    s.tunnel = Some(crate::peap::ClientTunnel::new(seed.as_bytes(), trust));
                }
                let tunnel = s.tunnel.as_mut().ok_or("no tunnel")?;
                let before = tunnel.report.clone();
                let answered = tunnel.answer(&q.data, &login.user, &login.password);
                let report = tunnel.report.clone();
                if report != before && (report.presented || !report.error.is_empty()) {
                    let seen = if report.presented {
                        format!("the server showed the certificate of {} (issued by {})", report.subject, report.issuer)
                    } else {
                        "the server showed no certificate".to_string()
                    };
                    let checked = if !report.error.is_empty() {
                        format!("the tunnel is refused: {}", report.error)
                    } else if report.verified {
                        "it chains to a CA this host trusts".to_string()
                    } else {
                        "it was not checked".to_string()
                    };
                    self.debug_log(d.id, DebugKind::Dot1x, &format!("{seen}; {checked}"));
                }
                match answered {
                    Ok(data) => {
                        self.supplicant_send(t, d, port, f.src, Eap { code: EAP_RESPONSE, id: q.id, kind: Some(EAP_PEAP), data });
                        Ok(())
                    }
                    Err(why) => {
                        if let Some(s) = self.st(d.id).supp.get_mut(&port) {
                            s.result = Some(false);
                        }
                        Err(format!("the login stops here: {why}"))
                    }
                }
            }
            Eapol::Eap(q) if q.code == EAP_SUCCESS || q.code == EAP_FAILURE => {
                let ok = q.code == EAP_SUCCESS;
                if let Some(s) = self.st(d.id).supp.get_mut(&port) {
                    s.result = Some(ok);
                    s.tunnel = None;
                }
                self.debug_log(d.id, DebugKind::Dot1x, if ok { "the switch accepted the login" } else { "the switch refused the login" });
                Ok(())
            }
            _ => Err("an EAPOL frame this supplicant has no use for".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_of_the_controls_are_the_words_of_the_command() {
        assert_eq!(Control::Auto.name(), "auto");
        assert_eq!(Control::ForceAuthorized.name(), "force-authorized");
        assert_eq!(Control::ForceUnauthorized.name(), "force-unauthorized");
    }

    #[test]
    fn the_pae_group_is_a_link_local_address_no_bridge_forwards() {
        assert!(netlabcraft_wire::control::link_local(PAE_GROUP));
    }
}
