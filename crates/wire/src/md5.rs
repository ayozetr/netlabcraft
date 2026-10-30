//! MD5 (RFC 1321), for the RADIUS authenticators and password hiding (RFC 2865 §3, §5.2).
//! Not for anything that needs to be secure.

/// Per-round shift amounts.
const S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23,
    4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// The 64-entry table: the integer part of 2^32 × |sin(i + 1)|.
fn table() -> [u32; 64] {
    let mut k = [0u32; 64];
    for (i, v) in k.iter_mut().enumerate() {
        // Exact for every i here: |sin| < 1, so the product is below 2^32.
        *v = ((i as f64 + 1.0).sin().abs() * 4_294_967_296.0) as u32;
    }
    k
}

pub fn md5(data: &[u8]) -> [u8; 16] {
    let k = table();
    let mut st: [u32; 4] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_le_bytes());
    for chunk in msg.as_chunks::<64>().0 {
        let mut m = [0u32; 16];
        for (w, b) in m.iter_mut().zip(chunk.as_chunks::<4>().0) {
            *w = u32::from_le_bytes(*b);
        }
        let [mut a, mut b, mut c, mut d] = st;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        st[0] = st[0].wrapping_add(a);
        st[1] = st[1].wrapping_add(b);
        st[2] = st[2].wrapping_add(c);
        st[3] = st[3].wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (o, w) in out.as_chunks_mut::<4>().0.iter_mut().zip(st) {
        o.copy_from_slice(&w.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: [u8; 16]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn rfc_1321_test_suite() {
        assert_eq!(hex(md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hex(md5(b"message digest")), "f96b697d7cb7938d525a2f31aaf161d0");
        assert_eq!(hex(md5(b"12345678901234567890123456789012345678901234567890123456789012345678901234567890")), "57edf4a22be3c955ac49da2e2107b67a");
    }
}
