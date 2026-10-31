//! Automation transports: a router's web server (RESTCONF, RFC 8040, on ports 80 and 443) and
//! NETCONF server (RFC 6241 over the RFC 6242 framing, port 830), and the TCP client sessions
//! hosts open to them, over IPv4 or IPv6.
//!
//! The simulator moves the real segments and keeps each connection's bytes; what a request
//! *means* (the datastore, the edits) is decided by handlers the engine installs
//! ([`Sim::set_restconf`], [`Sim::set_netconf`]), since that layer owns the console grammar. Accepted edits come back
//! as configuration lines ([`Sim::take_automation_changes`]) that the engine applies through the
//! grammar, so the running configuration reflects them.
//!
//! Client connections can stay open between commands ([`Sim::tcp_open`], [`Sim::tcp_exchange`],
//! [`Sim::keep_client`]), so two NETCONF sessions can be open at once and contend for a lock.
//!
//! Not simulated: TLS on port 443 and SSH under NETCONF. The bytes cross the wire in clear; a
//! NETCONF session opens with an SSH-style identification line and a user line instead of the
//! SSH handshake (see `netlabcraft_wire::netconf`).

use std::collections::{BTreeMap, VecDeque};
use std::net::{IpAddr, Ipv6Addr};

use netlabcraft_core::DeviceId;
use netlabcraft_model::{Device, DeviceKind, Topology};
use netlabcraft_wire::ipv6::Ipv6;
use netlabcraft_wire::netconf::{self, Framing};
use netlabcraft_wire::tcp::{self, Tcp};
use netlabcraft_wire::{PROTO_TCP, http};

use crate::v6::{l3_ifaces6, route6};
use crate::{Sim, SimError, l3_ifaces};

/// Most connections a router keeps open at once (the oldest is dropped).
pub const MAX_CONNS: usize = 32;
/// Most bytes buffered for one connection.
pub const MAX_CONN_BYTES: usize = 512 * 1024;
/// Most bytes a client session collects.
pub const MAX_RESPONSE: usize = 1024 * 1024;
/// NETCONF sessions remembered per router (`show netconf-yang sessions`).
pub const MAX_HISTORY: usize = 16;
/// Most client connections kept open between commands, lab-wide (the oldest is closed).
pub const MAX_OPEN_CLIENTS: usize = 8;
const SERVER_ISN: u32 = 80_000;
const MSS: usize = 1_440;

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
    pub peer: IpAddr,
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

/// (router, client address, client port, server port).
type ConnKey = (DeviceId, IpAddr, u16, u16);

/// The simulator's automation state.
#[derive(Debug, Clone, Default)]
pub(crate) struct AutoState {
    restconf: Option<RestconfFn>,
    netconf: Option<NetconfFn>,
    conns: BTreeMap<ConnKey, Conn>,
    changes: Vec<(DeviceId, Vec<String>)>,
    next_session: u32,
    history: BTreeMap<DeviceId, VecDeque<SessionInfo>>,
    clients: BTreeMap<u32, OpenClient>,
    next_client: u32,
}

/// The client end of a TCP connection a host opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpClient {
    pub dev: DeviceId,
    pub local: u16,
    pub src: IpAddr,
    pub server: IpAddr,
    pub port: u16,
    seq: u32,
    ack: u32,
    /// The server closed (or reset) the connection.
    pub closed: bool,
}

/// A client connection kept open between commands, with what its protocol needs to go on.
#[derive(Debug, Clone)]
pub struct OpenClient {
    pub tcp: TcpClient,
    /// Bytes received but not yet used.
    pub buf: Vec<u8>,
    /// The protocol's framing after the opening exchange (NETCONF: chunked or not).
    pub chunked: bool,
    /// The NETCONF session id the server gave, or 0.
    pub session_id: u32,
    /// The next message id the client uses.
    pub next_id: u32,
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
    pub(crate) fn automation_server_in(&mut self, t: &Topology, d: &Device, src: IpAddr, dst: IpAddr, seg: &Tcp) -> Option<Result<(), String>> {
        if !serves(d, seg.dst_port) {
            return None;
        }
        Some(self.app_in(t, d, src, dst, seg))
    }

    /// Sends a segment this device originates, over the IP version of its addresses.
    pub(crate) fn tcp_send_ip(&mut self, t: &Topology, d: &Device, src: IpAddr, dst: IpAddr, seg: &Tcp) -> Result<(), String> {
        match (src, dst) {
            (IpAddr::V4(s), IpAddr::V4(x)) => self.tcp_send(t, d, s, x, seg),
            (IpAddr::V6(s), IpAddr::V6(x)) => {
                let hop_limit = if d.kind.is_host() { 128 } else { 64 };
                self.send_ip6(t, d, Ipv6 { hop_limit, next_header: PROTO_TCP, src: s, dst: x, payload: seg.encode6(s, x) }, None)
            }
            _ => Err("TCP between an IPv4 and an IPv6 address".into()),
        }
    }

    /// TCP over IPv6 arriving at this device: an automation server, a client connection's
    /// segment, or a reset.
    pub(crate) fn tcp6_in(&mut self, t: &Topology, d: &Device, ip: &Ipv6) -> Result<(), String> {
        let seg = Tcp::decode6(&ip.payload, ip.src, ip.dst).map_err(|e| e.to_string())?;
        if let Some(r) = self.automation_server_in(t, d, IpAddr::V6(ip.src), IpAddr::V6(ip.dst), &seg) {
            return r;
        }
        let st = self.st(d.id);
        if st.tcp_wait.contains(&(seg.dst_port, seg.src_port)) {
            if st.tcp_inbox.len() < 64 {
                st.tcp_inbox.push(seg);
            }
            return Ok(());
        }
        if seg.flags & tcp::RST == 0 {
            let rst = Tcp {
                src_port: seg.dst_port,
                dst_port: seg.src_port,
                seq: seg.ack,
                ack: seg.seq.wrapping_add(1),
                flags: tcp::RST | tcp::ACK,
                window: 0,
                payload: Vec::new(),
            };
            let _ = self.tcp_send_ip(t, d, IpAddr::V6(ip.dst), IpAddr::V6(ip.src), &rst);
        }
        Err(format!("no service on TCP port {}", seg.dst_port))
    }

    fn app_in(&mut self, t: &Topology, d: &Device, src: IpAddr, dst: IpAddr, seg: &Tcp) -> Result<(), String> {
        let key = (d.id, src, seg.src_port, seg.dst_port);
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
            if let (IpAddr::V4(a), IpAddr::V4(b)) = (dst, src) {
                self.note_tcp(d.id, (a, seg.dst_port), (b, seg.src_port));
            }
            return self.tcp_send_ip(t, d, dst, src, &reply(tcp::SYN | tcp::ACK, SERVER_ISN, seg.seq.wrapping_add(1), Vec::new()));
        }
        if seg.flags & tcp::RST != 0 {
            self.auto.conns.remove(&key);
            return Ok(());
        }
        let Some(mut conn) = self.auto.conns.remove(&key) else {
            if !seg.payload.is_empty() || seg.flags & tcp::FIN != 0 {
                self.tcp_send_ip(t, d, dst, src, &reply(tcp::RST | tcp::ACK, seg.ack, seg.seq.wrapping_add(1), Vec::new()))?;
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
                    close = self.netconf_feed(t, d, src, seg.src_port, &mut conn, &mut out);
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
            self.tcp_send_ip(t, d, dst, src, &reply(flags, seq, ack, c.to_vec()))?;
            seq = seq.wrapping_add(c.len() as u32);
        }
        if chunks.is_empty() && close {
            self.tcp_send_ip(t, d, dst, src, &reply(tcp::FIN | tcp::ACK, seq, ack, Vec::new()))?;
        }
        conn.seq = seq;
        if !close {
            self.auto.conns.insert(key, conn);
        }
        Ok(())
    }

    /// Feeds a NETCONF connection's buffer through the handler; whether the session ends.
    fn netconf_feed(&mut self, t: &Topology, d: &Device, peer: IpAddr, port: u16, conn: &mut Conn, out: &mut Vec<u8>) -> bool {
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

    fn remember(&mut self, d: DeviceId, peer: IpAddr, port: u16, s: &NetconfSession) {
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

    /// The address a host sources a connection to `server` from (IPv4 or IPv6).
    fn client_source(&mut self, t: &Topology, d: &Device, server: IpAddr) -> Result<IpAddr, SimError> {
        match server {
            IpAddr::V4(s) => {
                if l3_ifaces(d).iter().any(|i| i.net.addr == s) {
                    return Err(SimError::NoSource(format!("{server} is this device's own address")));
                }
                self.source_for(t, d, s).map(IpAddr::V4)
            }
            IpAddr::V6(s) => {
                let ifs = l3_ifaces6(d);
                if ifs.is_empty() {
                    return Err(SimError::NoSource(format!("{} has no IPv6 address", d.name)));
                }
                if ifs.iter().any(|i| i.owns(s)) {
                    return Err(SimError::NoSource(format!("{server} is this device's own address")));
                }
                let dynamic = self.dynamic_routes6(t, d);
                let (out, _) = route6(t, d, s, None, &dynamic).ok_or_else(|| SimError::NoSource(format!("{} has no IPv6 route to {s}", d.name)))?;
                let src: Ipv6Addr = out.source_for(s);
                Ok(IpAddr::V6(src))
            }
        }
    }

    /// Opens a client TCP connection (SYN, SYN-ACK, ACK) from a host to `server:port`.
    pub fn tcp_open(&mut self, t: &Topology, dev: DeviceId, server: IpAddr, port: u16) -> Result<TcpClient, SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        let src = self.client_source(t, d, server)?;
        let local = self.next_port();
        let isn = 2_000u32;
        let seg = |flags: u8, seq: u32, ack: u32| Tcp { src_port: local, dst_port: port, seq, ack, flags, window: 65_535, payload: Vec::new() };
        self.st(dev).tcp_wait.push((local, port));
        let mut synack = None;
        for _ in 0..4 {
            let _ = self.tcp_send_ip(t, d, src, server, &seg(tcp::SYN, isn, 0));
            synack = self.tcp_wait(t, dev, |s| s.dst_port == local && s.flags & (tcp::SYN | tcp::RST) != 0);
            if synack.is_some() {
                break;
            }
        }
        let c = TcpClient { dev, local, src, server, port, seq: isn.wrapping_add(1), ack: 0, closed: false };
        let Some(synack) = synack else {
            self.tcp_forget(&c);
            return Err(SimError::NoSource(format!("{server} did not answer on port {port} (connection timed out)")));
        };
        if synack.flags & tcp::RST != 0 {
            self.tcp_forget(&c);
            return Err(SimError::NoSource(format!("{server} refused the connection on port {port} (nothing listens there)")));
        }
        let c = TcpClient { ack: synack.seq.wrapping_add(1), ..c };
        let _ = self.tcp_send_ip(t, d, src, server, &seg(tcp::ACK, c.seq, c.ack));
        if let (IpAddr::V4(a), IpAddr::V4(b)) = (src, server) {
            self.note_tcp(dev, (a, local), (b, port));
        }
        Ok(c)
    }

    /// Sends `data` on an open connection and collects what the server sends back, until
    /// `done` says the bytes so far are a complete answer, the server closes, or it stops
    /// answering.
    pub fn tcp_exchange(&mut self, t: &Topology, c: &mut TcpClient, data: &[u8], done: &dyn Fn(&[u8]) -> bool) -> Result<Vec<u8>, SimError> {
        let d = t.device(c.dev).ok_or(SimError::NoDevice(c.dev))?;
        if c.closed {
            return Err(SimError::NoSource(format!("{} already closed the connection", c.server)));
        }
        let (local, port, src, server) = (c.local, c.port, c.src, c.server);
        let st = self.st(c.dev);
        if !st.tcp_wait.contains(&(local, port)) {
            st.tcp_wait.push((local, port));
        }
        for chunk in data.chunks(MSS) {
            let seg =
                Tcp { src_port: local, dst_port: port, seq: c.seq, ack: c.ack, flags: tcp::PSH | tcp::ACK, window: 65_535, payload: chunk.to_vec() };
            let _ = self.tcp_send_ip(t, d, src, server, &seg);
            c.seq = c.seq.wrapping_add(chunk.len() as u32);
        }
        let mut got = Vec::new();
        loop {
            let Some(s) = self
                .tcp_wait(t, c.dev, |s| s.dst_port == local && s.src_port == port && (!s.payload.is_empty() || s.flags & (tcp::FIN | tcp::RST) != 0))
            else {
                if got.is_empty() && !done(&got) {
                    return Err(SimError::NoSource(format!("{server} stopped answering")));
                }
                break;
            };
            if s.flags & tcp::RST != 0 {
                c.closed = true;
                if got.is_empty() {
                    return Err(SimError::NoSource(format!("{server} reset the connection")));
                }
                break;
            }
            c.ack = s.seq.wrapping_add(s.payload.len() as u32).wrapping_add(u32::from(s.flags & tcp::FIN != 0));
            if got.len() < MAX_RESPONSE {
                got.extend_from_slice(&s.payload);
            }
            if s.flags & tcp::FIN != 0 {
                c.closed = true;
                break;
            }
            if done(&got) {
                break;
            }
        }
        Ok(got)
    }

    /// Closes a client connection (FIN) and forgets it.
    pub fn tcp_close(&mut self, t: &Topology, c: &TcpClient) {
        if let Some(d) = t.device(c.dev) {
            let seg =
                Tcp { src_port: c.local, dst_port: c.port, seq: c.seq, ack: c.ack, flags: tcp::FIN | tcp::ACK, window: 65_535, payload: Vec::new() };
            let _ = self.tcp_send_ip(t, d, c.src, c.server, &seg);
        }
        self.tcp_forget(c);
    }

    fn tcp_forget(&mut self, c: &TcpClient) {
        let st = self.st(c.dev);
        st.tcp_wait.retain(|w| *w != (c.local, c.port));
        st.tcp_inbox.retain(|s| s.dst_port != c.local);
    }

    /// A client TCP session: connects to `server:port`, sends `data` and collects what the
    /// server sends until it closes (or stops answering). Returns the bytes received.
    pub fn tcp_session(&mut self, t: &Topology, dev: DeviceId, server: IpAddr, port: u16, data: &[u8]) -> Result<Vec<u8>, SimError> {
        let mut c = self.tcp_open(t, dev, server, port)?;
        let got = self.tcp_exchange(t, &mut c, data, &|_| false);
        self.tcp_close(t, &c);
        got
    }

    /// Keeps a client connection open between commands; its handle.
    pub fn keep_client(&mut self, c: OpenClient) -> u32 {
        if self.auto.clients.len() >= MAX_OPEN_CLIENTS
            && let Some(old) = self.auto.clients.keys().next().copied()
            && let Some(o) = self.auto.clients.remove(&old)
        {
            self.tcp_forget(&o.tcp);
        }
        self.auto.next_client = self.auto.next_client.wrapping_add(1).max(1);
        let h = self.auto.next_client;
        self.auto.clients.insert(h, c);
        h
    }

    /// Takes a kept connection back (it is no longer kept until [`Sim::keep_client`] again).
    pub fn take_client(&mut self, handle: u32) -> Option<OpenClient> {
        self.auto.clients.remove(&handle)
    }

    /// Puts a taken connection back under the same handle.
    pub fn return_client(&mut self, handle: u32, c: OpenClient) {
        self.auto.clients.insert(handle, c);
    }

    /// The kept connections: (handle, host, server, port, NETCONF session id).
    pub fn open_clients(&self) -> Vec<(u32, DeviceId, IpAddr, u16, u32)> {
        self.auto.clients.iter().map(|(h, c)| (*h, c.tcp.dev, c.tcp.server, c.tcp.port, c.session_id)).collect()
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
