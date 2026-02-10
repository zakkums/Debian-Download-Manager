//! Segment completion bitmap for resume.

/// Segment completion bitmap for resume: one bit per segment (LSB = segment 0).
///
/// Serializes to/from bytes for DB BLOB. Only the first `ceil(segment_count/8)`
/// bytes are significant.
#[derive(Debug, Clone, Default)]
pub struct SegmentBitmap {
    bytes: Vec<u8>,
}

impl SegmentBitmap {
    /// New empty bitmap with capacity for `segment_count` bits.
    pub fn new(segment_count: usize) -> Self {
        let len = (segment_count + 7) / 8;
        SegmentBitmap {
            bytes: vec![0u8; len],
        }
    }

    /// Deserialize from DB BLOB. Extra bytes are ignored; missing bytes treated as 0.
    pub fn from_bytes(bytes: &[u8], segment_count: usize) -> Self {
        let len = (segment_count + 7) / 8;
        let mut b = vec![0u8; len];
        let copy = bytes.len().min(len);
        b[..copy].copy_from_slice(&bytes[..copy]);
        SegmentBitmap { bytes: b }
    }

    /// Serialize for DB BLOB (exactly the bytes needed for `segment_count` bits).
    pub fn to_bytes(&self, segment_count: usize) -> Vec<u8> {
        let len = (segment_count + 7) / 8;
        self.bytes.get(..len).unwrap_or(&self.bytes).to_vec()
    }

    /// Mark segment at `index` as completed.
    pub fn set_completed(&mut self, index: usize) {
        let byte_idx = index / 8;
        let bit = index % 8;
        if byte_idx >= self.bytes.len() {
            self.bytes.resize(byte_idx + 1, 0);
        }
        self.bytes[byte_idx] |= 1 << bit;
    }

    /// True if segment at `index` is marked completed.
    pub fn is_completed(&self, index: usize) -> bool {
        let byte_idx = index / 8;
        let bit = index % 8;
        self.bytes
            .get(byte_idx)
            .map(|&b| (b & (1 << bit)) != 0)
            .unwrap_or(false)
    }

    /// True if all segments in [0, segment_count) are completed.
    pub fn all_completed(&self, segment_count: usize) -> bool {
        if segment_count == 0 {
            return true;
        }
        let full_bytes = segment_count / 8;
        let remainder_bits = segment_count % 8;

        for (i, &b) in self.bytes.iter().enumerate() {
            let expected = if i < full_bytes {
                0xFF
            } else if i == full_bytes && remainder_bits > 0 {
                (1u8 << remainder_bits) - 1
            } else {
                break;
            };
            if (b & expected) != expected {
                return false;
            }
        }

        let needed_bytes = (segment_count + 7) / 8;
        self.bytes.len() >= needed_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_new_and_roundtrip() {
        let mut b = SegmentBitmap::new(10);
        assert!(!b.all_completed(10));
        b.set_completed(0);
        b.set_completed(3);
        b.set_completed(9);
        assert!(b.is_completed(0));
        assert!(!b.is_completed(1));
        assert!(b.is_completed(3));
        assert!(b.is_completed(9));

        let bytes = b.to_bytes(10);
        let b2 = SegmentBitmap::from_bytes(&bytes, 10);
        assert!(b2.is_completed(0));
        assert!(!b2.is_completed(1));
        assert!(b2.is_completed(3));
        assert!(b2.is_completed(9));
    }

    #[test]
    fn bitmap_all_completed() {
        let mut b = SegmentBitmap::new(5);
        assert!(!b.all_completed(5));
        for i in 0..5 {
            b.set_completed(i);
        }
        assert!(b.all_completed(5));
    }

    #[test]
    fn bitmap_from_bytes_extra_ignored() {
        let bytes = vec![0xFF, 0xFF];
        let b = SegmentBitmap::from_bytes(&bytes, 8);
        assert!(b.all_completed(8));
        let out = b.to_bytes(8);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], 0xFF);
    }

    #[test]
    fn bitmap_from_bytes_short() {
        let bytes = vec![0xFF];
        let b = SegmentBitmap::from_bytes(&bytes, 16);
        assert!(b.is_completed(0));
        assert!(b.is_completed(7));
        assert!(!b.is_completed(8));
    }
}

