//! Small console commands the command sweep found missing: `show terminal`, `show line`,
//! `show cdp interface`, `show mac address-table aging-time`, `show snmp community|host|group|user`,
//! `more` and `pwd`. The aliases (`write terminal`, `configure memory`, ...) reuse the commands they
//! stand for.

use std::fmt::Write as _;

use netlabcraft_model::knobs::Scope;
use netlabcraft_model::{Device, End, Line};
use netlabcraft_sim::port_up;

use super::{Ctx, R};
use crate::grammar::Id;

/// The number a kept setting ends with (`cdp timer 20` is 20).
fn knob_number(d: &Device, key: &str) -> Option<u32> {
    d.knobs.get(Scope::Global, key).and_then(|l| l.split_whitespace().last()).and_then(|w| w.parse().ok())
}

/// How a line asks for a login.
fn login_text(l: &Line) -> &'static str {
    if l.login_local {
        "local"
    } else if l.login {
        "password"
    } else {
        "none"
    }
}

/// Runs one of this module's commands.
pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::ShowTerminal => {
            let n = c.d()?.mgmt.history_size.unwrap_or(10);
            Ok(format!("Terminal length 24, width 80 (output is never paged)\nHistory: the last {n} commands are kept"))
        }
        Id::ShowLine => {
            let a = &c.d()?.access;
            let mut s = format!("{:<9} {:<9} {:<12} {}", "Line", "Login", "Transport in", "Access class");
            for (name, line, console) in [("con 0", &a.console, true), ("vty 0 4", &a.vty, false)] {
                let transport = if console {
                    "-".to_string()
                } else {
                    match &line.transport {
                        None => "all".to_string(),
                        Some(v) if v.is_empty() => "none".to_string(),
                        Some(v) => v.iter().map(|p| p.name()).collect::<Vec<_>>().join(" "),
                    }
                };
                let _ = write!(s, "\n{name:<9} {:<9} {transport:<12} {}", login_text(line), line.access_class.as_deref().unwrap_or("-"));
            }
            // The asynchronous lines of an octal card: one row per line.
            let d = c.d()?;
            let lines = d.octal_lines().min(16);
            if lines > 0 {
                s.push_str("\n\nLine  Type  Speed  State     Transport in  Users      Cabled to");
                for n in 1..=lines {
                    let cabled = c.t.octal_at_line(d.id, n - 1).and_then(|o| c.t.device(o.device)).map(|x| x.name.clone());
                    let owner = c.sim.async_owner(d.id, n).and_then(|o| c.t.device(o)).map(|x| x.name.clone());
                    let state = match (&cabled, &owner) {
                        (None, _) => "no cable",
                        (Some(_), Some(_)) => "in use",
                        (Some(_), None) => "idle",
                    };
                    let transport = match d.access.async_transport.get(&n) {
                        None => "none".to_string(),
                        Some(v) if v.is_empty() => "none".to_string(),
                        Some(v) => v.iter().map(|p| p.name()).collect::<Vec<_>>().join(" "),
                    };
                    let speed = if cabled.is_some() { "9600" } else { "-" };
                    let _ = write!(
                        s,
                        "\n{n:<5} TTY   {speed:<6} {state:<9} {transport:<13} {:<10} {}",
                        owner.as_deref().unwrap_or("-"),
                        cabled.as_deref().unwrap_or("-")
                    );
                }
            }
            Ok(s)
        }
        Id::ShowCdpInterface => {
            let d = c.d()?;
            if d.cdp_off {
                return Ok("Neighbour discovery is off on this device (`cdp run` turns it on).".into());
            }
            let timer = knob_number(d, "cdp timer").unwrap_or(60);
            let hold = knob_number(d, "cdp holdtime").unwrap_or(180);
            let mut rows = Vec::new();
            for (i, p) in d.ports.iter().enumerate().filter(|(_, p)| !p.cdp_off) {
                let up = port_up(c.t, End { device: d.id, port: i });
                let admin = if p.shutdown {
                    "administratively down"
                } else if up {
                    "up"
                } else {
                    "down"
                };
                rows.push(format!(
                    "{} is {admin}, line protocol is {}\n  CDP advertisements every {timer} seconds, held for {hold} seconds",
                    p.name,
                    if up { "up" } else { "down" }
                ));
            }
            Ok(if rows.is_empty() { "CDP is off on every interface.".to_string() } else { rows.join("\n") })
        }
        Id::ShowMacAging => {
            let d = c.d()?;
            let secs = knob_number(d, "mac address-table aging-time").or_else(|| knob_number(d, "mac-address-table aging-time")).unwrap_or(300);
            Ok(format!("Aging time: {secs} seconds (0 never ages an address)\nA learned address that stays silent longer than that is forgotten."))
        }
        Id::More => match arg(0).to_ascii_lowercase().as_str() {
            "system:running-config" => c.d().map(crate::render::running_config),
            "nvram:startup-config" => {
                c.d()?.startup.clone().ok_or_else(|| "% No saved configuration (use `copy running-config startup-config`)".into())
            }
            _ => Err("% Name system:running-config or nvram:startup-config".into()),
        },
        Id::Pwd => Ok("flash:".into()),
        Id::ShowSnmpCommunity => {
            let s = &c.d()?.snmp;
            if s.communities.is_empty() {
                return Ok("No communities are configured.".into());
            }
            Ok(s.communities
                .iter()
                .map(|(n, rw)| format!("Community name: {n}\nCommunity access: {}", if *rw { "read-write" } else { "read-only" }))
                .collect::<Vec<_>>()
                .join("\n\n"))
        }
        Id::ShowSnmpHost => {
            let s = &c.d()?.snmp;
            if s.trap_hosts.is_empty() {
                return Ok("No notification hosts are configured.".into());
            }
            Ok(s.trap_hosts.iter().map(|(h, comm)| format!("Notification host: {h}  version: 2c  community: {comm}")).collect::<Vec<_>>().join("\n"))
        }
        Id::ShowSnmpGroup => {
            let s = &c.d()?.snmp;
            if s.groups.is_empty() {
                return Ok("No SNMPv3 groups are configured.".into());
            }
            Ok(s.groups
                .iter()
                .map(|g| {
                    format!(
                        "Group name: {}  security model: v3 {}  access: {}",
                        g.name,
                        if g.auth { "auth" } else { "noauth" },
                        if g.write { "read-write" } else { "read-only" }
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Id::ShowSnmpUser => {
            let s = &c.d()?.snmp;
            if s.users.is_empty() {
                return Ok("No SNMPv3 users are configured.".into());
            }
            Ok(s.users
                .iter()
                .map(|u| {
                    let auth = u.auth.as_ref().map_or("none".to_string(), |(h, _)| h.to_ascii_uppercase());
                    format!("User name: {}\nGroup: {}\nAuthentication: {auth}", u.name, u.group)
                })
                .collect::<Vec<_>>()
                .join("\n\n"))
        }
        _ => Err("% Not a sweep command".into()),
    }
}
