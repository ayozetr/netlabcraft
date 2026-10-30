//! Network-management services on the wire: NTP clock sync, syslog to a server, TFTP copies
//! of configurations, and RADIUS logins.

use std::net::Ipv4Addr;

use packetcraft_core::DeviceId;
use packetcraft_model::{Device, DeviceKind, Topology};
use packetcraft_wire::mgmt::{NTP_PORT, Ntp, SYSLOG_PORT, TFTP_BLOCK, TFTP_PORT, Tftp, syslog_decode, syslog_encode};
use packetcraft_wire::radius::{RADIUS_PORT, Radius, reveal_password};
use packetcraft_wire::snmp::{self, PduKind, SNMP_PORT, Snmp};
use packetcraft_wire::{Ipv4, PROTO_UDP, Udp};

use crate::{L3If, Sim, SimError, default_ttl};

/// Most messages a syslog server keeps.
pub const MAX_SYSLOG: usize = 500;
const WAIT_US: u64 = 2_000_000;

impl Sim {
    fn udp_out(&mut self, t: &Topology, d: &Device, src: Ipv4Addr, dst: Ipv4Addr, sport: u16, dport: u16, payload: Vec<u8>) -> Result<(), String> {
        let u = Udp { src_port: sport, dst_port: dport, payload };
        let ip = Ipv4 { id: self.ip_id(), ttl: default_ttl(d.kind), protocol: PROTO_UDP, src, dst, payload: u.encode(src, dst) };
        self.send_ip(t, d, ip)
    }

    /// UDP for the management services; `None` when the port isn't one of theirs.
    pub(crate) fn mgmt_udp_in(&mut self, t: &Topology, d: &Device, ifc: &L3If, ip: &Ipv4, u: &Udp) -> Option<Result<(), String>> {
        let me = ip.dst;
        Some(match (u.dst_port, u.src_port) {
            (NTP_PORT, _) | (_, NTP_PORT) => {
                let n = match Ntp::decode(&u.payload) {
                    Ok(n) => n,
                    Err(e) => return Some(Err(e.to_string())),
                };
                if !n.server {
                    if d.kind != DeviceKind::Server || !d.services.ntp.on {
                        return Some(Err("no NTP service here".into()));
                    }
                    let (now, _) = self.clock(d.id);
                    let reply = Ntp { server: true, stratum: 1, time: now };
                    self.udp_out(t, d, me, ip.src, NTP_PORT, u.src_port, reply.encode())
                } else if d.ntp_servers.contains(&ip.src) {
                    let before = self.clock(d.id).0;
                    self.set_clock(d.id, n.time);
                    if (before - n.time).abs() > 1 {
                        self.log(d.id, &format!("Clock set from NTP server {}", ip.src));
                    }
                    self.st(d.id).ntp_synced = Some(ip.src);
                    Ok(())
                } else {
                    Err(format!("NTP time from {}, which isn't a configured server", ip.src))
                }
            }
            (SYSLOG_PORT, _) => {
                if d.kind != DeviceKind::Server || !d.services.syslog.on {
                    return Some(Err("no syslog service here".into()));
                }
                let (host, msg) = syslog_decode(&u.payload).unwrap_or_default();
                let now = self.now;
                let st = self.st(d.id);
                if st.syslog.len() >= MAX_SYSLOG {
                    st.syslog.pop_front();
                }
                st.syslog.push_back((now, ip.src, if host.is_empty() { msg } else { format!("{host}: {msg}") }));
                Ok(())
            }
            (TFTP_PORT, _) if d.kind == DeviceKind::Server => self.tftp_server_in(t, d, ip, u),
            (RADIUS_PORT, _) => {
                if d.kind != DeviceKind::Server || !d.services.aaa.on {
                    return Some(Err("no AAA service here".into()));
                }
                let req = match Radius::decode(&u.payload) {
                    Ok(r) => r,
                    Err(e) => return Some(Err(e.to_string())),
                };
                // Unknown clients are ignored, as RFC 2865 says.
                let Some(client) = d.services.aaa.clients.iter().find(|c| c.address == ip.src) else {
                    return Some(Err(format!("{} isn't one of this AAA server's clients", ip.src)));
                };
                let user = req.user().unwrap_or_default();
                let password = req.hidden_password().map(|h| reveal_password(h, &client.secret, &req.authenticator)).unwrap_or_default();
                let good = d.services.aaa.users.iter().any(|x| x.name == user && x.password == password);
                let answer = Radius::answer(&req, good, if good { "Welcome" } else { "Login refused" }, &client.secret);
                self.udp_out(t, d, me, ip.src, RADIUS_PORT, u.src_port, answer.encode())
            }
            (SNMP_PORT, _) if matches!(d.kind, DeviceKind::Router | DeviceKind::Switch) => {
                let Ok(req) = Snmp::decode(&u.payload) else { return Some(Err("not an SNMP message".into())) };
                // A wrong community gets no answer at all, as on real agents.
                let Some((_, rw)) = d.snmp.communities.iter().find(|(c, _)| *c == req.community) else {
                    return Some(Err(format!("SNMP community `{}` not known here", req.community)));
                };
                let answer = self.snmp_answer(t, d, &req, *rw);
                self.udp_out(t, d, me, ip.src, SNMP_PORT, u.src_port, answer.encode())
            }
            (port, SNMP_PORT) if self.state.get(&d.id).is_some_and(|s| s.snmp_port == Some(port)) => {
                if let Ok(r) = Snmp::decode(&u.payload) {
                    let st = self.st(d.id);
                    if st.snmp_inbox.len() < 16 {
                        st.snmp_inbox.push(r);
                    }
                }
                Ok(())
            }
            (port, RADIUS_PORT) if self.state.get(&d.id).is_some_and(|s| s.radius_port == Some(port)) => {
                if let Ok(r) = Radius::decode(&u.payload) {
                    let st = self.st(d.id);
                    if st.radius_inbox.len() < 16 {
                        st.radius_inbox.push(r);
                    }
                }
                Ok(())
            }
            (port, TFTP_PORT) if self.state.get(&d.id).is_some_and(|s| s.tftp_port == Some(port)) => {
                if let Ok(p) = Tftp::decode(&u.payload) {
                    let st = self.st(d.id);
                    if st.tftp_inbox.len() < 64 {
                        st.tftp_inbox.push(p);
                    }
                }
                Ok(())
            }
            _ => {
                let _ = ifc;
                return None;
            }
        })
    }

    fn tftp_server_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, u: &Udp) -> Result<(), String> {
        let msg = Tftp::decode(&u.payload).map_err(|e| e.to_string())?;
        let reply = |s: &mut Sim, p: Tftp| s.udp_out(t, d, ip.dst, ip.src, TFTP_PORT, u.src_port, p.encode());
        if !d.services.tftp.on {
            return reply(self, Tftp::Error(2, "TFTP service is off".into()));
        }
        let key = (ip.src, u.src_port);
        match msg {
            Tftp::Write(_) => reply(self, Tftp::Ack(0)),
            Tftp::Data(b, _) => reply(self, Tftp::Ack(b)),
            Tftp::Read(name) => match d.services.tftp.files.iter().find(|f| f.name == name) {
                Some(f) => {
                    let data = f.content.clone().into_bytes();
                    let first = data.get(..TFTP_BLOCK.min(data.len())).unwrap_or_default().to_vec();
                    self.st(d.id).tftp_tx.insert(key, data);
                    reply(self, Tftp::Data(1, first))
                }
                None => reply(self, Tftp::Error(1, format!("{name}: no such file"))),
            },
            Tftp::Ack(b) => {
                let Some(data) = self.state.get(&d.id).and_then(|s| s.tftp_tx.get(&key)).cloned() else { return Ok(()) };
                let start = usize::from(b) * TFTP_BLOCK;
                // The last block was shorter than a full one: the transfer is over.
                if start > data.len() {
                    self.st(d.id).tftp_tx.remove(&key);
                    return Ok(());
                }
                let block = data.get(start..(start + TFTP_BLOCK).min(data.len())).unwrap_or_default().to_vec();
                reply(self, Tftp::Data(b.wrapping_add(1), block))
            }
            Tftp::Error(..) => {
                self.st(d.id).tftp_tx.remove(&key);
                Ok(())
            }
        }
    }

    /// Waits for a TFTP packet the client is expecting.
    fn tftp_wait(&mut self, t: &Topology, dev: DeviceId, want: impl Fn(&Tftp) -> bool) -> Option<Tftp> {
        let start = self.now;
        self.advance(t, start.saturating_add(WAIT_US), |s| {
            s.state.get(&dev).is_some_and(|st| st.tftp_inbox.iter().any(|p| want(p) || matches!(p, Tftp::Error(..))))
        });
        let st = self.st(dev);
        let i = st.tftp_inbox.iter().position(|p| want(p) || matches!(p, Tftp::Error(..)))?;
        Some(st.tftp_inbox.remove(i))
    }

    fn tftp_start(&mut self, t: &Topology, dev: DeviceId, server: Ipv4Addr) -> Result<(Device, Ipv4Addr, u16), SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?.clone();
        let src = self.source_for(t, &d, server)?;
        let port = self.next_port();
        let st = self.st(dev);
        st.tftp_port = Some(port);
        st.tftp_inbox.clear();
        Ok((d, src, port))
    }

    /// `copy … tftp:`: writes a file to a TFTP server, block by block.
    pub fn tftp_put(&mut self, t: &Topology, dev: DeviceId, server: Ipv4Addr, name: &str, data: &[u8]) -> Result<(), SimError> {
        let (d, src, port) = self.tftp_start(t, dev, server)?;
        let fail = |m: String| SimError::NoSource(m);
        self.udp_out(t, &d, src, server, port, TFTP_PORT, Tftp::Write(name.to_string()).encode()).map_err(fail)?;
        // Block n carries bytes (n-1)*512 onwards; a block shorter than 512 (possibly empty) ends it.
        let mut block_no = 0u16;
        loop {
            match self.tftp_wait(t, dev, |p| matches!(p, Tftp::Ack(b) if *b == block_no)) {
                Some(Tftp::Error(_, m)) => return Err(fail(format!("the server refused: {m}"))),
                Some(_) => {}
                None => return Err(fail(format!("no answer from {server} (is its TFTP service on?)"))),
            }
            let done_before = usize::from(block_no) * TFTP_BLOCK;
            if block_no > 0 && done_before > data.len() {
                break;
            }
            let block = data.get(done_before.min(data.len())..(done_before + TFTP_BLOCK).min(data.len())).unwrap_or_default().to_vec();
            let last = block.len() < TFTP_BLOCK;
            block_no = block_no.wrapping_add(1);
            self.udp_out(t, &d, src, server, port, TFTP_PORT, Tftp::Data(block_no, block).encode()).map_err(fail)?;
            if last {
                match self.tftp_wait(t, dev, |p| matches!(p, Tftp::Ack(b) if *b == block_no)) {
                    Some(Tftp::Ack(_)) => break,
                    Some(Tftp::Error(_, m)) => return Err(fail(format!("the server refused: {m}"))),
                    _ => return Err(fail(format!("{server} stopped answering"))),
                }
            }
        }
        self.st(dev).tftp_port = None;
        Ok(())
    }

    /// `copy tftp: …`: reads a file from a TFTP server.
    pub fn tftp_get(&mut self, t: &Topology, dev: DeviceId, server: Ipv4Addr, name: &str) -> Result<Vec<u8>, SimError> {
        let (d, src, port) = self.tftp_start(t, dev, server)?;
        let fail = |m: String| SimError::NoSource(m);
        self.udp_out(t, &d, src, server, port, TFTP_PORT, Tftp::Read(name.to_string()).encode()).map_err(fail)?;
        let mut out = Vec::new();
        let mut expect = 1u16;
        loop {
            match self.tftp_wait(t, dev, |p| matches!(p, Tftp::Data(b, _) if *b == expect)) {
                Some(Tftp::Data(b, chunk)) => {
                    let last = chunk.len() < TFTP_BLOCK;
                    out.extend(chunk);
                    self.udp_out(t, &d, src, server, port, TFTP_PORT, Tftp::Ack(b).encode()).map_err(fail)?;
                    if last || out.len() > 1_000_000 {
                        break;
                    }
                    expect = expect.wrapping_add(1);
                }
                Some(Tftp::Error(_, m)) => return Err(fail(m)),
                _ => return Err(fail(format!("no answer from {server} (is its TFTP service on?)"))),
            }
        }
        // Let the server see the last acknowledgement.
        let now = self.now;
        self.advance(t, now.saturating_add(10_000), |_| false);
        self.st(dev).tftp_port = None;
        Ok(out)
    }

    /// Asks the device's RADIUS servers whether a login is good: `Some(accepted)` from the first
    /// server that answers with the right secret, `None` when none answers.
    pub fn radius_login(&mut self, t: &Topology, dev: DeviceId, user: &str, password: &str) -> Option<bool> {
        let d = t.device(dev)?.clone();
        for (server, key) in d.access.radius_servers.clone() {
            let Ok(src) = self.source_for(t, &d, server) else { continue };
            let port = self.next_port();
            self.seq += 1;
            let id = (self.seq % 256) as u8;
            let auth = packetcraft_wire::md5::md5(format!("{}:{}:{user}", self.now, self.seq).as_bytes());
            let st = self.st(dev);
            st.radius_port = Some(port);
            st.radius_inbox.clear();
            let req = Radius::request(id, auth, user, password, &key, src);
            // Sent up to three times, as real clients retransmit (the first may wait for ARP).
            for _ in 0..3 {
                let start = self.now;
                if self.udp_out(t, &d, src, server, port, RADIUS_PORT, req.encode()).is_err() {
                    self.advance(t, start.saturating_add(100_000), |_| false);
                    continue;
                }
                self.advance(t, start.saturating_add(WAIT_US), |s| s.state.get(&dev).is_some_and(|st| st.radius_inbox.iter().any(|r| r.id == id)));
                if self.state.get(&dev).is_some_and(|st| st.radius_inbox.iter().any(|r| r.id == id)) {
                    break;
                }
            }
            let st = self.st(dev);
            st.radius_port = None;
            let answer = st.radius_inbox.iter().find(|r| r.id == id).cloned();
            // An answer signed with another secret doesn't count.
            if let Some(a) = answer.filter(|a| a.verifies(&auth, &key)) {
                return Some(a.code == packetcraft_wire::radius::ACCESS_ACCEPT);
            }
        }
        None
    }

    /// TACACS+ arriving at a server: `None` when it isn't for port 49 on a server.
    pub(crate) fn tacacs_server_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, seg: &packetcraft_wire::tcp::Tcp) -> Option<Result<(), String>> {
        use packetcraft_wire::tacacs::{STATUS_FAIL, STATUS_PASS, TACACS_PORT, Tacacs};
        use packetcraft_wire::tcp::{self, Tcp};
        if d.kind != DeviceKind::Server || seg.dst_port != TACACS_PORT {
            return None;
        }
        let reply = |flags: u8, seq: u32, ack: u32, payload: Vec<u8>| Tcp {
            src_port: TACACS_PORT,
            dst_port: seg.src_port,
            seq,
            ack,
            flags,
            window: 65_535,
            payload,
        };
        let isn = 90_000u32;
        if !d.services.aaa.on {
            let _ = self.seg_to(t, d, ip.dst, ip.src, reply(tcp::RST | tcp::ACK, 0, seg.seq.wrapping_add(1), Vec::new()));
            return Some(Err("no AAA service here".into()));
        }
        if seg.flags & tcp::SYN != 0 {
            return Some(self.seg_to(t, d, ip.dst, ip.src, reply(tcp::SYN | tcp::ACK, isn, seg.seq.wrapping_add(1), Vec::new())));
        }
        if seg.payload.is_empty() {
            if seg.flags & tcp::FIN != 0 {
                return Some(self.seg_to(t, d, ip.dst, ip.src, reply(tcp::ACK | tcp::FIN, seg.ack, seg.seq.wrapping_add(1), Vec::new())));
            }
            return Some(Ok(()));
        }
        // Unknown clients get no answer.
        let Some(client) = d.services.aaa.clients.iter().find(|c| c.address == ip.src) else {
            return Some(Err(format!("{} isn't one of this AAA server's clients", ip.src)));
        };
        let Ok((Tacacs::Start { user, password }, session, seq)) = Tacacs::decode(&seg.payload, &client.secret) else {
            return Some(Err("a TACACS+ packet this server can't read (wrong key?)".into()));
        };
        let good = d.services.aaa.users.iter().any(|u| u.name == user && u.password == password);
        let answer = Tacacs::Reply {
            status: if good { STATUS_PASS } else { STATUS_FAIL },
            message: if good { "Welcome".into() } else { "Login refused".into() },
        };
        let ack = seg.seq.wrapping_add(seg.payload.len() as u32);
        Some(self.seg_to(
            t,
            d,
            ip.dst,
            ip.src,
            reply(tcp::PSH | tcp::ACK, isn.wrapping_add(1), ack, answer.encode(session, seq.wrapping_add(1), &client.secret)),
        ))
    }

    /// Asks the device's TACACS+ servers about a login, like [`Sim::radius_login`].
    pub fn tacacs_login(&mut self, t: &Topology, dev: DeviceId, user: &str, password: &str) -> Option<bool> {
        use packetcraft_wire::tacacs::{STATUS_PASS, TACACS_PORT, Tacacs};
        let d = t.device(dev)?.clone();
        for (server, key) in d.access.tacacs_servers.clone() {
            let Ok(mut c) = self.conn_open(t, dev, server, TACACS_PORT) else { continue };
            self.seq += 1;
            let session = u32::try_from(self.seq % u64::from(u32::MAX)).unwrap_or(1) ^ 0x5a5a_0000;
            self.conn_send(t, &mut c, &Tacacs::Start { user: user.into(), password: password.into() }.encode(session, 1, &key));
            let (got, _) = self.conn_recv(t, &mut c, |b| b.len() >= 12);
            self.conn_close(t, &mut c);
            // An answer that doesn't read with our key doesn't count.
            if let Ok((Tacacs::Reply { status, .. }, s, _)) = Tacacs::decode(&got, &key)
                && s == session
            {
                return Some(status == STATUS_PASS);
            }
        }
        None
    }

    /// The objects a device's agent serves, in MIB order: the system group and the interfaces
    /// table (RFC 1213).
    pub fn mib(&self, t: &Topology, d: &Device) -> Vec<(Vec<u32>, snmp::Value)> {
        use snmp::Value;
        let sys = |n: u32| vec![1, 3, 6, 1, 2, 1, 1, n, 0];
        let mut out = vec![
            (sys(1), Value::Text(format!("PacketCraft {} {}, simulated", d.catalogue_model().label, env!("CARGO_PKG_VERSION")))),
            (sys(3), Value::TimeTicks(u32::try_from(self.now / 10_000).unwrap_or(u32::MAX))),
            (sys(4), Value::Text(d.snmp.contact.clone())),
            (sys(5), Value::Text(d.name.clone())),
            (sys(6), Value::Text(d.snmp.location.clone())),
            (vec![1, 3, 6, 1, 2, 1, 2, 1, 0], Value::Integer(d.ports.len() as i64)),
        ];
        for col in [1u32, 2, 5, 7, 8] {
            for (i, p) in d.ports.iter().enumerate() {
                let n = u32::try_from(i + 1).unwrap_or(0);
                let up = crate::port_up(t, packetcraft_model::End { device: d.id, port: i });
                let v = match col {
                    1 => Value::Integer(i64::from(n)),
                    2 => Value::Text(p.name.clone()),
                    5 => Value::Integer(i64::from(crate::port_max_speed(&p.name).unwrap_or(2)) * 1_000_000),
                    7 => Value::Integer(if p.shutdown { 2 } else { 1 }),
                    _ => Value::Integer(if up { 1 } else { 2 }),
                };
                out.push((vec![1, 3, 6, 1, 2, 1, 2, 2, 1, col, n], v));
            }
        }
        out
    }

    fn snmp_answer(&mut self, t: &Topology, d: &Device, req: &Snmp, rw: bool) -> Snmp {
        use snmp::Value;
        let mib = self.mib(t, d);
        let mut answer = Snmp { kind: PduKind::Response, bindings: Vec::new(), error_status: snmp::NO_ERROR, error_index: 0, ..req.clone() };
        for (i, (oid, value)) in req.bindings.iter().enumerate() {
            let found = match req.kind {
                PduKind::GetNext => mib.iter().find(|(o, _)| o > oid).cloned().unwrap_or((oid.clone(), Value::EndOfMib)),
                _ => mib.iter().find(|(o, _)| o == oid).cloned().unwrap_or((oid.clone(), Value::NoSuchObject)),
            };
            if req.kind == PduKind::Set {
                let which = match oid.as_slice() {
                    [1, 3, 6, 1, 2, 1, 1, 4, 0] => Some("contact"),
                    [1, 3, 6, 1, 2, 1, 1, 5, 0] => Some("name"),
                    [1, 3, 6, 1, 2, 1, 1, 6, 0] => Some("location"),
                    _ => None,
                };
                let status = match (rw, which, value) {
                    (false, _, _) => Some(snmp::NO_ACCESS),
                    (true, None, _) => Some(snmp::NOT_WRITABLE),
                    (true, Some(_), v) if !matches!(v, Value::Text(_)) => Some(snmp::WRONG_TYPE),
                    (true, Some(w), Value::Text(s)) => {
                        self.snmp_changes.push((d.id, w, s.chars().take(255).collect()));
                        None
                    }
                    _ => Some(snmp::NOT_WRITABLE),
                };
                if let Some(e) = status {
                    answer.error_status = e;
                    answer.error_index = u8::try_from(i + 1).unwrap_or(u8::MAX);
                }
                answer.bindings.push((oid.clone(), value.clone()));
            } else {
                answer.bindings.push(found);
            }
        }
        answer
    }

    /// Sets agents accepted, for the lab to apply.
    pub fn take_snmp_changes(&mut self) -> Vec<(DeviceId, &'static str, String)> {
        std::mem::take(&mut self.snmp_changes)
    }

    /// An SNMP request from `dev` (a manager, such as a PC's MIB browser) to an agent.
    pub fn snmp_request(&mut self, t: &Topology, dev: DeviceId, agent: Ipv4Addr, req: Snmp) -> Result<Snmp, SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?.clone();
        let src = self.source_for(t, &d, agent)?;
        let port = self.next_port();
        let st = self.st(dev);
        st.snmp_port = Some(port);
        st.snmp_inbox.clear();
        let id = req.request_id;
        // Sent up to three times: the first may wait for ARP on the way.
        for _ in 0..3 {
            let start = self.now;
            if self.udp_out(t, &d, src, agent, port, SNMP_PORT, req.encode()).is_err() {
                self.advance(t, start.saturating_add(100_000), |_| false);
                continue;
            }
            self.advance(t, start.saturating_add(WAIT_US), |s| s.state.get(&dev).is_some_and(|st| st.snmp_inbox.iter().any(|r| r.request_id == id)));
            if self.state.get(&dev).is_some_and(|st| st.snmp_inbox.iter().any(|r| r.request_id == id)) {
                break;
            }
        }
        let st = self.st(dev);
        st.snmp_port = None;
        let got = st.snmp_inbox.iter().find(|r| r.request_id == id).cloned();
        got.ok_or_else(|| SimError::NoSource(format!("{agent} did not answer (no SNMP agent, or a wrong community)")))
    }

    /// Asks the device's NTP servers for the time; returns the server that answered.
    pub fn ntp_sync(&mut self, t: &Topology, dev: DeviceId) -> Option<Ipv4Addr> {
        let d = t.device(dev)?.clone();
        for server in d.ntp_servers.clone() {
            let Ok(src) = self.source_for(t, &d, server) else { continue };
            self.st(dev).ntp_synced = None;
            if self.udp_out(t, &d, src, server, NTP_PORT, NTP_PORT, Ntp { server: false, stratum: 0, time: self.clock(dev).0 }.encode()).is_err() {
                continue;
            }
            let start = self.now;
            self.advance(t, start.saturating_add(WAIT_US), |s| s.state.get(&dev).is_some_and(|st| st.ntp_synced.is_some()));
            if let Some(s) = self.state.get(&dev).and_then(|st| st.ntp_synced) {
                return Some(s);
            }
        }
        None
    }

    /// Sends queued log messages to the devices' syslog servers.
    pub(crate) fn flush_syslog(&mut self, t: &Topology) {
        let pending: Vec<(DeviceId, Vec<String>)> =
            self.state.iter_mut().filter(|(_, s)| !s.syslog_out.is_empty()).map(|(id, s)| (*id, std::mem::take(&mut s.syslog_out))).collect();
        for (id, lines) in pending {
            let Some(d) = t.device(id).cloned() else { continue };
            for host in d.log_hosts.clone() {
                let Ok(src) = self.source_for(t, &d, host) else { continue };
                for l in &lines {
                    let _ = self.udp_out(t, &d, src, host, 49_514, SYSLOG_PORT, syslog_encode(&d.name, l));
                }
            }
        }
    }
}
