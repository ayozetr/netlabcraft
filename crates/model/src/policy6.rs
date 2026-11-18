//! Routing policy for IPv6 routes: prefix lists (`ipv6 prefix-list`) and how a route map reads an
//! IPv6 route (`match ipv6 address prefix-list`, `set ipv6 next-hop`).
//!
//! The IPv4 side lives in [`crate::policy`]; this is the same machinery for a 128-bit prefix. A route
//! map holds both kinds of `match` line. A clause that only names IPv4 lists never matches an IPv6
//! route, and a clause that only names IPv6 lists never matches an IPv4 one.

use netlabcraft_core::Ipv6Net;
use serde::{Deserialize, Serialize};

use crate::Device;
use crate::policy::{Attrs, Clause, MAX_PREFIX_ENTRIES, MAX_PREFIX_LISTS, MAX_SEQ, RouteMap, SEQ_STEP, Sets, clean_name};

/// One line of an IPv6 prefix list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefixEntry6 {
    pub seq: u32,
    pub permit: bool,
    /// The prefix (host bits clear).
    pub net: Ipv6Net,
    /// `ge N`: the shortest prefix length matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ge: Option<u8>,
    /// `le N`: the longest prefix length matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub le: Option<u8>,
}

impl PrefixEntry6 {
    /// Whether the lengths are consistent: `len < ge <= le <= 128`, or `len <= le <= 128` alone.
    pub fn valid(&self) -> bool {
        let len = self.net.prefix;
        if len > 128 {
            return false;
        }
        match (self.ge, self.le) {
            (None, None) => true,
            (Some(ge), None) => ge > len && ge <= 128,
            (None, Some(le)) => (len..=128).contains(&le),
            (Some(ge), Some(le)) => ge > len && ge <= le && le <= 128,
        }
    }

    /// The shortest and the longest prefix length this line matches.
    pub fn range(&self) -> (u8, u8) {
        match (self.ge, self.le) {
            (None, None) => (self.net.prefix, self.net.prefix),
            (ge, le) => (ge.unwrap_or(self.net.prefix), le.unwrap_or(128)),
        }
    }

    /// Whether `net` lies inside this line's prefix with a length in its range.
    pub fn matches(&self, net: Ipv6Net) -> bool {
        let (lo, hi) = self.range();
        (lo..=hi).contains(&net.prefix) && self.net.contains(net.network().addr)
    }

    /// The line as configuration words after the sequence number: `permit 2001:db8::/32 le 48`.
    pub fn text(&self) -> String {
        let mut s = format!("{} {}", if self.permit { "permit" } else { "deny" }, self.net);
        if let Some(ge) = self.ge {
            s.push_str(&format!(" ge {ge}"));
        }
        if let Some(le) = self.le {
            s.push_str(&format!(" le {le}"));
        }
        s
    }

    /// Whether two lines say the same thing (sequence numbers aside).
    pub fn same_rule(&self, o: &PrefixEntry6) -> bool {
        self.permit == o.permit && self.net == o.net && self.ge == o.ge && self.le == o.le
    }
}

/// `ipv6 prefix-list NAME …`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefixList6 {
    pub name: String,
    /// Kept in sequence order.
    #[serde(default)]
    pub entries: Vec<PrefixEntry6>,
}

impl PrefixList6 {
    pub fn new(name: impl Into<String>) -> PrefixList6 {
        PrefixList6 { name: name.into(), entries: Vec::new() }
    }

    /// The sequence number the next line gets when none is typed: five past the last one.
    pub fn next_seq(&self) -> Option<u32> {
        match self.entries.last() {
            None => Some(SEQ_STEP),
            Some(e) => e.seq.checked_add(SEQ_STEP).filter(|s| *s <= MAX_SEQ),
        }
    }

    /// Adds a line in sequence order.
    pub fn add(&mut self, e: PrefixEntry6) -> Result<(), String> {
        if self.entries.iter().any(|x| x.seq == e.seq) {
            return Err(format!("% Sequence {} is already in use in prefix list {}", e.seq, self.name));
        }
        if self.entries.iter().any(|x| x.same_rule(&e)) {
            return Err(format!("% That line is already in prefix list {}", self.name));
        }
        if self.entries.len() >= MAX_PREFIX_ENTRIES {
            return Err(format!("% A prefix list holds at most {MAX_PREFIX_ENTRIES} lines"));
        }
        let at = self.entries.iter().position(|x| x.seq > e.seq).unwrap_or(self.entries.len());
        self.entries.insert(at, e);
        Ok(())
    }

    /// Whether the first matching line permits `net`; a prefix no line matches is denied.
    pub fn permits(&self, net: Ipv6Net) -> bool {
        self.entries.iter().find(|e| e.matches(net)).is_some_and(|e| e.permit)
    }

    /// The index of the first line that matches `net`.
    pub fn first_match(&self, net: Ipv6Net) -> Option<usize> {
        self.entries.iter().position(|e| e.matches(net))
    }
}

/// What a route map looks at in an IPv6 route: the prefix and the route's metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subject6 {
    pub net: Ipv6Net,
    pub metric: u32,
}

impl Clause {
    /// [`Clause::matches_with`] for an IPv6 route: the `match ipv6 address prefix-list` lines, the
    /// metric and the path attributes. A clause with a `match ip address` line and no IPv6 line does
    /// not match (it asks about an IPv4 address).
    pub fn matches_with6(&self, d: &Device, s: &Subject6, a: &Attrs) -> bool {
        if self.metric.is_some_and(|m| m != s.metric) {
            return false;
        }
        let asks_v4 = !(self.prefix_lists.is_empty() && self.acls.is_empty());
        if (asks_v4 || !self.prefix_lists6.is_empty()) && !self.prefix_lists6.iter().any(|n| d.prefix_list6(n).is_some_and(|l| l.permits(s.net))) {
            return false;
        }
        if !self.community_lists.is_empty() && !self.community_lists.iter().any(|n| d.community_list(n).is_some_and(|l| l.permits(a.communities))) {
            return false;
        }
        if !self.ext_community_lists.is_empty()
            && !self.ext_community_lists.iter().any(|n| d.ext_community_list(n).is_some_and(|l| l.permits(a.ext_communities)))
        {
            return false;
        }
        if !self.as_path_lists.is_empty() && !self.as_path_lists.iter().any(|n| d.as_path_list(n).is_some_and(|l| l.permits(a.as_path, a.as_set))) {
            return false;
        }
        if !self.tags.is_empty() && !self.tags.contains(&a.tag) {
            return false;
        }
        true
    }
}

impl RouteMap {
    /// [`RouteMap::run_with`] for an IPv6 route: the first clause whose lines hold decides.
    pub fn run_with6(&self, d: &Device, s: &Subject6, a: &Attrs) -> Option<&Sets> {
        let c = self.clauses.iter().find(|c| c.matches_with6(d, s, a))?;
        c.permit.then_some(&c.sets)
    }
}

impl Device {
    pub fn prefix_list6(&self, name: &str) -> Option<&PrefixList6> {
        self.prefix_lists6.iter().find(|l| l.name == name)
    }
}

/// What a neighbour's or a redistribution's policy does to an IPv6 route: `None` when it is
/// dropped, else the attributes to change. A prefix list is checked first, then the route map; a
/// list or map that is named but not defined lets everything through, as for IPv4.
pub fn apply_with6(d: &Device, list: Option<&str>, map: Option<&str>, s: &Subject6, a: &Attrs) -> Option<Sets> {
    if let Some(l) = list.and_then(|n| d.prefix_list6(n))
        && !l.permits(s.net)
    {
        return None;
    }
    match map.and_then(|n| d.route_map(n)) {
        Some(m) => m.run_with6(d, s, a).cloned(),
        None => Some(Sets::default()),
    }
}

/// Cleans the IPv6 prefix lists a lab file holds: bounded, unique names, valid lines in order.
pub fn sanitize_prefix_lists6(lists: &mut Vec<PrefixList6>) {
    for l in lists.iter_mut() {
        l.name = clean_name(&l.name);
        l.entries.retain(|e| e.valid() && (1..=MAX_SEQ).contains(&e.seq));
        for e in &mut l.entries {
            e.net = e.net.network();
        }
        l.entries.sort_by_key(|e| e.seq);
        l.entries.dedup_by_key(|e| e.seq);
        l.entries.truncate(MAX_PREFIX_ENTRIES);
    }
    lists.retain(|l| !l.name.is_empty());
    let mut seen = std::collections::BTreeSet::new();
    lists.retain(|l| seen.insert(l.name.clone()));
    lists.truncate(MAX_PREFIX_LISTS);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceKind;

    fn net(s: &str) -> Ipv6Net {
        let (a, p) = s.split_once('/').unwrap_or((s, "128"));
        Ipv6Net::new(a.parse().unwrap(), p.parse().unwrap())
    }

    fn line(seq: u32, permit: bool, p: &str, ge: Option<u8>, le: Option<u8>) -> PrefixEntry6 {
        PrefixEntry6 { seq, permit, net: net(p), ge, le }
    }

    #[test]
    fn a_line_without_ge_or_le_matches_exactly_that_prefix() {
        let e = line(5, true, "2001:db8:1::/48", None, None);
        assert!(e.matches(net("2001:db8:1::/48")));
        assert!(!e.matches(net("2001:db8:1::/64")) && !e.matches(net("2001:db8::/32")));
        let range = line(5, true, "2001:db8::/32", Some(48), Some(64));
        assert!(range.matches(net("2001:db8:7::/48")) && range.matches(net("2001:db8:7:1::/64")));
        assert!(!range.matches(net("2001:db8:7::/40")) && !range.matches(net("2001:db8:7:1::/80")) && !range.matches(net("2001:db9::/48")));
        assert!(!line(5, true, "2001:db8::/32", Some(32), None).valid(), "ge must exceed the length");
        assert!(!line(5, true, "2001:db8::/32", Some(130), None).valid());
        assert_eq!(range.text(), "permit 2001:db8::/32 ge 48 le 64");
    }

    #[test]
    fn the_first_matching_line_decides_and_an_unmatched_prefix_is_denied() {
        let mut l = PrefixList6::new("L");
        assert_eq!(l.next_seq(), Some(5));
        l.add(line(10, false, "2001:db8:bad::/48", None, None)).unwrap();
        l.add(line(20, true, "2001:db8::/32", None, Some(64))).unwrap();
        assert_eq!(l.next_seq(), Some(25));
        assert!(!l.permits(net("2001:db8:bad::/48")), "the deny line comes first");
        assert!(l.permits(net("2001:db8:1::/48")));
        assert!(!l.permits(net("2001:db9::/32")), "no line matches");
        assert_eq!(l.first_match(net("2001:db8:1::/48")), Some(1));
        assert!(l.add(line(10, true, "2001:db8:2::/48", None, None)).is_err(), "sequence in use");
        assert!(l.add(line(30, true, "2001:db8::/32", None, Some(64))).is_err(), "same rule twice");
    }

    #[test]
    fn a_route_map_reads_ipv6_lists_and_ignores_ipv4_ones() {
        let mut t = crate::Topology::new();
        let id = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        let mut d = t.device(id).unwrap().clone();
        let mut pl = PrefixList6::new("TEN");
        pl.add(line(5, true, "2001:db8:10::/48", None, Some(64))).unwrap();
        d.prefix_lists6.push(pl);
        let mut m = RouteMap::new("M");
        let c = m.clause_mut(10, true).unwrap();
        c.prefix_lists6.push("TEN".into());
        c.sets.next_hop6 = Some("2001:db8::99".parse().unwrap());
        m.clause_mut(20, true);
        d.route_maps.push(m);
        let inside = Subject6 { net: net("2001:db8:10:1::/64"), metric: 0 };
        let outside = Subject6 { net: net("2001:db8:20::/48"), metric: 0 };
        let map = d.route_map("M").unwrap();
        let attrs = Attrs::default();
        assert_eq!(map.run_with6(&d, &inside, &attrs).and_then(|s| s.next_hop6), Some("2001:db8::99".parse().unwrap()));
        assert_eq!(map.run_with6(&d, &outside, &attrs).and_then(|s| s.next_hop6), None, "the open clause 20 catches it");
        // The same list name does not exist for IPv4, so an IPv4 route skips clause 10.
        let v4 = crate::policy::Subject { net: "10.0.0.0/8".parse().unwrap(), metric: 0 };
        assert_eq!(map.run(&d, &v4).and_then(|s| s.next_hop6), None);
        // A named list that is not defined lets everything through.
        assert!(apply_with6(&d, Some("MISSING"), Some("NOMAP"), &outside, &attrs).is_some());
        assert!(apply_with6(&d, Some("TEN"), None, &outside, &attrs).is_none());
        assert!(apply_with6(&d, Some("TEN"), None, &inside, &attrs).is_some());
    }

    #[test]
    fn sanitize_drops_bad_lines_and_duplicate_lists() {
        let mut a = PrefixList6::new("A");
        a.entries.push(line(5, true, "2001:db8:1:2::/48", None, None));
        a.entries.push(line(5, false, "2001:db8::/32", None, None));
        a.entries.push(line(0, true, "2001:db8::/32", None, None));
        a.entries.push(line(7, true, "2001:db8::/32", Some(20), None));
        let mut lists = vec![a.clone(), a, PrefixList6::new("")];
        sanitize_prefix_lists6(&mut lists);
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].entries.len(), 1);
        assert_eq!(lists[0].entries[0].net, net("2001:db8:1::/48"));
    }
}
