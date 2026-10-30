//! Automation transports: a router's web server (RESTCONF, RFC 8040, on ports 80 and 443) and
//! NETCONF server (RFC 6241 over the RFC 6242 framing, port 830), and the TCP client sessions
//! hosts open to them.
//!
//! The simulator moves the real segments and keeps each connection's bytes; what a request
//! *means* (the datastore, the edits) is decided by handlers the engine installs
//! ([`Sim::set_restconf`], [`Sim::set_netconf`]), since that layer owns the console grammar. Accepted edits come back
//! as configuration lines ([`Sim::take_automation_changes`]) that the engine applies through the
//! grammar, so the running configuration reflects them.
//!
//! Not simulated: TLS on port 443 and SSH under NETCONF. The bytes cross the wire in clear; a
//! NETCONF session opens with an SSH-style identification line and a user line instead of the
//! SSH handshake (see `netlabcraft_wire::netconf`).

use std::collections::{BTreeMap, VecDeque};
use std::net::Ipv4Addr;

use netlabcraft_core::DeviceId;
use netlabcraft_model::{Device, DeviceKind, Topology};
use netlabcraft_wire::netconf::{self, Framing};
use netlabcraft_wire::tcp::{self, Tcp};
use netlabcraft_wire::{Ipv4, http};

use crate::{Sim, SimError, l3_ifaces};

/// Most connections a router keeps open at once (the oldest is dropped).
pub const MAX_CONNS: usize = 32;
/// Most bytes buffered for one connection.
pub const MAX_CONN_BYTES: usize = 512 * 1024;
/// Most bytes a client session collects.
pub const MAX_RESPONSE: usize = 1024 * 1024;
/// NETCONF sessions remembered per router (`show netconf-yang sessions`).
pub const MAX_HISTORY: usize = 16;
const SERVER_ISN: u32 = 80_000;
const MSS: usize = 1_460;

/// What the RESTCONF handler made of a request.
#[derive(Debug, Clone)]
pub struct Served {
    pub response: http::Response,
    /// Configuration lines to apply (in configuration mode), in order.
    pub lines: Vec<String>,
}

/// What a NETCONF handler is told.
#[derive(Debug, Clone, Copy)]
pub enum NetconfEvent<'a> {
    /// The session was authenticated: the server sends its `<hello>`.
    Open,
    /// A complete message from the client.
    Message(&'a str),
}

/// What the NETCONF handler answers.
#[derive(Debug, Clone, Default)]
pub struct NetconfStep {
    pub reply: Option<String>,
    /// End the session after the reply.
    pub close: bool,
    /// Configuration lines to apply.
    pub lines: Vec<String>,
}

/// One NETCONF session's state, kept by the simulator and changed by the handler.
#[derive(Debug, Clone, Default)]
pub struct NetconfSession {
    pub id: u32,
    pub user: String,
    /// Both hellos were exchanged.
    pub hello_done: bool,
    /// Both sides speak base:1.1: chunked framing after the hellos.
    pub chunked: bool,
    /// Lines this session's edits applied (a later `<get-config>` in the session sees them).
    pub applied: Vec<String>,
    pub rpcs: u32,
}

pub type RestconfFn = fn(&Topology, DeviceId, &http::Request) -> Served;
pub type NetconfFn = fn(&Topology, DeviceId, &mut NetconfSession, NetconfEvent<'_>) -> NetconfStep;

/// A NETCONF session as `show netconf-yang sessions` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: u32,
    pub peer: Ipv4Addr,
    pub port: u16,
    pub user: String,
    pub chunked: bool,
    pub rpcs: u32,
}

#[derive(Debug, Clone, Default)]
struct Conn {
    buf: Vec<u8>,
    /// The server's next sequence number.
    seq: u32,
    authed: bool,
    session: NetconfSession,
    opened_at: u64,
}

/// The simulator's automation state.
#[derive(Debug, Clone, Default)]
pub(crate) struct AutoState {
    restconf: Option<RestconfFn>,
    netconf: Option<NetconfFn>,
    /// (router, client address, client port, server port) → connection.
    conns: BTreeMap<(DeviceId, Ipv4Addr, u16, u16), Conn>,
    changes: Vec<(DeviceId, Vec<String>)>,
    next_session: u32,
    history: BTreeMap<DeviceId, VecDeque<SessionInfo>>,
}

/// Whether a router serves this TCP port now.
pub fn serves(d: &Device, port: u16) -> bool {
    let a = &d.automation;
    d.kind == DeviceKind::Router
        && !d.powered_off
        && ((port == http::PORT && a.http) || (port == http::TLS_PORT && a.https) || (port == netconf::PORT && a.netconf))
}

impl Sim {
    /// Installs the handler that answers RESTCONF requests.
    pub fn set_restconf(&mut self, f: RestconfFn) {
        self.auto.restconf = Some(f);
    }

    /// Installs the handler that answers NETCONF messages.
    pub fn set_netconf(&mut self, f: NetconfFn) {
        self.auto.netconf = Some(f);
    }

    /// Edits servers accepted since the last call: (router, configuration lines).
    pub fn take_automation_changes(&mut self) -> Vec<(DeviceId, Vec<String>)> {
        std::mem::take(&mut self.auto.changes)
    }

    /// The NETCONF sessions a router served lately, newest last.
    pub fn netconf_sessions(&self, d: DeviceId) -> Vec<SessionInfo> {
        self.auto.history.get(&d).map(|h| h.iter().cloned().collect()).unwrap_or_default()
    }

    /// A segment for a router's web or NETCONF server; `None` if this port isn't one.
    pub(crate) fn automation_server_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, seg: &Tcp) -> Option<Result<(), String>> {
        if !serves(d, seg.dst_port) {
            return None;
        }
        Some(self.app_in(t, d, ip, seg))
    }

    fn app_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, seg: &Tcp) -> Result<(), String> {
        let key = (d.id, ip.src, seg.src_port, seg.dst_port);
        let reply = |flags: u8, seq: u32, ack: u32, payload: Vec<u8>| Tcp {
            src_port: seg.dst_port,
            dst_port: seg.src_port,
            seq,
            ack,
            flags,
            window: 65_535,
            payload,
        };
        if seg.flags & tcp::SYN != 0 {
            let mine: Vec<_> = self.auto.conns.iter().filter(|(k, _)| k.0 == d.id).map(|(k, c)| (*k, c.opened_at)).collect();
            if mine.len() >= MAX_CONNS
                && let Some((old, _)) = mine.iter().min_by_key(|(_, at)| *at)
            {
                self.auto.conns.remove(old);
            }
            self.auto.conns.insert(key, Conn { seq: SERVER_ISN.wrapping_add(1), opened_at: self.now, ..Conn::default() });
            self.note_tcp(d.id, (ip.dst, seg.dst_port), (ip.src, seg.src_port));
            return self.tcp_send(t, d, ip.dst, ip.src, &reply(tcp::SYN | tcp::ACK, SERVER_ISN, seg.seq.wrapping_add(1), Vec::new()));
        }
        if seg.flags & tcp::RST != 0 {
            self.auto.conns.remove(&key);
            return Ok(());
        }
        let Some(mut conn) = self.auto.conns.remove(&key) else {
            if !seg.payload.is_empty() || seg.flags & tcp::FIN != 0 {
                self.tcp_send(t, d, ip.dst, ip.src, &reply(tcp::RST | tcp::ACK, seg.ack, seg.seq.wrapping_add(1), Vec::new()))?;
                return Err("a segment for no open connection".into());
            }
            return Ok(());
        };
        let fin = seg.flags & tcp::FIN != 0;
        let ack = seg.seq.wrapping_add(seg.payload.len() as u32).wrapping_add(u32::from(fin));
        let (mut out, mut close) = (Vec::new(), false);
        if conn.buf.len().saturating_add(seg.payload.len()) > MAX_CONN_BYTES {
            close = true;
            if seg.dst_port != netconf::PORT {
                out = error_page(413, "The request is larger than this server accepts.").encode();
            }
        } else {
            conn.buf.extend_from_slice(&seg.payload);
            if !seg.payload.is_empty() {
                if seg.dst_port == netconf::PORT {
                    close = self.netconf_feed(t, d, ip.src, seg.src_port, &mut conn, &mut out);
                } else {
                    match http::Request::parse(&conn.buf) {
                        Ok(Some((req, _))) => {
                            let served = match self.auto.restconf {
                                Some(f) => f(t, d.id, &req),
                                None => Served { response: error_page(503, "RESTCONF is not available."), lines: Vec::new() },
                            };
                            if !served.lines.is_empty() {
                                self.auto.changes.push((d.id, served.lines));
                            }
                            out = served.response.encode();
                            close = true;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            let status = if http::too_large(&e) { 413 } else { 400 };
                            out = error_page(status, &e.to_string()).encode();
                            close = true;
                        }
                    }
                }
            }
        }
        close |= fin;
        let chunks: Vec<&[u8]> = out.chunks(MSS).collect();
        let last = chunks.len().saturating_sub(1);
        let mut seq = conn.seq;
        for (i, c) in chunks.iter().enumerate() {
            let flags = if i == last && close { tcp::PSH | tcp::ACK | tcp::FIN } else { tcp::PSH | tcp::ACK };
            self.tcp_send(t, d, ip.dst, ip.src, &reply(flags, seq, ack, c.to_vec()))?;
            seq = seq.wrapping_add(c.len() as u32);
        }
        if chunks.is_empty() && close {
            self.tcp_send(t, d, ip.dst, ip.src, &reply(tcp::FIN | tcp::ACK, seq, ack, Vec::new()))?;
        }
        conn.seq = seq;
        if !close {
            self.auto.conns.insert(key, conn);
        }
        Ok(())
    }

    /// Feeds a NETCONF connection's buffer through the handler; whether the session ends.
    fn netconf_feed(&mut self, t: &Topology, d: &Device, peer: Ipv4Addr, port: u16, conn: &mut Conn, out: &mut Vec<u8>) -> bool {
        let Some(handler) = self.auto.netconf else { return true };
        if !conn.authed {
            match netconf::parse_auth(&conn.buf) {
                Ok(None) => return false,
                Err(_) => {
                    out.extend_from_slice(b"SSH-2.0-NetLabCraft\r\nProtocol mismatch.\r\n");
                    return true;
                }
                Ok(Some((user, password, used))) => {
                    conn.buf.drain(..used.min(conn.buf.len()));
                    out.extend_from_slice(b"SSH-2.0-NetLabCraft\r\n");
                    let ok = d.access.users.iter().any(|u| u.name == user && u.privilege >= 15 && u.matches(&password));
                    if !ok {
                        out.extend_from_slice(b"Permission denied (password).\r\n");
                        return true;
                    }
                    conn.authed = true;
                    self.auto.next_session = self.auto.next_session.wrapping_add(1).max(1);
                    conn.session = NetconfSession { id: self.auto.next_session, user, ..NetconfSession::default() };
                    let step = handler(t, d.id, &mut conn.session, NetconfEvent::Open);
                    if let Some(r) = step.reply {
                        out.extend_from_slice(&netconf::encode(&r, Framing::EndOfMessage));
                    }
                    self.remember(d.id, peer, port, &conn.session);
                }
            }
        }
        // Messages so far, one by one; framing is end-of-message until both hellos are done.
        for _ in 0..256 {
            let framing = if conn.session.hello_done && conn.session.chunked { Framing::Chunked } else { Framing::EndOfMessage };
            let (msg, used) = match netconf::decode(&conn.buf, framing) {
                Ok(Some(m)) => m,
                Ok(None) => return false,
                Err(_) => return true,
            };
            conn.buf.drain(..used.min(conn.buf.len()));
            let step = handler(t, d.id, &mut conn.session, NetconfEvent::Message(&msg));
            if !step.lines.is_empty() {
                self.auto.changes.push((d.id, step.lines));
            }
            let framing = if conn.session.hello_done && conn.session.chunked { Framing::Chunked } else { Framing::EndOfMessage };
            if let Some(r) = step.reply {
                out.extend_from_slice(&netconf::encode(&r, framing));
            }
            self.remember(d.id, peer, port, &conn.session);
            if step.close {
                return true;
            }
        }
        false
    }

    fn remember(&mut self, d: DeviceId, peer: Ipv4Addr, port: u16, s: &NetconfSession) {
        let h = self.auto.history.entry(d).or_default();
        let info = SessionInfo { id: s.id, peer, port, user: s.user.clone(), chunked: s.chunked, rpcs: s.rpcs };
        match h.iter_mut().find(|x| x.id == s.id) {
            Some(x) => *x = info,
            None => {
                if h.len() >= MAX_HISTORY {
                    h.pop_front();
                }
                h.push_back(info);
            }
        }
    }

    /// A client TCP session: connects to `server:port`, sends `data` and collects what the
    /// server sends until it closes (or stops answering). Returns the bytes received.
    pub fn tcp_session(&mut self, t: &Topology, dev: DeviceId, server: Ipv4Addr, port: u16, data: &[u8]) -> Result<Vec<u8>, SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        if l3_ifaces(d).iter().any(|i| i.net.addr == server) {
            return Err(SimError::NoSource(format!("{server} is this device's own address")));
        }
        let src = self.source_for(t, d, server)?;
        let local = self.next_port();
        let isn = 2_000u32;
        let seg = |flags: u8, seq: u32, ack: u32, payload: Vec<u8>| Tcp { src_port: local, dst_port: port, seq, ack, flags, window: 65_535, payload };
        self.st(dev).tcp_wait.push((local, port));
        let result = (|| {
            let mut synack = None;
            for _ in 0..4 {
                let _ = self.tcp_send(t, d, src, server, &seg(tcp::SYN, isn, 0, Vec::new()));
                synack = self.tcp_wait(t, dev, |s| s.dst_port == local && s.flags & (tcp::SYN | tcp::RST) != 0);
                if synack.is_some() {
                    break;
                }
            }
            let synack = synack.ok_or_else(|| SimError::NoSource(format!("{server} did not answer on port {port} (connection timed out)")))?;
            if synack.flags & tcp::RST != 0 {
                return Err(SimError::NoSource(format!("{server} refused the connection on port {port} (nothing listens there)")));
            }
            let srv = synack.seq.wrapping_add(1);
            let mut seq = isn.wrapping_add(1);
            let _ = self.tcp_send(t, d, src, server, &seg(tcp::ACK, seq, srv, Vec::new()));
            for c in data.chunks(MSS) {
                let _ = self.tcp_send(t, d, src, server, &seg(tcp::PSH | tcp::ACK, seq, srv, c.to_vec()));
                seq = seq.wrapping_add(c.len() as u32);
            }
            let mut got = Vec::new();
            let mut last = srv;
            loop {
                let Some(s) = self.tcp_wait(t, dev, |s| s.dst_port == local && (!s.payload.is_empty() || s.flags & (tcp::FIN | tcp::RST) != 0))
                else {
                    if got.is_empty() {
                        return Err(SimError::NoSource(format!("{server} stopped answering")));
                    }
                    break;
                };
                if s.flags & tcp::RST != 0 {
                    if got.is_empty() {
                        return Err(SimError::NoSource(format!("{server} reset the connection")));
                    }
                    break;
                }
                last = s.seq.wrapping_add(s.payload.len() as u32);
                if got.len() < MAX_RESPONSE {
                    got.extend_from_slice(&s.payload);
                }
                if s.flags & tcp::FIN != 0 {
                    break;
                }
            }
            let _ = self.tcp_send(t, d, src, server, &seg(tcp::FIN | tcp::ACK, seq, last.wrapping_add(1), Vec::new()));
            Ok(got)
        })();
        let st = self.st(dev);
        st.tcp_wait.retain(|w| *w != (local, port));
        st.tcp_inbox.retain(|s| s.dst_port != local);
        if result.is_ok() {
            self.note_tcp(dev, (src, local), (server, port));
        }
        result
    }
}

/// A small error page (outside RESTCONF's own errors: framing and size problems).
fn error_page(status: u16, text: &str) -> http::Response {
    let mut r = http::Response::new(status);
    r.headers.push(("Server".into(), "NetLabCraft".into()));
    r.headers.push(("Content-Type".into(), "text/plain".into()));
    r.headers.push(("Connection".into(), "close".into()));
    r.body = format!("{text}\n").into_bytes();
    r
}
