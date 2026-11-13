//! A line that repeats a rule already in the list (M9.7).

use netlabcraft_model::DeviceKind;

use super::Lab;

#[test]
fn a_repeated_rule_with_its_own_number_is_added_and_one_without_says_where_it_is() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t\nip access-list extended D\n10 permit tcp any any eq 80\n20 deny ip any any");
    // With its own number: a second line of the same rule.
    lab.run(r, "15 permit tcp any any eq 80");
    let show = lab.run(r, "do show access-lists D");
    assert!(show.contains("10 permit tcp any any eq www") || show.contains("10 permit tcp any any eq 80"), "{show}");
    assert_eq!(show.matches("permit tcp any any eq").count(), 2, "{show}");
    assert!(show.contains("    15 permit tcp any any eq"), "{show}");
    // With no number it would be appended as a copy of a line that is there: it says which one.
    let said = lab.run(r, "permit tcp any any eq 80");
    assert!(said.contains("Line 10 already holds this rule, so nothing was added"), "{said}");
    assert_eq!(lab.run(r, "do show access-lists D").matches("permit tcp any any eq").count(), 2);
    // A different option makes it a different rule.
    assert_eq!(lab.run(r, "permit tcp any any eq 80 log"), "");
    // Numbered lists behave the same way.
    lab.run(r, "exit\naccess-list 10 permit host 10.0.0.1");
    let said = lab.run(r, "access-list 10 permit host 10.0.0.1");
    assert!(said.contains("Line 10 already holds this rule"), "{said}");
    lab.run(r, "ip access-list standard 10\n5 permit host 10.0.0.1");
    assert_eq!(lab.run(r, "do show access-lists 10").matches("host 10.0.0.1").count(), 2);
}

#[test]
fn each_copy_of_a_rule_counts_on_its_own() {
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
         ip access-list extended IN\n10 permit icmp any any echo\n20 permit icmp any any echo\n30 permit ip any any\nexit\nint g0/0\nip access-group IN in\nend",
    );
    lab.run(pc, "ping 192.168.2.10");
    let show = lab.run(r, "show access-lists IN");
    // The first copy takes every match; the second is shadowed and never counts.
    assert!(show.contains("10 permit icmp any any echo ("), "{show}");
    assert!(show.contains("    20 permit icmp any any echo\n"), "{show}");
}
