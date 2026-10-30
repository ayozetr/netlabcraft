//! Site-to-site VPN commands: `interface Tunnel N` (GRE) and basic IPsec (`crypto isakmp`,
//! `crypto ipsec transform-set`, `crypto map`).

use std::net::Ipv4Addr;

use netlabcraft_core::{Ipv4Net, parse_ip};
use netlabcraft_model::vpn::{CryptoMapEntry, IsakmpPolicy, MAX_CRYPTO, MAX_CRYPTO_TEXT, TransformSet, TunnelSource, tunnel_number};
use netlabcraft_model::{DeviceKind, MAX_DESCRIPTION, ifname};

use super::{Ctx, Mode, R};
use crate::grammar::Id;
use crate::render;

/// The transforms a transform set accepts (`esp-aes` may be followed by a key size).
const TRANSFORMS: [&str; 9] =
    ["esp-aes", "esp-des", "esp-3des", "esp-null", "esp-sha-hmac", "esp-md5-hmac", "esp-sha256-hmac", "ah-sha-hmac", "ah-md5-hmac"];

fn router_only(c: &Ctx) -> Result<(), String> {
    if c.d()?.kind != DeviceKind::Router {
        return Err("% Tunnels and IPsec are router features".into());
    }
    Ok(())
}

fn name_ok(text: &str) -> Result<String, String> {
    if text.is_empty() || text.chars().count() > MAX_CRYPTO_TEXT || !text.chars().all(|ch| ch.is_ascii_graphic()) {
        return Err(format!("% Give a name of 1-{MAX_CRYPTO_TEXT} printable characters"));
    }
    Ok(text.to_string())
}

/// `interface Tunnel N`.
pub(super) fn enter_tunnel(c: &mut Ctx, n: u32) -> R {
    router_only(c)?;
    c.dm()?.tunnel_mut(n).ok_or("% Too many tunnel interfaces")?;
    c.cli.mode = Mode::Tunnel(n);
    Ok(String::new())
}

/// `no interface Tunnel N`.
pub(super) fn delete_tunnel(c: &mut Ctx, n: u32) -> R {
    let d = c.dm()?;
    let before = d.tunnels.len();
    d.tunnels.retain(|t| t.number != n);
    if d.tunnels.len() == before {
        return Err(format!("% There is no interface Tunnel{n}"));
    }
    d.ospf_if.remove(&format!("Tunnel{n}"));
    Ok(String::new())
}

/// `ip address`, `shutdown`, `description` and their `no` forms on a tunnel.
pub(super) fn tunnel_iface(c: &mut Ctx, id: Id, a0: &str, a1: &str) -> R {
    let Mode::Tunnel(n) = c.cli.mode else { return Err("% Not in a tunnel interface".into()) };
    let ip = if id == Id::IpAddress {
        let net = Ipv4Net::with_mask(parse_ip(a0).map_err(|e| format!("% {e}"))?, parse_ip(a1).map_err(|e| format!("% {e}"))?)
            .map_err(|e| format!("% {e}"))?;
        if net.is_network_or_broadcast() {
            return Err(format!("% {} is the network or broadcast address of {}", net.addr, net.network()));
        }
        Some(net)
    } else {
        None
    };
    let text: String = a0.chars().take(MAX_DESCRIPTION).collect();
    let tun = c.dm()?.tunnel_mut(n).ok_or("% Too many tunnel interfaces")?;
    match id {
        Id::IpAddress => tun.ip = ip,
        Id::NoIpAddress => tun.ip = None,
        Id::Shutdown => tun.shutdown = true,
        Id::NoShutdown => tun.shutdown = false,
        Id::Description => tun.description = text,
        _ => tun.description.clear(),
    }
    Ok(match id {
        Id::NoShutdown => format!("Tunnel{n} is now enabled"),
        Id::Shutdown => format!("Tunnel{n} is now administratively down"),
        _ => String::new(),
    })
}

/// The names of the interfaces an interface mode is configuring.
fn iface_names(c: &Ctx) -> Result<Vec<String>, String> {
    let Mode::Iface(ifs) = &c.cli.mode else { return Err("% Not in interface mode".into()) };
    let d = c.d()?;
    Ok(ifs.iter().filter_map(|r| d.ports.get(r.port).map(|p| r.sub.map_or(p.name.clone(), |s| format!("{}.{s}", p.name)))).collect())
}

/// `[0] KEY address A.B.C.D`: the key and the address.
fn key_words(text: &str) -> Result<(String, Ipv4Addr), String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let at = words.iter().position(|w| w.eq_ignore_ascii_case("address")).ok_or("% Usage: crypto isakmp key KEY address A.B.C.D")?;
    let before = words.get(..at).unwrap_or(&[]);
    let key = match before {
        [k] => *k,
        ["0", k] => *k,
        _ => return Err("% Usage: crypto isakmp key KEY address A.B.C.D (one word, no spaces)".into()),
    };
    if key.chars().count() > MAX_CRYPTO_TEXT {
        return Err(format!("% Keys are at most {MAX_CRYPTO_TEXT} characters"));
    }
    let addr = words.get(at + 1).ok_or("% Give the peer's address")?;
    let addr = parse_ip(addr).map_err(|e| format!("% {e}"))?;
    if words.len() > at + 2 && !words.get(at + 2).is_some_and(|w| w.eq_ignore_ascii_case("no-xauth")) {
        return Err("% Unexpected words after the address".into());
    }
    Ok((key.to_string(), addr))
}

/// The current crypto map entry (crypto map mode).
fn map_entry<'a>(c: &'a mut Ctx) -> Result<&'a mut CryptoMapEntry, String> {
    let Mode::CryptoMap(name, seq) = c.cli.mode.clone() else { return Err("% Not in crypto map mode".into()) };
    c.dm()?.crypto.maps.iter_mut().find(|e| e.map == name && e.seq == seq).ok_or_else(|| "% The crypto map entry was deleted".to_string())
}

/// The current ISAKMP policy (isakmp mode).
fn policy<'a>(c: &'a mut Ctx) -> Result<&'a mut IsakmpPolicy, String> {
    let Mode::Isakmp(p) = c.cli.mode else { return Err("% Not in ISAKMP policy mode".into()) };
    c.dm()?.crypto.policies.iter_mut().find(|x| x.priority == p).ok_or_else(|| "% The policy was deleted".to_string())
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("").trim();
    router_only(c)?;
    match id {
        Id::TunnelSource => {
            let Mode::Tunnel(n) = c.cli.mode else { return Err("% Not in a tunnel interface".into()) };
            let text = arg(0);
            let src = if let Ok(a) = text.parse::<Ipv4Addr>() {
                TunnelSource::Address(a)
            } else if tunnel_number(text).is_some() {
                return Err("% A tunnel cannot start from another tunnel".into());
            } else {
                let parsed = ifname::parse(text).ok_or_else(|| format!("% `{text}` is neither an interface nor an address"))?;
                let d = c.d()?;
                d.port_index(&parsed.port_name()).ok_or_else(|| format!("% {} has no interface {}", d.name, parsed.port_name()))?;
                TunnelSource::Interface(parsed.to_string())
            };
            c.dm()?.tunnel_mut(n).ok_or("% Too many tunnel interfaces")?.source = Some(src);
            Ok(String::new())
        }
        Id::NoTunnelSource | Id::NoTunnelDestination | Id::TunnelDestination => {
            let Mode::Tunnel(n) = c.cli.mode else { return Err("% Not in a tunnel interface".into()) };
            let dst = if id == Id::TunnelDestination { Some(parse_ip(arg(0)).map_err(|e| format!("% {e}"))?) } else { None };
            let tun = c.dm()?.tunnel_mut(n).ok_or("% Too many tunnel interfaces")?;
            match id {
                Id::NoTunnelSource => tun.source = None,
                _ => tun.destination = dst,
            }
            Ok(String::new())
        }
        Id::TunnelMode => {
            let words: Vec<String> = arg(0).split_whitespace().map(str::to_ascii_lowercase).collect();
            if words == ["gre", "ip"] { Ok(String::new()) } else { Err("% Only GRE over IPv4 is simulated: `tunnel mode gre ip`".into()) }
        }
        Id::IsakmpPolicy => {
            let p: u16 = arg(0).parse().ok().filter(|p| (1..=10_000).contains(p)).ok_or("% Policy priority is 1-10000")?;
            let crypto = &mut c.dm()?.crypto;
            if !crypto.policies.iter().any(|x| x.priority == p) {
                if crypto.policies.len() >= MAX_CRYPTO {
                    return Err(format!("% At most {MAX_CRYPTO} policies"));
                }
                crypto.policies.push(IsakmpPolicy::new(p));
                crypto.policies.sort_by_key(|x| x.priority);
            }
            c.cli.mode = Mode::Isakmp(p);
            Ok(String::new())
        }
        Id::NoIsakmpPolicy => {
            let p: u16 = arg(0).parse().map_err(|_| "% Policy priority is 1-10000")?;
            let crypto = &mut c.dm()?.crypto;
            let before = crypto.policies.len();
            crypto.policies.retain(|x| x.priority != p);
            if crypto.policies.len() == before { Err(format!("% There is no policy {p}")) } else { Ok(String::new()) }
        }
        Id::IsakmpEncryption => {
            let words: Vec<String> = arg(0).split_whitespace().map(str::to_ascii_lowercase).collect();
            let value = match words.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
                [c @ ("des" | "3des" | "aes")] => c.to_string(),
                ["aes", bits @ ("128" | "192" | "256")] => format!("aes {bits}"),
                _ => return Err("% des, 3des, or aes [128|192|256]".into()),
            };
            policy(c)?.encryption = value;
            Ok(String::new())
        }
        Id::IsakmpHash => {
            let h = arg(0).to_ascii_lowercase();
            if !["sha", "sha256", "sha384", "sha512", "md5"].contains(&h.as_str()) {
                return Err("% sha, sha256, sha384, sha512 or md5".into());
            }
            policy(c)?.hash = h;
            Ok(String::new())
        }
        Id::IsakmpAuthentication => {
            let a = arg(0).to_ascii_lowercase();
            if !["pre-share", "rsa-sig", "rsa-encr"].contains(&a.as_str()) {
                return Err("% pre-share, rsa-sig or rsa-encr".into());
            }
            policy(c)?.authentication = a;
            Ok(String::new())
        }
        Id::IsakmpGroup => {
            let g: u8 =
                arg(0).parse().ok().filter(|g| [1, 2, 5, 14, 15, 16, 19, 20, 24].contains(g)).ok_or("% Group 1, 2, 5, 14, 15, 16, 19, 20 or 24")?;
            policy(c)?.group = g;
            Ok(String::new())
        }
        Id::IsakmpLifetime => {
            let s: u32 = arg(0).parse().ok().filter(|s| (60..=86_400).contains(s)).ok_or("% Lifetime is 60-86400 seconds")?;
            policy(c)?.lifetime = Some(s);
            Ok(String::new())
        }
        Id::IsakmpKey => {
            let (key, addr) = key_words(arg(0))?;
            let crypto = &mut c.dm()?.crypto;
            if let Some(k) = crypto.keys.iter_mut().find(|(a, _)| *a == addr) {
                k.1 = key;
            } else {
                if crypto.keys.len() >= MAX_CRYPTO {
                    return Err(format!("% At most {MAX_CRYPTO} keys"));
                }
                crypto.keys.push((addr, key));
            }
            Ok(String::new())
        }
        Id::NoIsakmpKey => {
            let words: Vec<&str> = arg(0).split_whitespace().collect();
            let addr = words
                .iter()
                .position(|w| w.eq_ignore_ascii_case("address"))
                .and_then(|i| words.get(i + 1))
                .ok_or("% Usage: no crypto isakmp key KEY address A.B.C.D")?;
            let addr = parse_ip(addr).map_err(|e| format!("% {e}"))?;
            let crypto = &mut c.dm()?.crypto;
            let before = crypto.keys.len();
            crypto.keys.retain(|(a, _)| *a != addr);
            if crypto.keys.len() == before { Err(format!("% No key for {addr}")) } else { Ok(String::new()) }
        }
        Id::TransformSet => {
            let name = name_ok(arg(0))?;
            let mut transforms: Vec<String> = Vec::new();
            for w in arg(1).split_whitespace().map(str::to_ascii_lowercase) {
                match (w.as_str(), transforms.last_mut()) {
                    ("128" | "192" | "256", Some(last)) if last == "esp-aes" => {
                        last.push(' ');
                        last.push_str(&w);
                    }
                    (t, _) if TRANSFORMS.contains(&t) => transforms.push(w.clone()),
                    _ => return Err(format!("% `{w}` is not a transform (esp-aes, esp-3des, esp-sha-hmac, esp-md5-hmac, …)")),
                }
                if transforms.len() > 3 {
                    return Err("% At most three transforms".into());
                }
            }
            if transforms.is_empty() {
                return Err("% Give at least one transform, e.g. esp-aes esp-sha-hmac".into());
            }
            let crypto = &mut c.dm()?.crypto;
            match crypto.transform_sets.iter_mut().find(|t| t.name == name) {
                Some(t) => t.transforms = transforms,
                None => {
                    if crypto.transform_sets.len() >= MAX_CRYPTO {
                        return Err(format!("% At most {MAX_CRYPTO} transform sets"));
                    }
                    crypto.transform_sets.push(TransformSet { name: name.clone(), transforms, transport: false });
                }
            }
            c.cli.mode = Mode::Transform(name);
            Ok(String::new())
        }
        Id::NoTransformSet => {
            let name = arg(0).to_string();
            let crypto = &mut c.dm()?.crypto;
            let before = crypto.transform_sets.len();
            crypto.transform_sets.retain(|t| t.name != name);
            if crypto.transform_sets.len() == before { Err(format!("% There is no transform set {name}")) } else { Ok(String::new()) }
        }
        Id::TransformMode => {
            let Mode::Transform(name) = c.cli.mode.clone() else { return Err("% Not in transform set mode".into()) };
            let transport = match arg(0).to_ascii_lowercase().as_str() {
                "tunnel" => false,
                "transport" => true,
                _ => return Err("% tunnel or transport".into()),
            };
            c.dm()?.crypto.transform_sets.iter_mut().find(|t| t.name == name).ok_or("% The transform set was deleted")?.transport = transport;
            Ok(String::new())
        }
        Id::CryptoMap => {
            let name = name_ok(arg(0))?;
            let seq: u32 = arg(1).parse().ok().filter(|s| (1..=65_535).contains(s)).ok_or("% Sequence number is 1-65535")?;
            let crypto = &mut c.dm()?.crypto;
            if !crypto.maps.iter().any(|e| e.map == name && e.seq == seq) {
                if crypto.maps.len() >= MAX_CRYPTO {
                    return Err(format!("% At most {MAX_CRYPTO} crypto map entries"));
                }
                crypto.maps.push(CryptoMapEntry { map: name.clone(), seq, peer: None, transform_set: None, acl: None });
                crypto.maps.sort_by(|a, b| (&a.map, a.seq).cmp(&(&b.map, b.seq)));
            }
            c.cli.mode = Mode::CryptoMap(name, seq);
            Ok(String::new())
        }
        Id::NoCryptoMap => {
            let name = arg(0).to_string();
            let seq = match arg(1) {
                "" => None,
                s => Some(s.parse::<u32>().map_err(|_| "% Sequence number is 1-65535")?),
            };
            let crypto = &mut c.dm()?.crypto;
            let before = crypto.maps.len();
            crypto.maps.retain(|e| !(e.map == name && seq.is_none_or(|s| s == e.seq)));
            if crypto.maps.len() == before { Err(format!("% No such crypto map entry: {name}")) } else { Ok(String::new()) }
        }
        Id::CryptoSetPeer => {
            let a = parse_ip(arg(0)).map_err(|e| format!("% {e}"))?;
            map_entry(c)?.peer = Some(a);
            Ok(String::new())
        }
        Id::NoCryptoSetPeer => {
            map_entry(c)?.peer = None;
            Ok(String::new())
        }
        Id::CryptoSetTransformSet => {
            let name = name_ok(arg(0))?;
            let note =
                if c.d()?.crypto.transform_set(&name).is_none() { format!("Note: transform set {name} does not exist yet") } else { String::new() };
            map_entry(c)?.transform_set = Some(name);
            Ok(note)
        }
        Id::NoCryptoSetTransformSet => {
            map_entry(c)?.transform_set = None;
            Ok(String::new())
        }
        Id::CryptoMatchAddress => {
            let name = name_ok(arg(0))?;
            let note =
                if c.d()?.acls.iter().all(|a| a.name != name) { format!("Note: access list {name} does not exist yet") } else { String::new() };
            map_entry(c)?.acl = Some(name);
            Ok(note)
        }
        Id::NoCryptoMatchAddress => {
            map_entry(c)?.acl = None;
            Ok(String::new())
        }
        Id::CryptoMapIf => {
            let map = arg(0).to_string();
            if c.d()?.crypto.map(&map).next().is_none() {
                return Err(format!("% There is no crypto map {map}: define it with `crypto map {map} 10 ipsec-isakmp`"));
            }
            let names = iface_names(c)?;
            let crypto = &mut c.dm()?.crypto;
            for n in &names {
                if !crypto.applied.contains_key(n) && crypto.applied.len() >= MAX_CRYPTO {
                    return Err(format!("% At most {MAX_CRYPTO} interfaces with crypto maps"));
                }
                crypto.applied.insert(n.clone(), map.clone());
            }
            Ok(format!("IPsec on: crypto map {map} now protects matching traffic leaving {}", names.join(", ")))
        }
        Id::NoCryptoMapIf => {
            let names = iface_names(c)?;
            let crypto = &mut c.dm()?.crypto;
            for n in &names {
                crypto.applied.remove(n);
            }
            Ok(String::new())
        }
        Id::ShowCryptoIsakmpSa => Ok(render::crypto_isakmp_sa(c.sim, c.d()?)),
        Id::ShowCryptoIpsecSa => Ok(render::crypto_ipsec_sa(c.sim, c.d()?)),
        Id::ShowCryptoIsakmpPolicy => Ok(render::isakmp_policies(c.d()?)),
        Id::ShowCryptoMap => Ok(render::crypto_maps(c.d()?)),
        Id::ClearCryptoSa => {
            c.sim.clear_ipsec(c.dev);
            Ok(String::new())
        }
        _ => Err("% Not a VPN command".into()),
    }
}
