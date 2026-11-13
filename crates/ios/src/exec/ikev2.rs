//! IKEv2 commands: `crypto ikev2 proposal|policy|keyring|profile`, `set ikev2-profile` in a
//! crypto map entry, and the reports.

use std::fmt::Write as _;
use std::net::Ipv4Addr;

use netlabcraft_core::parse_ip;
use netlabcraft_model::Device;
use netlabcraft_model::vpn::{Ikev2Keyring, Ikev2Peer, Ikev2Policy, Ikev2Profile, Ikev2Proposal, MAX_CRYPTO, MAX_CRYPTO_TEXT};
use netlabcraft_sim::Sim;
use netlabcraft_sim::vpn::IkePhase;

use super::{Ctx, Mode, R};
use crate::grammar::Id;

/// The ciphers, integrity algorithms and groups a proposal accepts.
pub const CIPHERS: [&str; 4] = ["aes-cbc-128", "aes-cbc-192", "aes-cbc-256", "3des"];
pub const INTEGRITY: [&str; 3] = ["sha1", "sha256", "md5"];
pub const GROUPS: [u8; 7] = [2, 5, 14, 15, 16, 19, 20];

fn name_ok(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() || t.chars().count() > MAX_CRYPTO_TEXT || !t.chars().all(|ch| ch.is_ascii_graphic()) {
        return Err(format!("% Give a name of 1-{MAX_CRYPTO_TEXT} printable characters"));
    }
    Ok(t.to_string())
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_ascii_lowercase).collect()
}

fn find_or_add<T>(list: &mut Vec<T>, name: &str, get: impl Fn(&T) -> &str, make: impl FnOnce() -> T, what: &str) -> Result<(), String> {
    if !list.iter().any(|x| get(x) == name) {
        if list.len() >= MAX_CRYPTO {
            return Err(format!("% At most {MAX_CRYPTO} {what}"));
        }
        list.push(make());
    }
    Ok(())
}

fn proposal<'a>(c: &'a mut Ctx) -> Result<&'a mut Ikev2Proposal, String> {
    let Mode::Ikev2Proposal(n) = c.cli.mode.clone() else { return Err("% Not in an IKEv2 proposal".into()) };
    c.dm()?.crypto.ikev2.proposals.iter_mut().find(|p| p.name == n).ok_or_else(|| "% The proposal was deleted".to_string())
}

fn policy<'a>(c: &'a mut Ctx) -> Result<&'a mut Ikev2Policy, String> {
    let Mode::Ikev2Policy(n) = c.cli.mode.clone() else { return Err("% Not in an IKEv2 policy".into()) };
    c.dm()?.crypto.ikev2.policies.iter_mut().find(|p| p.name == n).ok_or_else(|| "% The policy was deleted".to_string())
}

fn peer<'a>(c: &'a mut Ctx) -> Result<&'a mut Ikev2Peer, String> {
    let Mode::Ikev2Peer(ring, n) = c.cli.mode.clone() else { return Err("% Not in a keyring peer".into()) };
    c.dm()?
        .crypto
        .ikev2
        .keyrings
        .iter_mut()
        .find(|k| k.name == ring)
        .and_then(|k| k.peers.iter_mut().find(|p| p.name == n))
        .ok_or_else(|| "% The peer was deleted".to_string())
}

fn profile<'a>(c: &'a mut Ctx) -> Result<&'a mut Ikev2Profile, String> {
    let Mode::Ikev2Profile(n) = c.cli.mode.clone() else { return Err("% Not in an IKEv2 profile".into()) };
    c.dm()?.crypto.ikev2.profiles.iter_mut().find(|p| p.name == n).ok_or_else(|| "% The profile was deleted".to_string())
}

pub(super) fn command(c: &mut Ctx, id: Id, args: &[String]) -> R {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("").trim().to_string();
    if c.d()?.kind != netlabcraft_model::DeviceKind::Router {
        return Err("% IKEv2 is a router feature".into());
    }
    match id {
        Id::Ikev2Proposal => {
            let name = name_ok(&arg(0))?;
            let v2 = &mut c.dm()?.crypto.ikev2;
            find_or_add(
                &mut v2.proposals,
                &name,
                |p| &p.name,
                || Ikev2Proposal { name: name.clone(), encryption: Vec::new(), integrity: Vec::new(), group: Vec::new() },
                "IKEv2 proposals",
            )?;
            c.cli.mode = Mode::Ikev2Proposal(name);
            Ok(String::new())
        }
        Id::Ikev2Policy => {
            let name = name_ok(&arg(0))?;
            let v2 = &mut c.dm()?.crypto.ikev2;
            find_or_add(
                &mut v2.policies,
                &name,
                |p| &p.name,
                || Ikev2Policy { name: name.clone(), proposals: Vec::new(), local: None },
                "IKEv2 policies",
            )?;
            c.cli.mode = Mode::Ikev2Policy(name);
            Ok(String::new())
        }
        Id::Ikev2Keyring => {
            let name = name_ok(&arg(0))?;
            let v2 = &mut c.dm()?.crypto.ikev2;
            find_or_add(&mut v2.keyrings, &name, |k| &k.name, || Ikev2Keyring { name: name.clone(), peers: Vec::new() }, "IKEv2 keyrings")?;
            c.cli.mode = Mode::Ikev2Keyring(name);
            Ok(String::new())
        }
        Id::Ikev2Profile => {
            let name = name_ok(&arg(0))?;
            let v2 = &mut c.dm()?.crypto.ikev2;
            find_or_add(
                &mut v2.profiles,
                &name,
                |p| &p.name,
                || Ikev2Profile { name: name.clone(), remote: Vec::new(), local_id: None, auth_local: None, auth_remote: None, keyring: None },
                "IKEv2 profiles",
            )?;
            c.cli.mode = Mode::Ikev2Profile(name);
            Ok(String::new())
        }
        Id::NoIkev2Proposal | Id::NoIkev2Policy | Id::NoIkev2Keyring | Id::NoIkev2Profile => {
            let name = arg(0);
            let v2 = &mut c.dm()?.crypto.ikev2;
            let (before, after) = match id {
                Id::NoIkev2Proposal => (v2.proposals.len(), {
                    v2.proposals.retain(|p| p.name != name);
                    v2.proposals.len()
                }),
                Id::NoIkev2Policy => (v2.policies.len(), {
                    v2.policies.retain(|p| p.name != name);
                    v2.policies.len()
                }),
                Id::NoIkev2Keyring => (v2.keyrings.len(), {
                    v2.keyrings.retain(|p| p.name != name);
                    v2.keyrings.len()
                }),
                _ => (v2.profiles.len(), {
                    v2.profiles.retain(|p| p.name != name);
                    v2.profiles.len()
                }),
            };
            if before == after { Err(format!("% Nothing named {name} to delete")) } else { Ok(String::new()) }
        }
        Id::Ikev2Encryption => {
            let w = words(&arg(0));
            if let Some(bad) = w.iter().find(|x| !CIPHERS.contains(&x.as_str())) {
                return Err(format!("% `{bad}` is not a cipher here ({})", CIPHERS.join(", ")));
            }
            proposal(c)?.encryption = w.into_iter().take(4).collect();
            Ok(String::new())
        }
        Id::Ikev2Integrity => {
            let w = words(&arg(0));
            if let Some(bad) = w.iter().find(|x| !INTEGRITY.contains(&x.as_str())) {
                return Err(format!("% `{bad}` is not an integrity algorithm here ({})", INTEGRITY.join(", ")));
            }
            proposal(c)?.integrity = w.into_iter().take(3).collect();
            Ok(String::new())
        }
        Id::Ikev2Group => {
            let mut g = Vec::new();
            for x in words(&arg(0)) {
                let n: u8 =
                    x.parse().ok().filter(|n| GROUPS.contains(n)).ok_or_else(|| format!("% `{x}` is not a group here (2, 5, 14, 15, 16, 19, 20)"))?;
                if !g.contains(&n) {
                    g.push(n);
                }
            }
            proposal(c)?.group = g;
            Ok(String::new())
        }
        Id::Ikev2PolicyProposal => {
            let n = name_ok(&arg(0))?;
            let p = policy(c)?;
            if !p.proposals.contains(&n) && p.proposals.len() < MAX_CRYPTO {
                p.proposals.push(n);
            }
            Ok(String::new())
        }
        Id::Ikev2PolicyMatchLocal => {
            let a = parse_ip(&arg(0)).map_err(|e| format!("% {e}"))?;
            policy(c)?.local = Some(a);
            Ok(String::new())
        }
        Id::Ikev2Peer => {
            let Mode::Ikev2Keyring(ring) = c.cli.mode.clone() else { return Err("% Not in a keyring".into()) };
            let name = name_ok(&arg(0))?;
            let k = c.dm()?.crypto.ikev2.keyrings.iter_mut().find(|k| k.name == ring).ok_or("% The keyring was deleted")?;
            find_or_add(
                &mut k.peers,
                &name,
                |p| &p.name,
                || Ikev2Peer { name: name.clone(), address: None, local_key: None, remote_key: None },
                "peers in a keyring",
            )?;
            c.cli.mode = Mode::Ikev2Peer(ring, name);
            Ok(String::new())
        }
        Id::NoIkev2Peer => {
            let ring = match c.cli.mode.clone() {
                Mode::Ikev2Keyring(r) | Mode::Ikev2Peer(r, _) => r,
                _ => return Err("% Not in a keyring".into()),
            };
            let name = arg(0);
            let k = c.dm()?.crypto.ikev2.keyrings.iter_mut().find(|k| k.name == ring).ok_or("% The keyring was deleted")?;
            let before = k.peers.len();
            k.peers.retain(|p| p.name != name);
            let removed = before != k.peers.len();
            c.cli.mode = Mode::Ikev2Keyring(ring);
            if removed { Ok(String::new()) } else { Err(format!("% No peer {name}")) }
        }
        Id::Ikev2PeerAddress => {
            let a = parse_ip(&arg(0)).map_err(|e| format!("% {e}"))?;
            peer(c)?.address = Some(a);
            Ok(String::new())
        }
        Id::Ikev2PeerKey => {
            let w: Vec<String> = arg(0).split_whitespace().map(str::to_string).collect();
            let (which, key) = match w.as_slice() {
                [k] => ("both", k.clone()),
                [d, k] if d.eq_ignore_ascii_case("local") => ("local", k.clone()),
                [d, k] if d.eq_ignore_ascii_case("remote") => ("remote", k.clone()),
                [z, k] if z == "0" => ("both", k.clone()),
                _ => return Err("% Usage: pre-shared-key [local|remote] KEY (one word)".into()),
            };
            if key.chars().count() > MAX_CRYPTO_TEXT {
                return Err(format!("% Keys are at most {MAX_CRYPTO_TEXT} characters"));
            }
            let p = peer(c)?;
            if which != "remote" {
                p.local_key = Some(key.clone());
            }
            if which != "local" {
                p.remote_key = Some(key);
            }
            Ok(String::new())
        }
        Id::Ikev2MatchRemote => {
            let w: Vec<String> = arg(0).split_whitespace().map(str::to_string).collect();
            let addr = parse_ip(w.first().map_or("", String::as_str)).map_err(|e| format!("% {e}"))?;
            let mask = match w.get(1) {
                Some(m) => parse_ip(m).map_err(|e| format!("% {e}"))?,
                None => Ipv4Addr::BROADCAST,
            };
            let p = profile(c)?;
            if !p.remote.contains(&(addr, mask)) && p.remote.len() < MAX_CRYPTO {
                p.remote.push((addr, mask));
            }
            Ok(String::new())
        }
        Id::Ikev2IdentityLocal => {
            let a = parse_ip(&arg(0)).map_err(|e| format!("% {e}"))?;
            profile(c)?.local_id = Some(a);
            Ok(String::new())
        }
        Id::Ikev2AuthLocal | Id::Ikev2AuthRemote => {
            let m = arg(0).to_ascii_lowercase();
            if m != "pre-share" {
                return Err("% Only pre-share authentication is simulated".into());
            }
            let p = profile(c)?;
            if id == Id::Ikev2AuthLocal {
                p.auth_local = Some(m);
            } else {
                p.auth_remote = Some(m);
            }
            Ok(String::new())
        }
        Id::Ikev2ProfileKeyring => {
            let k = name_ok(&arg(0))?;
            let note = if c.d()?.crypto.ikev2.keyrings.iter().any(|x| x.name == k) {
                String::new()
            } else {
                format!("Note: keyring {k} does not exist yet")
            };
            profile(c)?.keyring = Some(k);
            Ok(note)
        }
        Id::CryptoSetIkev2Profile | Id::NoCryptoSetIkev2Profile => {
            let Mode::CryptoMap(name, seq) = c.cli.mode.clone() else { return Err("% Not in crypto map mode".into()) };
            let prof = if id == Id::CryptoSetIkev2Profile { Some(name_ok(&arg(0))?) } else { None };
            let note = match &prof {
                Some(p) if c.d()?.crypto.ikev2.profile(p).is_none() => format!("Note: IKEv2 profile {p} does not exist yet"),
                _ => String::new(),
            };
            let e = c.dm()?.crypto.maps.iter_mut().find(|e| e.map == name && e.seq == seq).ok_or("% The crypto map entry was deleted")?;
            e.ikev2_profile = prof;
            Ok(note)
        }
        Id::ShowCryptoIkev2Sa => Ok(ikev2_sas(c.sim, c.d()?)),
        Id::ShowCryptoIkev2Proposal => Ok(proposals(c.d()?)),
        Id::ShowCryptoIkev2Profile => Ok(profiles(c.d()?)),
        _ => Err("% Not an IKEv2 command".into()),
    }
}

// ---- configuration ----

/// The `crypto ikev2` blocks of the running configuration (before the crypto maps that name
/// the profiles).
pub(crate) fn running_config(s: &mut String, d: &Device) {
    let v2 = &d.crypto.ikev2;
    for p in &v2.proposals {
        let _ = writeln!(s, "crypto ikev2 proposal {}", p.name);
        if !p.encryption.is_empty() {
            let _ = writeln!(s, " encryption {}", p.encryption.join(" "));
        }
        if !p.integrity.is_empty() {
            let _ = writeln!(s, " integrity {}", p.integrity.join(" "));
        }
        if !p.group.is_empty() {
            let g: Vec<String> = p.group.iter().map(u8::to_string).collect();
            let _ = writeln!(s, " group {}", g.join(" "));
        }
        let _ = writeln!(s, "!");
    }
    for p in &v2.policies {
        let _ = writeln!(s, "crypto ikev2 policy {}", p.name);
        if let Some(a) = p.local {
            let _ = writeln!(s, " match address local {a}");
        }
        for n in &p.proposals {
            let _ = writeln!(s, " proposal {n}");
        }
        let _ = writeln!(s, "!");
    }
    for k in &v2.keyrings {
        let _ = writeln!(s, "crypto ikev2 keyring {}", k.name);
        for p in &k.peers {
            let _ = writeln!(s, " peer {}", p.name);
            if let Some(a) = p.address {
                let _ = writeln!(s, "  address {a}");
            }
            match (&p.local_key, &p.remote_key) {
                (Some(l), Some(r)) if l == r => {
                    let _ = writeln!(s, "  pre-shared-key {l}");
                }
                (l, r) => {
                    if let Some(l) = l {
                        let _ = writeln!(s, "  pre-shared-key local {l}");
                    }
                    if let Some(r) = r {
                        let _ = writeln!(s, "  pre-shared-key remote {r}");
                    }
                }
            }
        }
        let _ = writeln!(s, "!");
    }
    for p in &v2.profiles {
        let _ = writeln!(s, "crypto ikev2 profile {}", p.name);
        for (a, m) in &p.remote {
            if *m == Ipv4Addr::BROADCAST {
                let _ = writeln!(s, " match identity remote address {a}");
            } else {
                let _ = writeln!(s, " match identity remote address {a} {m}");
            }
        }
        if let Some(a) = p.local_id {
            let _ = writeln!(s, " identity local address {a}");
        }
        if let Some(a) = &p.auth_remote {
            let _ = writeln!(s, " authentication remote {a}");
        }
        if let Some(a) = &p.auth_local {
            let _ = writeln!(s, " authentication local {a}");
        }
        if let Some(k) = &p.keyring {
            let _ = writeln!(s, " keyring local {k}");
        }
        let _ = writeln!(s, "!");
    }
}

// ---- reports ----

/// `show crypto ikev2 sa`.
pub(crate) fn ikev2_sas(sim: &Sim, d: &Device) -> String {
    let sas: Vec<_> = sim.ipsec_sas(d.id).into_iter().filter(|s| s.v2.is_some()).collect();
    if sas.is_empty() {
        return "No IKEv2 security associations (one is set up when traffic matches a crypto map with an IKEv2 profile)".into();
    }
    let mut s = String::new();
    for sa in sas {
        let Some(v2) = &sa.v2 else { continue };
        let hex = |b: &[u8; 8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        let _ = writeln!(
            s,
            "Peer {} (local {}), {}, {}",
            sa.peer,
            sa.local,
            if sa.initiator { "this router started it" } else { "the peer started it" },
            match sa.phase {
                IkePhase::Established => "established".to_string(),
                p => p.describe(),
            }
        );
        let _ = writeln!(s, "  SPIs: initiator {} responder {}", hex(&v2.spi_i), hex(&v2.spi_r));
        if !v2.algorithms.is_empty() {
            let _ = writeln!(s, "  Algorithms: {}", v2.algorithms);
        }
        if let Some(p) = &v2.profile {
            let _ = writeln!(s, "  Profile: {p}");
        }
        if sa.phase == IkePhase::Established {
            let esp = match v2.esp_suite {
                Some(suite) => format!("{} (real ciphers)", suite.describe()),
                None => "the simulator's stand-in cipher (AH next to ESP in one transform set is not implemented)".to_string(),
            };
            let _ = writeln!(s, "  ESP: {esp}");
        }
        if let Some(why) = &sa.failed {
            let _ = writeln!(s, "  Last failure: {why}");
        }
    }
    s.trim_end().to_string()
}

/// `show crypto ikev2 proposal`.
pub(crate) fn proposals(d: &Device) -> String {
    let v2 = &d.crypto.ikev2;
    if v2.proposals.is_empty() {
        return "No IKEv2 proposals".into();
    }
    let mut s = String::new();
    for p in &v2.proposals {
        let g: Vec<String> = p.group.iter().map(u8::to_string).collect();
        let _ = writeln!(
            s,
            "Proposal {}: encryption {}; integrity {}; DH groups {}{}",
            p.name,
            if p.encryption.is_empty() { "-".into() } else { p.encryption.join(" ") },
            if p.integrity.is_empty() { "-".into() } else { p.integrity.join(" ") },
            if g.is_empty() { "-".into() } else { g.join(" ") },
            if p.complete() { "" } else { " (incomplete: not offered)" }
        );
    }
    s.trim_end().to_string()
}

/// `show crypto ikev2 profile`.
pub(crate) fn profiles(d: &Device) -> String {
    let v2 = &d.crypto.ikev2;
    if v2.profiles.is_empty() {
        return "No IKEv2 profiles".into();
    }
    let mut s = String::new();
    for p in &v2.profiles {
        let _ = writeln!(s, "Profile {}", p.name);
        let remote: Vec<String> = p.remote.iter().map(|(a, m)| if *m == Ipv4Addr::BROADCAST { a.to_string() } else { format!("{a} {m}") }).collect();
        let _ = writeln!(s, "  Remote identities: {}", if remote.is_empty() { "none".into() } else { remote.join(", ") });
        let _ = writeln!(s, "  Local identity: {}", p.local_id.map_or("the interface's address".to_string(), |a| a.to_string()));
        let _ = writeln!(
            s,
            "  Authentication: local {}, remote {}",
            p.auth_local.as_deref().unwrap_or("not set"),
            p.auth_remote.as_deref().unwrap_or("not set")
        );
        let _ = writeln!(s, "  Keyring: {}", p.keyring.as_deref().unwrap_or("none"));
    }
    s.trim_end().to_string()
}
