//! Multiple spanning tree BPDUs (IEEE 802.1s, now clause 13 of IEEE 802.1Q).
//!
//! An MST BPDU is a rapid spanning tree BPDU (version 3 in the version field, type 2) followed by
//! the region's identity (name, revision and a digest of the VLAN-to-instance table), the CIST's
//! internal path cost and bridge, the remaining hop count and one record per MST instance. The
//! digest is the HMAC-MD5 of the 4096-entry VLAN table with the key the standard publishes, so
//! two switches that map the VLANs alike (and name the region alike) send the same digest.

use netlabcraft_core::MacAddr;

use crate::control::{Bpdu, BridgeId};
use crate::sha1::{Hash, hmac};
use crate::{Result, WireError, be16};

/// The key of the configuration digest (IEEE 802.1Q table 13-1).
pub const DIGEST_KEY: [u8; 16] = [0x13, 0xAC, 0x06, 0xA6, 0x2E, 0x47, 0xFD, 0x51, 0xF9, 0x5D, 0x2B, 0xA2, 0x43, 0xCD, 0x03, 0x46];

/// Longest region name (it fills 32 octets).
pub const MAX_NAME: usize = 32;
/// Most instance records a decoded BPDU may carry (the standard allows 64).
pub const MAX_RECORDS: usize = 64;
/// The bytes of the version 3 part before the instance records.
const V3_FIXED: usize = 64;
/// One instance record.
const RECORD: usize = 16;

/// The digest of a VLAN-to-instance table: `instance_of(v)` is the instance VLAN `v` belongs to
/// (0 for the CIST). VLANs 0 and 4095 are always in instance 0.
pub fn config_digest(instance_of: impl Fn(u16) -> u16) -> [u8; 16] {
    let mut table = Vec::with_capacity(4096 * 2);
    for v in 0u16..4096 {
        let i = if v == 0 || v == 4095 { 0 } else { instance_of(v) };
        table.extend_from_slice(&i.to_be_bytes());
    }
    let h = hmac(Hash::Md5, &DIGEST_KEY, &table);
    let mut out = [0u8; 16];
    for (o, b) in out.iter_mut().zip(h) {
        *o = b;
    }
    out
}

/// The MST configuration identifier: what two switches must share to be in one region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MstConfigId {
    pub name: String,
    pub revision: u16,
    pub digest: [u8; 16],
}

/// The message about one instance (IEEE 802.1Q 14.6.1): the port's role for it, the regional
/// root, the cost to it and the priorities of the sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MstiRecord {
    /// The flags octet: role (bits 3-4), proposal, learning, forwarding, agreement, master.
    pub flags: u8,
    /// Regional root: the priority carries the instance number in its low 12 bits.
    pub regional_root: BridgeId,
    pub internal_cost: u32,
    /// The sender's bridge priority for the instance (the high four bits, as an octet).
    pub bridge_priority: u8,
    /// The sender's port priority for the instance (the high four bits, as an octet).
    pub port_priority: u8,
    pub hops: u8,
}

/// A multiple spanning tree BPDU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MstBpdu {
    /// The version 2 part: `root` is the CIST root, `root_cost` the cost to it outside the
    /// region, `bridge` the CIST regional root, and `rstp` the port's flags.
    pub base: Bpdu,
    pub config: MstConfigId,
    /// The cost to the regional root, inside the region.
    pub internal_cost: u32,
    /// The sender.
    pub bridge: BridgeId,
    pub hops: u8,
    pub msti: Vec<MstiRecord>,
}

impl MstBpdu {
    /// The 802.3 payload: LLC header and the BPDU (the frame's type field holds its length).
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Bpdu { rstp: Some(self.base.rstp.unwrap_or(0)), ..self.base }.encode();
        // Protocol version 3 sits after the LLC header and the two protocol-id octets.
        if let Some(ver) = v.get_mut(5) {
            *ver = 3;
        }
        let records = self.msti.len().min(MAX_RECORDS);
        let body = V3_FIXED + RECORD * records;
        v.extend_from_slice(&u16::try_from(body).unwrap_or(u16::MAX).to_be_bytes());
        v.push(0); // format selector
        let mut name = [0u8; MAX_NAME];
        for (d, s) in name.iter_mut().zip(self.config.name.bytes()) {
            *d = s;
        }
        v.extend_from_slice(&name);
        v.extend_from_slice(&self.config.revision.to_be_bytes());
        v.extend_from_slice(&self.config.digest);
        v.extend_from_slice(&self.internal_cost.to_be_bytes());
        self.bridge.encode(&mut v);
        v.push(self.hops);
        for r in self.msti.iter().take(records) {
            v.push(r.flags);
            r.regional_root.encode(&mut v);
            v.extend_from_slice(&r.internal_cost.to_be_bytes());
            v.extend_from_slice(&[r.bridge_priority, r.port_priority, r.hops]);
        }
        v
    }

    /// The length field an 802.3 frame carries for this BPDU.
    pub fn length(&self) -> u16 {
        u16::try_from(self.encode().len()).unwrap_or(u16::MAX)
    }

    pub fn decode(b: &[u8]) -> Result<MstBpdu> {
        let t = || WireError::Truncated("mst bpdu");
        if b.get(3..7) != Some(&[0, 0, 3, 2][..]) {
            return Err(WireError::Unsupported("mst bpdu", "not a version 3 BPDU".into()));
        }
        let base = Bpdu::decode(b)?;
        let declared = usize::from(be16(b, 3 + 36).ok_or_else(t)?);
        if declared < V3_FIXED || (declared - V3_FIXED) % RECORD != 0 {
            return Err(WireError::Unsupported("mst bpdu", format!("version 3 length {declared}")));
        }
        let records = (declared - V3_FIXED) / RECORD;
        if records > MAX_RECORDS {
            return Err(WireError::Unsupported("mst bpdu", format!("{records} instance records")));
        }
        let start = 3 + 36 + 2;
        let body = b.get(start..start + declared).ok_or_else(t)?;
        let name_raw = body.get(1..1 + MAX_NAME).ok_or_else(t)?;
        let end = name_raw.iter().position(|c| *c == 0).unwrap_or(name_raw.len());
        let name = String::from_utf8_lossy(name_raw.get(..end).ok_or_else(t)?).into_owned();
        let revision = be16(body, 33).ok_or_else(t)?;
        let mut digest = [0u8; 16];
        digest.copy_from_slice(body.get(35..51).ok_or_else(t)?);
        let internal_cost = u32::from_be_bytes(body.get(51..55).ok_or_else(t)?.try_into().map_err(|_| t())?);
        let bridge = BridgeId::decode(body, 55).ok_or_else(t)?;
        let hops = *body.get(63).ok_or_else(t)?;
        let mut msti = Vec::with_capacity(records);
        for n in 0..records {
            let at = V3_FIXED + n * RECORD;
            let r = body.get(at..at + RECORD).ok_or_else(t)?;
            msti.push(MstiRecord {
                flags: *r.first().ok_or_else(t)?,
                regional_root: BridgeId::decode(r, 1).ok_or_else(t)?,
                internal_cost: u32::from_be_bytes(r.get(9..13).ok_or_else(t)?.try_into().map_err(|_| t())?),
                bridge_priority: *r.get(13).ok_or_else(t)?,
                port_priority: *r.get(14).ok_or_else(t)?,
                hops: *r.get(15).ok_or_else(t)?,
            });
        }
        Ok(MstBpdu { base, config: MstConfigId { name, revision, digest }, internal_cost, bridge, hops, msti })
    }

    /// A sentence for captures and `debug spanning-tree bpdu`.
    pub fn describe(&self) -> String {
        format!(
            "MST BPDU from {}: region \"{}\" revision {}, CIST root {}, cost {}, {} instance record{}",
            self.bridge,
            self.config.name,
            self.config.revision,
            self.base.root,
            self.base.root_cost,
            self.msti.len(),
            if self.msti.len() == 1 { "" } else { "s" }
        )
    }
}

/// The instance number inside a regional root's priority field.
pub fn instance_of_priority(priority: u16) -> u16 {
    priority & 0x0fff
}

/// A bridge identifier for an instance: the priority (a multiple of 4096) and the instance
/// number in the low 12 bits.
pub fn instance_bridge(priority: u16, instance: u16, mac: MacAddr) -> BridgeId {
    BridgeId { priority: (priority & 0xf000) | (instance & 0x0fff), mac }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{FLAG_FORWARDING, FLAG_LEARNING, ROLE_DESIGNATED, ROLE_SHIFT};

    fn sample() -> MstBpdu {
        let id = |p: u16, n: u8| BridgeId { priority: p, mac: MacAddr([2, 0, 0, 0, 0, n]) };
        MstBpdu {
            base: Bpdu {
                root: id(4096, 1),
                root_cost: 0,
                bridge: id(4096, 1),
                port_id: 0x8002,
                message_age: 0,
                rstp: Some((ROLE_DESIGNATED << ROLE_SHIFT) | FLAG_LEARNING | FLAG_FORWARDING),
            },
            config: MstConfigId { name: "LAB".into(), revision: 7, digest: config_digest(|v| if v == 10 { 1 } else { 0 }) },
            internal_cost: 4,
            bridge: id(32768, 2),
            hops: 19,
            msti: vec![
                MstiRecord {
                    flags: (ROLE_DESIGNATED << ROLE_SHIFT) | FLAG_LEARNING | FLAG_FORWARDING,
                    regional_root: instance_bridge(24576, 1, MacAddr([2, 0, 0, 0, 0, 2])),
                    internal_cost: 4,
                    bridge_priority: 0x80,
                    port_priority: 0x80,
                    hops: 19,
                },
                MstiRecord {
                    flags: 0,
                    regional_root: instance_bridge(32768, 2, MacAddr([2, 0, 0, 0, 0, 3])),
                    internal_cost: 0,
                    bridge_priority: 0x80,
                    port_priority: 0x70,
                    hops: 20,
                },
            ],
        }
    }

    #[test]
    fn an_mst_bpdu_round_trips_with_its_region_and_instance_records() {
        let b = sample();
        let v = b.encode();
        assert_eq!(v.len(), 3 + 36 + 2 + 64 + 2 * 16);
        assert_eq!(b.length(), 3 + 36 + 2 + 64 + 32);
        // LLC, protocol 0, version 3, type 2.
        assert_eq!(&v[..7], &[0x42, 0x42, 0x03, 0, 0, 3, 2]);
        assert_eq!(MstBpdu::decode(&v).unwrap(), b);
        // The first words of a version 2 reader still see a BPDU.
        assert_eq!(Bpdu::decode(&v).unwrap(), b.base);
        assert!(b.describe().contains("region \"LAB\" revision 7") && b.describe().contains("2 instance records"));
    }

    #[test]
    fn every_truncation_of_an_mst_bpdu_is_an_error_or_a_shorter_valid_one() {
        let v = sample().encode();
        for n in 0..v.len() {
            assert!(MstBpdu::decode(&v[..n]).is_err(), "{n}");
        }
        // A length that is not a whole number of records, or too many records.
        let mut bad = v.clone();
        bad[3 + 36 + 1] = 65;
        assert!(MstBpdu::decode(&bad).is_err());
        let mut many = v.clone();
        many[3 + 36 + 1] = 0;
        many[3 + 36] = 0xff;
        assert!(MstBpdu::decode(&many).is_err());
        // A version 2 BPDU is not an MST one.
        let rstp = Bpdu { rstp: Some(0), ..sample().base }.encode();
        assert!(MstBpdu::decode(&rstp).is_err());
    }

    #[test]
    fn the_digest_follows_the_vlan_table_and_nothing_else() {
        let none = config_digest(|_| 0);
        assert_eq!(none, config_digest(|_| 0));
        assert_ne!(none, config_digest(|v| u16::from(v == 10)));
        // VLANs 0 and 4095 never count.
        assert_eq!(config_digest(|_| 0), config_digest(|v| if v == 0 || v == 4095 { 5 } else { 0 }));
        // The default table (every VLAN in the CIST) has the digest every unconfigured MST
        // switch sends.
        assert_eq!(none.iter().map(|b| format!("{b:02x}")).collect::<String>(), "ac36177f50283cd4b83821d8ab26de62");
    }

    #[test]
    fn an_instance_bridge_keeps_the_instance_in_the_low_bits() {
        let b = instance_bridge(24576, 3, MacAddr([2, 0, 0, 0, 0, 1]));
        assert_eq!(b.priority, 24579);
        assert_eq!(instance_of_priority(b.priority), 3);
    }
}
