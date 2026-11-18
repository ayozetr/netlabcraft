//! IKE with certificates: the certificate request each end sends (CERTREQ, naming the
//! authorities it accepts), the chain a router answers with and the refusal when the answer
//! would be from an authority the peer did not name.

use netlabcraft_sim::pki::{Wanted, request_hashes, request_names};

use super::ike_rsa::{SITE1, SITE2, enroll, sites_with_certificates, v1_config, v2_config};
use super::ping_settled;

#[test]
fn an_identity_carries_the_authority_certificate_and_requests_name_authorities() {
    let (lab, r1, _isp, r2, _a) = sites_with_certificates();
    let (d1, d2) = (lab.t.device(r1).unwrap(), lab.t.device(r2).unwrap());
    let me = netlabcraft_sim::pki::ike_identity(d1, None).unwrap();
    // The certificate, then the certificate of the authority that issued it.
    assert_eq!(me.chain.len(), 2);
    assert!(me.chain[1].ca && me.chain[0].issuer_der() == me.chain[1].subject_der());
    // What the peer trusts is what its request names, and the chain satisfies it.
    let by_name = Wanted { names: request_names(d2), hashes: Vec::new() };
    let by_key = Wanted { names: Vec::new(), hashes: request_hashes(d2) };
    assert!(!by_name.names.is_empty() && by_name.accepts(&me.chain));
    assert!(!by_key.hashes.is_empty() && by_key.accepts(&me.chain));
    assert!(Wanted::default().is_empty() && Wanted::default().accepts(&me.chain));
    // An authority that is not in the chain is not accepted.
    assert!(!Wanted { names: vec![vec![0x30, 0]], hashes: vec![[9; 20]] }.accepts(&me.chain));
}

#[test]
fn ikev1_main_mode_asks_for_certificates_and_the_exchange_still_works() {
    let (mut lab, r1, _isp, r2, a) = sites_with_certificates();
    lab.run(r1, &v1_config(SITE1, "198.51.100.1", "rsa-sig"));
    lab.run(r2, &v1_config(SITE2, "203.0.113.1", "rsa-sig"));
    lab.sim.clear_trace();
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    let asks: Vec<String> = lab.sim.trace().iter().filter(|e| e.summary.contains("CERTREQ")).map(|e| e.summary.clone()).collect();
    // Messages 3 and 4 each carry a request.
    assert!(asks.len() >= 2, "{asks:#?}");
    assert!(asks.iter().all(|s| s.contains("[KE, NONCE, CERTREQ]")), "{asks:#?}");
    // A pre-shared key asks for nothing.
    let (mut lab, r1, _isp, r2, a) = sites_with_certificates();
    lab.run(r1, &v1_config(SITE1, "198.51.100.1", "pre-share"));
    lab.run(r2, &v1_config(SITE2, "203.0.113.1", "pre-share"));
    lab.sim.clear_trace();
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    assert!(!lab.sim.trace().iter().any(|e| e.summary.contains("CERTREQ")));
}

#[test]
fn ikev1_answers_a_request_with_the_certificate_from_the_authority_it_names() {
    let (mut lab, r1, _isp, r2, a) = sites_with_certificates();
    // r2's first usable trustpoint is a self-signed one; the one from the lab's authority is
    // second, and it is the one the peer's request names.
    lab.run(r2, "conf t\nno crypto pki trustpoint T\ncrypto pki trustpoint SELF\nenrollment selfsigned\nsubject-name CN=r2.lab.test, O=NetLab\nexit\ncrypto pki enroll SELF\nend");
    enroll(&mut lab, r2, "198.51.100.2", "r2.lab.test");
    let first = netlabcraft_sim::pki::ike_identity(lab.t.device(r2).unwrap(), None).unwrap();
    assert_eq!(first.trustpoint, "SELF");
    lab.run(r1, &v1_config(SITE1, "198.51.100.1", "rsa-sig"));
    lab.run(r2, &v1_config(SITE2, "203.0.113.1", "rsa-sig"));
    let ping = ping_settled(&mut lab, a, "192.168.2.10");
    assert!(ping.contains("4 answered"), "{ping}");
    assert!(lab.run(r1, "show crypto isakmp sa").contains("peer certificate CN=r2.lab.test, O=NetLab"));
}

#[test]
fn ike_says_so_when_it_holds_no_certificate_from_an_authority_the_peer_named() {
    // r2 has only a self-signed certificate; r1 trusts the lab's authority only.
    for v2 in [false, true] {
        let (mut lab, r1, _isp, r2, a) = sites_with_certificates();
        let (c1, c2) = if v2 {
            (v2_config(SITE1, "198.51.100.1"), v2_config(SITE2, "203.0.113.1"))
        } else {
            (v1_config(SITE1, "198.51.100.1", "rsa-sig"), v1_config(SITE2, "203.0.113.1", "rsa-sig"))
        };
        lab.run(r1, &c1);
        lab.run(r2, &c2);
        lab.run(
            r2,
            "conf t\ncrypto pki trustpoint SELF\nenrollment selfsigned\nsubject-name CN=r2.lab.test, O=NetLab\nexit\ncrypto pki enroll SELF\nno crypto pki trustpoint T\nend",
        );
        if v2 {
            lab.run(r2, "conf t\ncrypto ikev2 profile PROF\npki trustpoint SELF\nend");
        }
        assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"), "v2={v2}");
        let sa = lab.run(r1, if v2 { "show crypto ikev2 sa" } else { "show crypto isakmp sa" })
            + &lab.run(r2, if v2 { "show crypto ikev2 sa" } else { "show crypto isakmp sa" });
        assert!(sa.contains("asked (CERTREQ)") || sa.contains("trust"), "v2={v2}: {sa}");
    }
}

#[test]
fn ikev2_ike_sa_init_carries_a_request_naming_the_authority() {
    let (mut lab, r1, _isp, r2, a) = sites_with_certificates();
    lab.run(r1, &v2_config(SITE1, "198.51.100.1"));
    lab.run(r2, &v2_config(SITE2, "203.0.113.1"));
    lab.sim.clear_trace();
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    let init: Vec<String> = lab.sim.trace().iter().filter(|e| e.summary.contains("IKE_SA_INIT")).map(|e| e.summary.clone()).collect();
    assert!(init.iter().any(|s| s.contains("request") && s.contains("CERTREQ")), "{init:#?}");
    assert!(init.iter().any(|s| s.contains("response") && s.contains("CERTREQ")), "{init:#?}");
}
