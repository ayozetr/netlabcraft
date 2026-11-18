//! `set pfs` in a crypto map: the console words, a fresh Diffie-Hellman exchange in IKEv1 quick
//! mode and in IKEv2's CREATE_CHILD_SA, and the reason when the two ends disagree.

use super::{Lab, ping_settled, two_sites};

const ACL1: &str = "192.168.1.0 0.0.0.255 192.168.2.0 0.0.0.255";
const ACL2: &str = "192.168.2.0 0.0.0.255 192.168.1.0 0.0.0.255";

/// A site's IPsec configuration with a 120 second key lifetime, IKEv1 or IKEv2 (`v2`), and
/// `pfs` (`group14` and so on) in the crypto map when given.
fn config(v2: bool, acl: &str, peer: &str, pfs: Option<&str>) -> String {
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
    let pfs = pfs.map(|g| format!("set pfs {g}\n")).unwrap_or_default();
    format!(
        "conf t
access-list 110 permit ip {acl}
{ike}crypto ipsec transform-set TS esp-aes 256 esp-sha256-hmac
exit
crypto map VPN 10 ipsec-isakmp
set peer {peer}
set transform-set TS
set security-association lifetime seconds 120
{profile}{pfs}match address 110
exit
int g0/1
crypto map VPN
end"
    )
}

fn site(
    v2: bool,
    pfs1: Option<&str>,
    pfs2: Option<&str>,
) -> (Lab, netlabcraft_core::DeviceId, netlabcraft_core::DeviceId, netlabcraft_core::DeviceId) {
    let (mut lab, r1, _isp, r2, a) = two_sites();
    lab.run(r1, &config(v2, ACL1, "198.51.100.1", pfs1));
    lab.run(r2, &config(v2, ACL2, "203.0.113.1", pfs2));
    (lab, r1, r2, a)
}

#[test]
fn set_pfs_takes_the_listed_groups_and_is_shown_and_replayed() {
    let (mut lab, r1, ..) = two_sites();
    lab.run(r1, "conf t\ncrypto map VPN 10 ipsec-isakmp");
    for g in ["group2", "group5", "group14", "group15", "group16", "group19", "group20", "group24"] {
        lab.run(r1, &format!("set pfs {g}"));
        assert_eq!(lab.t.device(r1).unwrap().crypto.maps[0].pfs, g.strip_prefix("group").and_then(|n| n.parse().ok()));
    }
    for bad in ["group1", "group3", "group25", "14", "none"] {
        assert!(lab.try_run(r1, &format!("set pfs {bad}")).contains("The groups are"), "{bad}");
    }
    lab.run(r1, "set pfs group14\nend");
    assert!(lab.run(r1, "show running-config").contains(" set pfs group14\n"));
    assert!(lab.run(r1, "show crypto map").contains("PFS: group14"));
    lab.run(r1, "conf t\ncrypto map VPN 10 ipsec-isakmp\nno set pfs\nend");
    assert!(!lab.run(r1, "show running-config").contains("set pfs"));
    assert!(lab.run(r1, "show crypto map").contains("PFS: off"));
}

#[test]
fn ikev1_quick_mode_runs_a_fresh_exchange_for_every_set_of_keys() {
    for group in ["group14", "group19", "group24"] {
        let (mut lab, r1, r2, a) = site(false, Some(group), Some(group));
        assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"), "{group}");
        for r in [r1, r2] {
            let sa = lab.run(r, "show crypto ipsec sa");
            assert!(sa.contains(&format!("PFS: {group} (a fresh Diffie-Hellman exchange gave these keys)")), "{sa}");
        }
        // Renewing the keys runs the exchange again, and traffic keeps flowing.
        lab.sim.run_for(&lab.t, 110 * 1_000_000);
        assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"), "{group}");
        let sa = lab.run(r1, "show crypto ipsec sa");
        assert!(sa.contains("keys renewed 1 time") && sa.contains(&format!("PFS: {group} (a fresh")), "{sa}");
    }
    // Without it the keys come from the IKE keys alone.
    let (mut lab, r1, _r2, a) = site(false, None, None);
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    assert!(lab.run(r1, "show crypto ipsec sa").contains("PFS: no (the IPsec keys come from the IKE keys alone)"));
}

#[test]
fn ikev1_refuses_a_pfs_mismatch_and_says_which() {
    for (pfs1, pfs2) in [(Some("group14"), None), (None, Some("group14")), (Some("group14"), Some("group5"))] {
        let (mut lab, r1, r2, a) = site(false, pfs1, pfs2);
        assert!(ping_settled(&mut lab, a, "192.168.2.10").contains(" 0 answered"), "{pfs1:?} {pfs2:?}");
        let both = lab.run(r1, "show crypto isakmp sa") + &lab.run(r2, "show crypto isakmp sa") + &lab.run(r1, "show crypto ipsec sa");
        assert!(both.contains("PFS differs") && both.contains("both crypto map entries need the same `set pfs`"), "{both}");
        assert!(both.contains("PFS group14"), "{both}");
    }
}

#[test]
fn ikev2_gives_a_renewed_child_sa_a_fresh_exchange() {
    let (mut lab, r1, r2, a) = site(true, Some("group14"), Some("group14"));
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    // The first child SA (IKE_AUTH) has no key exchange.
    let sa = lab.run(r1, "show crypto ipsec sa");
    assert!(sa.contains("group14 is configured, but these keys were made without it"), "{sa}");
    lab.sim.run_for(&lab.t, 110 * 1_000_000);
    lab.sim.clear_trace();
    assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"));
    assert!(lab.sim.trace().iter().any(|e| e.summary.contains("IKEv2 CREATE_CHILD_SA request #2 [SK]")));
    for r in [r1, r2] {
        let sa = lab.run(r, "show crypto ipsec sa");
        assert!(sa.contains("keys renewed 1 time") && sa.contains("PFS: group14 (a fresh Diffie-Hellman exchange gave these keys)"), "{sa}");
    }
}

#[test]
fn ikev2_refuses_to_renew_a_child_sa_when_pfs_differs() {
    for (pfs1, pfs2) in [(Some("group14"), None), (None, Some("group14")), (Some("group14"), Some("group5"))] {
        let (mut lab, r1, r2, a) = site(true, pfs1, pfs2);
        assert!(ping_settled(&mut lab, a, "192.168.2.10").contains("4 answered"), "{pfs1:?} {pfs2:?}");
        lab.sim.run_for(&lab.t, 110 * 1_000_000);
        let _ = ping_settled(&mut lab, a, "192.168.2.10");
        let logs = lab.run(r1, "show logging") + &lab.run(r2, "show logging");
        assert!(logs.contains("PFS differs"), "{pfs1:?} {pfs2:?}: {logs}");
        assert!(logs.contains("NO_PROPOSAL_CHOSEN"), "{logs}");
        // Nothing was renewed.
        assert!(lab.run(r1, "show crypto ipsec sa").contains("keys renewed 0 times"));
    }
}
