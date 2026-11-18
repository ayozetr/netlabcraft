//! `debug crypto isakmp`, `debug crypto ipsec` and `debug crypto pki`: what IKE, the IPsec
//! associations and the certificate checks report when their topic is on, and nothing when it is
//! off.

use netlabcraft_core::DeviceId;

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
