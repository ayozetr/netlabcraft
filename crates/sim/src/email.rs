//! Email over the simulated TCP: SMTP (RFC 5321) takes a message for one of the server's
//! users, POP3 (RFC 1939) hands a user their messages and deletes them from the server.
//!
//! The server keeps no mailboxes itself: deliveries and deletions are reported (like FTP's
//! changes) for the lab to apply, since the mailboxes are part of the saved lab.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use packetcraft_core::DeviceId;
use packetcraft_model::services::{MAX_MAIL, Mail};
use packetcraft_model::{Device, DeviceKind, Topology};
use packetcraft_wire::tcp::{self, Tcp};
use packetcraft_wire::{Ipv4, POP3_PORT, SMTP_PORT};

use crate::{Sim, SimError};

/// What a mail server changes in its mailboxes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailChange {
    /// A message arrived for a user.
    Deliver(String, Mail),
    /// A user fetched and deleted their first `n` messages.
    Remove(String, usize),
}

/// One SMTP or POP3 connection, server side.
#[derive(Debug, Clone, Default)]
pub(crate) struct Session {
    seq: u32,
    from: String,
    to: Vec<String>,
    /// Inside DATA: the message so far.
    data: Option<String>,
    user: Option<String>,
    logged_in: Option<String>,
    deleted: Vec<usize>,
}

/// A server's mail connections.
#[derive(Debug, Clone, Default)]
pub(crate) struct ServerState {
    sessions: BTreeMap<(Ipv4Addr, u16, u16), Session>,
}

/// The message as sent: headers, a blank line, the body (lines starting with `.` doubled).
pub fn to_text(m: &Mail) -> String {
    let body: Vec<String> = m.body.lines().map(|l| if l.starts_with('.') { format!(".{l}") } else { l.to_string() }).collect();
    format!("From: {}\r\nTo: {}\r\nSubject: {}\r\n\r\n{}", m.from, m.to, m.subject, body.join("\r\n"))
}

/// A message read back from its text.
pub fn from_text(text: &str) -> Mail {
    let (head, body) = text.split_once("\r\n\r\n").or_else(|| text.split_once("\n\n")).unwrap_or((text, ""));
    let field = |name: &str| {
        head.lines()
            .find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case(name)).map(|(_, v)| v.trim().to_string()))
            .unwrap_or_default()
    };
    let body: Vec<&str> = body.lines().map(|l| l.strip_prefix('.').filter(|_| l.starts_with("..")).unwrap_or(l)).collect();
    Mail { from: field("From"), to: field("To"), subject: field("Subject"), body: body.join("\n") }
}

/// The user part of an address, if the address is for `domain`.
fn local_user(address: &str, domain: &str) -> Option<String> {
    let a = address.trim().trim_start_matches('<').trim_end_matches('>');
    let (user, dom) = a.split_once('@')?;
    dom.eq_ignore_ascii_case(domain.trim()).then(|| user.to_string())
}

impl Sim {
    /// TCP for the mail server: `None` when the segment isn't SMTP's or POP3's.
    pub(crate) fn mail_server_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, seg: &Tcp) -> Option<Result<(), String>> {
        if d.kind != DeviceKind::Server || !(seg.dst_port == SMTP_PORT || seg.dst_port == POP3_PORT) {
            return None;
        }
        Some(self.mail_in(t, d, ip, seg))
    }

    fn mail_reply(&mut self, t: &Topology, d: &Device, ip: &Ipv4, seg: &Tcp, text: &str, fin: bool) -> Result<(), String> {
        let key = (ip.src, seg.src_port, seg.dst_port);
        let ack = seg.seq.wrapping_add(seg.payload.len() as u32).wrapping_add(u32::from(seg.flags & (tcp::SYN | tcp::FIN) != 0));
        let st = self.st(d.id);
        let Some(s) = st.mail.sessions.get_mut(&key) else { return Ok(()) };
        let payload = format!("{text}\r\n").into_bytes();
        let seq = s.seq;
        s.seq = s.seq.wrapping_add(payload.len() as u32).wrapping_add(u32::from(fin));
        let flags = tcp::PSH | tcp::ACK | if fin { tcp::FIN } else { 0 };
        self.seg_to(t, d, ip.dst, ip.src, Tcp { src_port: seg.dst_port, dst_port: seg.src_port, seq, ack, flags, window: 65_535, payload })
    }

    fn mail_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, seg: &Tcp) -> Result<(), String> {
        let key = (ip.src, seg.src_port, seg.dst_port);
        let smtp = seg.dst_port == SMTP_PORT;
        let email = &d.services.email;
        if !email.on {
            let rst = Tcp {
                src_port: seg.dst_port,
                dst_port: seg.src_port,
                seq: 0,
                ack: seg.seq.wrapping_add(1),
                flags: tcp::RST | tcp::ACK,
                window: 0,
                payload: Vec::new(),
            };
            self.seg_to(t, d, ip.dst, ip.src, rst)?;
            return Err("the email service is off".into());
        }
        if seg.flags & tcp::SYN != 0 {
            let isn = 80_000u32;
            let st = self.st(d.id);
            if st.mail.sessions.len() >= 64 {
                st.mail.sessions.clear();
            }
            st.mail.sessions.insert(key, Session { seq: isn.wrapping_add(1), ..Session::default() });
            let synack = Tcp {
                src_port: seg.dst_port,
                dst_port: seg.src_port,
                seq: isn,
                ack: seg.seq.wrapping_add(1),
                flags: tcp::SYN | tcp::ACK,
                window: 65_535,
                payload: Vec::new(),
            };
            self.seg_to(t, d, ip.dst, ip.src, synack)?;
            let hello =
                if smtp { format!("220 {} PacketCraft mail service ready", email.domain) } else { "+OK PacketCraft POP3 service ready".into() };
            // The greeting follows the handshake.
            let fake = Tcp { flags: tcp::ACK, payload: Vec::new(), ..seg.clone() };
            return self.mail_reply(t, d, ip, &Tcp { seq: seg.seq, ..fake }, &hello, false);
        }
        if seg.flags & tcp::FIN != 0 && seg.payload.is_empty() {
            let s = self.st(d.id).mail.sessions.remove(&key);
            let ack = Tcp {
                src_port: seg.dst_port,
                dst_port: seg.src_port,
                seq: s.map_or(0, |s| s.seq),
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
        let Some(mut s) = self.state.get(&d.id).and_then(|st| st.mail.sessions.get(&key)).cloned() else { return Err("no such mail session".into()) };
        let text = String::from_utf8_lossy(&seg.payload).to_string();
        let (reply, close) = if smtp { self.smtp_line(d, &mut s, &text) } else { self.pop3_line(d, &mut s, &text) };
        if let Some(x) = self.st(d.id).mail.sessions.get_mut(&key) {
            let seq = x.seq;
            *x = Session { seq, ..s };
        }
        match reply {
            Some(r) => self.mail_reply(t, d, ip, seg, &r, close),
            None => {
                let ack = Tcp {
                    src_port: seg.dst_port,
                    dst_port: seg.src_port,
                    seq: self.state.get(&d.id).and_then(|st| st.mail.sessions.get(&key)).map_or(0, |x| x.seq),
                    ack: seg.seq.wrapping_add(seg.payload.len() as u32),
                    flags: tcp::ACK,
                    window: 65_535,
                    payload: Vec::new(),
                };
                self.seg_to(t, d, ip.dst, ip.src, ack)
            }
        }
    }

    /// An SMTP command (or a piece of a message): the reply, and whether the server closes.
    fn smtp_line(&mut self, d: &Device, s: &mut Session, text: &str) -> (Option<String>, bool) {
        let email = &d.services.email;
        if let Some(data) = &mut s.data {
            if data.len() < MAX_MAIL * 2 {
                data.push_str(text);
            }
            if !(data.ends_with("\r\n.\r\n") || data == ".\r\n") {
                return (None, false);
            }
            let body = data.trim_end_matches("\r\n.\r\n").to_string();
            let mut mail = crate::email::from_text(&body);
            mail.body = mail.body.chars().take(MAX_MAIL).collect();
            if mail.from.is_empty() {
                mail.from.clone_from(&s.from);
            }
            for to in &s.to {
                if let Some(user) = local_user(to, &email.domain) {
                    self.mail_changes.push((d.id, MailChange::Deliver(user, Mail { to: to.clone(), ..mail.clone() })));
                }
            }
            s.data = None;
            s.to.clear();
            return (Some("250 Message accepted for delivery".into()), false);
        }
        let line = text.trim();
        let upper = line.to_ascii_uppercase();
        let arg = |prefix: &str| line.get(prefix.len()..).unwrap_or("").trim().trim_start_matches('<').trim_end_matches('>').to_string();
        let reply = if upper.starts_with("HELO") || upper.starts_with("EHLO") {
            format!("250 {} greets you", email.domain)
        } else if upper.starts_with("MAIL FROM:") {
            s.from = arg("MAIL FROM:");
            "250 OK".into()
        } else if upper.starts_with("RCPT TO:") {
            let to = arg("RCPT TO:");
            match local_user(&to, &email.domain) {
                Some(u) if email.users.iter().any(|x| x.name.eq_ignore_ascii_case(&u)) => {
                    s.to.push(to);
                    "250 OK".into()
                }
                Some(_) => format!("550 {to}: no such user here"),
                None => format!("550 {to}: this server only takes mail for {}", email.domain),
            }
        } else if upper == "DATA" {
            if s.to.is_empty() {
                "503 Give a recipient first (RCPT TO)".into()
            } else {
                s.data = Some(String::new());
                "354 End the message with a line holding only a dot".into()
            }
        } else if upper == "QUIT" {
            return (Some("221 Bye".into()), true);
        } else {
            "502 Command not implemented".into()
        };
        (Some(reply), false)
    }

    /// A POP3 command: the reply, and whether the server closes.
    fn pop3_line(&mut self, d: &Device, s: &mut Session, text: &str) -> (Option<String>, bool) {
        let email = &d.services.email;
        let line = text.trim();
        let (cmd, arg) = line.split_once(' ').map_or((line, ""), |(c, a)| (c, a.trim()));
        let cmd = cmd.to_ascii_uppercase();
        let mine = |user: &str| -> Vec<&Mail> { email.mailboxes.iter().filter(|(u, _)| u.eq_ignore_ascii_case(user)).map(|(_, m)| m).collect() };
        let reply = match (cmd.as_str(), s.logged_in.clone()) {
            ("USER", _) => {
                s.user = Some(arg.to_string());
                "+OK send the password".into()
            }
            ("PASS", _) => {
                let user = s.user.clone().unwrap_or_default();
                if email.users.iter().any(|u| u.name.eq_ignore_ascii_case(&user) && u.password == arg) {
                    s.logged_in = Some(user);
                    "+OK logged in".into()
                } else {
                    "-ERR wrong user name or password".into()
                }
            }
            ("QUIT", Some(user)) => {
                if !s.deleted.is_empty() {
                    self.mail_changes.push((d.id, MailChange::Remove(user, s.deleted.len())));
                }
                return (Some("+OK bye".into()), true);
            }
            ("QUIT", None) => return (Some("+OK bye".into()), true),
            (_, None) => "-ERR log in first".into(),
            ("STAT", Some(user)) => {
                let m = mine(&user);
                format!("+OK {} {}", m.len(), m.iter().map(|x| crate::email::to_text(x).len()).sum::<usize>())
            }
            ("RETR", Some(user)) => match arg.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).and_then(|i| mine(&user).get(i).copied().cloned())
            {
                Some(m) => {
                    let text = crate::email::to_text(&m);
                    format!("+OK {} octets\r\n{text}\r\n.", text.len())
                }
                None => "-ERR no such message".into(),
            },
            ("DELE", Some(user)) => match arg.parse::<usize>().ok().filter(|n| *n >= 1 && *n <= mine(&user).len()) {
                Some(n) => {
                    if !s.deleted.contains(&n) {
                        s.deleted.push(n);
                    }
                    "+OK deleted".into()
                }
                None => "-ERR no such message".into(),
            },
            _ => "-ERR command not implemented".into(),
        };
        (Some(reply), false)
    }

    /// Mailbox changes servers made, for the lab to apply.
    pub fn take_mail_changes(&mut self) -> Vec<(DeviceId, MailChange)> {
        std::mem::take(&mut self.mail_changes)
    }

    fn mail_line(&mut self, t: &Topology, c: &mut crate::ftp::Conn) -> Result<String, SimError> {
        let (b, _) = self.conn_recv(t, c, |b| b.ends_with(b"\r\n"));
        let text = String::from_utf8_lossy(&b).trim_end().to_string();
        if text.is_empty() {
            return Err(SimError::NoSource("the mail server stopped answering".into()));
        }
        Ok(text)
    }

    fn mail_cmd(&mut self, t: &Topology, c: &mut crate::ftp::Conn, cmd: &str) -> Result<String, SimError> {
        self.conn_send(t, c, format!("{cmd}\r\n").as_bytes());
        self.mail_line(t, c)
    }

    /// Sends a message through an SMTP server. Returns the server's answer to the message.
    pub fn smtp_send(&mut self, t: &Topology, dev: DeviceId, server: Ipv4Addr, mail: &Mail) -> Result<String, SimError> {
        let fail = |m: String| SimError::NoSource(m);
        let mut c = self.conn_open(t, dev, server, SMTP_PORT)?;
        let result = (|| -> Result<String, SimError> {
            let hello = self.mail_line(t, &mut c)?;
            if !hello.starts_with("220") {
                return Err(fail(hello));
            }
            let me = t.device(dev).map_or("host".to_string(), |d| d.name.clone());
            self.mail_cmd(t, &mut c, &format!("HELO {me}"))?;
            self.mail_cmd(t, &mut c, &format!("MAIL FROM:<{}>", mail.from))?;
            let r = self.mail_cmd(t, &mut c, &format!("RCPT TO:<{}>", mail.to))?;
            if !r.starts_with("250") {
                return Err(fail(r));
            }
            let r = self.mail_cmd(t, &mut c, "DATA")?;
            if !r.starts_with("354") {
                return Err(fail(r));
            }
            self.conn_send(t, &mut c, format!("{}\r\n.\r\n", to_text(mail)).as_bytes());
            let done = self.mail_line(t, &mut c)?;
            if !done.starts_with("250") {
                return Err(fail(done));
            }
            Ok(done)
        })();
        let _ = self.mail_cmd(t, &mut c, "QUIT");
        self.conn_close(t, &mut c);
        result
    }

    /// Fetches a user's messages over POP3 and deletes them from the server.
    pub fn pop3_fetch(&mut self, t: &Topology, dev: DeviceId, server: Ipv4Addr, user: &str, password: &str) -> Result<Vec<Mail>, SimError> {
        let fail = |m: String| SimError::NoSource(m);
        let mut c = self.conn_open(t, dev, server, POP3_PORT)?;
        let result = (|| -> Result<Vec<Mail>, SimError> {
            let hello = self.mail_line(t, &mut c)?;
            if !hello.starts_with("+OK") {
                return Err(fail(hello));
            }
            self.mail_cmd(t, &mut c, &format!("USER {user}"))?;
            let r = self.mail_cmd(t, &mut c, &format!("PASS {password}"))?;
            if !r.starts_with("+OK") {
                return Err(fail(r));
            }
            let stat = self.mail_cmd(t, &mut c, "STAT")?;
            let n: usize = stat.split_whitespace().nth(1).and_then(|x| x.parse().ok()).unwrap_or(0);
            let mut out = Vec::new();
            for i in 1..=n.min(200) {
                self.conn_send(t, &mut c, format!("RETR {i}\r\n").as_bytes());
                let (b, _) = self.conn_recv(t, &mut c, |b| b.ends_with(b"\r\n.\r\n") || b.starts_with(b"-ERR"));
                let text = String::from_utf8_lossy(&b).to_string();
                if let Some((_, msg)) = text.split_once("\r\n") {
                    out.push(from_text(msg.trim_end_matches("\r\n.\r\n")));
                }
                self.mail_cmd(t, &mut c, &format!("DELE {i}"))?;
            }
            Ok(out)
        })();
        let _ = self.mail_cmd(t, &mut c, "QUIT");
        self.conn_close(t, &mut c);
        result
    }
}
