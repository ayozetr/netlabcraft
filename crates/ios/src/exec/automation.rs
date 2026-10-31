//! Automation settings: the web server (`ip http server`, `ip http secure-server`,
//! `ip http authentication local`), RESTCONF (`restconf`) and NETCONF (`netconf-yang`), and the
//! commands that show them. The protocols themselves are served by the simulator and the
//! engine (`netlabcraft_sim::automation`).

use std::fmt::Write as _;

use netlabcraft_model::Device;

use super::{Ctx, R};
use crate::grammar::Id;

/// The configuration lines of these settings, for `show running-config`.
pub fn running_config(s: &mut String, d: &Device) {
    let a = &d.automation;
    let before = s.len();
    if a.http {
        let _ = writeln!(s, "ip http server");
    }
    if a.https {
        let _ = writeln!(s, "ip http secure-server");
    }
    if a.http_auth_local {
        let _ = writeln!(s, "ip http authentication local");
    }
    if a.restconf {
        let _ = writeln!(s, "restconf");
    }
    if a.netconf {
        let _ = writeln!(s, "netconf-yang");
    }
    if s.len() != before {
        let _ = writeln!(s, "!");
    }
}

/// Who may log in to RESTCONF and NETCONF: local users with privilege 15.
pub fn admins(d: &Device) -> Vec<String> {
    d.access.users.iter().filter(|u| u.privilege >= 15).map(|u| u.name.clone()).collect()
}

fn status(d: &Device) -> String {
    let a = &d.automation;
    let on = |b: bool| if b { "on" } else { "off" };
    let mut s = String::new();
    let _ = writeln!(s, "Web server, port 80 (ip http server) .......... {}", on(a.http));
    let _ = writeln!(s, "Web server, port 443 (ip http secure-server) .. {} (TLS 1.2, anonymous DH, no certificate)", on(a.https));
    let _ = writeln!(s, "RESTCONF (restconf) ........................... {}", on(a.restconf));
    let _ = writeln!(s, "NETCONF, port 830 (netconf-yang) .............. {}", on(a.netconf));
    let admins = admins(d);
    let _ = writeln!(
        s,
        "Logins: local usernames with privilege 15{}",
        if admins.is_empty() { " (none configured: every request is refused)".to_string() } else { format!(": {}", admins.join(", ")) }
    );
    if a.restconf && !a.http && !a.https {
        let _ = writeln!(s, "Note: RESTCONF answers once the web server runs (ip http secure-server or ip http server).");
    }
    if a.restconf && (a.http || a.https) {
        let base = if a.https { "https" } else { "http" };
        let _ = writeln!(s, "RESTCONF root: {base}://<address>/restconf/data");
    }
    s.trim_end().to_string()
}

pub(super) fn command(c: &mut Ctx<'_>, id: Id, _args: &[String]) -> R {
    match id {
        Id::ShowIpHttpServerStatus => Ok(status(c.d()?)),
        Id::ShowNetconfYang => {
            let d = c.d()?;
            if !d.automation.netconf {
                return Ok("NETCONF is off (`netconf-yang` turns it on).".into());
            }
            let list = c.sim.netconf_sessions(c.dev);
            if list.is_empty() {
                return Ok("No NETCONF sessions are open.".into());
            }
            let mut s = format!("{:<8} {:<22} {:<12} {:<10}\n", "Session", "Peer", "User", "Framing");
            for x in list {
                let _ = writeln!(
                    s,
                    "{:<8} {:<22} {:<12} {:<10}",
                    x.id,
                    format!("{}:{}", x.peer, x.port),
                    x.user,
                    if x.chunked { "chunked" } else { "end-mark" }
                );
            }
            let ds = c.sim.datastores(c.dev);
            for (store, holder) in &ds.locks {
                let _ = writeln!(s, "The {store} datastore is locked by session {holder}.");
            }
            if !ds.candidate.is_empty() {
                let _ = writeln!(s, "The candidate holds {} uncommitted configuration line(s).", ds.candidate.len());
            }
            Ok(s.trim_end().to_string())
        }
        _ => {
            let a = &mut c.dm()?.automation;
            let mut note = String::new();
            match id {
                Id::IpHttpServer | Id::NoIpHttpServer => a.http = id == Id::IpHttpServer,
                Id::IpHttpSecureServer | Id::NoIpHttpSecureServer => a.https = id == Id::IpHttpSecureServer,
                Id::IpHttpAuthLocal | Id::NoIpHttpAuthLocal => a.http_auth_local = id == Id::IpHttpAuthLocal,
                Id::Restconf => {
                    a.restconf = true;
                    if !a.http && !a.https {
                        note = "RESTCONF answers once the web server runs: add `ip http secure-server`.".into();
                    }
                }
                Id::NoRestconf => a.restconf = false,
                Id::NetconfYang | Id::NoNetconfYang => a.netconf = id == Id::NetconfYang,
                _ => return Err("% Not an automation command".into()),
            }
            if matches!(id, Id::Restconf | Id::NetconfYang) && admins(c.d()?).is_empty() {
                if !note.is_empty() {
                    note.push('\n');
                }
                note.push_str("Logins need a local user with privilege 15: `username NAME privilege 15 secret PASSWORD`.");
            }
            Ok(note)
        }
    }
}

#[cfg(test)]
mod tests {
    use netlabcraft_model::{DeviceKind, Topology};
    use netlabcraft_sim::Sim;

    use crate::{Cli, execute, running_config};

    fn type_in(t: &mut Topology, sim: &mut Sim, cli: &mut Cli, dev: netlabcraft_core::DeviceId, lines: &str) -> String {
        let mut last = String::new();
        for l in lines.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let o = execute(t, sim, dev, cli, l);
            assert!(!o.output.starts_with("% "), "`{l}`: {}", o.output);
            last = o.output;
        }
        last
    }

    #[test]
    fn restconf_and_netconf_are_switched_on_and_shown() {
        let mut t = Topology::new();
        let mut sim = Sim::new();
        let r = t.add_device(DeviceKind::Router, Some("R1"), 0.0, 0.0).unwrap();
        let mut cli = Cli::default();
        type_in(&mut t, &mut sim, &mut cli, r, "enable\nconfigure terminal");
        // Without a web server or an admin user, the router says what's missing.
        let note = type_in(&mut t, &mut sim, &mut cli, r, "restconf");
        assert!(note.contains("ip http secure-server") && note.contains("privilege 15"), "{note}");
        type_in(
            &mut t,
            &mut sim,
            &mut cli,
            r,
            "username admin privilege 15 secret Admin2026
             ip http secure-server
             ip http server
             ip http authentication local
             netconf-yang
             end",
        );
        let run = running_config(t.device(r).unwrap());
        for l in ["ip http server", "ip http secure-server", "ip http authentication local", "restconf", "netconf-yang"] {
            assert!(run.lines().any(|x| x == l), "{l} missing:\n{run}");
        }
        let status = type_in(&mut t, &mut sim, &mut cli, r, "show ip http server status");
        assert!(
            status.contains("RESTCONF (restconf) ") && status.contains("admin") && status.contains("https://<address>/restconf/data"),
            "{status}"
        );
        assert!(type_in(&mut t, &mut sim, &mut cli, r, "show netconf-yang sessions").contains("No NETCONF sessions"));
        type_in(
            &mut t,
            &mut sim,
            &mut cli,
            r,
            "configure terminal\nno restconf\nno netconf-yang\nno ip http server\nno ip http secure-server\nno ip http authentication\nend",
        );
        let run = running_config(t.device(r).unwrap());
        assert!(!run.contains("restconf") && !run.contains("ip http") && !run.contains("netconf"), "{run}");
        assert!(type_in(&mut t, &mut sim, &mut cli, r, "show netconf-yang sessions").contains("off"));
        // Switches don't serve it.
        let s = t.add_device(DeviceKind::Switch, Some("S1"), 0.0, 0.0).unwrap();
        let mut scli = Cli::default();
        type_in(&mut t, &mut sim, &mut scli, s, "enable\nconfigure terminal");
        assert!(execute(&mut t, &mut sim, s, &mut scli, "restconf").output.contains("% "));
    }
}
