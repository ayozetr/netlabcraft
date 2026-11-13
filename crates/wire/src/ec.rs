//! Elliptic-curve Diffie-Hellman over the two random-curve groups IKE names, group 19 (NIST
//! P-256) and group 20 (NIST P-384), as RFC 5903 has them: the public value is the point's
//! x and y coordinates, each as wide as the prime, one after the other (the KE payload of
//! RFC 5903 §7), and the shared secret is the x coordinate of the common point (§7).
//!
//! The curves are y² = x³ − 3x + b over a prime field; the parameters are those of FIPS 186-4
//! (D.1.2.3 and D.1.2.4). The field arithmetic is [`crate::dh::Big`] with the Montgomery
//! product of [`crate::dh::ModMul`]; points are Jacobian during a scalar multiplication and
//! converted back with one inversion (Fermat) at the end. Nothing here is constant time (this is
//! a teaching simulator, not for protecting real data), but peer points are checked as
//! RFC 5903 §7 and SP 800-56A §5.6.2.3.3 ask: both coordinates below the prime, on the curve,
//! and not the point at infinity (which an x,y pair cannot spell). Every loop is bounded by the
//! bit length of the scalar, itself reduced below the group order.

use crate::dh::{Big, ModMul, mod_pow};
use std::sync::OnceLock;

/// The IKE group numbers of the two curves.
pub const GROUP_P256: u16 = 19;
pub const GROUP_P384: u16 = 20;

struct Params {
    p: &'static str,
    b: &'static str,
    gx: &'static str,
    gy: &'static str,
    n: &'static str,
}

const P256: Params = Params {
    p: "FFFFFFFF00000001000000000000000000000000FFFFFFFFFFFFFFFFFFFFFFFF",
    b: "5AC635D8AA3A93E7B3EBBD55769886BC651D06B0CC53B0F63BCE3C3E27D2604B",
    gx: "6B17D1F2E12C4247F8BCE6E563A440F277037D812DEB33A0F4A13945D898C296",
    gy: "4FE342E2FE1A7F9B8EE7EB4A7C0F9E162BCE33576B315ECECBB6406837BF51F5",
    n: "FFFFFFFF00000000FFFFFFFFFFFFFFFFBCE6FAADA7179E84F3B9CAC2FC632551",
};

const P384: Params = Params {
    p: "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFFFF0000000000000000FFFFFFFF",
    b: "B3312FA7E23EE7E4988E056BE3F82D19181D9C6EFE8141120314088F5013875AC656398D8A2ED19D2A85C8EDD3EC2AEF",
    gx: "AA87CA22BE8B05378EB1C71EF320AD746E1D3B628BA79B9859F741E082542A385502F25DBF55296C3A545E3872760AB7",
    gy: "3617DE4A96262C6F5D9E98BF9292DC29F8F41DBD289A147CE9DA3113B5F0B8C00A60B1CE1D7E819D7A431D7C90EA0E5F",
    n: "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFC7634D81F4372DDF581A0DB248B0A77AECEC196ACCC52973",
};

fn hex(s: &str) -> Vec<u8> {
    s.as_bytes().chunks(2).filter_map(|c| std::str::from_utf8(c).ok().and_then(|t| u8::from_str_radix(t, 16).ok())).collect()
}

/// A point in Jacobian coordinates (x/z², y/z³); z = 0 is the point at infinity.
#[derive(Clone)]
struct Jac {
    x: Big,
    y: Big,
    z: Big,
}

/// A curve: the field, the equation's b, the generator and the group order.
pub struct Curve {
    pub id: u16,
    /// Bytes in a coordinate.
    len: usize,
    f: ModMul,
    p: Big,
    b: Big,
    g: (Big, Big),
    n: Big,
}

impl std::fmt::Debug for Curve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Curve({})", self.id)
    }
}

fn build(id: u16, q: &Params) -> Option<Curve> {
    let p = Big::from_be_bytes(&hex(q.p));
    Some(Curve {
        id,
        len: hex(q.p).len(),
        f: ModMul::new(&p)?,
        p,
        b: Big::from_be_bytes(&hex(q.b)),
        g: (Big::from_be_bytes(&hex(q.gx)), Big::from_be_bytes(&hex(q.gy))),
        n: Big::from_be_bytes(&hex(q.n)),
    })
}

/// The curve of an IKE group number (19 or 20), built the first time it is asked for.
pub fn curve(id: u16) -> Option<&'static Curve> {
    static P256_CURVE: OnceLock<Option<Curve>> = OnceLock::new();
    static P384_CURVE: OnceLock<Option<Curve>> = OnceLock::new();
    match id {
        GROUP_P256 => P256_CURVE.get_or_init(|| build(GROUP_P256, &P256)).as_ref(),
        GROUP_P384 => P384_CURVE.get_or_init(|| build(GROUP_P384, &P384)).as_ref(),
        _ => None,
    }
}

/// Whether IKE group `id` is one of the curves here.
pub fn is_curve(id: u16) -> bool {
    id == GROUP_P256 || id == GROUP_P384
}

impl Curve {
    /// Bytes of a coordinate; a public value is twice that.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    fn add(&self, a: &Big, b: &Big) -> Big {
        let s = a.add(b);
        s.sub(&self.p).unwrap_or(s)
    }

    fn sub(&self, a: &Big, b: &Big) -> Big {
        a.sub(b).unwrap_or_else(|| a.add(&self.p).sub(b).unwrap_or_default())
    }

    fn mul(&self, a: &Big, b: &Big) -> Big {
        self.f.mul(a, b)
    }

    fn dbl(&self, a: &Big) -> Big {
        self.add(a, a)
    }

    /// 2P (a = −3: "dbl-2001-b").
    fn double(&self, q: &Jac) -> Jac {
        if q.z.is_zero() || q.y.is_zero() {
            return Jac { x: Big::from_u64(1), y: Big::from_u64(1), z: Big::zero() };
        }
        let delta = self.mul(&q.z, &q.z);
        let gamma = self.mul(&q.y, &q.y);
        let beta = self.mul(&q.x, &gamma);
        let t = self.mul(&self.sub(&q.x, &delta), &self.add(&q.x, &delta));
        let alpha = self.add(&self.dbl(&t), &t);
        let eight_beta = self.dbl(&self.dbl(&self.dbl(&beta)));
        let x3 = self.sub(&self.mul(&alpha, &alpha), &eight_beta);
        let yz = self.add(&q.y, &q.z);
        let z3 = self.sub(&self.sub(&self.mul(&yz, &yz), &gamma), &delta);
        let four_beta = self.dbl(&self.dbl(&beta));
        let g2 = self.mul(&gamma, &gamma);
        let eight_g2 = self.dbl(&self.dbl(&self.dbl(&g2)));
        let y3 = self.sub(&self.mul(&alpha, &self.sub(&four_beta, &x3)), &eight_g2);
        Jac { x: x3, y: y3, z: z3 }
    }

    /// P + Q ("add-2007-bl"), with the special cases: infinity, P = Q and P = −Q.
    fn add_points(&self, p1: &Jac, p2: &Jac) -> Jac {
        if p1.z.is_zero() {
            return p2.clone();
        }
        if p2.z.is_zero() {
            return p1.clone();
        }
        let z1z1 = self.mul(&p1.z, &p1.z);
        let z2z2 = self.mul(&p2.z, &p2.z);
        let u1 = self.mul(&p1.x, &z2z2);
        let u2 = self.mul(&p2.x, &z1z1);
        let s1 = self.mul(&self.mul(&p1.y, &p2.z), &z2z2);
        let s2 = self.mul(&self.mul(&p2.y, &p1.z), &z1z1);
        let h = self.sub(&u2, &u1);
        let r = self.sub(&s2, &s1);
        if h.is_zero() {
            return if r.is_zero() { self.double(p1) } else { Jac { x: Big::from_u64(1), y: Big::from_u64(1), z: Big::zero() } };
        }
        let two_h = self.dbl(&h);
        let i = self.mul(&two_h, &two_h);
        let j = self.mul(&h, &i);
        let rr = self.dbl(&r);
        let v = self.mul(&u1, &i);
        let x3 = self.sub(&self.sub(&self.mul(&rr, &rr), &j), &self.dbl(&v));
        let y3 = self.sub(&self.mul(&rr, &self.sub(&v, &x3)), &self.dbl(&self.mul(&s1, &j)));
        let zs = self.add(&p1.z, &p2.z);
        let z3 = self.mul(&self.sub(&self.sub(&self.mul(&zs, &zs), &z1z1), &z2z2), &h);
        Jac { x: x3, y: y3, z: z3 }
    }

    /// k·(x, y) in affine coordinates, `None` for the point at infinity. The scalar's bits are
    /// all there is to loop over.
    fn scalar_mul(&self, k: &Big, point: &(Big, Big)) -> Option<(Big, Big)> {
        let base = Jac { x: point.0.clone(), y: point.1.clone(), z: Big::from_u64(1) };
        let mut acc = Jac { x: Big::from_u64(1), y: Big::from_u64(1), z: Big::zero() };
        for i in (0..k.bits()).rev() {
            acc = self.double(&acc);
            if k.bit(i) {
                acc = self.add_points(&acc, &base);
            }
        }
        if acc.z.is_zero() {
            return None;
        }
        let exp = self.p.sub(&Big::from_u64(2))?;
        let zi = mod_pow(&acc.z, &exp, &self.p)?;
        let zi2 = self.mul(&zi, &zi);
        let zi3 = self.mul(&zi2, &zi);
        Some((self.mul(&acc.x, &zi2), self.mul(&acc.y, &zi3)))
    }

    /// A private value from bytes: reduced below the group order, never zero.
    fn scalar(&self, private: &[u8]) -> Big {
        let k = Big::from_be_bytes(private).rem(&self.n);
        if k.is_zero() { Big::from_u64(1) } else { k }
    }

    /// Whether (x, y) is a point on the curve: both below the prime and y² = x³ − 3x + b.
    fn on_curve(&self, x: &Big, y: &Big) -> bool {
        if x >= &self.p || y >= &self.p {
            return false;
        }
        let lhs = self.mul(y, y);
        let x3 = self.mul(&self.mul(x, x), x);
        let three_x = self.add(&self.dbl(x), x);
        lhs == self.add(&self.sub(&x3, &three_x), &self.b)
    }

    /// The public value for a private one: the generator times it, x then y, each [`Curve::len`]
    /// bytes (RFC 5903 §7).
    pub fn public(&self, private: &[u8]) -> Vec<u8> {
        let k = self.scalar(private);
        match self.scalar_mul(&k, &self.g) {
            Some((x, y)) => [x.to_be_bytes(self.len), y.to_be_bytes(self.len)].concat(),
            None => Vec::new(),
        }
    }

    fn peer_point(&self, peer: &[u8]) -> Option<(Big, Big)> {
        if peer.len() != 2 * self.len {
            return None;
        }
        let (xb, yb) = peer.split_at(self.len);
        let (x, y) = (Big::from_be_bytes(xb), Big::from_be_bytes(yb));
        self.on_curve(&x, &y).then_some((x, y))
    }

    /// Whether `peer` is a public value this curve accepts.
    pub fn valid_peer(&self, peer: &[u8]) -> bool {
        self.peer_point(peer).is_some()
    }

    /// The shared secret (the x coordinate of private × peer point, [`Curve::len`] bytes), or
    /// `None` when the peer's value isn't a point on the curve or the product is the point at
    /// infinity.
    pub fn shared(&self, private: &[u8], peer: &[u8]) -> Option<Vec<u8>> {
        let q = self.peer_point(peer)?;
        let (x, _) = self.scalar_mul(&self.scalar(private), &q)?;
        Some(x.to_be_bytes(self.len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(s: &str) -> Vec<u8> {
        hex(&s.split_whitespace().collect::<String>())
    }

    /// One RFC 5903 §8 exchange: both public values and both views of the shared secret.
    fn check(id: u16, i: &str, gi: (&str, &str), r: &str, gr: (&str, &str), shared: &str) {
        let c = curve(id).unwrap();
        let (i, r) = (h(i), h(r));
        let pub_i = [h(gi.0), h(gi.1)].concat();
        let pub_r = [h(gr.0), h(gr.1)].concat();
        assert_eq!(c.public(&i), pub_i, "g^i");
        assert_eq!(c.public(&r), pub_r, "g^r");
        assert_eq!(c.shared(&i, &pub_r).unwrap(), h(shared));
        assert_eq!(c.shared(&r, &pub_i).unwrap(), h(shared));
        assert!(c.valid_peer(&pub_i) && c.valid_peer(&pub_r));
    }

    #[test]
    fn rfc_5903_section_8_1_p256() {
        check(
            19,
            "C88F01F5 10D9AC3F 70A292DA A2316DE5 44E9AAB8 AFE84049 C62A9C57 862D1433",
            (
                "DAD0B653 94221CF9 B051E1FE CA5787D0 98DFE637 FC90B9EF 945D0C37 72581180",
                "5271A046 1CDB8252 D61F1C45 6FA3E59A B1F45B33 ACCF5F58 389E0577 B8990BB3",
            ),
            "C6EF9C5D 78AE012A 011164AC B397CE20 88685D8F 06BF9BE0 B283AB46 476BEE53",
            (
                "D12DFB52 89C8D4F8 1208B702 70398C34 2296970A 0BCCB74C 736FC755 4494BF63",
                "56FBF3CA 366CC23E 8157854C 13C58D6A AC23F046 ADA30F83 53E74F33 039872AB",
            ),
            "D6840F6B 42F6EDAF D13116E0 E1256520 2FEF8E9E CE7DCE03 812464D0 4B9442DE",
        );
    }

    #[test]
    fn rfc_5903_section_8_2_p384() {
        check(
            20,
            "099F3C70 34D4A2C6 99884D73 A375A67F 7624EF7C 6B3C0F16 0647B674 14DCE655 E35B5380 41E649EE 3FAEF896 783AB194",
            (
                "667842D7 D180AC2C DE6F74F3 7551F557 55C7645C 20EF73E3 1634FE72 B4C55EE6 DE3AC808 ACB4BDB4 C88732AE E95F41AA",
                "9482ED1F C0EEB9CA FC498462 5CCFC23F 65032149 E0E144AD A0241815 35A0F38E EB9FCFF3 C2C947DA E69B4C63 4573A81C",
            ),
            "41CB0779 B4BDB85D 47846725 FBEC3C94 30FAB46C C8DC5060 855CC9BD A0AA2942 E0308312 916B8ED2 960E4BD5 5A7448FC",
            (
                "E558DBEF 53EECDE3 D3FCCFC1 AEA08A89 A987475D 12FD950D 83CFA417 32BC509D 0D1AC43A 0336DEF9 6FDA41D0 774A3571",
                "DCFBEC7A ACF31964 72169E83 8430367F 66EEBE3C 6E70C416 DD5F0C68 759DD1FF F83FA401 42209DFF 5EAAD96D B9E6386C",
            ),
            "11187331 C279962D 93D60424 3FD592CB 9D0A926F 422E4718 7521287E 7156C5C4 D6031355 69B9E9D0 9CF5D4A2 70F59746",
        );
    }

    #[test]
    fn parameters_are_consistent_with_fips_186_4() {
        for id in [19, 20] {
            let c = curve(id).unwrap();
            assert!(c.on_curve(&c.g.0, &c.g.1), "G on the curve");
            // n·G is the point at infinity, (n−1)·G = −G and (n+1)·G = G.
            assert!(c.scalar_mul(&c.n, &c.g).is_none(), "order of G");
            let minus = c.scalar_mul(&c.n.sub(&Big::from_u64(1)).unwrap(), &c.g).unwrap();
            assert_eq!(minus.0, c.g.0);
            assert!(c.add(&minus.1, &c.g.1).is_zero());
            // The primes are the ones their definitions say.
            let two = |e: usize| Big::from_u64(1).shl(e);
            let want = if id == 19 {
                // 2^256 − 2^224 + 2^192 + 2^96 − 1
                two(256).sub(&two(224)).unwrap().add(&two(192)).add(&two(96)).sub(&Big::from_u64(1)).unwrap()
            } else {
                // 2^384 − 2^128 − 2^96 + 2^32 − 1
                two(384).sub(&two(128)).unwrap().sub(&two(96)).unwrap().add(&two(32)).sub(&Big::from_u64(1)).unwrap()
            };
            assert_eq!(c.p, want);
            assert_eq!(c.len(), if id == 19 { 32 } else { 48 });
        }
        // 2G on P-256 (a published doubling of the generator).
        let c = curve(19).unwrap();
        let two_g = c.scalar_mul(&Big::from_u64(2), &c.g).unwrap();
        assert_eq!(two_g.0.to_be_bytes(32), h("7CF27B18 8D034F7E 8A523803 04B51AC3 C08969E2 77F21B35 A60B48FC 47669978"));
        assert!(curve(14).is_none() && !is_curve(14) && is_curve(19) && is_curve(20));
    }

    #[test]
    fn hostile_peer_values_are_refused() {
        for id in [19, 20] {
            let c = curve(id).unwrap();
            let n = c.len();
            let good = c.public(&[7; 32]);
            assert!(c.shared(&[7; 32], &good).is_some());
            // Wrong lengths, all zeros (not on the curve: the point at infinity cannot be sent),
            // a point off the curve, coordinates not below the prime.
            for bad in [vec![], vec![0; 2 * n], vec![1; 2 * n], good[..2 * n - 1].to_vec(), [good.clone(), vec![0]].concat(), vec![0xff; 2 * n]] {
                assert!(c.shared(&[7; 32], &bad).is_none(), "{id}: {} bytes", bad.len());
                assert!(!c.valid_peer(&bad));
            }
            let mut off = good.clone();
            *off.last_mut().unwrap() ^= 1;
            assert!(!c.valid_peer(&off));
            // x + p is not a valid encoding of x either.
            let mut big = good.clone();
            big[..n].copy_from_slice(&c.p.to_be_bytes(n));
            assert!(!c.valid_peer(&big));
            // Empty, oversized and zero private values never panic and still give a point.
            for k in [&[][..], &[0; 64][..], &[0xff; 200][..]] {
                assert_eq!(c.public(k).len(), 2 * n);
                assert!(c.shared(k, &good).is_some());
            }
        }
    }
}
