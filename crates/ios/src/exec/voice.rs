//! Voice commands: a router's call control (`telephony-service`, `ephone-dn`, `ephone`), the
//! DHCP option that tells phones where to register (`option 150 ip`), voice VLANs on switch
//! ports, and the `show ephone` / `show ephone-dn` / `show telephony-service` reports.

use std::fmt::Write as _;

use netlabcraft_core::{MacAddr, VlanId, parse_ip};
use netlabcraft_model::voice::{Ephone, EphoneDn, MAX_DNS, MAX_EPHONES, TelephonyService, valid_number};
use netlabcraft_model::{Device, DeviceKind};

use super::{Ctx, Mode, R};
use crate::grammar::Id;

fn tag(text: &str, what: &str) -> Result<u16, String> {
    text.parse::<u16>().ok().filter(|n| (1..=1000).contains(n)).ok_or_else(|| format!("% The {what} number is 1-1000"))
}

fn service<'a>(c: &'a mut Ctx<'_>) -> Result<&'a mut TelephonyService, String> {
    c.dm()?.voice.service.as_mut().ok_or_else(|| "% Not in telephony-service".to_string())
}

pub(super) fn command(c: &mut Ctx<'_>, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::VoiceTelephonyService => {
            if c.d()?.kind != DeviceKind::Router {
                return Err("% Call control runs on routers".into());
            }
            let d = c.dm()?;
            d.voice.service.get_or_insert_with(TelephonyService::default);
            c.cli.mode = Mode::Telephony;
            Ok(String::new())
        }
        Id::VoiceNoTelephonyService => {
            c.dm()?.voice.service = None;
            Ok(String::new())
        }
        Id::VoiceMaxEphones | Id::VoiceMaxDn => {
            let n: u16 = arg(0).parse().ok().filter(|n| (1..=MAX_EPHONES as u16).contains(n)).ok_or(format!("% Give 1-{MAX_EPHONES}"))?;
            let s = service(c)?;
            if id == Id::VoiceMaxEphones {
                s.max_ephones = n;
            } else {
                s.max_dn = n;
            }
            Ok(String::new())
        }
        Id::VoiceSourceAddress => {
            let a = parse_ip(arg(0)).map_err(|e| format!("% {e}"))?;
            let port = match arg(1) {
                "" => netlabcraft_model::voice::SIGNAL_PORT,
                p => p.parse::<u16>().ok().filter(|p| *p > 0).ok_or("% The port is 1-65535")?,
            };
            let mine = netlabcraft_sim::l3_ifaces(c.d()?).iter().any(|i| i.net.addr == a);
            let s = service(c)?;
            s.source = Some(a);
            s.port = port;
            Ok(if mine { String::new() } else { format!("Note: {a} is not an address of this router yet; phones register there.") })
        }
        Id::VoiceNoSourceAddress => {
            let s = service(c)?;
            s.source = None;
            s.port = netlabcraft_model::voice::SIGNAL_PORT;
            Ok(String::new())
        }
        Id::VoiceAutoAssign => {
            let a = tag(arg(0), "directory")?;
            let b = tag(arg(1), "directory")?;
            if b < a {
                return Err("% The first number must not be above the second".into());
            }
            service(c)?.auto_assign = Some((a, b));
            Ok(String::new())
        }
        Id::VoiceNoAutoAssign => {
            service(c)?.auto_assign = None;
            Ok(String::new())
        }
        Id::VoiceEphoneDn | Id::VoiceEphone => {
            if c.d()?.kind != DeviceKind::Router {
                return Err("% Call control runs on routers".into());
            }
            let dn = id == Id::VoiceEphoneDn;
            let n = tag(arg(0), if dn { "directory" } else { "phone" })?;
            let d = c.dm()?;
            let Some(s) = d.voice.service.as_ref() else {
                return Err("% Set up `telephony-service` first".into());
            };
            let (max, have, exists) = if dn {
                (s.max_dn, d.voice.dns.len(), d.voice.dns.iter().any(|x| x.tag == n))
            } else {
                (s.max_ephones, d.voice.ephones.len(), d.voice.ephones.iter().any(|x| x.tag == n))
            };
            if !exists {
                if have >= usize::from(max) || have >= if dn { MAX_DNS } else { MAX_EPHONES } {
                    let what = if dn { "max-dn" } else { "max-ephones" };
                    return Err(format!("% {what} is {max}: raise it under telephony-service first"));
                }
                if dn {
                    d.voice.dns.push(EphoneDn { tag: n, number: None });
                    d.voice.dns.sort_by_key(|x| x.tag);
                } else {
                    d.voice.ephones.push(Ephone { tag: n, mac: None, buttons: Vec::new() });
                    d.voice.ephones.sort_by_key(|x| x.tag);
                }
            }
            c.cli.mode = if dn { Mode::EphoneDn(n) } else { Mode::Ephone(n) };
            Ok(String::new())
        }
        Id::VoiceNoEphoneDn | Id::VoiceNoEphone => {
            let n = tag(arg(0), "entry")?;
            let d = c.dm()?;
            let before = d.voice.dns.len() + d.voice.ephones.len();
            if id == Id::VoiceNoEphoneDn {
                d.voice.dns.retain(|x| x.tag != n);
            } else {
                d.voice.ephones.retain(|x| x.tag != n);
            }
            if d.voice.dns.len() + d.voice.ephones.len() == before { Err(format!("% No entry {n}")) } else { Ok(String::new()) }
        }
        Id::VoiceNumber | Id::VoiceNoNumber => {
            let Mode::EphoneDn(n) = c.cli.mode else { return Err("% `number` belongs under ephone-dn".into()) };
            let number = if id == Id::VoiceNumber {
                if !valid_number(arg(0)) {
                    return Err("% A directory number is 1-16 digits".into());
                }
                Some(arg(0).to_string())
            } else {
                None
            };
            let d = c.dm()?;
            if number.as_ref().is_some_and(|x| d.voice.dns.iter().any(|o| o.tag != n && o.number.as_ref() == Some(x))) {
                return Err(format!("% {} is already another directory number's", arg(0)));
            }
            let dn = d.voice.dns.iter_mut().find(|x| x.tag == n).ok_or("% The directory number was deleted")?;
            dn.number = number;
            Ok(String::new())
        }
        Id::VoiceMacAddress | Id::VoiceButton | Id::VoiceNoButton => {
            let Mode::Ephone(n) = c.cli.mode else { return Err("% This belongs under ephone".into()) };
            let parsed = match id {
                Id::VoiceMacAddress => Some(arg(0).parse::<MacAddr>().map_err(|_| "% A MAC address: H.H.H (aabb.ccdd.eeff) or aa:bb:cc:dd:ee:ff")?),
                _ => None,
            };
            let button = match id {
                Id::VoiceButton => {
                    let (b, dn) = arg(0).split_once(':').ok_or("% A button is BUTTON:DN, such as 1:1")?;
                    let b: u8 = b.parse().ok().filter(|b| (1..=8).contains(b)).ok_or("% Buttons are 1-8")?;
                    Some((b, tag(dn, "directory")?))
                }
                _ => None,
            };
            let d = c.dm()?;
            if let Some(mac) = parsed
                && d.voice.ephones.iter().any(|e| e.tag != n && e.mac == Some(mac))
            {
                return Err(format!("% {mac} is already another ephone's"));
            }
            if let Some((_, dn)) = button
                && d.voice.dn(dn).is_none()
            {
                return Err(format!("% There is no ephone-dn {dn}"));
            }
            let e = d.voice.ephones.iter_mut().find(|x| x.tag == n).ok_or("% The ephone was deleted")?;
            match id {
                Id::VoiceMacAddress => e.mac = parsed,
                Id::VoiceButton => {
                    if let Some((b, dn)) = button {
                        e.buttons.retain(|(x, _)| *x != b);
                        e.buttons.push((b, dn));
                        e.buttons.sort_unstable();
                    }
                }
                _ => e.buttons.clear(),
            }
            Ok(String::new())
        }
        Id::VoiceDhcpOption150 | Id::VoiceNoDhcpOption150 => {
            let Mode::DhcpPool(pool) = c.cli.mode.clone() else { return Err("% Not in DHCP pool mode".into()) };
            let d = c.dm()?;
            if id == Id::VoiceDhcpOption150 {
                let a = parse_ip(arg(0)).map_err(|e| format!("% {e}"))?;
                d.voice.option150.insert(pool, a);
            } else {
                d.voice.option150.remove(&pool);
            }
            Ok(String::new())
        }
        Id::SwVoiceVlan => {
            let v: VlanId = arg(0).parse().map_err(|e| format!("% {e}"))?;
            let created = {
                let d = c.dm()?;
                let new = !d.vlans.contains_key(&v);
                d.vlans.entry(v).or_insert_with(|| Device::default_vlan_name(v));
                new
            };
            c.each_if(|p, _| {
                p.switchport.voice_vlan = Some(v);
                Ok(())
            })?;
            Ok(if created { format!("VLAN {v} did not exist; created it.") } else { String::new() })
        }
        Id::NoSwVoiceVlan => c.each_if(|p, _| {
            p.switchport.voice_vlan = None;
            Ok(())
        }),
        Id::ShowEphone => show_ephone(c),
        Id::ShowEphoneDn => show_dns(c),
        Id::ShowTelephony => show_service(c),
        _ => Err("% Not a voice command".into()),
    }
}

fn show_service(c: &Ctx<'_>) -> R {
    let d = c.d()?;
    let Some(s) = &d.voice.service else { return Ok("Call control is off (no telephony-service).".into()) };
    let mut out = String::from("Call control (telephony-service)\n");
    let src = s.source.map_or("any address of this router".to_string(), |a| a.to_string());
    let _ = writeln!(out, "  Phones register at:   {src}, UDP port {}", s.port);
    let _ = writeln!(out, "  Phones allowed:       {}", s.max_ephones);
    let _ = writeln!(out, "  Directory numbers:    {} allowed, {} defined", s.max_dn, d.voice.dns.len());
    let auto = s.auto_assign.map_or("off".to_string(), |(a, b)| format!("directory numbers {a} to {b}"));
    let _ = writeln!(out, "  Automatic numbers:    {auto}");
    let st = c.sim.call_agent(d.id);
    let _ = write!(
        out,
        "  Registered phones:    {}\n  Calls up:             {}",
        st.map_or(0, |a| a.registrations.len()),
        st.map_or(0, |a| a.calls.iter().filter(|x| x.connected).count())
    );
    Ok(out)
}

fn show_dns(c: &Ctx<'_>) -> R {
    let d = c.d()?;
    if d.voice.dns.is_empty() {
        return Ok("No directory numbers (ephone-dn).".into());
    }
    let st = c.sim.call_agent(d.id);
    let mut out = format!("{:<6} {:<12} {}\n", "DN", "Number", "State");
    for dn in &d.voice.dns {
        let number = dn.number.clone().unwrap_or_else(|| "-".into());
        let state = match st {
            Some(a) if a.calls.iter().any(|x| x.connected && (x.from == number || x.to == number)) => "in a call",
            Some(a) if a.calls.iter().any(|x| !x.connected && (x.from == number || x.to == number)) => "ringing",
            Some(a) if a.registrations.iter().any(|r| r.number == number) => "idle (registered)",
            _ => "not registered",
        };
        let _ = writeln!(out, "{:<6} {number:<12} {state}", dn.tag);
    }
    Ok(out.trim_end().to_string())
}

fn show_ephone(c: &Ctx<'_>) -> R {
    let d = c.d()?;
    let regs = c.sim.call_agent(d.id).map(|a| a.registrations.clone()).unwrap_or_default();
    if d.voice.ephones.is_empty() && regs.is_empty() {
        return Ok("No phones configured or registered.".into());
    }
    let mut out = format!("{:<7} {:<18} {:<16} {:<10} {}\n", "Ephone", "MAC address", "Address", "Number", "State");
    for e in &d.voice.ephones {
        let mac = e.mac.map_or("-".to_string(), |m| m.to_string());
        let reg = regs.iter().find(|r| Some(r.mac) == e.mac);
        let number = d.voice.number_for(e.mac.unwrap_or_default()).map(|(_, n)| n).unwrap_or_else(|| "-".into());
        let (addr, state) = reg.map_or(("-".to_string(), "not registered"), |r| (r.ip.to_string(), "registered"));
        let _ = writeln!(out, "{:<7} {mac:<18} {addr:<16} {number:<10} {state}", e.tag);
    }
    for r in regs.iter().filter(|r| r.ephone == 0) {
        let _ = writeln!(out, "{:<7} {:<18} {:<16} {:<10} registered (auto assign)", "auto", r.mac.to_string(), r.ip.to_string(), r.number);
    }
    Ok(out.trim_end().to_string())
}

/// The voice lines of the running configuration (after the DHCP pools).
pub(crate) fn running_config(s: &mut String, d: &Device) {
    let v = &d.voice;
    if let Some(t) = &v.service {
        let _ = writeln!(s, "telephony-service");
        if t.max_ephones > 0 {
            let _ = writeln!(s, " max-ephones {}", t.max_ephones);
        }
        if t.max_dn > 0 {
            let _ = writeln!(s, " max-dn {}", t.max_dn);
        }
        if let Some(a) = t.source {
            let _ = writeln!(s, " ip source-address {a} port {}", t.port);
        }
        if let Some((a, b)) = t.auto_assign {
            let _ = writeln!(s, " auto assign {a} to {b}");
        }
        let _ = writeln!(s, "!");
    }
    for dn in &v.dns {
        let _ = writeln!(s, "ephone-dn {}", dn.tag);
        if let Some(n) = &dn.number {
            let _ = writeln!(s, " number {n}");
        }
        let _ = writeln!(s, "!");
    }
    for e in &v.ephones {
        let _ = writeln!(s, "ephone {}", e.tag);
        if let Some(m) = e.mac {
            let b = m.0;
            let _ = writeln!(s, " mac-address {:02x}{:02x}.{:02x}{:02x}.{:02x}{:02x}", b[0], b[1], b[2], b[3], b[4], b[5]);
        }
        for (b, dn) in &e.buttons {
            let _ = writeln!(s, " button {b}:{dn}");
        }
        let _ = writeln!(s, "!");
    }
}

/// `option 150 ip` under a DHCP pool, for the running configuration.
pub(crate) fn pool_lines(s: &mut String, d: &Device, pool: &str) {
    if let Some(a) = d.voice.option150.get(pool) {
        let _ = writeln!(s, " option 150 ip {a}");
    }
}
