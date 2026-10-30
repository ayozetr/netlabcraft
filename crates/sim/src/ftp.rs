//! FTP (RFC 959) over the simulated TCP: a server with accounts and files, and a client that
//! logs in, opens a passive data connection and sends, fetches, lists or deletes a file.
//!
//! Each client operation is one session on the wire: connect, USER, PASS, TYPE I, PASV, the
//! command with its data connection, QUIT.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use netlabcraft_core::DeviceId;
use netlabcraft_model::services::{FtpUser, MAX_FILE, MAX_ITEMS, WebFile};
use netlabcraft_model::{Device, DeviceKind, Topology};
use netlabcraft_wire::tcp::{self, Tcp};
use netlabcraft_wire::{FTP_DATA_PORTS, FTP_PORT, Ipv4};

use crate::{Sim, SimError};

/// The server side of one control connection.
#[derive(Debug, Clone, Default)]
pub(crate) struct Control {
    /// Our next sequence number on it.
    seq: u32,
    user: Option<String>,
    logged_in: Option<String>,
    /// The passive data port opened for the next transfer.
    data_port: Option<u16>,
}

/// What a data connection carries.
#[derive(Debug, Clone)]
enum Job {
    /// Waiting for the command that says.
    Open,
    Send(Vec<u8>),
    Store(String, Vec<u8>),
}

#[derive(Debug, Clone)]
pub(crate) struct Data {
    client: Ipv4Addr,
    /// The control connection it belongs to.
    control: (Ipv4Addr, u16),
    job: Job,
    seq: u32,
}

/// A server's FTP connections.
#[derive(Debug, Clone, Default)]
pub(crate) struct ServerState {
    control: BTreeMap<(Ipv4Addr, u16), Control>,
    data: BTreeMap<u16, Data>,
    next_data: u16,
}

/// What an FTP client asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FtpOp {
    /// Log in and out (checks the account).
    Login,
    Put(String, Vec<u8>),
    Get(String),
    List,
    Delete(String),
}

/// A client's TCP connection, for the length of one operation.
pub(crate) struct Conn {
    dev: DeviceId,
    src: Ipv4Addr,
    dst: Ipv4Addr,
    local: u16,
    remote: u16,
    seq: u32,
    ack: u32,
}

fn may(u: &FtpUser, what: &str) -> bool {
    match what {
        "RETR" => u.read,
        "STOR" => u.write,
        "DELE" => u.delete,
        _ => u.list,
    }
}

impl Sim {
    pub(crate) fn seg_to(&mut self, t: &Topology, d: &Device, src: Ipv4Addr, dst: Ipv4Addr, seg: Tcp) -> Result<(), String> {
        let ip = Ipv4 {
            id: self.ip_id(),
            ttl: crate::default_ttl(d.kind),
            protocol: netlabcraft_wire::PROTO_TCP,
            src,
            dst,
            payload: seg.encode(src, dst),
        };
        self.send_ip(t, d, ip)
    }

    /// TCP for the FTP server: `None` when the segment isn't FTP's.
    pub(crate) fn ftp_server_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, seg: &Tcp) -> Option<Result<(), String>> {
        if d.kind != DeviceKind::Server {
            return None;
        }
        if seg.dst_port == FTP_PORT {
            return Some(self.ftp_control_in(t, d, ip, seg));
        }
        if FTP_DATA_PORTS.contains(&seg.dst_port) && self.state.get(&d.id).is_some_and(|s| s.ftp.data.contains_key(&seg.dst_port)) {
            return Some(self.ftp_data_in(t, d, ip, seg));
        }
        None
    }

    /// Sends a reply line on a control connection.
    fn ftp_reply(&mut self, t: &Topology, d: &Device, me: Ipv4Addr, key: (Ipv4Addr, u16), ack: u32, text: &str, fin: bool) -> Result<(), String> {
        let st = self.st(d.id);
        let Some(c) = st.ftp.control.get_mut(&key) else { return Ok(()) };
        let payload = format!("{text}\r\n").into_bytes();
        let seq = c.seq;
        c.seq = c.seq.wrapping_add(payload.len() as u32).wrapping_add(u32::from(fin));
        let flags = tcp::PSH | tcp::ACK | if fin { tcp::FIN } else { 0 };
        self.seg_to(t, d, me, key.0, Tcp { src_port: FTP_PORT, dst_port: key.1, seq, ack, flags, window: 65_535, payload })
    }

    fn ftp_control_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, seg: &Tcp) -> Result<(), String> {
        let key = (ip.src, seg.src_port);
        if !d.services.ftp.on {
            let rst = Tcp {
                src_port: FTP_PORT,
                dst_port: seg.src_port,
                seq: 0,
                ack: seg.seq.wrapping_add(1),
                flags: tcp::RST | tcp::ACK,
                window: 0,
                payload: Vec::new(),
            };
            self.seg_to(t, d, ip.dst, ip.src, rst)?;
            return Err("the FTP service is off".into());
        }
        if seg.flags & tcp::SYN != 0 {
            let isn = 60_000u32;
            let st = self.st(d.id);
            if st.ftp.control.len() >= 64 {
                st.ftp.control.clear();
            }
            st.ftp.control.insert(key, Control { seq: isn.wrapping_add(1), ..Control::default() });
            let synack = Tcp {
                src_port: FTP_PORT,
                dst_port: seg.src_port,
                seq: isn,
                ack: seg.seq.wrapping_add(1),
                flags: tcp::SYN | tcp::ACK,
                window: 65_535,
                payload: Vec::new(),
            };
            self.seg_to(t, d, ip.dst, ip.src, synack)?;
            return self.ftp_reply(t, d, ip.dst, key, seg.seq.wrapping_add(1), "220 NetLabCraft FTP server ready", false);
        }
        if seg.flags & tcp::FIN != 0 && seg.payload.is_empty() {
            let c = self.st(d.id).ftp.control.remove(&key);
            let ack = Tcp {
                src_port: FTP_PORT,
                dst_port: seg.src_port,
                seq: c.map_or(0, |c| c.seq),
                ack: seg.seq.wrapping_add(1),
                flags: tcp::ACK,
                window: 65_535,
                payload: Vec::new(),
            };
            return self.seg_to(t, d, ip.dst, ip.src, ack);
        }
        if seg.payload.is_empty() {
            return Ok(());
        }
        let ack = seg.seq.wrapping_add(seg.payload.len() as u32);
        let line = String::from_utf8_lossy(&seg.payload).trim().to_string();
        let (cmd, arg) = line.split_once(' ').map_or((line.as_str(), ""), |(c, a)| (c, a.trim()));
        let cmd = cmd.to_ascii_uppercase();
        let Some(ctl) = self.state.get(&d.id).and_then(|s| s.ftp.control.get(&key)).cloned() else {
            return Err("no such FTP session".into());
        };
        let user = ctl.logged_in.as_ref().and_then(|n| d.services.ftp.users.iter().find(|u| u.name == *n)).cloned();
        let reply = match (cmd.as_str(), &user) {
            ("USER", _) => {
                if let Some(c) = self.st(d.id).ftp.control.get_mut(&key) {
                    c.user = Some(arg.to_string());
                    c.logged_in = None;
                }
                format!("331 Password required for {arg}")
            }
            ("PASS", _) => {
                let name = ctl.user.clone().unwrap_or_default();
                let good = d.services.ftp.users.iter().any(|u| u.name == name && u.password == arg);
                if let Some(c) = self.st(d.id).ftp.control.get_mut(&key) {
                    c.logged_in = good.then_some(name);
                }
                if good { "230 Logged in".into() } else { "530 Login incorrect".into() }
            }
            ("QUIT", _) => {
                self.ftp_reply(t, d, ip.dst, key, ack, "221 Goodbye", true)?;
                return Ok(());
            }
            (_, None) => "530 Log in with USER and PASS first".into(),
            ("TYPE", _) => "200 Binary mode".into(),
            ("PASV", _) => {
                let st = self.st(d.id);
                if st.ftp.data.len() >= 64 {
                    st.ftp.data.clear();
                }
                st.ftp.next_data = st.ftp.next_data.wrapping_add(1) % 10_000;
                let port = FTP_DATA_PORTS.start + st.ftp.next_data;
                st.ftp.data.insert(port, Data { client: ip.src, control: key, job: Job::Open, seq: 70_000 });
                if let Some(c) = st.ftp.control.get_mut(&key) {
                    c.data_port = Some(port);
                }
                let o = ip.dst.octets();
                format!("227 Entering Passive Mode ({},{},{},{},{},{})", o[0], o[1], o[2], o[3], port >> 8, port & 0xff)
            }
            (c @ ("RETR" | "STOR" | "LIST" | "NLST" | "DELE"), Some(u)) => {
                if !may(u, c) {
                    "550 Permission denied".into()
                } else if c == "DELE" {
                    if d.services.ftp.files.iter().any(|f| f.name == arg) {
                        self.ftp_changes.push((d.id, arg.to_string(), None));
                        format!("250 {arg} deleted")
                    } else {
                        format!("550 {arg}: no such file")
                    }
                } else {
                    let job = match c {
                        "RETR" => d.services.ftp.files.iter().find(|f| f.name == arg).map(|f| Job::Send(f.content.clone().into_bytes())),
                        "STOR" => Some(Job::Store(arg.chars().filter(|c| !c.is_whitespace() && *c != '/').take(64).collect(), Vec::new())),
                        _ => {
                            let list: String = d.services.ftp.files.iter().map(|f| format!("{:>8}  {}\r\n", f.content.len(), f.name)).collect();
                            Some(Job::Send(list.into_bytes()))
                        }
                    };
                    match (job, ctl.data_port) {
                        (None, _) => format!("550 {arg}: no such file"),
                        (_, None) => "425 Use PASV first".into(),
                        (Some(j), Some(port)) => {
                            if let Some(data) = self.st(d.id).ftp.data.get_mut(&port) {
                                data.job = j;
                            }
                            "150 Opening data connection".into()
                        }
                    }
                }
            }
            _ => format!("502 {cmd} not implemented"),
        };
        self.ftp_reply(t, d, ip.dst, key, ack, &reply, false)?;
        // A transfer whose data connection is already open starts now.
        if let Some(port) = ctl.data_port {
            self.ftp_send_ready(t, d, ip.dst, port)?;
        }
        Ok(())
    }

    /// Sends a waiting download once its data connection is open (SYN answered).
    fn ftp_send_ready(&mut self, t: &Topology, d: &Device, me: Ipv4Addr, port: u16) -> Result<(), String> {
        let Some(data) = self.state.get(&d.id).and_then(|s| s.ftp.data.get(&port)).cloned() else { return Ok(()) };
        let (Job::Send(bytes), Some(client_port)) = (&data.job, self.ftp_data_peer.get(&(d.id, port)).copied()) else { return Ok(()) };
        let mut seq = data.seq.wrapping_add(1);
        let chunks: Vec<&[u8]> = if bytes.is_empty() { vec![&[][..]] } else { bytes.chunks(1460).collect() };
        let last = chunks.len().saturating_sub(1);
        for (i, c) in chunks.into_iter().enumerate() {
            let flags = if i == last { tcp::PSH | tcp::ACK | tcp::FIN } else { tcp::ACK };
            self.seg_to(
                t,
                d,
                me,
                data.client,
                Tcp { src_port: port, dst_port: client_port, seq, ack: 0, flags, window: 65_535, payload: c.to_vec() },
            )?;
            seq = seq.wrapping_add(c.len() as u32);
        }
        self.st(d.id).ftp.data.remove(&port);
        self.ftp_data_peer.remove(&(d.id, port));
        self.ftp_reply(t, d, me, data.control, 0, "226 Transfer complete", false)
    }

    fn ftp_data_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, seg: &Tcp) -> Result<(), String> {
        let port = seg.dst_port;
        let Some(data) = self.state.get(&d.id).and_then(|s| s.ftp.data.get(&port)).cloned() else { return Ok(()) };
        if ip.src != data.client {
            return Err("data connection from another address".into());
        }
        if seg.flags & tcp::SYN != 0 {
            self.ftp_data_peer.insert((d.id, port), seg.src_port);
            let synack = Tcp {
                src_port: port,
                dst_port: seg.src_port,
                seq: data.seq,
                ack: seg.seq.wrapping_add(1),
                flags: tcp::SYN | tcp::ACK,
                window: 65_535,
                payload: Vec::new(),
            };
            self.seg_to(t, d, ip.dst, ip.src, synack)?;
            return self.ftp_send_ready(t, d, ip.dst, port);
        }
        let ack = seg.seq.wrapping_add(seg.payload.len() as u32).wrapping_add(u32::from(seg.flags & tcp::FIN != 0));
        if let Job::Store(name, mut buf) = data.job.clone() {
            if buf.len() < MAX_FILE {
                buf.extend_from_slice(&seg.payload);
            }
            if seg.flags & tcp::FIN != 0 {
                let ackseg = Tcp {
                    src_port: port,
                    dst_port: seg.src_port,
                    seq: data.seq.wrapping_add(1),
                    ack,
                    flags: tcp::ACK | tcp::FIN,
                    window: 65_535,
                    payload: Vec::new(),
                };
                self.seg_to(t, d, ip.dst, ip.src, ackseg)?;
                self.st(d.id).ftp.data.remove(&port);
                self.ftp_data_peer.remove(&(d.id, port));
                let content = String::from_utf8_lossy(&buf).chars().take(MAX_FILE).collect();
                self.ftp_changes.push((d.id, name, Some(content)));
                return self.ftp_reply(t, d, ip.dst, data.control, 0, "226 Transfer complete", false);
            }
            if let Some(x) = self.st(d.id).ftp.data.get_mut(&port) {
                x.job = Job::Store(name, buf);
            }
        }
        if !seg.payload.is_empty() {
            let ackseg = Tcp {
                src_port: port,
                dst_port: seg.src_port,
                seq: data.seq.wrapping_add(1),
                ack,
                flags: tcp::ACK,
                window: 65_535,
                payload: Vec::new(),
            };
            self.seg_to(t, d, ip.dst, ip.src, ackseg)?;
        }
        Ok(())
    }

    /// File changes FTP clients made on servers, for the lab to apply: (server, name, new
    /// content or `None` for deleted).
    pub fn take_ftp_changes(&mut self) -> Vec<(DeviceId, String, Option<String>)> {
        std::mem::take(&mut self.ftp_changes)
    }

    // ---- client ----

    pub(crate) fn conn_open(&mut self, t: &Topology, dev: DeviceId, dst: Ipv4Addr, port: u16) -> Result<Conn, SimError> {
        let d = t.device(dev).ok_or(SimError::NoDevice(dev))?;
        let src = self.source_for(t, d, dst)?;
        let local = self.next_port();
        self.st(dev).tcp_wait.push((local, port));
        let mut c = Conn { dev, src, dst, local, remote: port, seq: 4_000, ack: 0 };
        let mut answer = None;
        // SYN retransmissions: every router on the way may drop the first one while it
        // resolves the next hop's address.
        for _ in 0..4 {
            let syn = Tcp { src_port: local, dst_port: port, seq: c.seq, ack: 0, flags: tcp::SYN, window: 65_535, payload: Vec::new() };
            let _ = self.seg_to(t, d, src, dst, syn);
            answer = self.tcp_wait(t, dev, |s| s.dst_port == local && s.flags & (tcp::SYN | tcp::RST) != 0);
            if answer.is_some() {
                break;
            }
        }
        let a = match answer {
            Some(a) if a.flags & tcp::RST == 0 => a,
            Some(_) => {
                self.conn_forget(&c);
                return Err(SimError::NoSource(format!("{dst} refused the connection (is its FTP service on?)")));
            }
            None => {
                self.conn_forget(&c);
                return Err(SimError::NoSource(format!("{dst} did not answer (connection timed out)")));
            }
        };
        c.seq = c.seq.wrapping_add(1);
        c.ack = a.seq.wrapping_add(1);
        let ack = Tcp { src_port: local, dst_port: port, seq: c.seq, ack: c.ack, flags: tcp::ACK, window: 65_535, payload: Vec::new() };
        let _ = self.seg_to(t, d, src, dst, ack);
        Ok(c)
    }

    pub(crate) fn conn_send(&mut self, t: &Topology, c: &mut Conn, data: &[u8]) {
        let Some(d) = t.device(c.dev) else { return };
        let chunks: Vec<&[u8]> = data.chunks(1460).collect();
        for chunk in chunks {
            let seg = Tcp {
                src_port: c.local,
                dst_port: c.remote,
                seq: c.seq,
                ack: c.ack,
                flags: tcp::PSH | tcp::ACK,
                window: 65_535,
                payload: chunk.to_vec(),
            };
            let _ = self.seg_to(t, d, c.src, c.dst, seg);
            c.seq = c.seq.wrapping_add(chunk.len() as u32);
        }
    }

    /// Reads what arrives on a connection until `done` says the text is complete, the far end
    /// closes, or nothing comes for two seconds. Returns the bytes and whether it closed.
    pub(crate) fn conn_recv(&mut self, t: &Topology, c: &mut Conn, done: impl Fn(&[u8]) -> bool) -> (Vec<u8>, bool) {
        let mut got = Vec::new();
        let local = c.local;
        loop {
            let Some(s) = self.tcp_wait(t, c.dev, |s| s.dst_port == local && (!s.payload.is_empty() || s.flags & tcp::FIN != 0)) else {
                return (got, false);
            };
            got.extend_from_slice(&s.payload);
            let fin = s.flags & tcp::FIN != 0;
            c.ack = s.seq.wrapping_add(s.payload.len() as u32).wrapping_add(u32::from(fin));
            if let Some(d) = t.device(c.dev) {
                let ack = Tcp { src_port: c.local, dst_port: c.remote, seq: c.seq, ack: c.ack, flags: tcp::ACK, window: 65_535, payload: Vec::new() };
                let _ = self.seg_to(t, d, c.src, c.dst, ack);
            }
            if fin || got.len() > MAX_FILE * 2 {
                return (got, fin);
            }
            if done(&got) {
                return (got, false);
            }
        }
    }

    pub(crate) fn conn_close(&mut self, t: &Topology, c: &mut Conn) {
        if let Some(d) = t.device(c.dev) {
            let fin = Tcp {
                src_port: c.local,
                dst_port: c.remote,
                seq: c.seq,
                ack: c.ack,
                flags: tcp::FIN | tcp::ACK,
                window: 65_535,
                payload: Vec::new(),
            };
            let _ = self.seg_to(t, d, c.src, c.dst, fin);
        }
        let now = self.now;
        self.advance(t, now.saturating_add(10_000), |_| false);
        self.conn_forget(c);
    }

    pub(crate) fn conn_forget(&mut self, c: &Conn) {
        let st = self.st(c.dev);
        st.tcp_wait.retain(|w| *w != (c.local, c.remote));
        st.tcp_inbox.retain(|s| s.dst_port != c.local);
    }

    /// One reply line on the control connection.
    fn ftp_line(&mut self, t: &Topology, c: &mut Conn) -> Result<String, SimError> {
        let (b, _) = self.conn_recv(t, c, |b| b.ends_with(b"\r\n"));
        let text = String::from_utf8_lossy(&b).trim().to_string();
        if text.is_empty() {
            return Err(SimError::NoSource("the FTP server stopped answering".into()));
        }
        Ok(text)
    }

    fn ftp_cmd(&mut self, t: &Topology, c: &mut Conn, cmd: &str) -> Result<String, SimError> {
        self.conn_send(t, c, format!("{cmd}\r\n").as_bytes());
        self.ftp_line(t, c)
    }

    /// One FTP operation from `dev` to `server`: log in, transfer, log out. Returns the server's
    /// last reply and the bytes fetched (for Get and List).
    pub fn ftp(
        &mut self,
        t: &Topology,
        dev: DeviceId,
        server: Ipv4Addr,
        user: &str,
        password: &str,
        op: &FtpOp,
    ) -> Result<(String, Vec<u8>), SimError> {
        let fail = |m: String| SimError::NoSource(m);
        let mut ctl = self.conn_open(t, dev, server, FTP_PORT)?;
        let result = (|| -> Result<(String, Vec<u8>), SimError> {
            let banner = self.ftp_line(t, &mut ctl)?;
            if !banner.starts_with("220") {
                return Err(fail(banner));
            }
            self.ftp_cmd(t, &mut ctl, &format!("USER {user}"))?;
            let r = self.ftp_cmd(t, &mut ctl, &format!("PASS {password}"))?;
            if !r.starts_with("230") {
                return Err(fail(r));
            }
            if *op == FtpOp::Login {
                return Ok((r, Vec::new()));
            }
            if let FtpOp::Delete(name) = op {
                let r = self.ftp_cmd(t, &mut ctl, &format!("DELE {name}"))?;
                return if r.starts_with('2') { Ok((r, Vec::new())) } else { Err(fail(r)) };
            }
            self.ftp_cmd(t, &mut ctl, "TYPE I")?;
            let pasv = self.ftp_cmd(t, &mut ctl, "PASV")?;
            let nums: Vec<u16> = pasv
                .split_once('(')
                .and_then(|(_, r)| r.split_once(')'))
                .map(|(x, _)| x.split(',').filter_map(|n| n.trim().parse().ok()).collect())
                .unwrap_or_default();
            let port = match nums.as_slice() {
                [_, _, _, _, hi, lo] => (hi << 8) | lo,
                _ => return Err(fail(format!("unexpected answer to PASV: {pasv}"))),
            };
            let mut data = self.conn_open(t, dev, server, port)?;
            let command = match op {
                FtpOp::Put(name, _) => format!("STOR {name}"),
                FtpOp::Get(name) => format!("RETR {name}"),
                _ => "LIST".to_string(),
            };
            let r = self.ftp_cmd(t, &mut ctl, &command)?;
            if !r.starts_with("150") {
                self.conn_forget(&data);
                return Err(fail(r));
            }
            let mut fetched = Vec::new();
            match op {
                FtpOp::Put(_, bytes) => {
                    self.conn_send(t, &mut data, bytes);
                    self.conn_close(t, &mut data);
                }
                _ => {
                    let (b, _) = self.conn_recv(t, &mut data, |_| false);
                    fetched = b;
                    self.conn_close(t, &mut data);
                }
            }
            let done = self.ftp_line(t, &mut ctl)?;
            Ok((done, fetched))
        })();
        let _ = self.ftp_cmd(t, &mut ctl, "QUIT");
        self.conn_close(t, &mut ctl);
        result
    }
}

/// Applies a client's change to a server's files (the lab owns the files; the simulator only
/// reports what FTP did).
pub fn apply_change(files: &mut Vec<WebFile>, name: &str, content: Option<String>) {
    files.retain(|f| f.name != name);
    if let Some(content) = content
        && files.len() < MAX_ITEMS
    {
        files.push(WebFile { name: name.to_string(), content });
    }
}
