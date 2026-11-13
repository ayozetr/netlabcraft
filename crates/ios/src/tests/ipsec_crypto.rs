//! Typed console tests for IPsec with real Diffie-Hellman and ciphers: IKEv1 and IKEv2 with
//! every cipher combination, mismatches, and keys renewed over simulated time.

use super::{Lab, ping_settled, two_sites};

fn v1_config(acl: &str, peer: &str, key: &str, enc: &str, hash: &str, group: u8, ts: &str) -> String {
    format!(
        "conf t
access-list 110 permit ip {acl}
crypto isakmp policy 10
encryption {enc}
hash {hash}
authentication pre-share
group {group}
exit
crypto isakmp key {key} address {peer}
crypto ipsec transform-set TS {ts}
exit
crypto map VPN 10 ipsec-isakmp
set peer {peer}
set transform-set TS
match address 110
exit
int g0/1
crypto map VPN
end"
    )
}

const SITE1: (&str, &str) = ("192.168.1.0 0.0.0.255 192.168.2.0 0.0.0.255", "198.51.100.1");
const SITE2: (&str, &str) = ("192.168.2.0 0.0.0.255 192.168.1.0 0.0.0.255", "203.0.113.1");

/// The outbound SPI `show crypto ipsec sa` reports.
fn spi_out(lab: &mut Lab, r: netlabcraft_core::DeviceId) -> String {
    let sa = lab.run(r, "show crypto ipsec sa");
    sa.split("Outbound ESP SPI ").nth(1).and_then(|t| t.split(',').next()).unwrap_or_default().to_string()
}

#[test]
fn ikev1_runs_every_cipher_combination_for_real() {
    for (enc, hash, group, ts, esp) in [
        ("aes 256", "sha", 14, "esp-aes 256 esp-sha256-hmac", "AES-CBC-256 / HMAC-SHA256-128"),
        ("aes", "sha256", 5, "esp-aes esp-sha-hmac", "AES-CBC-128 / HMAC-SHA1-96"),
        ("aes 192", "md5", 2, "esp-aes 192 esp-md5-hmac", "AES-CBC-192 / HMAC-MD5-96"),
        ("aes 128", "sha", 1, "esp-null esp-sha256-hmac", "NULL / HMAC-SHA256-128"),
        ("3des", "sha", 2, "esp-3des esp-sha-hmac", "3DES-CBC / HMAC-SHA1-96"),
        ("des", "md5", 1, "esp-des esp-md5-hmac", "DES-CBC / HMAC-MD5-96"),
        ("3des", "sha256", 14, "esp-3des esp-sha256-hmac", "3DES-CBC / HMAC-SHA256-128"),
    ] {
        let (mut lab, r1, isp, r2, a) = two_sites();
        lab.run(r1, &v1_config(SITE1.0, SITE1.1, "s3cret", enc, hash, group, ts));
        lab.run(r2, &v1_config(SITE2.0, SITE2.1, "s3cret", enc, hash, group, ts));
        lab.sim.clear_trace();
        let ping = ping_settled(&mut lab, a, "192.168.2.10");
        assert!(ping.contains("4 answered"), "{enc} {hash} {group} {ts}: {ping}");
        let isakmp = lab.run(r1, "show crypto isakmp sa");
        assert!(isakmp.contains("established (pre-shared key; "), "{isakmp}");
        assert!(isakmp.contains(&format!("DH group {group}")), "{isakmp}");
        let cipher = match enc {
            "des" => "DES-CBC",
            "3des" => "3DES-CBC",
            _ => "AES-CBC",
        };
        assert!(isakmp.contains(cipher), "{enc}: {isakmp}");
        let sa = lab.run(r1, "show crypto ipsec sa");
        assert!(sa.contains(esp), "{ts}: {sa}");
        assert!(sa.contains("Lifetime left: ") && sa.contains("of 3600 seconds, ") && sa.contains("of 4608000 kilobytes"), "{sa}");
        // Main mode 5 and 6 and quick mode travel encrypted; the private addresses never show
        // on the ISP's links.
        assert!(lab.sim.trace().iter().filter(|e| e.to.device == isp).all(|e| !(e.summary.starts_with("ICMP") && e.summary.contains("192.168."))));
    }
}

#[test]
fn ikev1_mismatches_fail_where_the_exchange_finds_them() {
    // Different pre-shared keys: message 5 doesn't decrypt to a valid HASH_I.
    let (mut lab, r1, _isp, r2, a) = two_sites();
    lab.run(r1, &v1_config(SITE1.0, SITE1.1, "one", "aes 256", "sha256", 14, "esp-aes 256 esp-sha256-hmac"));
    lab.run(r2, &v1_config(SITE2.0, SITE2.1, "two", "aes 256", "sha256", 14, "esp-aes 256 esp-sha256-hmac"));
    lab.sim.clear_trace();
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    assert!(lab.run(r2, "show crypto isakmp sa").contains("main mode message 5 failed authentication"));
    assert!(lab.run(r1, "show crypto isakmp sa").contains("AUTHENTICATION-FAILED"));
    // Policies that differ only in their DH group: no proposal chosen at message 2.
    let (mut lab, r1, _isp, r2, a) = two_sites();
    lab.run(r1, &v1_config(SITE1.0, SITE1.1, "k", "aes 256", "sha", 14, "esp-aes esp-sha-hmac"));
    lab.run(r2, &v1_config(SITE2.0, SITE2.1, "k", "aes 256", "sha", 5, "esp-aes esp-sha-hmac"));
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    assert!(lab.run(r2, "show crypto isakmp sa").contains("refused main mode message 1"));
    assert!(lab.run(r1, "show crypto isakmp sa").contains("NO-PROPOSAL-CHOSEN"));
    // ESP key sizes that differ: refused at quick mode.
    let (mut lab, r1, _isp, r2, a) = two_sites();
    lab.run(r1, &v1_config(SITE1.0, SITE1.1, "k", "aes", "sha", 14, "esp-aes 256 esp-sha-hmac"));
    lab.run(r2, &v1_config(SITE2.0, SITE2.1, "k", "aes", "sha", 14, "esp-aes 128 esp-sha-hmac"));
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    assert!(lab.run(r2, "show crypto isakmp sa").contains("refused quick mode message 1"), "{}", lab.run(r2, "show crypto isakmp sa"));
    assert!(lab.run(r1, "show crypto isakmp sa").contains("transform sets differ"));
}

fn v2_config(acl: &str, peer: &str, enc: &str, integ: &str, groups: &str, ts: &str) -> String {
    format!(
        "conf t
access-list 110 permit ip {acl}
crypto ikev2 proposal PROP
encryption {enc}
integrity {integ}
group {groups}
exit
crypto ikev2 policy POL
proposal PROP
exit
crypto ikev2 keyring KR
peer SITE
address {peer}
pre-shared-key v2key
exit
exit
crypto ikev2 profile PROF
match identity remote address {peer} 255.255.255.255
authentication remote pre-share
authentication local pre-share
keyring local KR
exit
crypto ipsec transform-set TS {ts}
exit
crypto map VPN 10 ipsec-isakmp
set peer {peer}
set transform-set TS
set ikev2-profile PROF
match address 110
exit
int g0/1
crypto map VPN
end"
    )
}

#[test]
fn ikev2_runs_every_cipher_combination_for_real() {
    for (enc, integ, group, ts, ike, esp) in [
        (
            "aes-cbc-256",
            "sha256",
            "14",
            "esp-aes 256 esp-sha256-hmac",
            "AES-CBC-256, PRF HMAC-SHA256, HMAC-SHA256-128, DH group 14",
            "AES-CBC-256 / HMAC-SHA256-128",
        ),
        ("aes-cbc-128", "sha1", "2", "esp-aes esp-sha-hmac", "AES-CBC-128, PRF HMAC-SHA1, HMAC-SHA1-96, DH group 2", "AES-CBC-128 / HMAC-SHA1-96"),
        ("aes-cbc-192", "md5", "5", "esp-aes 192 esp-md5-hmac", "AES-CBC-192, PRF HMAC-MD5, HMAC-MD5-96, DH group 5", "AES-CBC-192 / HMAC-MD5-96"),
        ("aes-cbc-256", "sha256", "19", "esp-null esp-sha256-hmac", "DH group 19", "NULL / HMAC-SHA256-128"),
        ("3des", "sha1", "14", "esp-3des esp-sha-hmac", "3DES, PRF HMAC-SHA1, HMAC-SHA1-96, DH group 14", "3DES-CBC / HMAC-SHA1-96"),
        ("3des", "sha256", "5", "esp-des esp-md5-hmac", "3DES, PRF HMAC-SHA256, HMAC-SHA256-128, DH group 5", "DES-CBC / HMAC-MD5-96"),
    ] {
        let (mut lab, r1, _isp, r2, a) = two_sites();
        lab.run(r1, &v2_config(SITE1.0, SITE1.1, enc, integ, group, ts));
        lab.run(r2, &v2_config(SITE2.0, SITE2.1, enc, integ, group, ts));
        let ping = ping_settled(&mut lab, a, "192.168.2.10");
        assert!(ping.contains("4 answered"), "{enc} {integ} {group}: {ping}");
        let sa = lab.run(r1, "show crypto ikev2 sa");
        assert!(sa.contains(ike), "{sa}");
        assert!(sa.contains(&format!("ESP: {esp} (real ciphers)")), "{sa}");
        assert!(lab.run(r2, "show crypto ikev2 sa").contains(&format!("ESP: {esp} (real ciphers)")));
    }
}

#[test]
fn ikev2_asks_for_another_key_exchange_group() {
    // R1 offers groups 5 and 14 and sends its key exchange for 5; R2 only takes 14, so it asks
    // for a group 14 value with INVALID_KE_PAYLOAD and R1 starts again with one.
    let (mut lab, r1, _isp, r2, a) = two_sites();
    lab.run(r1, &v2_config(SITE1.0, SITE1.1, "aes-cbc-256", "sha256", "5 14", "esp-aes 256 esp-sha256-hmac"));
    lab.run(r2, &v2_config(SITE2.0, SITE2.1, "aes-cbc-256", "sha256", "14", "esp-aes 256 esp-sha256-hmac"));
    lab.sim.clear_trace();
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    assert!(lab.sim.trace().iter().any(|e| e.summary.contains("IKEv2 IKE_SA_INIT response #0 [Notify]")));
    assert!(lab.run(r1, "show crypto ikev2 sa").contains("DH group 14"));
    assert!(lab.run(r1, "show logging").contains("wants a key exchange for DH group 14"));
    // Different keys still fail authentication, with real keys behind the AUTH values.
    lab.run(r2, "conf t\ncrypto ikev2 keyring KR\npeer SITE\npre-shared-key other\nend");
    lab.run(r1, "clear crypto sa");
    lab.run(r2, "clear crypto sa");
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    assert!(lab.run(r1, "show crypto ikev2 sa").contains("AUTHENTICATION_FAILED"));
}

#[test]
fn lifetimes_are_configured_shown_and_replayed() {
    let mut lab = Lab::new();
    let r = lab.add(netlabcraft_model::DeviceKind::Router);
    lab.run(r, "en\nconf t");
    lab.run(r, "crypto ipsec security-association lifetime seconds 600");
    lab.run(r, "crypto ipsec security-association lifetime kilobytes 10000");
    assert!(lab.try_run(r, "crypto ipsec security-association lifetime seconds 5").contains("120-86400"));
    assert!(lab.try_run(r, "crypto ipsec security-association lifetime kilobytes 1").contains("2560-"));
    assert!(lab.try_run(r, "crypto ipsec security-association lifetime hours 1").contains("seconds or kilobytes"));
    lab.run(r, "crypto map VPN 10 ipsec-isakmp");
    lab.run(r, "set security-association lifetime seconds 300");
    lab.run(r, "end");
    let run = lab.run(r, "show running-config");
    assert!(
        run.contains("crypto ipsec security-association lifetime seconds 600\ncrypto ipsec security-association lifetime kilobytes 10000\n"),
        "{run}"
    );
    assert!(run.contains(" set security-association lifetime seconds 300\n"), "{run}");
    let d = lab.t.device(r).unwrap();
    assert_eq!(d.crypto.lifetime(&d.crypto.maps[0]), (300, 10_000));
    lab.run(r, "conf t\ncrypto map VPN 10 ipsec-isakmp\nno set security-association lifetime seconds\nexit\nno crypto ipsec security-association lifetime kilobytes\nend");
    let d = lab.t.device(r).unwrap();
    assert_eq!(d.crypto.lifetime(&d.crypto.maps[0]), (600, 4_608_000));
}

/// Pings, lets `secs` of simulated time pass, pings again.
fn ping_after(lab: &mut Lab, from: netlabcraft_core::DeviceId, secs: u64) -> String {
    lab.sim.run_for(&lab.t, secs * 1_000_000);
    ping_settled(lab, from, "192.168.2.10")
}

#[test]
fn ikev1_renews_keys_with_quick_mode_before_the_lifetime_ends() {
    let (mut lab, r1, _isp, r2, a) = two_sites();
    for (r, site) in [(r1, SITE1), (r2, SITE2)] {
        lab.run(r, &v1_config(site.0, site.1, "k", "aes 256", "sha", 14, "esp-aes 256 esp-sha256-hmac"));
        lab.run(r, "conf t\ncrypto ipsec security-association lifetime seconds 120\nend");
    }
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    let first = spi_out(&mut lab, r1);
    assert!(lab.run(r1, "show crypto ipsec sa").contains("of 120 seconds"));
    // Past 90 % of the lifetime the next packet starts a new quick mode; traffic keeps flowing.
    lab.sim.clear_trace();
    let ping = ping_after(&mut lab, a, 110);
    assert!(ping.contains("4 answered"), "{ping}");
    let sa = lab.run(r1, "show crypto ipsec sa");
    assert!(sa.contains("keys renewed 1 time;") || sa.contains("keys renewed 1 time\n") || sa.ends_with("keys renewed 1 time"), "{sa}");
    assert_ne!(spi_out(&mut lab, r1), first);
    assert!(lab.run(r2, "show crypto ipsec sa").contains("keys renewed 1 time"));
    assert!(lab.run(r1, "show logging").contains("quick mode renews its keys"));
    // Only quick mode ran again (no new main mode): three ISAKMP messages.
    let isakmp = lab.sim.trace().iter().filter(|e| e.summary.contains("ISAKMP") && e.to.device == r2).count();
    assert!(isakmp <= 2, "{isakmp}");
    // Left idle past the whole lifetime, the SA is expired; the next packet negotiates anew.
    lab.sim.run_for(&lab.t, 200 * 1_000_000);
    assert!(lab.run(r1, "show crypto ipsec sa").contains("expired"));
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    assert!(lab.run(r1, "show logging").contains("reached the end of its lifetime"));
    assert!(lab.run(r1, "show crypto ipsec sa").contains("keys renewed 0 times"));
}

#[test]
fn ikev2_renews_the_child_sa_and_deletes_the_old_one() {
    let (mut lab, r1, _isp, r2, a) = two_sites();
    for (r, site) in [(r1, SITE1), (r2, SITE2)] {
        lab.run(r, &v2_config(site.0, site.1, "aes-cbc-256", "sha256", "14", "esp-aes 256 esp-sha256-hmac"));
        lab.run(r, "conf t\ncrypto map VPN 10 ipsec-isakmp\nset security-association lifetime seconds 120\nend");
    }
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    let first = spi_out(&mut lab, r1);
    lab.sim.clear_trace();
    assert!(ping_after(&mut lab, a, 110).contains("4 answered"));
    let ike: Vec<String> = lab.sim.trace().iter().filter(|e| e.summary.contains("IKEv2")).map(|e| e.summary.clone()).collect();
    assert!(ike.iter().any(|s| s.contains("IKEv2 CREATE_CHILD_SA request #2 [SK]")), "{ike:#?}");
    assert!(ike.iter().any(|s| s.contains("IKEv2 CREATE_CHILD_SA response #2 [SK]")), "{ike:#?}");
    assert!(ike.iter().any(|s| s.contains("IKEv2 INFORMATIONAL request #3 [SK]")), "{ike:#?}");
    assert!(ike.iter().any(|s| s.contains("IKEv2 INFORMATIONAL response #3 [SK]")), "{ike:#?}");
    assert!(!ike.iter().any(|s| s.contains("IKE_SA_INIT")), "{ike:#?}");
    assert_ne!(spi_out(&mut lab, r1), first);
    for r in [r1, r2] {
        let sa = lab.run(r, "show crypto ipsec sa");
        assert!(sa.contains("keys renewed 1 time") && !sa.contains("Previous inbound"), "{sa}");
        assert!(lab.run(r, "show logging").contains("the old child SA (inbound SPI 0x"));
    }
    // A second rekey later uses the next message IDs.
    lab.sim.clear_trace();
    assert!(ping_after(&mut lab, a, 110).contains("4 answered"));
    assert!(lab.sim.trace().iter().any(|e| e.summary.contains("IKEv2 CREATE_CHILD_SA request #4 [SK]")));
    assert!(lab.run(r2, "show crypto ipsec sa").contains("keys renewed 2 times"));
    // Past the lifetime with no traffic: negotiated again from IKE_SA_INIT.
    lab.sim.run_for(&lab.t, 200 * 1_000_000);
    lab.sim.clear_trace();
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    assert!(lab.sim.trace().iter().any(|e| e.summary.contains("IKE_SA_INIT")));
}
