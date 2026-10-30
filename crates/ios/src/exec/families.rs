//! Device-family commands: power over Ethernet on PoE switches (`power inline`,
//! `show power inline`) and SIP call control on routers (`voice register global`,
//! `voice register dn`, `voice register pool`, `show voice register all`).

use std::fmt::Write as _;

use netlabcraft_core::{MacAddr, parse_ip};
use netlabcraft_model::poe::{self, PortPower};
use netlabcraft_model::voice::{EphoneDn, MAX_DNS, MAX_EPHONES, RegisterPool, SIP_PORT, VoiceRegister, valid_number};
use netlabcraft_model::{Device, DeviceKind, ifname};

use super::{Ctx, Mode, R};
use crate::grammar::Id;

pub(super) fn command(c: &mut Ctx<'_>, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::PowerInline => {
            let never = match arg(0).to_ascii_lowercase().as_str() {
                "auto" => false,
                "never" => true,
                _ => return Err("% Give `auto` (power a device that asks for it) or `never`".into()),
            };
            let d = c.d()?;
            if poe::budget_mw(d.catalogue_model()).is_none() {
                return Err(format!("% A {} gives no power over Ethernet; use a PoE switch", d.catalogue_model().label));
            }
            c.each_if(|p, sub| {
                if sub.is_some() {
                    return Err("% Power goes on the physical port".into());
                }
                p.poe_never = never;
                Ok(())
            })
        }
        Id::ShowPowerInline => show_power_inline(c),
        _ => voice_register(c, id, args),
    }
}

/// Whether the session is in one of the `voice register` sub-modes.
pub(crate) fn voice_register_mode(m: &Mode) -> bool {
    matches!(m, Mode::VoiceRegGlobal | Mode::VoiceRegDn(_) | Mode::VoiceRegPool(_))
}

fn tag(text: &str, what: &str) -> Result<u16, String> {
    text.parse::<u16>().ok().filter(|n| (1..=1000).contains(n)).ok_or_else(|| format!("% The {what} number is 1-1000"))
}

fn register<'a>(c: &'a mut Ctx<'_>) -> Result<&'a mut VoiceRegister, String> {
    if c.cli.mode != Mode::VoiceRegGlobal {
        return Err("% This belongs under `voice register global`".into());
    }
    c.dm()?.voice.register.as_mut().ok_or_else(|| "% Not in voice register global".to_string())
}

fn voice_register(c: &mut Ctx<'_>, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::VoiceRegGlobal => {
            if c.d()?.kind != DeviceKind::Router {
                return Err("% Call control runs on routers".into());
            }
            c.dm()?.voice.register.get_or_insert_with(VoiceRegister::default);
            c.cli.mode = Mode::VoiceRegGlobal;
            Ok(String::new())
        }
        Id::VoiceNoRegGlobal => {
            c.dm()?.voice.register = None;
            Ok(String::new())
        }
        Id::VoiceRegMode => {
            register(c)?.cme = true;
            Ok(String::new())
        }
        Id::VoiceRegSource => {
            let a = parse_ip(arg(0)).map_err(|e| format!("% {e}"))?;
            let port = match arg(1) {
                "" => SIP_PORT,
                p => p.parse::<u16>().ok().filter(|p| *p > 0).ok_or("% The port is 1-65535")?,
            };
            let mine = netlabcraft_sim::l3_ifaces(c.d()?).iter().any(|i| i.net.addr == a);
            let r = register(c)?;
            r.source = Some(a);
            r.port = port;
            Ok(if mine { String::new() } else { format!("Note: {a} is not an address of this router yet; SIP phones register there.") })
        }
        Id::VoiceNoSourceAddress => {
            let r = register(c)?;
            r.source = None;
            r.port = SIP_PORT;
            Ok(String::new())
        }
        Id::VoiceRegMaxPool | Id::VoiceMaxDn => {
            let n: u16 = arg(0).parse().ok().filter(|n| (1..=MAX_EPHONES as u16).contains(n)).ok_or(format!("% Give 1-{MAX_EPHONES}"))?;
            let r = register(c)?;
            if id == Id::VoiceRegMaxPool {
                r.max_pool = n;
            } else {
                r.max_dn = n;
            }
            Ok(String::new())
        }
        Id::VoiceRegDn | Id::VoiceRegPool => {
            if c.d()?.kind != DeviceKind::Router {
                return Err("% Call control runs on routers".into());
            }
            let dn = id == Id::VoiceRegDn;
            let n = tag(arg(0), if dn { "directory" } else { "pool" })?;
            let d = c.dm()?;
            let Some(r) = d.voice.register.as_ref() else {
                return Err("% Set up `voice register global` first".into());
            };
            let (max, have, exists) = if dn {
                (r.max_dn, d.voice.reg_dns.len(), d.voice.reg_dns.iter().any(|x| x.tag == n))
            } else {
                (r.max_pool, d.voice.pools.len(), d.voice.pools.iter().any(|x| x.tag == n))
            };
            if !exists {
                if have >= usize::from(max) || have >= if dn { MAX_DNS } else { MAX_EPHONES } {
                    let what = if dn { "max-dn" } else { "max-pool" };
                    return Err(format!("% {what} is {max}: raise it under voice register global first"));
                }
                if dn {
                    d.voice.reg_dns.push(EphoneDn { tag: n, number: None });
                    d.voice.reg_dns.sort_by_key(|x| x.tag);
                } else {
                    d.voice.pools.push(RegisterPool { tag: n, mac: None, dn: None });
                    d.voice.pools.sort_by_key(|x| x.tag);
                }
            }
            c.cli.mode = if dn { Mode::VoiceRegDn(n) } else { Mode::VoiceRegPool(n) };
            Ok(String::new())
        }
        Id::VoiceNoRegDn | Id::VoiceNoRegPool => {
            let n = tag(arg(0), "entry")?;
            let d = c.dm()?;
            let before = d.voice.reg_dns.len() + d.voice.pools.len();
            if id == Id::VoiceNoRegDn {
                d.voice.reg_dns.retain(|x| x.tag != n);
            } else {
                d.voice.pools.retain(|x| x.tag != n);
            }
            if d.voice.reg_dns.len() + d.voice.pools.len() == before { Err(format!("% No entry {n}")) } else { Ok(String::new()) }
        }
        Id::VoiceNumber | Id::VoiceNoNumber => {
            let Mode::VoiceRegDn(n) = c.cli.mode else { return Err("% `number` belongs under voice register dn".into()) };
            let number = if id == Id::VoiceNumber {
                if !valid_number(arg(0)) {
                    return Err("% A directory number is 1-16 digits".into());
                }
                Some(arg(0).to_string())
            } else {
                None
            };
            let d = c.dm()?;
            if number.as_ref().is_some_and(|x| d.voice.reg_dns.iter().any(|o| o.tag != n && o.number.as_ref() == Some(x))) {
                return Err(format!("% {} is already another directory number's", arg(0)));
            }
            let dn = d.voice.reg_dns.iter_mut().find(|x| x.tag == n).ok_or("% The directory number was deleted")?;
            dn.number = number;
            Ok(String::new())
        }
        Id::VoiceRegIdMac | Id::VoiceRegNumberDn => {
            let Mode::VoiceRegPool(n) = c.cli.mode else { return Err("% This belongs under voice register pool".into()) };
            let d = c.dm()?;
            let pool_dn = if id == Id::VoiceRegNumberDn {
                let line: u8 = arg(0).parse().ok().filter(|b| (1..=8).contains(b)).ok_or("% Lines are 1-8")?;
                if line != 1 {
                    return Err("% Only line 1 rings here".into());
                }
                let t = tag(arg(1), "directory")?;
                if !d.voice.reg_dns.iter().any(|x| x.tag == t) {
                    return Err(format!("% There is no voice register dn {t}"));
                }
                Some(t)
            } else {
                None
            };
            let mac = if id == Id::VoiceRegIdMac {
                let m = arg(0).parse::<MacAddr>().map_err(|_| "% A MAC address: H.H.H (aabb.ccdd.eeff) or aa:bb:cc:dd:ee:ff")?;
                if d.voice.pools.iter().any(|p| p.tag != n && p.mac == Some(m)) {
                    return Err(format!("% {m} is already another pool's"));
                }
                Some(m)
            } else {
                None
            };
            let p = d.voice.pools.iter_mut().find(|x| x.tag == n).ok_or("% The pool was deleted")?;
            if mac.is_some() {
                p.mac = mac;
            }
            if pool_dn.is_some() {
                p.dn = pool_dn;
            }
            Ok(String::new())
        }
        Id::ShowVoiceRegister => show_voice_register(c),
        _ => Err("% This belongs under telephony-service or ephone, not voice register".into()),
    }
}

fn show_voice_register(c: &Ctx<'_>) -> R {
    let d = c.d()?;
    let Some(r) = &d.voice.register else { return Ok("SIP call control is off (no voice register global).".into()) };
    let mut out = String::from("SIP call control (voice register global)\n");
    let src = r.source.map_or("any address of this router".to_string(), |a| a.to_string());
    let _ = writeln!(out, "  Phones register at:   {src}, UDP port {}{}", r.port, if r.cme { " (mode cme)" } else { "" });
    let _ = writeln!(out, "  Pools allowed:        {}", r.max_pool);
    let _ = writeln!(out, "  Directory numbers:    {} allowed, {} defined", r.max_dn, d.voice.reg_dns.len());
    let regs: Vec<_> = c.sim.call_agent(d.id).map(|a| a.registrations.iter().filter(|x| x.sip).cloned().collect()).unwrap_or_default();
    let calls = c.sim.sip_calls(d.id);
    let _ = writeln!(out);
    let _ = writeln!(out, "{:<6} {:<12} State", "DN", "Number");
    for dn in &d.voice.reg_dns {
        let number = dn.number.clone().unwrap_or_else(|| "-".into());
        let state = if calls.iter().any(|x| x.connected && (x.from == number || x.to == number)) {
            "in a call"
        } else if calls.iter().any(|x| x.from == number || x.to == number) {
            "ringing"
        } else if regs.iter().any(|x| x.number == number) {
            "idle (registered)"
        } else {
            "not registered"
        };
        let _ = writeln!(out, "{:<6} {number:<12} {state}", dn.tag);
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "{:<6} {:<16} {:<5} {:<16} State", "Pool", "MAC address", "DN", "Address");
    for p in &d.voice.pools {
        let mac = p.mac.map_or("-".to_string(), |m| m.to_string());
        let reg = regs.iter().find(|x| Some(x.mac) == p.mac);
        let (addr, state) = reg.map_or(("-".to_string(), "not registered"), |x| (x.ip.to_string(), "registered"));
        let _ = writeln!(out, "{:<6} {mac:<16} {:<5} {addr:<16} {state}", p.tag, p.dn.map_or("-".to_string(), |t| t.to_string()));
    }
    Ok(out.trim_end().to_string())
}

/// The `voice register` lines of the running configuration.
pub(crate) fn running_config(s: &mut String, d: &Device) {
    let v = &d.voice;
    if let Some(r) = &v.register {
        let _ = writeln!(s, "voice register global");
        if r.cme {
            let _ = writeln!(s, " mode cme");
        }
        if let Some(a) = r.source {
            let _ = writeln!(s, " source-address {a} port {}", r.port);
        }
        if r.max_dn > 0 {
            let _ = writeln!(s, " max-dn {}", r.max_dn);
        }
        if r.max_pool > 0 {
            let _ = writeln!(s, " max-pool {}", r.max_pool);
        }
        let _ = writeln!(s, "!");
    }
    for dn in &v.reg_dns {
        let _ = writeln!(s, "voice register dn {}", dn.tag);
        if let Some(n) = &dn.number {
            let _ = writeln!(s, " number {n}");
        }
        let _ = writeln!(s, "!");
    }
    for p in &v.pools {
        let _ = writeln!(s, "voice register pool {}", p.tag);
        if let Some(m) = p.mac {
            let b = m.0;
            let _ = writeln!(s, " id mac {:02x}{:02x}.{:02x}{:02x}.{:02x}{:02x}", b[0], b[1], b[2], b[3], b[4], b[5]);
        }
        if let Some(t) = p.dn {
            let _ = writeln!(s, " number 1 dn {t}");
        }
        let _ = writeln!(s, "!");
    }
}

/// The `power inline never` line of a port's configuration.
pub(crate) fn port_lines(s: &mut String, p: &netlabcraft_model::Port) {
    if p.poe_never {
        let _ = writeln!(s, " power inline never");
    }
}

fn watts(mw: u32) -> String {
    format!("{}.{}", mw / 1000, (mw % 1000) / 100)
}

fn show_power_inline(c: &Ctx<'_>) -> R {
    let d: &Device = c.d()?;
    let Some(budget) = poe::budget_mw(d.catalogue_model()) else {
        return Err(format!("% A {} gives no power over Ethernet", d.catalogue_model().label));
    };
    let rows = c.t.power_inline(d.id);
    let used: u32 = rows.iter().filter_map(|r| r.device.map(|(_, mw)| mw)).sum();
    let mut s = String::new();
    let _ = writeln!(s, "Power budget {} W, in use {} W, left {} W", watts(budget), watts(used), watts(budget.saturating_sub(used)));
    let _ = writeln!(s);
    let _ = writeln!(s, "{:<10} {:<7} {:<8} {:>7}  Device", "Port", "Admin", "Power", "Watts");
    for r in &rows {
        let Some(p) = d.ports.get(r.port) else { continue };
        if !p.name.starts_with("FastEthernet") {
            continue;
        }
        let PortPower { never, device, denied, .. } = r;
        let (state, w, who) = match device {
            Some((pd, mw)) => ("on", watts(*mw), c.t.device(*pd).map_or("?".to_string(), |x| format!("{} ({})", x.name, x.catalogue_model().label))),
            None if *denied => ("denied", "0.0".into(), "budget used up".into()),
            None => ("off", "0.0".into(), String::new()),
        };
        let line = format!("{:<10} {:<7} {:<8} {:>7}  {who}", ifname::short(&p.name), if *never { "never" } else { "auto" }, state, w);
        let _ = writeln!(s, "{}", line.trim_end());
    }
    Ok(s.trim_end().to_string())
}
