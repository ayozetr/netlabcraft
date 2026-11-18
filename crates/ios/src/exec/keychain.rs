//! Key chains: `key chain NAME`, `key N`, `key-string`, `accept-lifetime`, `send-lifetime`,
//! `show key chain`, and `ip ospf authentication key-chain NAME` on an interface.
//!
//! A chain is a list of keys with lifetimes. OSPF signs with the key whose send lifetime began
//! last and accepts every key whose accept lifetime covers the time of the device's clock.

use std::fmt::Write as _;

use netlabcraft_model::Device;
use netlabcraft_model::keychain::{Key, Lifetime, MAX_KEYS, MAX_TEXT};

use super::{Ctx, Mode, R};
use crate::grammar::Id;

fn name_ok(text: &str) -> Result<String, String> {
    if text.is_empty() || text.chars().count() > MAX_TEXT || !text.chars().all(|ch| ch.is_ascii_graphic()) {
        return Err(format!("% Give a name of 1-{MAX_TEXT} printable characters"));
    }
    Ok(text.to_string())
}

fn key_number(text: &str) -> Result<u32, String> {
    text.parse::<u32>().ok().filter(|n| *n <= 2_147_483_647).ok_or_else(|| "% The key number is 0-2147483647".to_string())
}

/// The chain and key the session is in.
fn current(c: &Ctx) -> Result<(String, u32), String> {
    match &c.cli.mode {
        Mode::KeyChainKey(chain, n) => Ok((chain.clone(), *n)),
        _ => Err("% Not in key mode".into()),
    }
}

fn key_mut<'a>(c: &'a mut Ctx, chain: &str, n: u32) -> Result<&'a mut Key, String> {
    c.dm()?
        .key_chains
        .chains
        .iter_mut()
        .find(|x| x.name == chain)
        .and_then(|x| x.keys.get_mut(&n))
        .ok_or_else(|| "% That key no longer exists".to_string())
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match id {
        Id::KeyChain => {
            let name = name_ok(arg(0))?;
            c.dm()?.key_chains.get_or_add(&name).ok_or_else(|| format!("% At most {} key chains", netlabcraft_model::keychain::MAX_CHAINS))?;
            c.cli.mode = Mode::KeyChain(name);
            Ok(String::new())
        }
        Id::NoKeyChain => {
            let name = arg(0).to_string();
            let d = c.dm()?;
            let before = d.key_chains.chains.len();
            d.key_chains.chains.retain(|x| x.name != name);
            if d.key_chains.chains.len() == before {
                return Err(format!("% There is no key chain {name}"));
            }
            Ok(String::new())
        }
        Id::KeyChainKey => {
            let chain = match &c.cli.mode {
                Mode::KeyChain(n) | Mode::KeyChainKey(n, _) => n.clone(),
                _ => return Err("% Not in key chain mode".into()),
            };
            let n = key_number(arg(0))?;
            let x = c.dm()?.key_chains.chains.iter_mut().find(|x| x.name == chain).ok_or("% That key chain no longer exists")?;
            if !x.keys.contains_key(&n) && x.keys.len() >= MAX_KEYS {
                return Err(format!("% At most {MAX_KEYS} keys in a chain"));
            }
            x.keys.entry(n).or_default();
            c.cli.mode = Mode::KeyChainKey(chain, n);
            Ok(String::new())
        }
        Id::NoKeyChainKey => {
            let chain = match &c.cli.mode {
                Mode::KeyChain(n) | Mode::KeyChainKey(n, _) => n.clone(),
                _ => return Err("% Not in key chain mode".into()),
            };
            let n = key_number(arg(0))?;
            let x = c.dm()?.key_chains.chains.iter_mut().find(|x| x.name == chain).ok_or("% That key chain no longer exists")?;
            if x.keys.remove(&n).is_none() {
                return Err(format!("% There is no key {n} in chain {chain}"));
            }
            if matches!(&c.cli.mode, Mode::KeyChainKey(_, k) if *k == n) {
                c.cli.mode = Mode::KeyChain(chain);
            }
            Ok(String::new())
        }
        Id::KeyString => {
            let text = arg(0).trim().to_string();
            if text.is_empty() || text.chars().count() > MAX_TEXT || !text.chars().all(|ch| ch.is_ascii_graphic()) {
                return Err(format!("% The key string is 1-{MAX_TEXT} printable characters with no spaces"));
            }
            let (chain, n) = current(c)?;
            let ospf_limit = text.chars().count() > 16 && n <= 255;
            key_mut(c, &chain, n)?.string = Some(text);
            Ok(if ospf_limit {
                "Note: OSPF message-digest keys are at most 16 characters; this key is only used by protocols that take longer ones".into()
            } else {
                String::new()
            })
        }
        Id::NoKeyString => {
            let (chain, n) = current(c)?;
            key_mut(c, &chain, n)?.string = None;
            Ok(String::new())
        }
        Id::AcceptLifetime | Id::SendLifetime => {
            let life: Lifetime = crate::clock::parse_lifetime(arg(0))?;
            let (chain, n) = current(c)?;
            let k = key_mut(c, &chain, n)?;
            if id == Id::AcceptLifetime {
                k.accept = Some(life);
            } else {
                k.send = Some(life);
            }
            Ok(String::new())
        }
        Id::NoAcceptLifetime | Id::NoSendLifetime => {
            let (chain, n) = current(c)?;
            let k = key_mut(c, &chain, n)?;
            if id == Id::NoAcceptLifetime {
                k.accept = None;
            } else {
                k.send = None;
            }
            Ok(String::new())
        }
        Id::ShowKeyChain => {
            let now = c.sim.clock(c.dev).0;
            show(c.d()?, now, (!arg(0).is_empty()).then(|| arg(0)))
        }
        _ => Err("% Not a key chain command".into()),
    }
}

fn life_text(l: Option<Lifetime>, now: i64) -> String {
    match l {
        None => "always (valid now)".to_string(),
        Some(l) => {
            let end = l.end.map_or("infinite".to_string(), crate::clock::stamp);
            let state = if l.covers(now) {
                "valid now"
            } else if now < l.start {
                "not yet valid"
            } else {
                "expired"
            };
            format!("{} to {end} ({state})", crate::clock::stamp(l.start))
        }
    }
}

/// `show key chain [NAME]`, read against the device's clock `now` (Unix seconds).
pub(crate) fn show(d: &Device, now: i64, only: Option<&str>) -> R {
    let chains: Vec<_> = d.key_chains.chains.iter().filter(|c| only.is_none_or(|n| c.name == n)).collect();
    if chains.is_empty() {
        return Ok(match only {
            Some(n) => format!("% There is no key chain {n}"),
            None => "No key chains are configured.".into(),
        });
    }
    let mut s = String::new();
    for c in chains {
        let _ = writeln!(s, "Key chain {}: {} key{}", c.name, c.keys.len(), if c.keys.len() == 1 { "" } else { "s" });
        for (n, k) in &c.keys {
            let _ = writeln!(s, "  key {n}: {}", if k.string.is_some() { "string set" } else { "no string yet" });
            let _ = writeln!(s, "    accepted: {}", life_text(k.accept, now));
            let _ = writeln!(s, "    signs:    {}", life_text(k.send, now));
        }
        if let Some((id, _)) = c.signing(now) {
            let _ = writeln!(s, "  signing now with key {id}");
        } else {
            let _ = writeln!(s, "  no key signs now");
        }
    }
    Ok(s.trim_end().to_string())
}

/// The `key chain` blocks of the running configuration.
pub(crate) fn config_lines(s: &mut String, d: &Device) {
    for c in &d.key_chains.chains {
        let _ = writeln!(s, "key chain {}", c.name);
        for (n, k) in &c.keys {
            let _ = writeln!(s, " key {n}");
            if let Some(t) = &k.string {
                let _ = writeln!(s, "  key-string {t}");
            }
            for (word, l) in [("accept-lifetime", k.accept), ("send-lifetime", k.send)] {
                if let Some(l) = l {
                    let end = l.end.map_or("infinite".to_string(), crate::clock::stamp);
                    let _ = writeln!(s, "  {word} {} {end}", crate::clock::stamp(l.start));
                }
            }
        }
        let _ = writeln!(s, "!");
    }
}
