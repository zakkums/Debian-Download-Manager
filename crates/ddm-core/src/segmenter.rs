//! Range math and segment planning.
//!
//! Splits a download into N segments, computes HTTP Range header bounds,
//! and provides a completion bitmap for resume (serialized to DB BLOB).

/// A single segment: byte range [start, end) (half-open).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// Start offset (inclusive).
    pub start: u64,
    /// End offset (exclusive).
    pub end: u64,
}

impl Segment {
    /// Length of this segment in bytes.
    pub fn len(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// HTTP Range header value (inclusive end): `bytes=start-(end-1)`.
    pub fn range_header_value(&self) -> String {
        if self.start >= self.end {
            "bytes=0-0".to_string()
        } else {
            format!("bytes={}-{}", self.start, self.end - 1)
        }
    }
}

/// Builds a segment plan for a given total size and segment count.
///
/// Segments are as equal as possible; the last segment may be shorter.
/// Returns an empty vec if `total_size` is 0 or `segment_count` is 0.
pub fn plan_segments(total_size: u64, segment_count: usize) -> Vec<Segment> {
    if total_size == 0 || segment_count == 0 {
        return Vec::new();
    }

    let segment_count = segment_count as u64;
    let base = total_size / segment_count;
    let remainder = total_size % segment_count;

    let mut out = Vec::with_capacity(segment_count as usize);
    let mut offset = 0u64;

    for i in 0..segment_count {
        let len = base + if i < remainder { 1 } else { 0 };
        let end = (offset + len).min(total_size);
        out.push(Segment {
            start: offset,
            end,
        });
        offset = end;
    }

    out
}

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
    fn plan_segments_even() {
        let segs = plan_segments(1000, 4);
        assert_eq!(segs.len(), 4);
        assert_eq!(segs[0].start, 0);
        assert_eq!(segs[0].end, 250);
        assert_eq!(segs[1].start, 250);
        assert_eq!(segs[1].end, 500);
        assert_eq!(segs[2].start, 500);
        assert_eq!(segs[2].end, 750);
        assert_eq!(segs[3].start, 750);
        assert_eq!(segs[3].end, 1000);
    }

    #[test]
    fn plan_segments_remainder() {
        let segs = plan_segments(10, 4);
        assert_eq!(segs.len(), 4);
        // 10/4 -> base 2, remainder 2: first 2 segments get 3, next 2 get 2
        assert_eq!(segs[0].start, 0);
        assert_eq!(segs[0].end, 3);
        assert_eq!(segs[1].start, 3);
        assert_eq!(segs[1].end, 6);
        assert_eq!(segs[2].start, 6);
        assert_eq!(segs[2].end, 8);
        assert_eq!(segs[3].start, 8);
        assert_eq!(segs[3].end, 10);
    }

    #[test]
    fn plan_segments_one() {
        let segs = plan_segments(100, 1);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].start, 0);
        assert_eq!(segs[0].end, 100);
    }

    #[test]
    fn plan_segments_empty() {
        assert!(plan_segments(0, 4).is_empty());
        assert!(plan_segments(100, 0).is_empty());
    }

    #[test]
    fn segment_range_header() {
        let s = Segment { start: 0, end: 99 };
        assert_eq!(s.range_header_value(), "bytes=0-98");
        assert_eq!(s.len(), 99);
    }

    #[test]
    fn segment_range_header_single_byte() {
        let s = Segment { start: 42, end: 43 };
        assert_eq!(s.range_header_value(), "bytes=42-42");
    }

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
