//! IPsec transform sets with no cipher (ESP with null encryption) and transport mode, under
//! IKEv1 and IKEv2.

use super::{ping_settled, two_sites};

const LAN1: &str = "192.168.1.0 0.0.0.255 192.168.2.0 0.0.0.255";
const LAN2: &str = "192.168.2.0 0.0.0.255 192.168.1.0 0.0.0.255";
const HOSTS1: &str = "host 203.0.113.1 host 198.51.100.1";
const HOSTS2: &str = "host 198.51.100.1 host 203.0.113.1";

/// A site's IPsec configuration: IKEv1 or IKEv2 (`v2`) with a pre-shared key, one transform set.
fn config(v2: bool, acl: &str, peer: &str, transform_set: &str, mode: &str) -> String {
    let ike = if v2 {
        format!(
            "crypto ikev2 proposal PROP
encryption aes-cbc-256
integrity sha256
group 14
exit
crypto ikev2 policy POL
proposal PROP
exit
crypto ikev2 keyring KR
peer SITE
address {peer}
pre-shared-key s3cret
exit
exit
crypto ikev2 profile PROF
match identity remote address {peer} 255.255.255.255
authentication remote pre-share
authentication local pre-share
keyring local KR
exit
"
        )
    } else {
        format!(
            "crypto isakmp policy 10
encryption aes 256
hash sha
authentication pre-share
group 14
exit
crypto isakmp key s3cret address {peer}
"
        )
    };
    let profile = if v2 { "set ikev2-profile PROF\n" } else { "" };
    format!(
        "conf t
access-list 110 permit ip {acl}
{ike}crypto ipsec transform-set TS {transform_set}
{mode}exit
crypto map VPN 10 ipsec-isakmp
set peer {peer}
set transform-set TS
{profile}match address 110
exit
int g0/1
crypto map VPN
end"
    )
}

#[test]
fn a_transform_set_with_no_cipher_is_esp_with_null_encryption() {
    for (v2, integ, shown) in [
        (false, "esp-sha-hmac", "NULL / HMAC-SHA1-96"),
        (true, "esp-sha256-hmac", "NULL / HMAC-SHA256-128"),
        (false, "esp-md5-hmac", "NULL / HMAC-MD5-96"),
    ] {
        let (mut lab, r1, _isp, r2, a) = two_sites();
        lab.run(r1, &config(v2, LAN1, "198.51.100.1", integ, ""));
        lab.run(r2, &config(v2, LAN2, "203.0.113.1", integ, ""));
        lab.sim.clear_trace();
        let ping = ping_settled(&mut lab, a, "192.168.2.10");
        assert!(ping.contains("4 answered"), "{integ}: {ping}");
        let sa = lab.run(r1, "show crypto ipsec sa");
        assert!(sa.contains(&format!("ESP: {shown} (real ciphers)")) && !sa.contains("stand-in"), "{sa}");
        assert!(sa.contains("Mode: tunnel"), "{sa}");
        // The same suite on the far end.
        assert!(lab.run(r2, "show crypto ipsec sa").contains(shown));
    }
    // A set with a cipher and one with nothing real to run are told apart.
    let (mut lab, r1, ..) = two_sites();
    lab.run(r1, "conf t\ncrypto ipsec transform-set NULLONLY esp-null\nend");
    assert!(lab.run(r1, "show running-config").contains("crypto ipsec transform-set NULLONLY esp-null"));
}

#[test]
fn transport_mode_protects_only_the_payload_of_traffic_between_the_peers() {
    for (v2, ts) in [(false, "esp-aes esp-sha-hmac"), (true, "esp-aes esp-sha-hmac"), (false, "esp-sha-hmac"), (false, "ah-sha-hmac")] {
        let (mut lab, r1, isp, r2, _a) = two_sites();
        lab.run(r1, &config(v2, HOSTS1, "198.51.100.1", ts, "mode transport\n"));
        lab.run(r2, &config(v2, HOSTS2, "203.0.113.1", ts, "mode transport\n"));
        lab.sim.clear_trace();
        let ping = ping_settled(&mut lab, r1, "198.51.100.1");
        assert!(ping.contains("100% answered"), "{v2} {ts}: {ping}");
        let sa = lab.run(r1, "show crypto ipsec sa");
        assert!(sa.contains("Mode: transport"), "{sa}");
        assert!(lab.run(r1, "show crypto map").contains("transport mode"));
        // On the ISP's links the packets are ESP (or AH) between the public addresses, with the
        // peers' own header, and no ICMP shows in the clear.
        let proto = if ts.starts_with("ah") { "AH SPI" } else { "ESP SPI" };
        assert!(lab.sim.trace().iter().any(|e| e.to.device == isp && e.summary.starts_with(proto)), "{ts}");
        assert!(lab.sim.trace().iter().filter(|e| e.to.device == isp).all(|e| !e.summary.starts_with("ICMP")), "{ts}");
        let (enc, dec) = (count_after(&sa, "encapsulated "), count_after(&lab.run(r2, "show crypto ipsec sa"), "decapsulated "));
        assert!(enc >= 4 && dec >= 4, "{sa}");
    }
}

fn count_after(text: &str, what: &str) -> u64 {
    text.split(what).nth(1).and_then(|t| t.trim().split(|c: char| !c.is_ascii_digit()).next()).and_then(|n| n.parse().ok()).unwrap_or(0)
}

#[test]
fn transport_mode_drops_traffic_that_is_not_between_the_peers_and_modes_must_agree() {
    for v2 in [false, true] {
        // Site traffic through a transport set: the association comes up, the packets are dropped.
        let (mut lab, r1, _isp, r2, a) = two_sites();
        lab.run(r1, &config(v2, LAN1, "198.51.100.1", "esp-aes esp-sha-hmac", "mode transport\n"));
        lab.run(r2, &config(v2, LAN2, "203.0.113.1", "esp-aes esp-sha-hmac", "mode transport\n"));
        lab.sim.clear_trace();
        assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"), "v2={v2}");
        assert!(
            lab.sim
                .trace()
                .iter()
                .any(|e| e.dropped.as_deref().is_some_and(|d| d.contains("transport mode protects only traffic between this router"))),
            "v2={v2}"
        );
        // One end says transport and the other tunnel: no association.
        let (mut lab, r1, _isp, r2, a) = two_sites();
        lab.run(r1, &config(v2, LAN1, "198.51.100.1", "esp-aes esp-sha-hmac", "mode transport\n"));
        lab.run(r2, &config(v2, LAN2, "203.0.113.1", "esp-aes esp-sha-hmac", ""));
        assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"), "v2={v2}");
        let sa = if v2 { lab.run(r1, "show crypto ikev2 sa") + &lab.run(r2, "show crypto ikev2 sa") } else { lab.run(r1, "show crypto isakmp sa") };
        assert!(sa.contains("transform sets differ") || sa.contains("another mode"), "v2={v2}: {sa}");
    }
}

#[test]
fn esp_null_alone_is_refused_because_it_protects_nothing() {
    for v2 in [false, true] {
        let (mut lab, r1, _isp, r2, a) = two_sites();
        lab.run(r1, &config(v2, LAN1, "198.51.100.1", "esp-null", ""));
        lab.run(r2, &config(v2, LAN2, "203.0.113.1", "esp-null", ""));
        assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"), "v2={v2}");
        let sa = lab.run(r1, "show crypto ipsec sa");
        assert!(sa.contains("protects nothing") && sa.contains("RFC 4303"), "v2={v2}: {sa}");
        // With an integrity transform next to it the set negotiates, with nothing simulated.
        lab.run(r1, "conf t\ncrypto ipsec transform-set TS esp-null esp-sha-hmac\nend");
        lab.run(r2, "conf t\ncrypto ipsec transform-set TS esp-null esp-sha-hmac\nend");
        lab.run(r1, "clear crypto sa");
        lab.run(r2, "clear crypto sa");
        let ping = ping_settled(&mut lab, a, "192.168.2.10");
        assert!(ping.contains("4 answered"), "v2={v2}: {ping}");
        assert!(lab.run(r1, "show crypto ipsec sa").contains("NULL / HMAC-SHA1-96 (real ciphers)"));
    }
}
