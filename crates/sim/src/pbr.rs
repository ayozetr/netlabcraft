//! Policy-based routing: a route map applied to the packets of an interface (`ip policy
//! route-map`) or to the device's own packets (`ip local policy route-map`) decides where a
//! packet goes before the routing table is asked. The first clause whose `match` lines hold
//! decides: a `deny` clause sends the packet on to ordinary routing; a `permit` clause sets the
//! next hop, the exit interface (`set interface`), a next hop for destinations the table has no
//! route to (`set ip default next-hop`) and the packet's precedence or DSCP. A next hop or
//! interface that is not usable leaves the packet to ordinary routing.

use std::net::Ipv4Addr;

use netlabcraft_core::DeviceId;
use netlabcraft_model::policy::Clause;
use netlabcraft_model::{Device, Topology};
use netlabcraft_wire::Ipv4;

use crate::{DebugKind, L3If, Sim, acl_key, up_ifaces};

/// What a route map decided for one packet.
#[derive(Debug, Clone, Default)]
pub struct Decision {
    /// Where to send it; `None`: ordinary routing.
    pub route: Option<(L3If, Ipv4Addr)>,
    /// The DSCP to mark it with.
    pub dscp: Option<u8>,
}

/// Whether a clause's `match` lines hold for a packet of `total` bytes.
fn clause_matches(d: &Device, c: &Clause, key: &netlabcraft_model::acl::PacketKey, total: u16) -> bool {
    if let Some((lo, hi)) = c.length
        && !(lo..=hi).contains(&total)
    {
        return false;
    }
    if !c.acls.is_empty() && !c.acls.iter().any(|n| d.acls.iter().find(|a| a.name == *n).is_some_and(|a| a.evaluate(key).0)) {
        return false;
    }
    true
}

impl Sim {
    /// The decision of the route map that applies to a packet that came in on `input` (`None`: one
    /// this device sends); `None` when no policy applies or the clause that matched denies.
    pub(crate) fn policy_route(&mut self, t: &Topology, d: &Device, input: Option<&L3If>, ip: &Ipv4) -> Option<Decision> {
        let name = match input {
            Some(i) => d.pbr.for_interface(&i.name),
            None => d.pbr.local.as_deref(),
        }?;
        let map = d.route_maps.iter().find(|m| m.name == name)?;
        let total = u16::try_from(ip.payload.len().saturating_add(20)).unwrap_or(u16::MAX);
        let key = acl_key(ip);
        let clause = map.clauses.iter().find(|c| clause_matches(d, c, &key, total))?;
        let where_ = input.map_or("local policy".to_string(), |i| format!("on {}", i.name));
        if !clause.permit {
            self.debug_log(
                d.id,
                DebugKind::Policy,
                &format!("{} -> {} {where_}: route map {name} sequence {} denies, routed normally", ip.src, ip.dst, clause.seq),
            );
            return None;
        }
        let hit = self.st(d.id).pbr_hits.entry((map.name.clone(), clause.seq)).or_default();
        hit.0 = hit.0.saturating_add(1);
        hit.1 = hit.1.saturating_add(u64::from(total));
        let mut decision = Decision { route: None, dscp: clause.sets.dscp.or(clause.sets.precedence.map(|p| p << 3)) };
        let ifs = up_ifaces(t, d);
        let on_link = |a: Ipv4Addr| ifs.iter().filter(|i| i.net.contains(a)).max_by_key(|i| i.net.prefix).cloned();
        if let Some(nh) = clause.sets.next_hop
            && clause.sets.next_hop_track.is_none_or(|n| crate::track::is_up(t, d, n))
            && let Some(i) = on_link(nh)
        {
            decision.route = Some((i, nh));
        }
        if decision.route.is_none()
            && let Some(want) = &clause.sets.interface
            && let Some(i) = ifs.iter().find(|i| i.name.eq_ignore_ascii_case(want))
        {
            decision.route = Some((i.clone(), ip.dst));
        }
        if decision.route.is_none()
            && let Some(nh) = clause.sets.default_next_hop
        {
            // Only for destinations the table has no specific route to: a default route does not count.
            let explicit = self.routing_table(t, d).iter().any(|r| r.net.prefix > 0 && r.code != 'L' && r.net.contains(ip.dst));
            if !explicit && let Some(i) = on_link(nh) {
                decision.route = Some((i, nh));
            }
        }
        if self.debug_on(d.id, DebugKind::Policy) {
            let verdict = match &decision.route {
                Some((i, nh)) => format!("next hop {nh} out {}", i.name),
                None => "no usable next hop, routed normally".to_string(),
            };
            let mark = decision.dscp.map_or(String::new(), |m| format!(", DSCP {m}"));
            self.debug_log(
                d.id,
                DebugKind::Policy,
                &format!("{} -> {} {where_}: route map {name} sequence {} matched: {verdict}{mark}", ip.src, ip.dst, clause.seq),
            );
        }
        Some(decision)
    }

    /// Packets and bytes a route-map clause has taken.
    pub fn pbr_hits(&self, dev: DeviceId, map: &str, seq: u32) -> (u64, u64) {
        self.state.get(&dev).and_then(|s| s.pbr_hits.get(&(map.to_string(), seq)).copied()).unwrap_or((0, 0))
    }
}
