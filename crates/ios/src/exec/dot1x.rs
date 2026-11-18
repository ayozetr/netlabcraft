//! 802.1X on switch ports: `show dot1x`, `show dot1x all`, `show dot1x interface` and
//! `dot1x re-authenticate interface`. The settings (`dot1x system-auth-control`,
//! `authentication port-control`, the guest and auth-fail VLANs) are kept as console settings
//! (`knobs`) and read by the simulator (`netlabcraft_sim::dot1x`).

use std::fmt::Write as _;

use netlabcraft_model::Device;
use netlabcraft_sim::dot1x::{self, Verdict, Via};

use super::{Ctx, R};
use crate::grammar::Id;

fn status_text(v: &Verdict) -> String {
    match v {
        Verdict::Authorized(Via::Forced) => "AUTHORIZED (forced by the port control)".into(),
        Verdict::Authorized(Via::Authenticated) => "AUTHORIZED (the server accepted the login)".into(),
        Verdict::Authorized(Via::GuestVlan(v)) => format!("AUTHORIZED (guest VLAN {v}: nobody answered)"),
        Verdict::Authorized(Via::AuthFailVlan(v)) => format!("AUTHORIZED (auth-fail VLAN {v}: the server refused the login)"),
        Verdict::Unauthorized(why) => format!("UNAUTHORIZED ({why})"),
    }
}

fn port_block(c: &Ctx, d: &Device, port: usize) -> Option<String> {
    let info = c.sim.dot1x_port(d, port)?;
    let p = d.ports.get(port)?;
    let mut s = String::new();
    let _ = writeln!(s, "Port {}", p.name);
    let _ = writeln!(s, "  Role:           authenticator");
    let _ = writeln!(s, "  Port control:   {}", info.control.name());
    let _ = writeln!(s, "  Status:         {}", status_text(&info.verdict));
    let _ = writeln!(s, "  Login:          {}", info.phase);
    if !info.identity.is_empty() {
        let _ = writeln!(s, "  Identity:       {}", info.identity);
    }
    if let Some(m) = info.mac {
        let _ = writeln!(s, "  Host address:   {m}");
    }
    let _ = writeln!(s, "  Access VLAN:    {}", c.sim.dot1x_vlan(d, port).unwrap_or(p.switchport.access_vlan));
    if let Some(v) = dot1x::guest_vlan(d, port) {
        let _ = writeln!(s, "  Guest VLAN:     {v}");
    }
    if let Some(v) = dot1x::auth_fail_vlan(d, port) {
        let _ = writeln!(s, "  Auth-fail VLAN: {v}");
    }
    let _ = writeln!(s, "  EAPOL frames:   {} received, {} sent", info.rx, info.tx);
    Some(s.trim_end().to_string())
}

fn controlled(d: &Device) -> Vec<usize> {
    (0..d.ports.len()).filter(|p| dot1x::control(d, *p).is_some()).collect()
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::ShowDot1x | Id::ShowDot1xAll => {
            let d = c.d()?;
            let mut s = String::new();
            let _ = writeln!(s, "802.1X port access control: {}", if dot1x::system_on(d) { "enabled" } else { "disabled" });
            let _ = writeln!(
                s,
                "Login method: {}",
                if dot1x::method_ready(d) {
                    let server = d.access.radius_servers.first().map(|(a, _)| a.to_string()).unwrap_or_default();
                    format!("RADIUS server {server}")
                } else {
                    "none (needs aaa new-model, radius-server host and aaa authentication dot1x default group radius)".to_string()
                }
            );
            let ports = controlled(d);
            let _ = writeln!(s, "Controlled ports: {}", ports.len());
            if id == Id::ShowDot1xAll {
                for p in ports {
                    if let Some(b) = port_block(c, d, p) {
                        let _ = write!(s, "\n{b}\n");
                    }
                }
            }
            Ok(s.trim_end().to_string())
        }
        Id::ShowDot1xInterface => {
            let r = c.iface_ref(arg(0).trim())?;
            let d = c.d()?;
            port_block(c, d, r.port).ok_or_else(|| "% 802.1X does not control that port (dot1x system-auth-control and authentication port-control)".to_string())
        }
        Id::Dot1xReauth => {
            let r = c.iface_ref(arg(0).trim())?;
            let d = c.d()?;
            if dot1x::control(d, r.port).is_none() {
                return Err("% 802.1X does not control that port".into());
            }
            let dev = c.dev;
            c.sim.dot1x_reauthenticate(dev, r.port);
            Ok("The login starts again the next time the simulation runs.".into())
        }
        _ => Err("% Not a 802.1X command".into()),
    }
}
