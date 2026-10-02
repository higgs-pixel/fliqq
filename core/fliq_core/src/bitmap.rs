//! Chunk bitmap for out-of-order arrival and repair requests.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bitmap {
    bits: Vec<u8>,
    len: u32,
    ones: u32,
}

impl Bitmap {
    pub fn new(len: u32) -> Self {
        Bitmap { bits: vec![0; len.div_ceil(8) as usize], len, ones: 0 }
    }
    pub fn len(&self) -> u32 {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn get(&self, i: u32) -> bool {
        i < self.len && self.bits[(i / 8) as usize] & (1 << (i % 8)) != 0
    }
    /// Returns true if the bit was newly set.
    pub fn set(&mut self, i: u32) -> bool {
        assert!(i < self.len);
        let b = &mut self.bits[(i / 8) as usize];
        let m = 1 << (i % 8);
        if *b & m != 0 {
            return false;
        }
        *b |= m;
        self.ones += 1;
        true
    }
    pub fn count(&self) -> u32 {
        self.ones
    }
    pub fn is_complete(&self) -> bool {
        self.ones == self.len
    }
    pub fn missing(&self) -> impl Iterator<Item = u32> + '_ {
        (0..self.len).filter(|&i| !self.get(i))
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.bits
    }
    /// Rebuild from wire bytes; rejects inconsistent sizes.
    pub fn from_bytes(len: u32, bytes: &[u8]) -> Option<Self> {
        if bytes.len() != len.div_ceil(8) as usize {
            return None;
        }
        let mut bm = Bitmap { bits: bytes.to_vec(), len, ones: 0 };
        // Clear padding bits beyond len.
        if len % 8 != 0 {
            let last = bm.bits.len() - 1;
            bm.bits[last] &= (1u8 << (len % 8)) - 1;
        }
        bm.ones = bm.bits.iter().map(|b| b.count_ones()).sum();
        Some(bm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basics() {
        let mut b = Bitmap::new(10);
        assert!(!b.is_complete());
        assert!(b.set(3));
        assert!(!b.set(3));
        assert_eq!(b.count(), 1);
        for i in 0..10 {
            b.set(i);
        }
        assert!(b.is_complete());
        assert_eq!(b.missing().count(), 0);
        assert!(Bitmap::new(0).is_complete());
    }
    #[test]
    fn wire_roundtrip_and_padding() {
        let mut b = Bitmap::new(11);
        b.set(0);
        b.set(10);
        let back = Bitmap::from_bytes(11, b.as_bytes()).unwrap();
        assert_eq!(back, b);
        let forged = Bitmap::from_bytes(11, &[0xff, 0xff]).unwrap();
        assert_eq!(forged.count(), 11);
        assert!(Bitmap::from_bytes(11, &[0]).is_none());
        assert_eq!(b.missing().collect::<Vec<_>>().len(), 9);
    }
}
