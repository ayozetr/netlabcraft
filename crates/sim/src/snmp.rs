//! SNMP beyond get and set: SNMPv2-Trap notifications (RFC 3416 §4.2.6) agents send to their
//! trap hosts on link changes and restarts, the hosts that receive them, and SNMPv3 with the
//! user-based security model (RFC 3414): engine discovery, noAuthNoPriv and authNoPriv.

use std::net::Ipv4Addr;

use netlabcraft_core::DeviceId;
use netlabcraft_model::{Device, DeviceKind, Topology};
use netlabcraft_wire::sha1::Hash;
use netlabcraft_wire::snmp::{self, FLAG_AUTH, FLAG_REPORTABLE, PduKind, SNMP_TRAP_PORT, Snmp, V3, Value, localize_key, oids};

use crate::{Sim, SimError};

/// Most traps a host keeps, and most waiting to go out of an agent.
pub const MAX_TRAPS: usize = 200;
const MAX_TRAP_OUT: usize = 64;

/// A notification an agent has to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapEvent {
    /// The device (re)started.
    ColdStart,
    /// A port (by index) went up (`true`) or down.
    Link(usize, bool),
}

impl TrapEvent {
    /// The `snmp-server enable traps` keyword that turns it on.
    fn keyword(self) -> &'static str {
        match self {
            TrapEvent::ColdStart => "coldstart",
            TrapEvent::Link(_, true) => "linkup",
            TrapEvent::Link(_, false) => "linkdown",
        }
    }
}

/// A trap a host received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedTrap {
    /// Simulated µs.
    pub at_us: u64,
    pub from: Ipv4Addr,
    pub community: String,
    /// `linkDown`, `linkUp`, `coldStart`, or the trap's OID.
    pub name: String,
    /// The other variable bindings: (OID, value).
    pub bindings: Vec<(String, String)>,
}

/// An SNMPv3 manager's credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V3Login {
    pub user: String,
    /// The hash and password, for authNoPriv; `None` for noAuthNoPriv.
    pub auth: Option<(Hash, String)>,
}

/// The agent's SNMP engine ID (RFC 3411 §5, the MAC-address format).
pub fn engine_id(d: &Device) -> Vec<u8> {
    let mac = d.ports.first().map(|p| p.mac.0).unwrap_or_default();
    [&[0x80, 0, 0, 0, 3][..], &mac].concat()
}

fn hash_of(name: &str) -> Hash {
    if name.eq_ignore_ascii_case("md5") { Hash::Md5 } else { Hash::Sha1 }
}

impl Sim {
    /// Queues a notification; it goes out (if the agent sends that kind) when time next runs.
    pub(crate) fn queue_trap(&mut self, d: DeviceId, e: TrapEvent) {
        let st = self.st(d);
        if st.trap_out.len() < MAX_TRAP_OUT {
            st.trap_out.push(e);
        }
    }

    /// Sends the queued notifications to each agent's trap hosts.
    pub(crate) fn flush_traps(&mut self, t: &Topology) {
        let pending: Vec<(DeviceId, Vec<TrapEvent>)> =
            self.state.iter_mut().filter(|(_, s)| !s.trap_out.is_empty()).map(|(id, s)| (*id, std::mem::take(&mut s.trap_out))).collect();
        for (id, events) in pending {
            let Some(d) = t.device(id).cloned() else { continue };
            if !matches!(d.kind, DeviceKind::Router | DeviceKind::Switch) || d.snmp.trap_hosts.is_empty() {
                continue;
            }
            for e in events.into_iter().filter(|e| d.snmp.traps.iter().any(|x| x == e.keyword())) {
                let mut bindings = vec![(oids::SYS_UPTIME.to_vec(), Value::TimeTicks(u32::try_from(self.now / 10_000).unwrap_or(u32::MAX)))];
                match e {
                    TrapEvent::ColdStart => bindings.push((oids::SNMP_TRAP_OID.to_vec(), Value::Oid(oids::COLD_START.to_vec()))),
                    TrapEvent::Link(port, up) => {
                        let which = if up { oids::LINK_UP } else { oids::LINK_DOWN };
                        bindings.push((oids::SNMP_TRAP_OID.to_vec(), Value::Oid(which.to_vec())));
                        let n = u32::try_from(port + 1).unwrap_or(0);
                        let row = |col: &[u32]| [col, &[n]].concat();
                        let admin = d.ports.get(port).is_some_and(|p| !p.shutdown);
                        bindings.push((row(oids::IF_INDEX), Value::Integer(i64::from(n))));
                        bindings.push((row(oids::IF_ADMIN), Value::Integer(if admin { 1 } else { 2 })));
                        bindings.push((row(oids::IF_OPER), Value::Integer(if up { 1 } else { 2 })));
                        // ifDescr, so a person sees which port.
                        let name = d.ports.get(port).map(|p| p.name.clone()).unwrap_or_default();
                        bindings.push((vec![1, 3, 6, 1, 2, 1, 2, 2, 1, 2, n], Value::Text(name)));
                    }
                }
                for (host, community) in d.snmp.trap_hosts.clone() {
                    let Ok(src) = self.source_for(t, &d, host) else { continue };
                    self.seq += 1;
                    let request_id = i32::try_from(self.seq % 1_000_000).unwrap_or(0);
                    let trap = Snmp { community, kind: PduKind::Trap, request_id, error_status: 0, error_index: 0, bindings: bindings.clone() };
                    let port = self.next_port();
                    self.hold_for_arp = true;
                    let sent = self.udp_out(t, &d, src, host, port, SNMP_TRAP_PORT, trap.encode());
                    self.hold_for_arp = false;
                    if sent.is_ok() {
                        let st = self.st(id);
                        st.traps_sent = st.traps_sent.saturating_add(1);
                    }
                }
            }
        }
    }

    /// A trap arriving at a host.
    pub(crate) fn trap_in(&mut self, d: &Device, from: Ipv4Addr, payload: &[u8]) -> Result<(), String> {
        let m = Snmp::decode(payload).map_err(|e| e.to_string())?;
        if m.kind != PduKind::Trap {
            return Err(format!("an SNMP {} on the trap port", m.kind.name()));
        }
        let mut name = "trap".to_string();
        let mut bindings = Vec::new();
        for (oid, v) in &m.bindings {
            match (oid.as_slice(), v) {
                (oids::SNMP_TRAP_OID, Value::Oid(o)) => name = oids::trap_name(o).map_or_else(|| snmp::oid_text(o), str::to_string),
                _ => bindings.push((snmp::oid_text(oid), v.show())),
            }
        }
        let rec = ReceivedTrap { at_us: self.now, from, community: m.community, name, bindings };
        let st = self.st(d.id);
        if st.traps.len() >= MAX_TRAPS {
            st.traps.pop_front();
        }
        st.traps.push_back(rec);
        Ok(())
    }

    /// The traps a host received, oldest first.
    pub fn traps(&self, d: DeviceId) -> Vec<ReceivedTrap> {
        self.state.get(&d).map(|s| s.traps.iter().cloned().collect()).unwrap_or_default()
    }

    pub fn clear_traps(&mut self, d: DeviceId) {
        if let Some(s) = self.state.get_mut(&d) {
            s.traps.clear();
        }
    }

    /// Empties a syslog server's message list.
    pub fn clear_syslog(&mut self, d: DeviceId) {
        if let Some(s) = self.state.get_mut(&d) {
            s.syslog.clear();
        }
    }

    /// How many traps an agent sent since it started.
    pub fn traps_sent(&self, d: DeviceId) -> u32 {
        self.state.get(&d).map_or(0, |s| s.traps_sent)
    }

    fn snmp_key(&mut self, hash: Hash, password: &str, engine: &[u8]) -> Vec<u8> {
        let k = (u8::from(hash == Hash::Sha1), password.to_string(), engine.to_vec());
        if let Some(v) = self.snmp_keys.get(&k) {
            return v.clone();
        }
        let v = localize_key(hash, password, engine);
        if self.snmp_keys.len() >= 64 {
            self.snmp_keys.clear();
        }
        self.snmp_keys.insert(k, v.clone());
        v
    }

    /// An agent's answer to an SNMPv3 message: a Response, or a Report saying what was wrong.
    pub(crate) fn snmp_v3_answer(&mut self, t: &Topology, d: &Device, payload: &[u8]) -> Result<Vec<u8>, String> {
        let (m, _) = V3::decode(payload).map_err(|e| e.to_string())?;
        if !d.snmp.agent_on() {
            return Err("no SNMP agent here".into());
        }
        let mine = engine_id(d);
        let time = u32::try_from(self.now / 1_000_000).unwrap_or(u32::MAX);
        let base = V3 {
            msg_id: m.msg_id,
            flags: 0,
            engine_id: mine.clone(),
            boots: 1,
            time,
            user: m.user.clone(),
            auth: Vec::new(),
            context_name: m.context_name.clone(),
            pdu: m.pdu.clone(),
        };
        let report = |oid: &[u32]| {
            let pdu =
                Snmp { kind: PduKind::Report, bindings: vec![(oid.to_vec(), Value::Counter(1))], error_status: 0, error_index: 0, ..m.pdu.clone() };
            V3 { pdu, ..base.clone() }.encode()
        };
        if m.engine_id != mine {
            // Discovery (RFC 3414 §4): the manager learns the engine ID, boots and time.
            return Ok(report(oids::UNKNOWN_ENGINE_IDS));
        }
        let Some(user) = d.snmp.users.iter().find(|u| u.name == m.user) else { return Ok(report(oids::UNKNOWN_USER_NAMES)) };
        let group = d.snmp.groups.iter().find(|g| g.name == user.group);
        let authed = m.flags & FLAG_AUTH != 0;
        let mut key = None;
        if authed {
            let Some((h, pw)) = &user.auth else { return Ok(report(oids::UNSUPPORTED_SEC_LEVELS)) };
            let hash = hash_of(h);
            let k = self.snmp_key(hash, pw, &mine);
            if !V3::verify(payload, hash, &k) {
                return Ok(report(oids::WRONG_DIGESTS));
            }
            key = Some((hash, k));
        } else if user.auth.is_some() || group.is_some_and(|g| g.auth) {
            return Ok(report(oids::UNSUPPORTED_SEC_LEVELS));
        }
        let pdu = match group {
            Some(g) => self.snmp_answer(t, d, &m.pdu, g.write),
            // A user in no group may do nothing (authorizationError).
            None => Snmp { kind: PduKind::Response, error_status: 16, error_index: 0, ..m.pdu.clone() },
        };
        let answer = V3 { pdu, ..base };
        Ok(match key {
            Some((hash, k)) => answer.encode_signed(hash, &k),
            None => answer.encode(),
        })
    }

    /// An SNMPv3 request from `dev` to an agent: discovers its engine, then sends `pdu` as
    /// `login`. Returns the agent's Response PDU.
    pub fn snmp_request_v3(&mut self, t: &Topology, dev: DeviceId, agent: Ipv4Addr, pdu: Snmp, login: &V3Login) -> Result<Snmp, SimError> {
        let fail = |m: String| SimError::NoSource(m);
        let no_answer = || fail(format!("{agent} did not answer (no SNMPv3 agent there)"));
        self.seq += 1;
        let id = i32::try_from(self.seq % 1_000_000).unwrap_or(0) + 1;
        let probe = V3 {
            msg_id: id,
            flags: FLAG_REPORTABLE,
            engine_id: Vec::new(),
            boots: 0,
            time: 0,
            user: String::new(),
            auth: Vec::new(),
            context_name: String::new(),
            pdu: Snmp { community: String::new(), kind: PduKind::Get, request_id: id, error_status: 0, error_index: 0, bindings: Vec::new() },
        };
        let mine = move |want: i32| move |b: &[u8]| V3::decode(b).is_ok_and(|(m, _)| m.msg_id == want);
        let got = self.snmp_exchange(t, dev, agent, probe.encode(), &mine(id))?.ok_or_else(no_answer)?;
        let (disc, _) = V3::decode(&got).map_err(|e| fail(e.to_string()))?;
        let id2 = id.wrapping_add(1);
        let req = V3 {
            msg_id: id2,
            flags: FLAG_REPORTABLE,
            engine_id: disc.engine_id.clone(),
            boots: disc.boots,
            time: disc.time,
            user: login.user.clone(),
            auth: Vec::new(),
            context_name: String::new(),
            pdu,
        };
        let key = login.auth.as_ref().map(|(h, pw)| (*h, self.snmp_key(*h, pw, &disc.engine_id)));
        let bytes = match &key {
            Some((h, k)) => req.encode_signed(*h, k),
            None => req.encode(),
        };
        let got = self.snmp_exchange(t, dev, agent, bytes, &mine(id2))?.ok_or_else(no_answer)?;
        let (answer, _) = V3::decode(&got).map_err(|e| fail(e.to_string()))?;
        if answer.pdu.kind == PduKind::Report {
            let why = match answer.pdu.bindings.first().map(|(o, _)| o.as_slice()) {
                Some(oids::UNKNOWN_USER_NAMES) => "unknown user",
                Some(oids::WRONG_DIGESTS) => "authentication failed (wrong password or hash)",
                Some(oids::UNSUPPORTED_SEC_LEVELS) => "this security level isn't allowed for the user",
                Some(oids::UNKNOWN_ENGINE_IDS) => "unknown engine ID",
                _ => "the agent reported an error",
            };
            return Err(fail(format!("{agent}: {why}")));
        }
        if let Some((h, k)) = &key
            && !V3::verify(&got, *h, k)
        {
            return Err(fail(format!("{agent}: the answer failed authentication")));
        }
        Ok(answer.pdu)
    }
}
