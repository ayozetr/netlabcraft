//! IKE with RSA signatures: main mode (IKEv1) and IKE_AUTH (IKEv2) sign with the certificate of a
//! trustpoint and verify the peer's chain against the authorities this router trusts.

use netlabcraft_core::DeviceId;

use super::{Lab, ping_settled, two_sites};

const SITE1: &str = "192.168.1.0 0.0.0.255 192.168.2.0 0.0.0.255";
const SITE2: &str = "192.168.2.0 0.0.0.255 192.168.1.0 0.0.0.255";

/// Gets a certificate for `r` from the CA at `ca` (a trustpoint named `T`).
fn enroll(lab: &mut Lab, r: DeviceId, ca: &str, cn: &str) {
    lab.run(
        r,
        &format!(
            "conf t\ncrypto pki trustpoint T\nenrollment url http://{ca}\nsubject-name CN={cn}, O=NetLab\nexit\ncrypto pki authenticate T\ncrypto pki enroll T\nend"
        ),
    );
}

/// The two sites of `two_sites`, the ISP running the certificate authority, both routers enrolled.
fn sites_with_certificates() -> (Lab, DeviceId, DeviceId, DeviceId, DeviceId) {
    let (mut lab, r1, isp, r2, a) = two_sites();
    lab.run(isp, "conf t\nip http server\ncrypto pki server ROOT\nissuer-name CN=Lab Root, O=NetLab\ngrant auto\nno shutdown\nend");
    enroll(&mut lab, r1, "203.0.113.2", "r1.lab.test");
    enroll(&mut lab, r2, "198.51.100.2", "r2.lab.test");
    (lab, r1, isp, r2, a)
}

fn v1_config(acl: &str, peer: &str, auth: &str) -> String {
    let key = if auth == "pre-share" { format!("crypto isakmp key unused address {peer}\n") } else { String::new() };
    format!(
        "conf t
access-list 110 permit ip {acl}
crypto isakmp policy 10
encryption aes 256
hash sha256
authentication {auth}
group 14
exit
{key}crypto ipsec transform-set TS esp-aes esp-sha-hmac
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

fn v2_config(acl: &str, peer: &str) -> String {
    format!(
        "conf t
access-list 110 permit ip {acl}
crypto ikev2 proposal PROP
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
pre-shared-key unused
exit
exit
crypto ikev2 profile PROF
match identity remote address {peer} 255.255.255.255
authentication remote rsa-sig
authentication local rsa-sig
pki trustpoint T
keyring local KR
exit
crypto ipsec transform-set TS esp-aes esp-sha-hmac
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
fn ikev1_main_mode_signs_and_verifies_with_certificates() {
    let (mut lab, r1, isp, r2, a) = sites_with_certificates();
    // No pre-shared key is needed: only the policy says `rsa-sig`.
    lab.run(r1, &v1_config(SITE1, "198.51.100.1", "rsa-sig"));
    lab.run(r2, &v1_config(SITE2, "203.0.113.1", "rsa-sig"));
    lab.sim.clear_trace();
    let ping = ping_settled(&mut lab, a, "192.168.2.10");
    assert!(ping.contains("4 answered"), "{ping}");
    let sa = lab.run(r1, "show crypto isakmp sa");
    assert!(sa.contains("established (RSA signature, peer certificate CN=r2.lab.test, O=NetLab;"), "{sa}");
    let sa2 = lab.run(r2, "show crypto isakmp sa");
    assert!(sa2.contains("established (RSA signature, peer certificate CN=r1.lab.test, O=NetLab;"), "{sa2}");
    assert!(lab.run(r1, "show crypto isakmp policy").contains("authentication rsa-sig"));
    // Messages 5 and 6 carry the certificate and the signature, and travel encrypted.
    assert!(lab.sim.trace().iter().filter(|e| e.to.device == isp).all(|e| !e.summary.contains("192.168.")));
}

#[test]
fn ikev1_signature_failures_say_why() {
    // No certificate at one end.
    let (mut lab, r1, _isp, r2, a) = sites_with_certificates();
    lab.run(r1, &v1_config(SITE1, "198.51.100.1", "rsa-sig"));
    lab.run(r2, &v1_config(SITE2, "203.0.113.1", "rsa-sig"));
    lab.run(r2, "conf t\nno crypto pki trustpoint T\nend");
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    let sa = lab.run(r1, "show crypto isakmp sa");
    assert!(sa.contains("failed:") && sa.contains("certificate"), "{sa}");

    // A peer whose certificate was issued by an authority this router does not trust.
    let (mut lab, r1, isp, r2, a) = sites_with_certificates();
    lab.run(r1, &v1_config(SITE1, "198.51.100.1", "rsa-sig"));
    lab.run(r2, &v1_config(SITE2, "203.0.113.1", "rsa-sig"));
    lab.run(r2, "conf t\ncrypto pki trustpoint SELF\nenrollment selfsigned\nsubject-name CN=r2.lab.test, O=NetLab\nexit\ncrypto pki enroll SELF\nno crypto pki trustpoint T\nend");
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    let sa = lab.run(r1, "show crypto isakmp sa");
    assert!(sa.contains("failed:") && sa.contains("does not trust the authority"), "{sa}");
    let _ = isp;

    // An expired certificate: the router whose clock is past its end refuses the peer.
    let (mut lab, r1, _isp, r2, a) = sites_with_certificates();
    lab.run(r1, &v1_config(SITE1, "198.51.100.1", "rsa-sig"));
    lab.run(r2, &v1_config(SITE2, "203.0.113.1", "rsa-sig"));
    lab.run(r1, "clock set 08:00:00 1 Jan 2031");
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    let sa = lab.run(r1, "show crypto isakmp sa");
    assert!(sa.contains("failed:") && sa.contains("has expired"), "{sa}");

    // One policy says `rsa-sig`, the other `pre-share`: no policy in common.
    let (mut lab, r1, _isp, r2, a) = sites_with_certificates();
    lab.run(r1, &v1_config(SITE1, "198.51.100.1", "rsa-sig"));
    lab.run(r2, &v1_config(SITE2, "203.0.113.1", "pre-share"));
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    let sa = lab.run(r1, "show crypto isakmp sa");
    assert!(sa.contains("failed:") && sa.contains("no ISAKMP policy matches"), "{sa}");
}

#[test]
fn ikev2_ike_auth_signs_and_verifies_with_certificates() {
    let (mut lab, r1, _isp, r2, a) = sites_with_certificates();
    lab.run(r1, &v2_config(SITE1, "198.51.100.1"));
    lab.run(r2, &v2_config(SITE2, "203.0.113.1"));
    let ping = ping_settled(&mut lab, a, "192.168.2.10");
    assert!(ping.contains("4 answered"), "{ping}");
    let sa = lab.run(r1, "show crypto ikev2 sa");
    assert!(sa.contains("established") && sa.contains("Peer authenticated with the certificate of CN=r2.lab.test, O=NetLab"), "{sa}");
    assert!(lab.run(r2, "show crypto ikev2 sa").contains("Peer authenticated with the certificate of CN=r1.lab.test, O=NetLab"));
    let prof = lab.run(r1, "show crypto ikev2 profile");
    assert!(prof.contains("Authentication: local rsa-sig, remote rsa-sig") && prof.contains("Trustpoint: T"), "{prof}");
    let run = lab.run(r1, "show running-config");
    assert!(run.contains(" authentication remote rsa-sig\n authentication local rsa-sig\n keyring local KR\n pki trustpoint T\n"), "{run}");
}

#[test]
fn ikev2_refuses_a_peer_that_cannot_prove_itself_with_a_certificate() {
    // The peer holds no certificate.
    let (mut lab, r1, _isp, r2, a) = sites_with_certificates();
    lab.run(r1, &v2_config(SITE1, "198.51.100.1"));
    lab.run(r2, &v2_config(SITE2, "203.0.113.1"));
    lab.run(r2, "conf t\nno crypto pki trustpoint T\nend");
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    let sa = lab.run(r2, "show crypto ikev2 sa");
    assert!(sa.contains("no certificate") || sa.contains("Last failure"), "{sa}");
    // One end signs, the other wants a shared key.
    let (mut lab, r1, _isp, r2, a) = sites_with_certificates();
    lab.run(r1, &v2_config(SITE1, "198.51.100.1"));
    lab.run(r2, &v2_config(SITE2, "203.0.113.1"));
    lab.run(r2, "conf t\ncrypto ikev2 profile PROF\nauthentication remote pre-share\nend");
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    let sa = lab.run(r2, "show crypto ikev2 sa");
    assert!(sa.contains("wants a shared key"), "{sa}");
}

#[test]
fn ike_refuses_a_revoked_certificate_where_the_revocation_check_is_on() {
    let (mut lab, r1, isp, r2, a) = sites_with_certificates();
    lab.run(r1, &v1_config(SITE1, "198.51.100.1", "rsa-sig"));
    lab.run(r2, &v1_config(SITE2, "203.0.113.1", "rsa-sig"));
    let leaf = netlabcraft_wire::x509::Certificate::from_pem(&lab.t.device(r2).unwrap().pki.trustpoint("T").unwrap().certificate).unwrap();
    let serial = netlabcraft_sim::pki::serial_hex(&leaf.serial);
    // The CA revokes r2's certificate; r1 insists on a revocation list and fetches it.
    lab.run(isp, &format!("conf t\ncrypto pki server ROOT revoke {serial}\nend"));
    lab.run(r1, "conf t\ncrypto pki trustpoint T\nrevocation-check crl\nexit\ncrypto pki crl request T\nend");
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    let sa = lab.run(r1, "show crypto isakmp sa");
    assert!(sa.contains("failed:") && sa.contains("was revoked by CN=Lab Root, O=NetLab"), "{sa}");
}

#[test]
fn ike_checks_the_address_a_certificate_names_against_the_identity() {
    let (mut lab, r1, _isp, r2, a) = sites_with_certificates();
    lab.run(r1, &v1_config(SITE1, "198.51.100.1", "rsa-sig"));
    lab.run(r2, &v1_config(SITE2, "203.0.113.1", "rsa-sig"));
    // r2's new certificate is for an address that is not the one r2 identifies itself with.
    lab.run(r2, "conf t\ncrypto pki trustpoint T\nip-address 10.9.9.9\nexit\ncrypto pki enroll T\nend");
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"));
    let sa = lab.run(r1, "show crypto isakmp sa");
    assert!(sa.contains("is for 10.9.9.9, not for the address 198.51.100.1"), "{sa}");
    // With the right address in it, the exchange works.
    lab.run(r2, "conf t\ncrypto pki trustpoint T\nip-address 198.51.100.1\nexit\ncrypto pki enroll T\nend\nclear crypto sa");
    lab.run(r1, "clear crypto sa");
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
}
