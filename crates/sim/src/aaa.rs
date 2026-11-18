//! AAA beyond the login: what a TACACS+ server grants a user (`aaa authorization exec`: the
//! privilege level; `aaa authorization commands N`: which commands), and the accounting records
//! (`aaa accounting exec|commands|connection`) a device sends to TACACS+ (RFC 8907 §7) or RADIUS
//! (RFC 2866) servers and a Server keeps for people to read.
//!
//! The Server's side is here too: it answers authorization from the user's profile
//! ([`netlabcraft_model::services::AaaProfile`]) and keeps the records it receives (the newest
//! [`MAX_RECORDS`]; they are not saved with the lab, as a syslog server's messages are not).

use std::net::Ipv4Addr;

use netlabcraft_core::DeviceId;
use netlabcraft_model::{Device, DeviceKind, Topology};
use netlabcraft_wire::radius::{ACCOUNTING_REQUEST, ACCT_PORT, ACCT_START, ACCT_STOP, Radius};
use netlabcraft_wire::tacacs::{ACCT_FLAG_START, ACCT_FLAG_STOP, ACCT_SUCCESS, AUTHOR_FAIL, AUTHOR_PASS_ADD, TACACS_PORT, Tacacs};
use netlabcraft_wire::{Ipv4, Udp};

use crate::Sim;

/// Records a Server keeps.
pub const MAX_RECORDS: usize = 500;
const WAIT_US: u64 = 2_000_000;

/// One accounting record as the Server stored it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcctRecord {
    pub at_us: u64,
    /// The device that sent it.
    pub from: Ipv4Addr,
    /// `TACACS+` or `RADIUS`.
    pub protocol: &'static str,
    pub user: String,
    /// `start`, `stop` or `update`.
    pub kind: String,
    /// What it is about: `exec`, `connection to 10.0.0.1`, `command: show version`.
    pub detail: String,
}

/// What a device counted of its own accounting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AcctStats {
    pub sent: u32,
    pub acknowledged: u32,
}

/// The text of a command from the `cmd=` and `cmd-arg=` arguments of an authorization request.
fn command_of(args: &[String]) -> Option<String> {
    let first = args.iter().find_map(|a| a.strip_prefix("cmd="))?;
    let mut words = vec![first.to_string()];
    words.extend(args.iter().filter_map(|a| a.strip_prefix("cmd-arg=")).map(str::to_string));
    Some(words.join(" ").trim().to_string())
}

impl Sim {
    fn aaa_record(&mut self, d: DeviceId, r: AcctRecord) {
        let st = self.st(d);
        if st.acct.len() >= MAX_RECORDS {
            st.acct.pop_front();
        }
        st.acct.push_back(r);
    }

    /// The accounting records a Server holds, oldest first.
    pub fn aaa_records(&self, d: DeviceId) -> Vec<AcctRecord> {
        self.state.get(&d).map(|s| s.acct.iter().cloned().collect()).unwrap_or_default()
    }

    /// Forgets the records of a Server.
    pub fn clear_aaa_records(&mut self, d: DeviceId) {
        if let Some(s) = self.state.get_mut(&d) {
            s.acct.clear();
        }
    }

    /// What a device counted of the accounting records it sent.
    pub fn aaa_stats(&self, d: DeviceId) -> AcctStats {
        self.state.get(&d).map(|s| s.acct_stats).unwrap_or_default()
    }

    /// The Server's answer to a TACACS+ request other than a login (which the caller handles):
    /// the privilege level or the verdict on a command, or the receipt of an accounting record.
    pub(crate) fn tacacs_answer(&mut self, d: &Device, from: Ipv4Addr, pkt: Tacacs) -> Option<Tacacs> {
        match pkt {
            Tacacs::AuthorRequest { user, args, .. } => {
                if !d.services.aaa.users.iter().any(|u| u.name == user) {
                    return Some(Tacacs::AuthorReply { status: AUTHOR_FAIL, args: Vec::new() });
                }
                let profile = d.services.aaa.profiles.iter().find(|p| p.user == user);
                if let Some(cmd) = command_of(&args) {
                    let denied = profile.is_some_and(|p| p.deny.iter().any(|x| cmd == *x || cmd.starts_with(&format!("{x} "))));
                    return Some(if denied {
                        Tacacs::AuthorReply { status: AUTHOR_FAIL, args: Vec::new() }
                    } else {
                        Tacacs::AuthorReply { status: AUTHOR_PASS_ADD, args: Vec::new() }
                    });
                }
                let level = profile.map_or(1, |p| p.privilege.min(15));
                Some(Tacacs::AuthorReply { status: AUTHOR_PASS_ADD, args: vec![format!("priv-lvl={level}")] })
            }
            Tacacs::AcctRequest { flags, user, args, .. } => {
                let kind = if flags & ACCT_FLAG_START != 0 {
                    "start"
                } else if flags & ACCT_FLAG_STOP != 0 {
                    "stop"
                } else {
                    "update"
                };
                let detail = args.iter().find_map(|a| a.strip_prefix("detail=")).unwrap_or_default().to_string();
                let at_us = self.now;
                self.aaa_record(d.id, AcctRecord { at_us, from, protocol: "TACACS+", user, kind: kind.into(), detail });
                Some(Tacacs::AcctReply { status: ACCT_SUCCESS })
            }
            _ => None,
        }
    }

    /// An Accounting-Request at a Server (UDP 1813).
    pub(crate) fn radius_acct_in(&mut self, t: &Topology, d: &Device, ip: &Ipv4, u: &Udp) -> Result<(), String> {
        if d.kind != DeviceKind::Server || !d.services.aaa.on {
            return Err("no AAA service here".into());
        }
        let req = Radius::decode(&u.payload).map_err(|e| e.to_string())?;
        if req.code != ACCOUNTING_REQUEST {
            return Err("not an Accounting-Request".into());
        }
        let client = d
            .services
            .aaa
            .clients
            .iter()
            .find(|c| c.address == ip.src)
            .ok_or_else(|| format!("{} isn't one of this AAA server's clients", ip.src))?;
        if !req.accounting_valid(&client.secret) {
            return Err("an Accounting-Request signed with another secret: discarded".into());
        }
        let (status, _session, detail) = req.accounting_fields().ok_or("an Accounting-Request without Acct-Status-Type")?;
        let kind = match status {
            ACCT_START => "start",
            ACCT_STOP => "stop",
            _ => "update",
        };
        let at_us = self.now;
        self.aaa_record(
            d.id,
            AcctRecord { at_us, from: ip.src, protocol: "RADIUS", user: req.user().unwrap_or_default(), kind: kind.into(), detail },
        );
        let answer = Radius::accounting_response(&req, &client.secret);
        self.udp_out(t, d, ip.dst, ip.src, ACCT_PORT, u.src_port, answer.encode())
    }

    /// One request to a TACACS+ server and its answer.
    fn tacacs_ask(&mut self, t: &Topology, dev: DeviceId, server: Ipv4Addr, key: &str, req: &Tacacs) -> Option<Tacacs> {
        let mut c = self.conn_open(t, dev, server, TACACS_PORT).ok()?;
        self.seq += 1;
        let session = u32::try_from(self.seq % u64::from(u32::MAX)).unwrap_or(1) ^ 0x3c3c_0000;
        self.conn_send(t, &mut c, &req.encode(session, 1, key));
        let (got, _) = self.conn_recv(t, &mut c, |b| b.len() >= 12);
        self.conn_close(t, &mut c);
        match Tacacs::decode(&got, key) {
            Ok((p, s, _)) if s == session => Some(p),
            _ => None,
        }
    }

    /// The privilege level a TACACS+ server grants a user at login (`aaa authorization exec`).
    /// The outer `None`: no server answered. The inner one: the server refused.
    pub fn tacacs_authorize_exec(&mut self, t: &Topology, dev: DeviceId, user: &str) -> Option<Option<u8>> {
        let d = t.device(dev)?.clone();
        for (server, key) in d.access.tacacs_servers.clone() {
            let req = Tacacs::AuthorRequest { user: user.into(), privilege: 1, args: vec!["service=shell".into(), "cmd*".into()] };
            if let Some(Tacacs::AuthorReply { status, args }) = self.tacacs_ask(t, dev, server, &key, &req) {
                if status != AUTHOR_PASS_ADD {
                    return Some(None);
                }
                let level = args.iter().find_map(|a| a.strip_prefix("priv-lvl=")).and_then(|n| n.parse::<u8>().ok()).unwrap_or(1);
                return Some(Some(level.min(15)));
            }
        }
        None
    }

    /// Whether a TACACS+ server lets a user run a command (`aaa authorization commands N`);
    /// `None` when no server answered.
    pub fn tacacs_authorize_command(&mut self, t: &Topology, dev: DeviceId, user: &str, level: u8, words: &[&str]) -> Option<bool> {
        let d = t.device(dev)?.clone();
        for (server, key) in d.access.tacacs_servers.clone() {
            let mut args = vec!["service=shell".to_string(), format!("cmd={}", words.first().copied().unwrap_or(""))];
            args.extend(words.iter().skip(1).take(24).map(|w| format!("cmd-arg={w}")));
            let req = Tacacs::AuthorRequest { user: user.into(), privilege: level, args };
            if let Some(Tacacs::AuthorReply { status, .. }) = self.tacacs_ask(t, dev, server, &key, &req) {
                return Some(status == AUTHOR_PASS_ADD);
            }
        }
        None
    }

    /// Sends one accounting record (`start`: the beginning of a task, else its end or a command)
    /// to the device's servers of the protocols asked for. Whether a server acknowledged it.
    #[allow(clippy::too_many_arguments)]
    pub fn aaa_account(
        &mut self,
        t: &Topology,
        dev: DeviceId,
        tacacs: bool,
        radius: bool,
        user: &str,
        start: bool,
        privilege: u8,
        detail: &str,
    ) -> bool {
        let Some(d) = t.device(dev).cloned() else { return false };
        self.seq += 1;
        let task = self.seq;
        let mut acked = false;
        if tacacs {
            for (server, key) in d.access.tacacs_servers.clone() {
                let req = Tacacs::AcctRequest {
                    flags: if start { ACCT_FLAG_START } else { ACCT_FLAG_STOP },
                    user: user.into(),
                    privilege,
                    args: vec![format!("task_id={task}"), "service=shell".into(), format!("detail={detail}")],
                };
                {
                    let a = &mut self.st(dev).acct_stats;
                    a.sent = a.sent.saturating_add(1);
                }
                if let Some(Tacacs::AcctReply { status }) = self.tacacs_ask(t, dev, server, &key, &req) {
                    if status == ACCT_SUCCESS {
                        {
                            let a = &mut self.st(dev).acct_stats;
                            a.acknowledged = a.acknowledged.saturating_add(1);
                        }
                        acked = true;
                    }
                    break;
                }
            }
        }
        if radius {
            for (server, key) in d.access.radius_servers.clone() {
                let Ok(src) = self.source_for(t, &d, server) else { continue };
                let port = self.next_port();
                let id = (task % 256) as u8;
                let req = Radius::accounting_request(id, user, if start { ACCT_START } else { ACCT_STOP }, &format!("{task:08x}"), detail, &key, src);
                let st = self.st(dev);
                st.radius_port = Some(port);
                st.radius_inbox.clear();
                st.acct_stats.sent = st.acct_stats.sent.saturating_add(1);
                for _ in 0..3 {
                    let begin = self.now;
                    if self.udp_out(t, &d, src, server, port, ACCT_PORT, req.encode()).is_err() {
                        self.advance(t, begin.saturating_add(100_000), |_| false);
                        continue;
                    }
                    self.advance(t, begin.saturating_add(WAIT_US), |s| {
                        s.state.get(&dev).is_some_and(|st| st.radius_inbox.iter().any(|r| r.id == id))
                    });
                    if self.state.get(&dev).is_some_and(|st| st.radius_inbox.iter().any(|r| r.id == id)) {
                        break;
                    }
                }
                let st = self.st(dev);
                st.radius_port = None;
                let answer = st.radius_inbox.iter().find(|r| r.id == id).cloned();
                if answer.is_some_and(|a| a.verifies(&req.authenticator, &key)) {
                    {
                        let a = &mut self.st(dev).acct_stats;
                        a.acknowledged = a.acknowledged.saturating_add(1);
                    }
                    acked = true;
                    break;
                }
            }
        }
        acked
    }
}
