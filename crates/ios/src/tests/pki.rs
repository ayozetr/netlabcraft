//! Typed console tests for keys, trustpoints, self-signed certificates and the certificate
//! authority a router can run, including enrolling over the simulated network.

use netlabcraft_model::DeviceKind;
use netlabcraft_wire::x509::{self, Certificate};

use super::Lab;

fn two_routers() -> (Lab, netlabcraft_core::DeviceId, netlabcraft_core::DeviceId) {
    let mut lab = Lab::new();
    let ca = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(ca, "g0/0", r2, "g0/0");
    lab.run(ca, "en\nconf t\nhostname CA1\nint g0/0\nip address 10.0.0.1 255.255.255.0\nno shut\nend");
    lab.run(r2, "en\nconf t\nhostname Edge\nint g0/0\nip address 10.0.0.2 255.255.255.0\nno shut\nend");
    (lab, ca, r2)
}

#[test]
fn rsa_keys_are_generated_listed_and_deleted() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t\nhostname R1");
    // The default name needs a domain; a label does not.
    assert!(lab.try_run(r, "crypto key generate rsa modulus 512").starts_with("% Set `ip domain-name`"));
    assert!(lab.run(r, "crypto key generate rsa modulus 512 label web").contains("RSA key pair web generated (512 bits)"));
    lab.run(r, "ip domain-name lab.test");
    assert!(lab.run(r, "crypto key generate rsa general-keys modulus 1024").contains("RSA keys generated (1024 bits). SSH is available."));
    let shown = lab.run(r, "end\nshow crypto key mypubkey rsa");
    assert!(shown.contains("Key pair web") && shown.contains("Key pair R1.lab.test (SSH host key)"), "{shown}");
    assert!(shown.contains("512 bits") && shown.contains("1024 bits"), "{shown}");
    assert!(lab.run(r, "show ip ssh").contains("Key size 1024 bits"));
    // Sizes the simulator cannot generate for real are refused, not faked.
    for bad in ["4096", "360", "1000", "0", "abc"] {
        lab.try_run(r, "conf t");
        let out = lab.try_run(r, &format!("crypto key generate rsa modulus {bad}"));
        assert!(out.starts_with("% "), "{bad}: {out}");
        lab.try_run(r, "end");
    }
    // Generating again replaces the key with another one (the count of keys made is in the seed).
    let first = lab.run(r, "show crypto key mypubkey rsa");
    let again = lab.run(r, "conf t\ncrypto key generate rsa general-keys modulus 1024\nend\nshow crypto key mypubkey rsa");
    assert_ne!(first, again);
    assert!(lab.run(r, "conf t\ncrypto key zeroize rsa web").contains("RSA key pair web deleted."));
    assert!(lab.try_run(r, "crypto key zeroize rsa web").starts_with("% No key pair named web"));
    assert!(lab.run(r, "crypto key zeroize rsa").contains("SSH is off"));
    assert!(lab.run(r, "end\nshow crypto key mypubkey rsa").contains("No RSA key pairs."));
    assert!(lab.run(r, "show ip ssh").contains("SSH disabled"));
}

#[test]
fn the_modulus_prompt_still_works_and_makes_a_real_key() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t\nip domain-name lab.test\ncrypto key generate rsa");
    assert_eq!(lab.prompt(r), crate::MODULUS_PROMPT);
    assert!(lab.try_run(r, "768").contains("RSA keys generated (768 bits)"));
    assert!(lab.run(r, "end\nshow crypto key mypubkey rsa").contains("768 bits"));
    lab.run(r, "conf t\ncrypto key generate rsa");
    assert!(lab.try_run(r, "5000").starts_with("% The key size is"));
}

#[test]
fn a_self_signed_trustpoint_makes_a_certificate() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(
        r,
        "en\nconf t\nhostname R1\ncrypto pki trustpoint SELF\nenrollment selfsigned\nsubject-name CN=r1.lab.test, O=NetLab\nrevocation-check none",
    );
    assert_eq!(lab.prompt(r), "R1(ca-trustpoint)#");
    // No certificate yet; the key pair is made on enrollment when the trustpoint names none.
    lab.run(r, "exit");
    let out = lab.run(r, "crypto pki enroll SELF");
    assert!(out.contains("Generated a 1024-bit key pair named SELF") && out.contains("CN=r1.lab.test, O=NetLab"), "{out}");
    let run = lab.run(r, "end\nshow running-config");
    assert!(
        run.contains(
            "crypto pki trustpoint SELF\n enrollment selfsigned\n subject-name CN=r1.lab.test, O=NetLab\n rsakeypair SELF\n revocation-check none\n!"
        ),
        "{run}"
    );
    let certs = lab.run(r, "show crypto pki certificates");
    assert!(certs.contains("Certificate of trustpoint SELF") && certs.contains("Subject       : CN=r1.lab.test, O=NetLab"), "{certs}");
    assert!(certs.contains("RSA 1024 bits") && certs.contains("(valid)"), "{certs}");
    let tps = lab.run(r, "show crypto pki trustpoints");
    assert!(tps.contains("Enrollment : self-signed") && tps.contains("Key pair   : SELF"), "{tps}");
    // The certificate is a real one and verifies against itself.
    let tp = lab.t.device(r).unwrap().pki.trustpoint("SELF").unwrap().clone();
    let c = Certificate::from_pem(&tp.certificate).unwrap();
    assert!(c.self_issued() && c.signed_by(&c.key));
    let now = u64::try_from(lab.sim.clock(r).0).unwrap();
    assert_eq!(x509::verify_chain(std::slice::from_ref(&c), std::slice::from_ref(&c), now), Ok(()));
    // Zeroizing the key leaves the certificate but `show` names the trustpoint's key as gone.
    lab.run(r, "conf t");
    assert!(lab.try_run(r, "crypto pki enroll NOPE").starts_with("% No trustpoint named NOPE"));
    assert!(lab.try_run(r, "crypto pki trustpoint BAD NAME").contains("% "));
    assert!(lab.try_run(r, "no crypto pki trustpoint SELF").is_empty());
    assert!(lab.run(r, "end\nshow crypto pki trustpoints").contains("No trustpoints."));
}

#[test]
fn a_router_enrolls_with_the_certificate_authority_over_the_network() {
    let (mut lab, ca, r2) = two_routers();
    let out = lab.run(
        ca,
        "conf t\nip http server\ncrypto pki server LABCA\nissuer-name CN=Lab Root CA, O=NetLab\ngrant auto\nlifetime certificate 90\nno shutdown",
    );
    assert!(out.contains("Made the CA certificate for CN=Lab Root CA, O=NetLab") && out.contains("running"), "{out}");
    assert_eq!(lab.prompt(ca), "CA1(cs-server)#");
    let show = lab.run(ca, "end\nshow crypto pki server");
    assert!(show.contains("Certificate authority LABCA: running") && show.contains("certificates last 90 day(s)"), "{show}");
    let run = lab.run(ca, "show running-config");
    assert!(
        run.contains("crypto pki server LABCA\n issuer-name CN=Lab Root CA, O=NetLab\n grant auto\n lifetime certificate 90\n no shutdown\n!"),
        "{run}"
    );

    // The client must trust the CA before it enrolls.
    lab.run(r2, "conf t\ncrypto pki trustpoint LAB\nenrollment url http://10.0.0.1\nsubject-name CN=edge.lab.test, O=NetLab\nexit");
    assert!(lab.try_run(r2, "crypto pki enroll LAB").starts_with("% Authenticate the CA first"));
    let auth = lab.run(r2, "crypto pki authenticate LAB");
    assert!(auth.contains("Trusting the CA certificate of CN=Lab Root CA, O=NetLab"), "{auth}");
    let out = lab.run(r2, "crypto pki enroll LAB");
    assert!(out.contains("for CN=edge.lab.test, O=NetLab"), "{out}");
    assert!(lab.t.device(r2).unwrap().pki.key("LAB").is_some());
    let certs = lab.run(r2, "end\nshow crypto pki certificates");
    assert!(certs.contains("Issuer        : CN=Lab Root CA, O=NetLab") && certs.contains("CA certificate of trustpoint LAB"), "{certs}");

    // The issued certificate chains to the CA's, for 90 days.
    let d2 = lab.t.device(r2).unwrap();
    let tp = d2.pki.trustpoint("LAB").unwrap();
    let (leaf, root) = (Certificate::from_pem(&tp.certificate).unwrap(), Certificate::from_pem(&tp.ca_certificate).unwrap());
    let now = u64::try_from(lab.sim.clock(r2).0).unwrap();
    assert_eq!(x509::verify_chain(std::slice::from_ref(&leaf), std::slice::from_ref(&root), now), Ok(()));
    assert_eq!(leaf.not_after - leaf.not_before, 90 * 86_400);
    assert!(!leaf.ca && root.ca);
    // A later clock past the lifetime makes it expire.
    assert!(x509::verify_chain(std::slice::from_ref(&leaf), std::slice::from_ref(&root), now + 91 * 86_400).is_err());
    // The CA wrote the same certificate for the same request: enrolling again is repeatable.
    let again = lab.run(r2, "conf t\ncrypto pki enroll LAB");
    assert!(again.contains("received"), "{again}");
    assert_eq!(Certificate::from_pem(&lab.t.device(r2).unwrap().pki.trustpoint("LAB").unwrap().certificate).unwrap().serial, leaf.serial);
}

#[test]
fn the_certificate_authority_refuses_what_it_should() {
    let (mut lab, ca, r2) = two_routers();
    lab.run(r2, "conf t\ncrypto pki trustpoint LAB\nenrollment url http://10.0.0.1\nexit");
    // No web server: nothing listens.
    let out = lab.try_run(r2, "crypto pki authenticate LAB");
    assert!(out.starts_with("% The CA at 10.0.0.1 did not answer"), "{out}");
    lab.run(ca, "conf t\nip http server\ncrypto pki server LABCA");
    // Stopped, then running without `grant auto`.
    assert!(lab.try_run(r2, "crypto pki authenticate LAB").contains("503"));
    let out = lab.run(ca, "no shutdown");
    assert!(out.contains("No requests are signed until `grant auto` is set."), "{out}");
    lab.run(r2, "crypto pki authenticate LAB");
    let out = lab.try_run(r2, "crypto pki enroll LAB");
    assert!(out.contains("403"), "{out}");
    // Shut down again: it stops answering, but keeps its key and certificate.
    lab.run(ca, "shutdown");
    assert!(lab.try_run(r2, "crypto pki authenticate LAB").contains("503"));
    assert!(lab.run(ca, "end\nshow crypto pki certificates").contains("Certificate of this router's own certificate authority"));
    // Only routers have the commands.
    let mut other = Lab::new();
    let sw = other.add(DeviceKind::Switch);
    other.run(sw, "en\nconf t");
    assert!(other.try_run(sw, "crypto pki trustpoint X").contains("% "));
}

#[test]
fn trustpoint_commands_reject_bad_input() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t\ncrypto pki trustpoint T");
    for bad in [
        "enrollment",
        "enrollment ftp://1.2.3.4",
        "enrollment url http://name.example",
        "enrollment url http://1.2.3.4:0",
        "subject-name XX=1",
        "subject-name CN=",
        "subject-name C=ESP",
        "revocation-check crl",
        "rsakeypair a b",
    ] {
        assert!(lab.try_run(r, bad).contains("% "), "{bad}");
    }
    assert!(lab.try_run(r, "enrollment url http://1.2.3.4:8080").is_empty());
    assert!(lab.run(r, "end\nshow running-config").contains(" enrollment url http://1.2.3.4:8080"));
    lab.run(r, "conf t\ncrypto pki server CA");
    assert!(lab.try_run(r, "lifetime certificate 0").starts_with("% "));
    assert!(lab.try_run(r, "grant manual").starts_with("% "));
    assert!(lab.try_run(r, "issuer-name nonsense").starts_with("% "));
    lab.run(r, "exit");
    assert!(lab.try_run(r, "crypto pki server OTHER").starts_with("% The certificate authority is CA"));
    assert!(lab.try_run(r, "no crypto pki server OTHER").starts_with("% No certificate authority"));
    assert!(lab.try_run(r, "no crypto pki server CA").is_empty());
}

#[test]
fn keys_and_certificates_survive_saving_and_loading() {
    let (mut lab, ca, _r2) = two_routers();
    lab.run(ca, "conf t\nip http server\ncrypto pki server LABCA\ngrant auto\nno shutdown\nexit\ncrypto pki trustpoint SELF\nenrollment selfsigned\nexit\ncrypto pki enroll SELF");
    let before = lab.t.device(ca).unwrap().pki.clone();
    assert!(before.keys.len() == 2 && before.server.as_ref().is_some_and(|s| s.enabled));
    let json = netlabcraft_model::file::to_json(&lab.t).unwrap();
    let back = netlabcraft_model::file::from_json(&json).unwrap();
    assert_eq!(back.device(ca).unwrap().pki, before);
    assert!(netlabcraft_sim::pki::load_key(lab.t.device(ca).unwrap(), "LABCA").is_some());
}

#[test]
fn the_https_server_takes_its_certificate_from_a_trustpoint() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t\nhostname R1\nip http secure-server");
    assert!(lab.try_run(r, "ip http secure-trustpoint NOPE").contains("% No trustpoint named NOPE"));
    lab.run(r, "crypto pki trustpoint WEB\nenrollment selfsigned\nexit");
    lab.run(r, "ip http secure-trustpoint WEB");
    let status = lab.run(r, "end\nshow ip http server status");
    assert!(status.contains("trustpoint WEB has no usable certificate and key"), "{status}");
    lab.run(r, "conf t\ncrypto pki enroll WEB\nend");
    let status = lab.run(r, "show ip http server status");
    assert!(status.contains("DHE-RSA with the certificate of trustpoint WEB"), "{status}");
    assert!(lab.run(r, "show running-config").contains("ip http secure-trustpoint WEB"));
    lab.run(r, "conf t\nno ip http secure-trustpoint\nend");
    assert!(lab.run(r, "show ip http server status").contains("anonymous DH, no certificate"));
    assert!(lab.run(r, "show crypto key mypubkey rsa").contains("SSH          : SHA256:"));
}
