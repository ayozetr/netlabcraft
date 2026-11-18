//! `debug crypto isakmp`, `debug crypto ipsec` and `debug crypto pki`: what IKE, the IPsec
//! associations and the certificate checks report when their topic is on, and nothing when it is
//! off.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::{Lab, ping_settled, two_sites};

const SITE1: &str = "192.168.1.0 0.0.0.255 192.168.2.0 0.0.0.255";
const SITE2: &str = "192.168.2.0 0.0.0.255 192.168.1.0 0.0.0.255";

fn site_config(acl: &str, peer: &str, auth: &str) -> String {
    let key = if auth == "pre-share" { format!("crypto isakmp key s3cret address {peer}\n") } else { String::new() };
    format!(
        "conf t
access-list 110 permit ip {acl}
crypto isakmp policy 10
encryption aes 256
hash sha
authentication {auth}
group 14
exit
{key}crypto ipsec transform-set TS esp-aes esp-sha-hmac
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

fn enroll(lab: &mut Lab, r: DeviceId, ca: &str, cn: &str) {
    lab.run(
        r,
        &format!(
            "conf t\ncrypto pki trustpoint T\nenrollment url http://{ca}\nsubject-name CN={cn}, O=NetLab\nexit\ncrypto pki authenticate T\ncrypto pki enroll T\nend"
        ),
    );
}

#[test]
fn isakmp_and_ipsec_debugging_follow_the_exchange_and_the_association() {
    let (mut lab, r1, _isp, r2, a) = two_sites();
    lab.run(r1, &site_config(SITE1, "198.51.100.1", "pre-share"));
    lab.run(r2, &site_config(SITE2, "203.0.113.1", "pre-share"));
    // Off: the exchange says nothing.
    ping_settled(&mut lab, a, "192.168.2.10");
    assert!(!lab.run(r1, "show logging").contains("ISAKMP debug:"));
    lab.run(r1, "clear crypto sa");
    lab.run(r2, "clear crypto sa");
    lab.run(r1, "debug crypto isakmp\ndebug crypto ipsec");
    assert!(lab.run(r1, "show debugging").contains("ISAKMP") && lab.run(r1, "show debugging").contains("IPsec"));
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    let log = lab.run(r1, "show logging");
    assert!(log.contains("ISAKMP debug: main mode with 198.51.100.1 starts: message 1 offers 1 policy"), "{log}");
    assert!(log.contains("ISAKMP debug: main mode message from 198.51.100.1: SA"), "{log}");
    assert!(log.contains("ISAKMP debug: quick mode message from 198.51.100.1: encrypted payloads"), "{log}");
    assert!(log.contains("IPsec debug: IPsec SA with 198.51.100.1 is up: inbound SPI 0x"), "{log}");
    // The other router has no topic on: nothing from it.
    assert!(!lab.run(r2, "show logging").contains("ISAKMP debug:"));
    // A failure is reported with its reason.
    lab.run(r2, "conf t\ncrypto isakmp key different address 203.0.113.1\nend");
    lab.run(r1, "clear crypto sa");
    lab.run(r2, "clear crypto sa");
    ping_settled(&mut lab, a, "192.168.2.10");
    assert!(lab.run(r1, "show logging").contains("ISAKMP debug: the exchange with 198.51.100.1 failed:"));
    lab.run(r1, "no debug crypto isakmp\nundebug all");
    assert!(lab.run(r1, "show debugging").contains("No debugging is on"));
}

#[test]
fn pki_debugging_reports_the_certificate_checks_of_ike() {
    let (mut lab, r1, isp, r2, a) = two_sites();
    lab.run(isp, "conf t\nip http server\ncrypto pki server ROOT\nissuer-name CN=Lab Root, O=NetLab\ngrant auto\nno shutdown\nend");
    enroll(&mut lab, r1, "203.0.113.2", "r1.lab.test");
    enroll(&mut lab, r2, "198.51.100.2", "r2.lab.test");
    lab.run(r1, &site_config(SITE1, "198.51.100.1", "rsa-sig"));
    lab.run(r2, &site_config(SITE2, "203.0.113.1", "rsa-sig"));
    lab.run(r1, "debug crypto pki");
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    let log = lab.run(r1, "show logging");
    assert!(
        log.contains(
            "PKI debug: the certificate of CN=r2.lab.test, O=NetLab from 198.51.100.1 chains to a trusted authority and its signature checks out"
        ),
        "{log}"
    );
    lab.run(r1, "no debug crypto pki");
    assert!(lab.run(r1, "show debugging").contains("No debugging is on"));
}

#[test]
fn pki_debugging_reports_enrollment_and_revocation() {
    let mut lab = Lab::new();
    let ca = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(ca, "g0/0", r2, "g0/0");
    lab.run(
        ca,
        "en\nconf t\nhostname CA1\nint g0/0\nip address 10.0.0.1 255.255.255.0\nno shut\nexit\nip http server\ncrypto pki server LABCA\nissuer-name CN=Lab Root CA, O=NetLab\nno shutdown\nend",
    );
    lab.run(
        r2,
        "en\nconf t\nhostname Edge\nint g0/0\nip address 10.0.0.2 255.255.255.0\nno shut\nexit\ncrypto pki trustpoint LAB\nenrollment url http://10.0.0.1\nsubject-name CN=edge.lab.test\nexit\nend",
    );
    lab.run(r2, "debug crypto pki");
    lab.run(ca, "debug crypto pki");
    lab.run(r2, "conf t\ncrypto pki authenticate LAB\ncrypto pki enroll LAB\nend");
    lab.run(ca, "conf t\ncrypto pki server LABCA grant all");
    lab.run(r2, "conf t\ncrypto pki enroll LAB\nend");
    let edge = lab.run(r2, "show logging");
    assert!(edge.contains("PKI debug: trustpoint LAB trusts the CA CN=Lab Root CA, O=NetLab"), "{edge}");
    assert!(edge.contains("PKI debug: trustpoint LAB enrolled: certificate "), "{edge}");
    let granted = lab.run(ca, "end\nshow logging");
    assert!(granted.contains("PKI debug: request 1 for CN=edge.lab.test granted: certificate "), "{granted}");
    let serial = netlabcraft_sim::pki::serial_hex(
        &netlabcraft_wire::x509::Certificate::from_pem(&lab.t.device(r2).unwrap().pki.trustpoint("LAB").unwrap().certificate).unwrap().serial,
    );
    lab.run(ca, &format!("conf t\ncrypto pki server LABCA revoke {serial}\nend"));
    assert!(lab.run(ca, "show logging").contains(&format!("PKI debug: certificate {serial} revoked: revocation list number 1 published")));
    // Pasted text that is refused is reported with the reason; off, nothing more is said.
    lab.run(r2, "conf t\ncrypto pki trustpoint HAND\nenrollment terminal\nexit\ncrypto pki authenticate HAND\nnot a certificate");
    lab.try_run(r2, "quit");
    lab.run(r2, "end");
    assert!(lab.run(r2, "show logging").contains("PKI debug: the pasted text for HAND was refused: The pasted text is not a certificate"));
    lab.run(r2, "no debug crypto pki");
    let before = lab.run(r2, "show logging");
    lab.run(r2, "conf t\ncrypto pki authenticate HAND\nstill not a certificate");
    lab.try_run(r2, "quit");
    lab.run(r2, "end");
    assert_eq!(lab.run(r2, "show logging").matches("PKI debug:").count(), before.matches("PKI debug:").count());
}
