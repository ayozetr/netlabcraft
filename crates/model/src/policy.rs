//! Routing policy: prefix lists (`ip prefix-list`) and route maps (`route-map`).
//!
//! A prefix list is an ordered list of permit and deny lines, each naming a prefix and, with
//! `ge` and `le`, the range of prefix lengths it covers; the first line that matches decides and
//! a prefix no line matches is denied. A route map is an ordered list of clauses; the first
//! clause whose `match` lines all hold decides: a permit clause lets the route through and
//! applies its `set` lines, a deny clause drops it, and a route no clause matches is dropped.
//!
//! The model only keeps the configuration and answers "may this route pass, and what does the
//! map change"; where policy applies (a BGP neighbour, a redistribution) is up to the
//! simulator, which reads the answer with [`apply`].

use std::net::Ipv4Addr;

use netlabcraft_core::Ipv4Net;
use serde::{Deserialize, Serialize};

use crate::Device;
use crate::acl::PacketKey;

/// Most prefix lists, and most lines in one, a device keeps.
pub const MAX_PREFIX_LISTS: usize = 128;
pub const MAX_PREFIX_ENTRIES: usize = 512;
/// Most route maps, and most clauses in one.
pub const MAX_ROUTE_MAPS: usize = 128;
pub const MAX_CLAUSES: usize = 64;
/// Most list names a `match` line holds, and most AS numbers a `set as-path prepend` holds.
pub const MAX_MATCH_NAMES: usize = 16;
pub const MAX_PREPEND: usize = 16;
/// Longest list, map or ACL name kept.
pub const MAX_NAME: usize = 64;
/// The highest sequence number a prefix-list line or a clause can have.
pub const MAX_SEQ: u32 = 4_294_967_294;
/// The step between automatically numbered prefix-list lines.
pub const SEQ_STEP: u32 = 5;
/// The sequence number a route map's first clause gets when none is typed.
pub const FIRST_CLAUSE: u32 = 10;

/// A name as a lab file may hold it: no control characters, no spaces, bounded.
pub fn clean_name(s: &str) -> String {
    s.chars().filter(|c| !c.is_control() && !c.is_whitespace()).take(MAX_NAME).collect()
}

/// One line of a prefix list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefixEntry {
    pub seq: u32,
    pub permit: bool,
    /// The prefix (host bits clear).
    pub net: Ipv4Net,
    /// `ge N`: the shortest prefix length matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ge: Option<u8>,
    /// `le N`: the longest prefix length matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub le: Option<u8>,
}

impl PrefixEntry {
    /// Whether the lengths are consistent: `len < ge <= le <= 32`, or `len <= le <= 32` alone.
    pub fn valid(&self) -> bool {
        let len = self.net.prefix;
        if len > 32 {
            return false;
        }
        match (self.ge, self.le) {
            (None, None) => true,
            (Some(ge), None) => ge > len && ge <= 32,
            (None, Some(le)) => (len..=32).contains(&le),
            (Some(ge), Some(le)) => ge > len && ge <= le && le <= 32,
        }
    }

    /// The shortest and the longest prefix length this line matches.
    pub fn range(&self) -> (u8, u8) {
        match (self.ge, self.le) {
            (None, None) => (self.net.prefix, self.net.prefix),
            (ge, le) => (ge.unwrap_or(self.net.prefix), le.unwrap_or(32)),
        }
    }

    /// Whether `net` lies inside this line's prefix with a length in its range.
    pub fn matches(&self, net: Ipv4Net) -> bool {
        let (lo, hi) = self.range();
        (lo..=hi).contains(&net.prefix) && self.net.contains(net.network().addr)
    }

    /// The line as configuration words after the sequence number: `permit 10.0.0.0/8 le 24`.
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
    pub fn same_rule(&self, o: &PrefixEntry) -> bool {
        self.permit == o.permit && self.net == o.net && self.ge == o.ge && self.le == o.le
    }
}

/// `ip prefix-list NAME …`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefixList {
    pub name: String,
    /// Kept in sequence order.
    #[serde(default)]
    pub entries: Vec<PrefixEntry>,
}

impl PrefixList {
    pub fn new(name: impl Into<String>) -> PrefixList {
        PrefixList { name: name.into(), entries: Vec::new() }
    }

    /// The sequence number the next line gets when none is typed: five past the last one.
    pub fn next_seq(&self) -> Option<u32> {
        match self.entries.last() {
            None => Some(SEQ_STEP),
            Some(e) => e.seq.checked_add(SEQ_STEP).filter(|s| *s <= MAX_SEQ),
        }
    }

    /// Adds a line in sequence order.
    pub fn add(&mut self, e: PrefixEntry) -> Result<(), String> {
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
    pub fn permits(&self, net: Ipv4Net) -> bool {
        self.entries.iter().find(|e| e.matches(net)).is_some_and(|e| e.permit)
    }

    /// The index of the first line that matches `net`.
    pub fn first_match(&self, net: Ipv4Net) -> Option<usize> {
        self.entries.iter().position(|e| e.matches(net))
    }
}

/// The well-known communities (RFC 1997): never advertise to an external neighbour, never
/// advertise to any neighbour, never leave the local AS (no confederations here, so it acts like
/// no-export), and the internet community that every route belongs to.
pub const NO_EXPORT: u32 = 0xFFFF_FF01;
pub const NO_ADVERTISE: u32 = 0xFFFF_FF02;
pub const LOCAL_AS: u32 = 0xFFFF_FF03;
pub const INTERNET: u32 = 0;
/// Most communities one route carries, and most community lists and AS-path lists a device keeps.
pub const MAX_COMMUNITIES: usize = 32;
pub const MAX_COMMUNITY_LISTS: usize = 64;
pub const MAX_AS_PATH_LISTS: usize = 64;
/// Most lines in one community list or AS-path list.
pub const MAX_LIST_LINES: usize = 64;

/// A community as written: `65001:100`, a plain number, or a well-known word.
pub fn parse_community(text: &str) -> Option<u32> {
    let t = text.trim().to_ascii_lowercase();
    match t.as_str() {
        "no-export" => return Some(NO_EXPORT),
        "no-advertise" => return Some(NO_ADVERTISE),
        "local-as" => return Some(LOCAL_AS),
        "internet" => return Some(INTERNET),
        _ => {}
    }
    if let Some((hi, lo)) = t.split_once(':') {
        let hi = hi.parse::<u16>().ok()?;
        let lo = lo.parse::<u16>().ok()?;
        return Some((u32::from(hi) << 16) | u32::from(lo));
    }
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    t.parse::<u32>().ok()
}

/// The `AA:NN` form of a community.
pub fn community_pair(c: u32) -> String {
    format!("{}:{}", c >> 16, c & 0xFFFF)
}

/// A community as `show` and the running configuration write it: a well-known one by name, else
/// `AA:NN` (the new format) or one decimal number (the old format).
pub fn community_text(c: u32, new_format: bool) -> String {
    match c {
        NO_EXPORT => "no-export".into(),
        NO_ADVERTISE => "no-advertise".into(),
        LOCAL_AS => "local-AS".into(),
        INTERNET => "internet".into(),
        _ if new_format => community_pair(c),
        _ => c.to_string(),
    }
}

/// What a regular expression on communities is matched against: each community as `AA:NN`,
/// separated by spaces.
pub fn communities_for_regex(c: &[u32]) -> String {
    c.iter().map(|x| community_pair(*x)).collect::<Vec<_>>().join(" ")
}

/// What a regular expression on AS paths is matched against: the path as numbers separated by
/// spaces, then the AS set (if any) in braces.
pub fn as_path_for_regex(path: &[u32], set: &[u32]) -> String {
    let mut s = path.iter().map(u32::to_string).collect::<Vec<_>>().join(" ");
    if !set.is_empty() {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push('{');
        s.push_str(&set.iter().map(u32::to_string).collect::<Vec<_>>().join(","));
        s.push('}');
    }
    s
}

/// One line of a community list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommunityEntry {
    pub permit: bool,
    /// A standard line: the communities a route must all carry (`internet` matches every route).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub communities: Vec<u32>,
    /// An expanded line: a regular expression over `AA:NN` text.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub regex: String,
}

impl CommunityEntry {
    /// Whether the line matches a route carrying `have`.
    pub fn matches(&self, have: &[u32], expanded: bool) -> bool {
        if expanded {
            return crate::textmatch::is_match(&self.regex, &communities_for_regex(have));
        }
        !self.communities.is_empty() && self.communities.iter().all(|c| *c == INTERNET || have.contains(c))
    }
}

/// `ip community-list standard|expanded NAME …`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommunityList {
    pub name: String,
    #[serde(default)]
    pub expanded: bool,
    #[serde(default)]
    pub entries: Vec<CommunityEntry>,
}

impl CommunityList {
    pub fn new(name: impl Into<String>, expanded: bool) -> CommunityList {
        CommunityList { name: name.into(), expanded, entries: Vec::new() }
    }

    /// Whether the first matching line permits a route carrying `have`; no line matching denies.
    pub fn permits(&self, have: &[u32]) -> bool {
        self.entries.iter().find(|e| e.matches(have, self.expanded)).is_some_and(|e| e.permit)
    }
}

/// One line of an AS-path list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AsPathEntry {
    pub permit: bool,
    pub regex: String,
}

/// `ip as-path access-list NAME permit|deny REGEX`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AsPathList {
    pub name: String,
    #[serde(default)]
    pub entries: Vec<AsPathEntry>,
}

impl AsPathList {
    pub fn new(name: impl Into<String>) -> AsPathList {
        AsPathList { name: name.into(), entries: Vec::new() }
    }

    /// Whether the first line whose expression matches the path permits it; none matching denies.
    pub fn permits(&self, path: &[u32], set: &[u32]) -> bool {
        let text = as_path_for_regex(path, set);
        self.entries.iter().find(|e| crate::textmatch::is_match(&e.regex, &text)).is_some_and(|e| e.permit)
    }
}

/// The path attributes a route map can match on (everything but the prefix and the metric).
#[derive(Debug, Clone, Copy, Default)]
pub struct Attrs<'a> {
    pub communities: &'a [u32],
    pub as_path: &'a [u32],
    pub as_set: &'a [u32],
}

/// The communities a route carries after a `set community`: the new ones replace the old ones, or
/// join them with `additive`; `set community none` leaves none.
pub fn merge_communities(sets: &Sets, have: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = match &sets.community {
        None => have.to_vec(),
        Some(new) if sets.community_additive => have.iter().chain(new.iter()).copied().collect(),
        Some(new) => new.clone(),
    };
    out.sort_unstable();
    out.dedup();
    out.truncate(MAX_COMMUNITIES);
    out
}

/// The attributes a `set` line changes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_pref: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<u32>,
    /// `set as-path prepend`: the AS numbers put in front of the path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prepend: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_hop: Option<Ipv4Addr>,
    /// 0 igp, 1 egp, 2 incomplete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<u8>,
    /// `set community …`: the communities to give (empty list: `none`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub community: Option<Vec<u32>>,
    /// `additive`: keep the communities the route already has.
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub community_additive: bool,
}

impl Sets {
    pub fn is_empty(&self) -> bool {
        *self == Sets::default()
    }
}

/// The word BGP gives an ORIGIN code.
pub fn origin_word(o: u8) -> &'static str {
    match o {
        0 => "igp",
        1 => "egp",
        _ => "incomplete",
    }
}

/// One `route-map NAME permit|deny SEQ` clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clause {
    pub seq: u32,
    pub permit: bool,
    /// `match ip address prefix-list NAME…`: any of these lists permitting the prefix matches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prefix_lists: Vec<String>,
    /// `match ip address NAME|NUMBER…`: any of these access lists permitting the prefix matches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acls: Vec<String>,
    /// `match metric N`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<u32>,
    /// `match community NAME…`: any of these community lists permitting the route matches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub community_lists: Vec<String>,
    /// `match as-path NAME…`: any of these AS-path lists permitting the route matches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub as_path_lists: Vec<String>,
    #[serde(default, skip_serializing_if = "Sets::is_empty")]
    pub sets: Sets,
}

impl Clause {
    pub fn new(seq: u32, permit: bool) -> Clause {
        Clause {
            seq,
            permit,
            prefix_lists: Vec::new(),
            acls: Vec::new(),
            metric: None,
            community_lists: Vec::new(),
            as_path_lists: Vec::new(),
            sets: Sets::default(),
        }
    }

    /// Whether every kind of `match` line holds (a clause with none matches everything). Several
    /// names on one kind are alternatives; a list or an access list that does not exist matches
    /// nothing.
    pub fn matches(&self, d: &Device, s: &Subject) -> bool {
        self.matches_with(d, s, &Attrs::default())
    }

    /// [`Clause::matches`] with the path attributes `match community` and `match as-path` read.
    pub fn matches_with(&self, d: &Device, s: &Subject, a: &Attrs) -> bool {
        if self.metric.is_some_and(|m| m != s.metric) {
            return false;
        }
        if !(self.prefix_lists.is_empty() && self.acls.is_empty()) {
            let by_list = self.prefix_lists.iter().any(|n| d.prefix_list(n).is_some_and(|l| l.permits(s.net)));
            let key = PacketKey::bare(0, s.net.addr, s.net.mask());
            let by_acl = self.acls.iter().any(|n| d.acls.iter().find(|x| x.name == *n).is_some_and(|x| x.evaluate(&key).0));
            if !(by_list || by_acl) {
                return false;
            }
        }
        if !self.community_lists.is_empty() && !self.community_lists.iter().any(|n| d.community_list(n).is_some_and(|l| l.permits(a.communities))) {
            return false;
        }
        if !self.as_path_lists.is_empty() && !self.as_path_lists.iter().any(|n| d.as_path_list(n).is_some_and(|l| l.permits(a.as_path, a.as_set))) {
            return false;
        }
        true
    }
}

/// What a route map looks at: the prefix and the route's metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subject {
    pub net: Ipv4Net,
    pub metric: u32,
}

/// `route-map NAME …`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteMap {
    pub name: String,
    /// Kept in sequence order.
    #[serde(default)]
    pub clauses: Vec<Clause>,
}

impl RouteMap {
    pub fn new(name: impl Into<String>) -> RouteMap {
        RouteMap { name: name.into(), clauses: Vec::new() }
    }

    pub fn clause(&self, seq: u32) -> Option<&Clause> {
        self.clauses.iter().find(|c| c.seq == seq)
    }

    /// The clause with this sequence number, added in order when missing; `None` when the map is
    /// full.
    pub fn clause_mut(&mut self, seq: u32, permit: bool) -> Option<&mut Clause> {
        if !self.clauses.iter().any(|c| c.seq == seq) {
            if self.clauses.len() >= MAX_CLAUSES {
                return None;
            }
            let at = self.clauses.iter().position(|c| c.seq > seq).unwrap_or(self.clauses.len());
            self.clauses.insert(at, Clause::new(seq, permit));
        }
        self.clauses.iter_mut().find(|c| c.seq == seq)
    }

    /// The first clause whose `match` lines hold: `Some(sets)` when it permits, `None` when it
    /// denies or when no clause matches.
    pub fn run(&self, d: &Device, s: &Subject) -> Option<&Sets> {
        self.run_with(d, s, &Attrs::default())
    }

    /// [`RouteMap::run`] with the path attributes the `match` lines may read.
    pub fn run_with(&self, d: &Device, s: &Subject, a: &Attrs) -> Option<&Sets> {
        let c = self.clauses.iter().find(|c| c.matches_with(d, s, a))?;
        c.permit.then_some(&c.sets)
    }

    /// The index of the clause that decides for `s`.
    pub fn deciding(&self, d: &Device, s: &Subject) -> Option<usize> {
        self.clauses.iter().position(|c| c.matches(d, s))
    }
}

impl Device {
    pub fn prefix_list(&self, name: &str) -> Option<&PrefixList> {
        self.prefix_lists.iter().find(|l| l.name == name)
    }

    pub fn route_map(&self, name: &str) -> Option<&RouteMap> {
        self.route_maps.iter().find(|m| m.name == name)
    }

    pub fn community_list(&self, name: &str) -> Option<&CommunityList> {
        self.community_lists.iter().find(|l| l.name == name)
    }

    pub fn as_path_list(&self, name: &str) -> Option<&AsPathList> {
        self.as_path_lists.iter().find(|l| l.name == name)
    }
}

/// What a neighbour's or a redistribution's policy does to a route: `None` when it is dropped,
/// else the attributes to change. A prefix list is checked first, then the route map; a list or
/// map that is named but not defined lets everything through (a name can be typed before its
/// list is, and a configuration read back lists neighbours before the maps).
pub fn apply(d: &Device, list: Option<&str>, map: Option<&str>, s: &Subject) -> Option<Sets> {
    apply_with(d, list, map, s, &Attrs::default())
}

/// [`apply`] for a route whose communities and AS path the map may match on.
pub fn apply_with(d: &Device, list: Option<&str>, map: Option<&str>, s: &Subject, a: &Attrs) -> Option<Sets> {
    if let Some(l) = list.and_then(|n| d.prefix_list(n))
        && !l.permits(s.net)
    {
        return None;
    }
    match map.and_then(|n| d.route_map(n)) {
        Some(m) => m.run_with(d, s, a).cloned(),
        None => Some(Sets::default()),
    }
}

/// Cleans the prefix lists a lab file holds: bounded, unique names, valid lines in order.
pub fn sanitize_prefix_lists(lists: &mut Vec<PrefixList>) {
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

/// Cleans the community lists a lab file holds.
pub fn sanitize_community_lists(lists: &mut Vec<CommunityList>) {
    for l in lists.iter_mut() {
        l.name = clean_name(&l.name);
        l.entries.truncate(MAX_LIST_LINES);
        let expanded = l.expanded;
        for e in &mut l.entries {
            e.communities.truncate(MAX_COMMUNITIES);
            e.regex = e.regex.chars().filter(|c| !c.is_control()).take(crate::textmatch::MAX_PATTERN).collect();
        }
        l.entries.retain(|e| if expanded { crate::textmatch::valid(&e.regex) } else { !e.communities.is_empty() });
    }
    lists.retain(|l| !l.name.is_empty() && !l.entries.is_empty());
    let mut seen = std::collections::BTreeSet::new();
    lists.retain(|l| seen.insert(l.name.clone()));
    lists.truncate(MAX_COMMUNITY_LISTS);
}

/// Cleans the AS-path lists a lab file holds.
pub fn sanitize_as_path_lists(lists: &mut Vec<AsPathList>) {
    for l in lists.iter_mut() {
        l.name = clean_name(&l.name);
        for e in &mut l.entries {
            e.regex = e.regex.chars().filter(|c| !c.is_control()).take(crate::textmatch::MAX_PATTERN).collect();
        }
        l.entries.retain(|e| crate::textmatch::valid(&e.regex));
        l.entries.truncate(MAX_LIST_LINES);
    }
    lists.retain(|l| !l.name.is_empty() && !l.entries.is_empty());
    let mut seen = std::collections::BTreeSet::new();
    lists.retain(|l| seen.insert(l.name.clone()));
    lists.truncate(MAX_AS_PATH_LISTS);
}

/// Cleans the route maps a lab file holds.
pub fn sanitize_route_maps(maps: &mut Vec<RouteMap>) {
    for m in maps.iter_mut() {
        m.name = clean_name(&m.name);
        m.clauses.retain(|c| (1..=MAX_SEQ).contains(&c.seq));
        m.clauses.sort_by_key(|c| c.seq);
        m.clauses.dedup_by_key(|c| c.seq);
        m.clauses.truncate(MAX_CLAUSES);
        for c in &mut m.clauses {
            c.prefix_lists = c.prefix_lists.iter().map(|n| clean_name(n)).filter(|n| !n.is_empty()).take(MAX_MATCH_NAMES).collect();
            c.acls = c.acls.iter().map(|n| clean_name(n)).filter(|n| !n.is_empty()).take(MAX_MATCH_NAMES).collect();
            c.community_lists = c.community_lists.iter().map(|n| clean_name(n)).filter(|n| !n.is_empty()).take(MAX_MATCH_NAMES).collect();
            c.as_path_lists = c.as_path_lists.iter().map(|n| clean_name(n)).filter(|n| !n.is_empty()).take(MAX_MATCH_NAMES).collect();
            if let Some(v) = &mut c.sets.community {
                v.sort_unstable();
                v.dedup();
                v.truncate(MAX_COMMUNITIES);
            }
            c.sets.prepend.truncate(MAX_PREPEND);
            c.sets.origin = c.sets.origin.map(|o| o.min(2));
        }
    }
    maps.retain(|m| !m.name.is_empty());
    let mut seen = std::collections::BTreeSet::new();
    maps.retain(|m| seen.insert(m.name.clone()));
    maps.truncate(MAX_ROUTE_MAPS);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceKind;

    fn net(s: &str) -> Ipv4Net {
        s.parse().unwrap()
    }

    fn line(seq: u32, permit: bool, p: &str, ge: Option<u8>, le: Option<u8>) -> PrefixEntry {
        PrefixEntry { seq, permit, net: net(p), ge, le }
    }

    #[test]
    fn a_line_without_ge_or_le_matches_exactly_that_prefix() {
        let e = line(5, true, "10.1.0.0/16", None, None);
        assert!(e.matches(net("10.1.0.0/16")));
        assert!(!e.matches(net("10.1.1.0/24")) && !e.matches(net("10.0.0.0/8")) && !e.matches(net("10.2.0.0/16")));
    }

    #[test]
    fn ge_and_le_set_the_range_of_lengths() {
        let le = line(5, true, "10.0.0.0/8", None, Some(24));
        assert_eq!(le.range(), (8, 24));
        assert!(le.matches(net("10.0.0.0/8")) && le.matches(net("10.9.9.0/24")) && !le.matches(net("10.9.9.4/30")));
        let ge = line(5, true, "10.0.0.0/8", Some(16), None);
        assert_eq!(ge.range(), (16, 32));
        assert!(!ge.matches(net("10.0.0.0/8")) && ge.matches(net("10.1.0.0/16")) && ge.matches(net("10.1.2.3/32")));
        let both = line(5, true, "0.0.0.0/0", Some(25), Some(28));
        assert!(both.matches(net("172.16.0.0/26")) && !both.matches(net("172.16.0.0/24")) && !both.matches(net("172.16.0.0/30")));
        assert!(line(5, true, "0.0.0.0/0", None, Some(32)).matches(net("1.2.3.0/24")));
        assert!(line(5, true, "0.0.0.0/0", None, None).matches(net("0.0.0.0/0")));
        assert!(!line(5, true, "0.0.0.0/0", None, None).matches(net("1.2.3.0/24")));
    }

    #[test]
    fn lengths_must_be_consistent() {
        assert!(line(5, true, "10.0.0.0/8", Some(9), Some(24)).valid());
        assert!(!line(5, true, "10.0.0.0/8", Some(8), None).valid());
        assert!(!line(5, true, "10.0.0.0/8", Some(24), Some(16)).valid());
        assert!(!line(5, true, "10.0.0.0/8", None, Some(7)).valid());
        assert!(!line(5, true, "10.0.0.0/8", None, Some(33)).valid());
        assert!(line(5, true, "10.0.0.0/8", None, Some(8)).valid());
    }

    #[test]
    fn the_first_matching_line_decides_and_the_rest_is_denied() {
        let mut l = PrefixList::new("L");
        l.add(line(10, false, "10.1.0.0/16", None, Some(32))).unwrap();
        l.add(line(5, true, "10.0.0.0/8", None, Some(24))).unwrap();
        assert_eq!(l.entries.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![5, 10]);
        assert!(l.permits(net("10.9.0.0/16")));
        assert!(l.permits(net("10.1.0.0/16")), "line 5 comes first and covers it");
        assert!(!l.permits(net("192.168.0.0/24")), "no line matches: implicit deny");
        assert!(!PrefixList::new("empty").permits(net("10.0.0.0/8")));
    }

    #[test]
    fn sequence_numbers_go_up_by_five_and_clash_is_refused() {
        let mut l = PrefixList::new("L");
        assert_eq!(l.next_seq(), Some(5));
        l.add(line(5, true, "10.0.0.0/8", None, None)).unwrap();
        l.add(line(12, true, "11.0.0.0/8", None, None)).unwrap();
        assert_eq!(l.next_seq(), Some(17));
        assert!(l.add(line(12, true, "12.0.0.0/8", None, None)).unwrap_err().contains("already in use"));
        assert!(l.add(line(20, true, "11.0.0.0/8", None, None)).unwrap_err().contains("already in"));
        l.add(line(MAX_SEQ, true, "13.0.0.0/8", None, None)).unwrap();
        assert_eq!(l.next_seq(), None);
    }

    fn router() -> Device {
        let mut t = crate::Topology::new();
        let id = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        t.device(id).unwrap().clone()
    }

    #[test]
    fn a_route_map_takes_the_first_matching_clause() {
        let mut d = router();
        let mut pl = PrefixList::new("ten");
        pl.add(line(5, true, "10.0.0.0/8", None, Some(24))).unwrap();
        d.prefix_lists.push(pl);
        let mut m = RouteMap::new("M");
        let c = m.clause_mut(20, true).unwrap();
        c.sets.local_pref = Some(50);
        let c = m.clause_mut(10, true).unwrap();
        c.prefix_lists.push("ten".into());
        c.sets.local_pref = Some(200);
        assert_eq!(m.clauses.iter().map(|c| c.seq).collect::<Vec<_>>(), vec![10, 20]);
        let ten = Subject { net: net("10.1.0.0/16"), metric: 0 };
        let other = Subject { net: net("172.16.0.0/16"), metric: 0 };
        assert_eq!(m.run(&d, &ten).and_then(|s| s.local_pref), Some(200));
        assert_eq!(m.run(&d, &other).and_then(|s| s.local_pref), Some(50), "clause 20 has no match lines: it takes the rest");
        // A deny clause drops what it matches; a map with no clause for a route drops it too.
        m.clause_mut(10, true).unwrap().permit = false;
        assert!(m.run(&d, &ten).is_none());
        m.clauses.retain(|c| c.seq == 10);
        assert!(m.run(&d, &other).is_none());
        assert_eq!(m.deciding(&d, &other), None);
    }

    #[test]
    fn match_lines_combine_with_and_across_kinds_and_or_within_one() {
        let mut d = router();
        let mut a = PrefixList::new("a");
        a.add(line(5, true, "10.1.0.0/16", None, None)).unwrap();
        let mut b = PrefixList::new("b");
        b.add(line(5, true, "10.2.0.0/16", None, None)).unwrap();
        d.prefix_lists.extend([a, b]);
        let mut c = Clause::new(10, true);
        c.prefix_lists = vec!["a".into(), "b".into(), "missing".into()];
        let s = |p: &str, metric| Subject { net: net(p), metric };
        assert!(c.matches(&d, &s("10.1.0.0/16", 0)) && c.matches(&d, &s("10.2.0.0/16", 0)) && !c.matches(&d, &s("10.3.0.0/16", 0)));
        c.metric = Some(7);
        assert!(!c.matches(&d, &s("10.1.0.0/16", 0)) && c.matches(&d, &s("10.1.0.0/16", 7)));
        // Access lists: a standard list names the network, an extended one the network and the mask.
        let mut acl = crate::acl::Acl::new("std", false);
        acl.entries.push(crate::acl::AclEntry::new(
            true,
            crate::acl::AclProto::Ip,
            crate::acl::AclAddr { addr: "192.168.0.0".parse().unwrap(), wildcard: "0.0.255.255".parse().unwrap() },
            crate::acl::AclAddr::ANY,
        ));
        d.acls.push(acl);
        let mut c = Clause::new(10, true);
        c.acls = vec!["std".into()];
        assert!(c.matches(&d, &s("192.168.5.0/24", 0)) && !c.matches(&d, &s("10.0.0.0/8", 0)));
    }

    #[test]
    fn apply_checks_the_list_then_the_map_and_lets_unknown_names_through() {
        let mut d = router();
        let mut l = PrefixList::new("only10");
        l.add(line(5, true, "10.0.0.0/8", None, Some(32))).unwrap();
        d.prefix_lists.push(l);
        let mut m = RouteMap::new("pref");
        m.clause_mut(10, true).unwrap().sets.local_pref = Some(300);
        d.route_maps.push(m);
        let ok = Subject { net: net("10.5.0.0/16"), metric: 0 };
        let bad = Subject { net: net("11.5.0.0/16"), metric: 0 };
        assert_eq!(apply(&d, Some("only10"), Some("pref"), &ok).and_then(|s| s.local_pref), Some(300));
        assert!(apply(&d, Some("only10"), Some("pref"), &bad).is_none());
        assert_eq!(apply(&d, None, None, &bad), Some(Sets::default()));
        assert_eq!(apply(&d, Some("nope"), Some("nope"), &bad), Some(Sets::default()));
    }

    #[test]
    fn a_hostile_file_is_cleaned() {
        let mut lists = vec![PrefixList::new("x\ny z".repeat(40)), PrefixList::new(""), PrefixList::new("ok"), PrefixList::new("ok")];
        lists[2].entries = vec![
            line(10, true, "10.1.2.3/16", None, None),
            line(10, true, "11.0.0.0/8", None, None),
            line(0, true, "12.0.0.0/8", None, None),
            line(20, true, "13.0.0.0/8", Some(4), None),
        ];
        sanitize_prefix_lists(&mut lists);
        assert_eq!(lists.len(), 2);
        assert!(lists[0].name.chars().count() <= MAX_NAME && !lists[0].name.contains(['\n', ' ']));
        assert_eq!(lists[1].entries.len(), 1);
        assert_eq!(lists[1].entries[0].net, net("10.1.0.0/16"));
        let mut maps = vec![RouteMap::new("m"), RouteMap::new("m"), RouteMap::new("")];
        maps[0].clauses = vec![Clause::new(0, true), Clause::new(10, true), Clause::new(10, false)];
        maps[0].clauses[1].sets.origin = Some(9);
        maps[0].clauses[1].sets.prepend = vec![65_001; 100];
        sanitize_route_maps(&mut maps);
        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0].clauses.len(), 1);
        assert_eq!(maps[0].clauses[0].sets.origin, Some(2));
        assert_eq!(maps[0].clauses[0].sets.prepend.len(), MAX_PREPEND);
    }

    #[test]
    fn communities_are_read_and_written_in_both_formats() {
        assert_eq!(parse_community("65001:100"), Some((65_001 << 16) | 100));
        assert_eq!(parse_community("4259840100"), Some(4_259_840_100));
        assert_eq!(parse_community("No-Export"), Some(NO_EXPORT));
        assert_eq!(parse_community("local-AS"), Some(LOCAL_AS));
        assert_eq!(parse_community("65536:1"), None);
        assert_eq!(parse_community("x"), None);
        assert_eq!(parse_community(""), None);
        let c = (65_001 << 16) | 100;
        assert_eq!(community_text(c, true), "65001:100");
        assert_eq!(community_text(c, false), c.to_string());
        assert_eq!(community_text(NO_ADVERTISE, false), "no-advertise");
    }

    #[test]
    fn a_standard_list_needs_every_community_and_an_expanded_one_a_match() {
        let a = (65_001 << 16) | 100;
        let b = (65_001 << 16) | 200;
        let mut l = CommunityList::new("C", false);
        l.entries.push(CommunityEntry { permit: true, communities: vec![a, b], regex: String::new() });
        assert!(l.permits(&[a, b, 7]) && !l.permits(&[a]) && !l.permits(&[]));
        let mut any = CommunityList::new("ANY", false);
        any.entries.push(CommunityEntry { permit: true, communities: vec![INTERNET], regex: String::new() });
        assert!(any.permits(&[]) && any.permits(&[a]));
        let mut deny_first = CommunityList::new("D", false);
        deny_first.entries.push(CommunityEntry { permit: false, communities: vec![a], regex: String::new() });
        deny_first.entries.push(CommunityEntry { permit: true, communities: vec![INTERNET], regex: String::new() });
        assert!(!deny_first.permits(&[a]) && deny_first.permits(&[b]));
        let mut x = CommunityList::new("X", true);
        x.entries.push(CommunityEntry { permit: true, communities: Vec::new(), regex: "^65001:".into() });
        assert!(x.permits(&[a]) && !x.permits(&[1]));
    }

    #[test]
    fn an_as_path_list_matches_the_path_text() {
        let mut l = AsPathList::new("P");
        l.entries.push(AsPathEntry { permit: false, regex: "_65009_".into() });
        l.entries.push(AsPathEntry { permit: true, regex: "^65002".into() });
        assert!(l.permits(&[65_002, 65_003], &[]));
        assert!(!l.permits(&[65_002, 65_009], &[]));
        assert!(!l.permits(&[65_005], &[]), "no line matches: denied");
        assert!(!l.permits(&[65_002], &[65_009]), "the AS set counts too");
    }

    #[test]
    fn set_community_replaces_adds_or_clears_and_match_community_reads_the_route() {
        let have = [1u32, 2];
        let replace = Sets { community: Some(vec![9]), ..Sets::default() };
        let add = Sets { community: Some(vec![9, 1]), community_additive: true, ..Sets::default() };
        let none = Sets { community: Some(Vec::new()), ..Sets::default() };
        assert_eq!(merge_communities(&replace, &have), vec![9]);
        assert_eq!(merge_communities(&add, &have), vec![1, 2, 9]);
        assert!(merge_communities(&none, &have).is_empty());
        assert_eq!(merge_communities(&Sets::default(), &have), vec![1, 2]);
        let mut d = router();
        let mut l = CommunityList::new("tagged", false);
        l.entries.push(CommunityEntry { permit: true, communities: vec![2], regex: String::new() });
        d.community_lists.push(l);
        let mut c = Clause::new(10, true);
        c.community_lists = vec!["tagged".into()];
        let s = Subject { net: net("10.0.0.0/8"), metric: 0 };
        assert!(c.matches_with(&d, &s, &Attrs { communities: &have, ..Attrs::default() }));
        assert!(!c.matches_with(&d, &s, &Attrs::default()));
        assert!(!c.matches(&d, &s), "the plain match sees no communities");
    }

    #[test]
    fn serde_leaves_defaults_out() {
        let mut m = RouteMap::new("M");
        m.clause_mut(10, true).unwrap().sets.local_pref = Some(200);
        let json = serde_json::to_string(&m).unwrap();
        assert!(!json.contains("prefix_lists") && !json.contains("weight") && !json.contains("prepend"), "{json}");
        assert_eq!(serde_json::from_str::<RouteMap>(&json).unwrap(), m);
        let mut l = PrefixList::new("L");
        l.add(line(5, false, "10.0.0.0/8", None, Some(24))).unwrap();
        let json = serde_json::to_string(&l).unwrap();
        assert!(!json.contains("ge"), "{json}");
        assert_eq!(serde_json::from_str::<PrefixList>(&json).unwrap(), l);
    }
}
