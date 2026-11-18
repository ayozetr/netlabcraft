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
    assert!(out.contains("Requests wait until an administrator grants them"), "{out}");
    lab.run(r2, "crypto pki authenticate LAB");
    let out = lab.try_run(r2, "crypto pki enroll LAB");
    assert!(out.contains("Request 1 is waiting for the administrator"), "{out}");
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
        "revocation-check crl bogus",
        "revocation-check none crl",
        "fqdn bad name",
        "fqdn",
        "ip-address 1.2.3",
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

/// The first PEM block of `text` with this label.
fn pem_in(text: &str, label: &str) -> String {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let from = text.find(&begin).unwrap_or_else(|| panic!("no {label} in {text}"));
    let to = text[from..].find(&end).unwrap() + from + end.len();
    text[from..to].to_string()
}

/// Pastes a block of text into a command that asked for it, ending with `quit`.
fn paste(lab: &mut Lab, r: netlabcraft_core::DeviceId, text: &str) -> String {
    lab.run(r, text);
    lab.try_run(r, "quit")
}

const GRANTING: &str = "conf t\nip http server\ncrypto pki server LABCA\nissuer-name CN=Lab Root CA, O=NetLab\nno shutdown\nend";

#[test]
fn requests_wait_for_the_administrator_and_are_granted_by_hand() {
    let (mut lab, ca, r2) = two_routers();
    lab.run(ca, GRANTING);
    lab.run(
        r2,
        "conf t\ncrypto pki trustpoint LAB\nenrollment url http://10.0.0.1\nsubject-name CN=edge.lab.test, O=NetLab\nfqdn edge.lab.test\nip-address 10.0.0.2\nexit\ncrypto pki authenticate LAB",
    );
    let out = lab.run(r2, "crypto pki enroll LAB");
    assert!(out.contains("Request 1 is waiting for the administrator to grant it"), "{out}");
    // Asking again is the same request, not a second one.
    lab.run(r2, "crypto pki enroll LAB");
    let shown = lab.run(ca, "show crypto pki server");
    assert!(shown.contains("1: CN=edge.lab.test, O=NetLab (waiting)") && !shown.contains("    2: "), "{shown}");
    lab.run(r2, "end");
    assert!(!lab.run(r2, "show crypto pki certificates").contains("Certificate of trustpoint LAB"));
    // The administrator grants it; the router gets the certificate the next time, with its names.
    lab.run(ca, "conf t");
    assert!(lab.try_run(ca, "crypto pki server LABCA grant 7").starts_with("% The certificate authority holds no request number 7"));
    assert!(lab.try_run(ca, "crypto pki server OTHER grant 1").starts_with("% No certificate authority named OTHER"));
    let out = lab.run(ca, "crypto pki server LABCA grant 1");
    assert!(out.contains("Request 1 for CN=edge.lab.test, O=NetLab granted") && out.contains("-----BEGIN CERTIFICATE-----"), "{out}");
    assert!(lab.run(ca, "crypto pki server LABCA grant all").contains("No request changed"));
    lab.run(r2, "conf t");
    let out = lab.run(r2, "crypto pki enroll LAB");
    assert!(out.contains("received"), "{out}");
    let tp = lab.t.device(r2).unwrap().pki.trustpoint("LAB").unwrap().clone();
    let leaf = Certificate::from_pem(&tp.certificate).unwrap();
    let root = Certificate::from_pem(&tp.ca_certificate).unwrap();
    assert_eq!((leaf.alt.dns.clone(), leaf.alt.ips.clone()), (vec!["edge.lab.test".to_string()], vec![[10, 0, 0, 2]]));
    assert!(leaf.matches_host("edge.lab.test") && leaf.matches_host("10.0.0.2") && !leaf.matches_host("10.0.0.3"));
    let now = u64::try_from(lab.sim.clock(r2).0).unwrap();
    assert_eq!(x509::verify_chain(std::slice::from_ref(&leaf), std::slice::from_ref(&root), now), Ok(()));
    let certs = lab.run(r2, "end\nshow crypto pki certificates");
    assert!(certs.contains("Also for      : edge.lab.test, 10.0.0.2"), "{certs}");
    let run = lab.run(r2, "show running-config");
    assert!(run.contains(" fqdn edge.lab.test\n ip-address 10.0.0.2\n"), "{run}");
    // A third router's request is rejected, and it is told so.
    let r3 = lab.add(DeviceKind::Router);
    lab.cable(ca, "g0/1", r3, "g0/0");
    lab.run(ca, "int g0/1\nip address 10.0.1.1 255.255.255.0\nno shut\nend");
    lab.run(
        r3,
        "en\nconf t\nhostname Spare\nint g0/0\nip address 10.0.1.2 255.255.255.0\nno shut\nexit\ncrypto pki trustpoint LAB\nenrollment url http://10.0.1.1\nexit\ncrypto pki authenticate LAB",
    );
    lab.run(r3, "crypto pki enroll LAB");
    let out = lab.run(ca, "conf t\ncrypto pki server LABCA reject all");
    assert!(out.contains("Request 2 for CN=Spare rejected."), "{out}");
    assert!(lab.try_run(r3, "crypto pki enroll LAB").contains("403"));
    // `grant auto` is the other mode, and goes back with `no grant auto`; the list's hours are kept.
    lab.run(ca, "crypto pki server LABCA\ngrant auto");
    assert!(lab.run(ca, "end\nshow running-config").contains(" grant auto\n"));
    lab.run(ca, "conf t\ncrypto pki server LABCA\nno grant auto\nlifetime crl 12\nend");
    let run = lab.run(ca, "show running-config");
    assert!(!run.contains(" grant auto\n") && run.contains(" lifetime crl 12\n"), "{run}");
}

#[test]
fn certificates_are_copied_by_hand_with_a_terminal_enrollment() {
    let (mut lab, ca, r2) = two_routers();
    lab.run(ca, "conf t\ncrypto pki server LABCA\nissuer-name CN=Lab Root CA, O=NetLab\ngrant auto\nno shutdown\nend");
    let ca_pem = lab.t.device(ca).unwrap().pki.server.as_ref().unwrap().certificate.clone();
    lab.run(r2, "conf t\ncrypto pki trustpoint HAND\nenrollment terminal\nsubject-name CN=hand.lab.test, O=NetLab\nexit");
    assert!(lab.run(r2, "end\nshow running-config").contains("crypto pki trustpoint HAND\n enrollment terminal\n"));
    lab.run(r2, "conf t");
    // Pasting text that is not a CA certificate is refused, and nothing is kept.
    let out = lab.run(r2, "crypto pki authenticate HAND");
    assert!(out.contains("Paste the CA's certificate"), "{out}");
    assert_eq!(lab.prompt(r2), "");
    let out = paste(&mut lab, r2, "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----");
    assert!(out.starts_with("% The pasted text is not a certificate"), "{out}");
    assert_eq!(lab.prompt(r2), "Edge(config)#");
    lab.run(r2, "crypto pki authenticate HAND");
    let out = paste(&mut lab, r2, &ca_pem);
    assert!(out.contains("Trusting the CA certificate of CN=Lab Root CA, O=NetLab (fingerprint "), "{out}");
    // The request is printed; the CA signs it from a paste and prints the certificate.
    let out = lab.run(r2, "crypto pki enroll HAND");
    assert!(out.contains("Copy this request to the certificate authority"), "{out}");
    let csr = pem_in(&out, "CERTIFICATE REQUEST");
    let out = lab.run(ca, "conf t\ncrypto pki server LABCA request pkcs10 terminal");
    assert!(out.contains("Paste the certificate request"), "{out}");
    let out = paste(&mut lab, ca, &csr);
    assert!(out.contains("Request 1 for CN=hand.lab.test, O=NetLab granted"), "{out}");
    let leaf_pem = pem_in(&out, "CERTIFICATE");
    // The certificate goes in by paste; one for another key is refused.
    lab.run(r2, "crypto pki import HAND certificate");
    let out = paste(&mut lab, r2, &ca_pem);
    assert!(out.starts_with("% The pasted certificate is for another key"), "{out}");
    lab.run(r2, "crypto pki import HAND certificate");
    let out = paste(&mut lab, r2, &leaf_pem);
    assert!(out.contains("for CN=hand.lab.test, O=NetLab imported"), "{out}");
    let tp = lab.t.device(r2).unwrap().pki.trustpoint("HAND").unwrap().clone();
    let (leaf, root) = (Certificate::from_pem(&tp.certificate).unwrap(), Certificate::from_pem(&tp.ca_certificate).unwrap());
    let now = u64::try_from(lab.sim.clock(r2).0).unwrap();
    assert_eq!(x509::verify_chain(std::slice::from_ref(&leaf), std::slice::from_ref(&root), now), Ok(()));
    // No such trustpoint; a trustpoint whose CA is not trusted yet; a stopped authority.
    assert!(lab.try_run(r2, "crypto pki import NOPE certificate").starts_with("% No trustpoint named NOPE"));
    lab.run(r2, "crypto pki trustpoint BARE\nenrollment terminal\nexit\ncrypto pki enroll BARE\ncrypto pki import BARE certificate");
    let out = paste(&mut lab, r2, &leaf_pem);
    assert!(out.starts_with("% The pasted certificate is for another key") || out.contains("Authenticate the CA first"), "{out}");
    lab.run(ca, "crypto pki server LABCA\nshutdown\nexit");
    assert!(lab.try_run(ca, "crypto pki server LABCA request pkcs10 terminal").contains("is stopped"));
    assert!(lab.try_run(ca, "crypto pki server LABCA grant 1").contains("is stopped"));
}

#[test]
fn a_revoked_certificate_is_on_the_crl_and_refused_where_the_check_is_on() {
    let (mut lab, ca, r2) = two_routers();
    lab.run(
        ca,
        "conf t\nip http server\ncrypto pki server LABCA\nissuer-name CN=Lab Root CA, O=NetLab\ngrant auto\nlifetime crl 24\nno shutdown\nend",
    );
    lab.run(
        r2,
        "conf t\ncrypto pki trustpoint LAB\nenrollment url http://10.0.0.1\nsubject-name CN=edge.lab.test\nrevocation-check crl\nexit\ncrypto pki authenticate LAB\ncrypto pki enroll LAB\nend",
    );
    let leaf = Certificate::from_pem(&lab.t.device(r2).unwrap().pki.trustpoint("LAB").unwrap().certificate).unwrap();
    let serial = netlabcraft_sim::pki::serial_hex(&leaf.serial);
    // The chain is checked through the code IKE uses: a signature over nothing fails after the chain.
    let verdict =
        |d: &netlabcraft_model::Device, at: i64| netlabcraft_sim::pki::verify_peer(d, at, &[leaf.der()], b"data", b"sig", None).unwrap_err();
    // No list yet: the check that needs one fails, and `none` after `crl` lets it go on.
    let (d2, now) = (lab.t.device(r2).unwrap().clone(), lab.sim.clock(r2).0);
    assert!(verdict(&d2, now).contains("holds no CRL"), "{}", verdict(&d2, now));
    lab.run(r2, "conf t\ncrypto pki trustpoint LAB\nrevocation-check crl none\nend");
    let (d2, now) = (lab.t.device(r2).unwrap().clone(), lab.sim.clock(r2).0);
    assert!(verdict(&d2, now).contains("does not verify"), "{}", verdict(&d2, now));
    lab.run(r2, "conf t\ncrypto pki trustpoint LAB\nrevocation-check crl\nend");
    assert!(lab.run(r2, "show running-config").contains(" revocation-check crl\n"));
    // Fetch the (empty) list: the check passes down to the signature.
    let out = lab.run(r2, "conf t\ncrypto pki crl request LAB");
    assert!(out.contains("Revocation list number 0 of CN=Lab Root CA, O=NetLab with 0 revoked certificate(s)"), "{out}");
    let (d2, now) = (lab.t.device(r2).unwrap().clone(), lab.sim.clock(r2).0);
    assert!(verdict(&d2, now).contains("does not verify"), "{}", verdict(&d2, now));
    // The CA revokes the certificate; the router's old list does not know yet.
    lab.run(ca, "conf t");
    assert!(lab.try_run(ca, "crypto pki server LABCA revoke zz").starts_with("% The serial number is hexadecimal"));
    let out = lab.run(ca, &format!("crypto pki server LABCA revoke {serial}"));
    assert!(out.contains(&format!("Certificate {serial} revoked; revocation list number 1")), "{out}");
    assert!(lab.try_run(ca, &format!("crypto pki server LABCA revoke {serial}")).contains("already revoked"));
    let (dca, at) = (lab.t.device(ca).unwrap().clone(), lab.sim.clock(ca).0);
    let e = netlabcraft_sim::pki::verify_peer(&dca, at, &[leaf.der()], b"data", b"sig", None).unwrap_err();
    assert!(e.contains("was revoked by this router's authority"), "{e}");
    let (d2, now) = (lab.t.device(r2).unwrap().clone(), lab.sim.clock(r2).0);
    assert!(verdict(&d2, now).contains("does not verify"), "{}", verdict(&d2, now));
    let out = lab.run(r2, "crypto pki crl request LAB");
    assert!(out.contains("Revocation list number 1") && out.contains("with 1 revoked certificate(s)"), "{out}");
    let (d2, now) = (lab.t.device(r2).unwrap().clone(), lab.sim.clock(r2).0);
    assert!(verdict(&d2, now).contains(&format!("(serial {serial}) was revoked by CN=Lab Root CA, O=NetLab")), "{}", verdict(&d2, now));
    let crls = lab.run(r2, "end\nshow crypto pki crls");
    assert!(crls.contains("Revocation list held for trustpoint LAB") && crls.contains(&format!("serial {serial}")), "{crls}");
    assert!(crls.contains("(current)") && crls.contains("made by the CA's key"), "{crls}");
    assert!(lab.run(ca, "end\nshow crypto pki crls").contains("Revocation list this router's own CA publishes"));
    assert!(lab.run(ca, "show crypto pki server").contains(&format!("serial {serial}")));
    // A list is good for its hours: past them the check that needs it fails, and `crl none` goes on.
    lab.run(r2, "clock set 10:00:00 5 Jan 2026");
    let (d2, later) = (lab.t.device(r2).unwrap().clone(), lab.sim.clock(r2).0);
    assert!(verdict(&d2, later).contains("is out of date"), "{}", verdict(&d2, later));
    lab.run(r2, "conf t\ncrypto pki trustpoint LAB\nrevocation-check crl none\nend");
    let d2 = lab.t.device(r2).unwrap().clone();
    assert!(verdict(&d2, later).contains("does not verify"), "{}", verdict(&d2, later));
    // Fetching for a trustpoint that does not exist, or that cannot fetch, says so.
    lab.run(r2, "conf t");
    assert!(lab.try_run(r2, "crypto pki crl request NOPE").starts_with("% No trustpoint named NOPE"));
    lab.run(r2, "crypto pki trustpoint HAND\nenrollment terminal\nexit");
    assert!(lab.try_run(r2, "crypto pki crl request HAND").starts_with("% Only a trustpoint with `enrollment url`"));
}
