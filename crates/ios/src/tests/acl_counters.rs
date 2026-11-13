//! ACL hit counters follow their lines when lines are added, removed or renumbered (M9.7).

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// PC -- R -- Server, with an inbound list on the PC's side of the router.
fn setup() -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let pc = lab.add(DeviceKind::Pc);
    let r = lab.add(DeviceKind::Router);
    let srv = lab.add(DeviceKind::Server);
    lab.cable(pc, "fa0", r, "g0/0");
    lab.cable(srv, "g0", r, "g0/1");
    lab.run(pc, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(srv, "ipconfig 192.168.2.10 255.255.255.0 192.168.2.1");
    lab.run(
        r,
        "en\nconf t\nint g0/0\nip address 192.168.1.1 255.255.255.0\nno shut\nint g0/1\nip address 192.168.2.1 255.255.255.0\nno shut\nexit\n\
         ip access-list extended IN\n10 permit icmp any any echo\n20 permit tcp any any eq 80\n30 deny ip any any\nexit\nint g0/0\nip access-group IN in\nend",
    );
    lab.run(pc, "ping 192.168.2.10");
    lab.run(r, "clear access-list counters IN");
    (lab, pc, r)
}

/// The match count of the line with this sequence number (0 when none is shown).
fn matches(lab: &mut Lab, r: DeviceId, seq: u32) -> u64 {
    let show = lab.run(r, "show access-lists IN");
    let line = show.lines().find(|l| l.trim_start().starts_with(&format!("{seq} "))).unwrap_or("").to_string();
    line.split('(').nth(1).and_then(|c| c.split_whitespace().next()).and_then(|n| n.parse().ok()).unwrap_or(0)
}

#[test]
fn counters_stay_with_their_lines_when_a_line_is_inserted_above() {
    let (mut lab, pc, r) = setup();
    assert!(lab.run(pc, "ping 192.168.2.10").contains("4 answered"));
    assert_eq!(matches(&mut lab, r, 10), 4);
    // A new first line pushes the others down; their counters must not move with the positions.
    lab.run(r, "conf t\nip access-list extended IN\n5 deny udp any any eq 9999\nend");
    assert_eq!(matches(&mut lab, r, 5), 0);
    assert_eq!(matches(&mut lab, r, 10), 4);
    assert!(lab.run(pc, "ping 192.168.2.10").contains("4 answered"));
    assert_eq!(matches(&mut lab, r, 10), 8);
    assert_eq!(matches(&mut lab, r, 5), 0);
    // Removing the new line changes nothing for the others.
    lab.run(r, "conf t\nip access-list extended IN\nno 5\nend");
    assert_eq!(matches(&mut lab, r, 10), 8);
    // Removing a line above the counted one leaves its count, and the removed line's count is gone.
    lab.run(r, "conf t\nip access-list extended IN\n15 permit udp any any eq 53\nend");
    lab.run(r, "conf t\nip access-list extended IN\nno 10\n10 permit icmp any any echo-reply\nend");
    assert_eq!(matches(&mut lab, r, 10), 0, "a new line 10 does not inherit the old line's count");
}

#[test]
fn counters_follow_their_lines_through_a_resequence_and_the_implicit_deny_stays_apart() {
    let (mut lab, pc, r) = setup();
    assert!(lab.run(pc, "ping 192.168.2.10").contains("4 answered"));
    lab.run(r, "conf t\nip access-list resequence IN 100 20\nend");
    assert_eq!(matches(&mut lab, r, 100), 4);
    assert_eq!(matches(&mut lab, r, 120), 0);
    // Swapping which numbers are taken (the second line's old number is the first's new one).
    lab.run(r, "conf t\nip access-list resequence IN 120 10\nend");
    assert_eq!(matches(&mut lab, r, 120), 4);
    // The implicit deny at the end of a list that has no match keeps its own count.
    lab.run(r, "conf t\nip access-list extended IN\nno 140\nend");
    let show = lab.run(r, "show access-lists IN");
    assert!(show.contains("(implicit) deny everything else"), "{show}");
}

#[test]
fn deleting_a_line_by_its_text_drops_its_counter() {
    let (mut lab, pc, r) = setup();
    lab.run(pc, "ping 192.168.2.10");
    assert!(matches(&mut lab, r, 10) > 0);
    lab.run(r, "conf t\nip access-list extended IN\nno permit icmp any any echo\n10 permit icmp any any echo\nend");
    assert_eq!(matches(&mut lab, r, 10), 0);
}
