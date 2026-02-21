//! Segment type and range planning.

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
        out.push(Segment { start: offset, end });
        offset = end;
    }

    out
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
}
