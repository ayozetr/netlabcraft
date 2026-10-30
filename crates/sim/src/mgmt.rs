//! Network-management services on the wire: NTP clock sync, syslog to a server, and TFTP
//! copies of configurations.

use std::net::Ipv4Addr;

use packetcraft_core::DeviceId;
use packetcraft_model::{Device, DeviceKind, Topology};
use packetcraft_wire::mgmt::{NTP_PORT, Ntp, SYSLOG_PORT, TFTP_BLOCK, TFTP_PORT, Tftp, syslog_decode, syslog_encode};
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
