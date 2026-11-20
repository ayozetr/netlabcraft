//! `crypto ipsec profile` and `tunnel protection ipsec profile`: the console words, the running
//! configuration and `show crypto ipsec profile`, and a GRE tunnel whose packets are protected
//! with the profile's transform set, PFS group and lifetime (IKEv1 and IKEv2).

use super::{Lab, ping_settled, two_sites};

/// One site of the GRE tunnel protected by profile `PROT`: `site` 1 or 2, IKEv1 or IKEv2 (`v2`),
/// `pfs` (`group14` and so on) and `mode transport` when asked, and a 120 second key lifetime.
fn config(site: u8, v2: bool, pfs: Option<&str>, transport: bool) -> String {
    let (peer, tunnel_ip, far_net, far_hop) =
        if site == 1 { ("198.51.100.1", "10.0.0.1", "192.168.2.0", "10.0.0.2") } else { ("203.0.113.1", "10.0.0.2", "192.168.1.0", "10.0.0.1") };
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
    let mode = if transport { "mode transport\n" } else { "" };
    let ikev2 = if v2 { "set ikev2-profile PROF\n" } else { "" };
    let pfs = pfs.map(|g| format!("set pfs {g}\n")).unwrap_or_default();
    let source = if site == 1 { "203.0.113.1" } else { "198.51.100.1" };
    format!(
        "conf t
{ike}crypto ipsec transform-set TS esp-aes 256 esp-sha256-hmac
{mode}exit
crypto ipsec profile PROT
set transform-set TS
set security-association lifetime seconds 120
{ikev2}{pfs}exit
interface tunnel 0
ip address {tunnel_ip} 255.255.255.252
tunnel source {source}
tunnel destination {peer}
tunnel protection ipsec profile PROT
exit
ip route {far_net} 255.255.255.0 {far_hop}
end"
    )
}

fn sites(
    v2: bool,
    pfs1: Option<&str>,
    pfs2: Option<&str>,
    transport: bool,
) -> (Lab, netlabcraft_core::DeviceId, netlabcraft_core::DeviceId, netlabcraft_core::DeviceId) {
    let (mut lab, r1, _isp, r2, a) = two_sites();
    lab.run(r1, &config(1, v2, pfs1, transport));
    lab.run(r2, &config(2, v2, pfs2, transport));
    (lab, r1, r2, a)
}

#[test]
fn an_ipsec_profile_is_typed_shown_and_replayed() {
    let (mut lab, r1, ..) = two_sites();
    lab.run(r1, "conf t\ncrypto ipsec transform-set TS esp-aes esp-sha-hmac\nexit");
    lab.run(r1, "crypto ipsec profile PROT");
    assert_eq!(lab.prompt(r1), "R1(ipsec-profile)#");
    lab.run(
        r1,
        "set transform-set TS\nset pfs group14\nset security-association lifetime seconds 300\nset security-association lifetime kilobytes 5000",
    );
    for bad in ["group1", "group3", "14", "none"] {
        assert!(lab.try_run(r1, &format!("set pfs {bad}")).contains("The groups are"), "{bad}");
    }
    assert!(lab.try_run(r1, "set security-association lifetime seconds 5").contains("The lifetime is"));
    lab.run(r1, "exit\ninterface tunnel 0\nip address 10.0.0.1 255.255.255.252\ntunnel source 203.0.113.1\ntunnel destination 198.51.100.1");
    let missing = lab.run(r1, "tunnel protection ipsec profile NOPE");
    assert!(missing.contains("does not exist yet"), "{missing}");
    lab.run(r1, "tunnel protection ipsec profile PROT\nend");
    let cfg = lab.run(r1, "show running-config");
    assert!(
        cfg.contains(
            "crypto ipsec profile PROT\n set transform-set TS\n set security-association lifetime seconds 300\n set security-association lifetime kilobytes 5000\n set pfs group14\n!\n"
        ),
        "{cfg}"
    );
    assert!(cfg.contains(" tunnel protection ipsec profile PROT\n"), "{cfg}");
    let shown = lab.run(r1, "show crypto ipsec profile");
    assert!(shown.contains("IPsec profile PROT"), "{shown}");
    assert!(shown.contains("Transform set: TS (esp-aes esp-sha-hmac)"), "{shown}");
    assert!(shown.contains("PFS: group14") && shown.contains("Lifetime: 300 seconds, 5000 kilobytes"), "{shown}");
    assert!(shown.contains("Negotiated with: IKEv1") && shown.contains("Protects: Tunnel0"), "{shown}");
    // `no` forms.
    lab.run(r1, "conf t\ncrypto ipsec profile PROT\nno set pfs\nno set transform-set\nno set security-association lifetime seconds\nend");
    let shown = lab.run(r1, "show crypto ipsec profile");
    assert!(shown.contains("PFS: off") && shown.contains("Transform set: not set") && shown.contains("Lifetime: 3600 seconds, 5000"), "{shown}");
    // A crypto map's own words are not the profile's.
    lab.run(r1, "conf t\ncrypto ipsec profile PROT");
    assert!(lab.try_run(r1, "match address 101").contains('%'));
    lab.run(r1, "end");
    let cfg = lab.run(r1, "show running-config");
    assert!(!cfg.contains("match address"), "{cfg}");
    // Removing the protection and the profile.
    lab.run(r1, "conf t\ninterface tunnel 0\nno tunnel protection ipsec profile\nexit\nno crypto ipsec profile PROT\nend");
    let cfg = lab.run(r1, "show running-config");
    assert!(!cfg.contains("tunnel protection") && !cfg.contains("crypto ipsec profile"), "{cfg}");
    assert!(lab.run(r1, "show crypto ipsec profile").contains("No IPsec profiles configured"));
    lab.run(r1, "conf t");
    assert!(lab.try_run(r1, "no crypto ipsec profile PROT").contains("There is no IPsec profile"));
}

#[test]
fn a_protected_tunnel_negotiates_with_pfs_and_the_profile_lifetime() {
    let (mut lab, r1, r2, a) = sites(false, Some("group14"), Some("group14"), false);
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    for r in [r1, r2] {
        let sa = lab.run(r, "show crypto ipsec sa");
        assert!(sa.contains("Tunnel protection on Tunnel0: IPsec profile PROT"), "{sa}");
        assert!(sa.contains("PFS: group14 (a fresh Diffie-Hellman exchange gave these keys)"), "{sa}");
        assert!(sa.contains("of 120 seconds"), "{sa}");
        assert!(sa.contains("Mode: tunnel") && !sa.contains("Packets encapsulated 0,"), "{sa}");
    }
    // The keys are renewed with a fresh exchange when the profile's lifetime nears its end.
    lab.sim.run_for(&lab.t, 110 * 1_000_000);
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    let sa = lab.run(r1, "show crypto ipsec sa");
    assert!(sa.contains("keys renewed 1 time") && sa.contains("PFS: group14 (a fresh"), "{sa}");
}

#[test]
fn a_protected_tunnel_can_run_in_transport_mode() {
    let (mut lab, r1, _r2, a) = sites(false, None, None, true);
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    let sa = lab.run(r1, "show crypto ipsec sa");
    assert!(sa.contains("Mode: transport") && sa.contains("PFS: no"), "{sa}");
}

#[test]
fn a_protected_tunnel_is_refused_when_the_profiles_pfs_differs() {
    for (pfs1, pfs2) in [(Some("group14"), None), (None, Some("group14")), (Some("group14"), Some("group5"))] {
        let (mut lab, r1, r2, a) = sites(false, pfs1, pfs2, false);
        assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"), "{pfs1:?} {pfs2:?}");
        let both = lab.run(r1, "show crypto isakmp sa") + &lab.run(r2, "show crypto isakmp sa") + &lab.run(r1, "show crypto ipsec sa");
        assert!(both.contains("PFS differs"), "{both}");
    }
}

#[test]
fn a_protected_tunnel_negotiates_with_ikev2() {
    let (mut lab, r1, r2, a) = sites(true, Some("group14"), Some("group14"), false);
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    // The first child SA (IKE_AUTH) has no key exchange.
    let sa = lab.run(r1, "show crypto ipsec sa");
    assert!(sa.contains("Tunnel protection on Tunnel0") && sa.contains("group14 is configured, but these keys were made without it"), "{sa}");
    lab.sim.run_for(&lab.t, 110 * 1_000_000);
    lab.sim.clear_trace();
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    assert!(lab.sim.trace().iter().any(|e| e.summary.contains("IKEv2 CREATE_CHILD_SA request #2 [SK]")));
    for r in [r1, r2] {
        let sa = lab.run(r, "show crypto ipsec sa");
        assert!(sa.contains("keys renewed 1 time") && sa.contains("PFS: group14 (a fresh Diffie-Hellman exchange gave these keys)"), "{sa}");
    }
}
