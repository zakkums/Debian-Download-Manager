//! Easy2 Handler for a single segment in the curl multi backend.
//! Validates 206 and Content-Range before writing; writes to storage at segment offset.

use std::str;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::segmenter::Segment;
use crate::storage::StorageWriter;

use super::super::segment::{parse_content_range, parse_http_status};

/// Handler state for one segment transfer. Implements curl's Handler for Easy2.
pub struct SegmentHandler {
    pub(super) segment_index: usize,
    pub(super) segment: Segment,
    pub(super) storage: StorageWriter,
    pub(super) response_headers: Vec<String>,
    /// None = not yet checked; Some(true) = 206 + Content-Range ok; Some(false) = abort.
    pub(super) range_ok: Option<bool>,
    pub(super) bytes_written: u64,
    pub(super) in_flight: Option<Arc<Vec<AtomicU64>>>,
}

impl SegmentHandler {
    pub(super) fn new(
        segment_index: usize,
        segment: Segment,
        storage: StorageWriter,
        in_flight: Option<Arc<Vec<AtomicU64>>>,
    ) -> Self {
        Self {
            segment_index,
            segment,
            storage,
            response_headers: Vec::new(),
            range_ok: None,
            bytes_written: 0,
            in_flight,
        }
    }
}

impl curl::easy::Handler for SegmentHandler {
    fn header(&mut self, data: &[u8]) -> bool {
        if let Ok(s) = str::from_utf8(data) {
            let line = s.trim_end();
            if line.starts_with("HTTP/") {
                self.response_headers.clear();
                self.response_headers.push(line.to_string());
            } else {
                self.response_headers.push(line.to_string());
            }
        }
        true
    }

    fn write(&mut self, data: &[u8]) -> Result<usize, curl::easy::WriteError> {
        if self.range_ok.is_none() {
            let status = parse_http_status(&self.response_headers);
            let content_ok = parse_content_range(&self.response_headers)
                .map(|(s, e)| {
                    s == self.segment.start
                        && e == self.segment.end.saturating_sub(1)
                })
                .unwrap_or(false);
            self.range_ok = Some(status == Some(206) && content_ok);
        }
        if self.range_ok == Some(false) {
            return Ok(0);
        }
        let offset = self.segment.start + self.bytes_written;
        match self.storage.write_at(offset, data) {
            Ok(()) => {
                let n = data.len();
                self.bytes_written += n as u64;
                if let Some(ref v) = self.in_flight {
                    if let Some(a) = v.get(self.segment_index) {
                        a.store(self.bytes_written, Ordering::Relaxed);
                    }
                }
                Ok(n)
            }
            Err(_) => Ok(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segmenter::plan_segments;
    use curl::easy::Handler;

    #[test]
    fn handler_header_clears_on_http_status_line() {
        let segments = plan_segments(1000, 1);
        let dir = tempfile::tempdir().unwrap();
        let tp = crate::storage::temp_path(&dir.path().join("out.bin"));
        let mut builder = crate::storage::StorageWriterBuilder::create(&tp).unwrap();
        builder.preallocate(1000).unwrap();
        let storage = builder.build();
        let mut h = SegmentHandler::new(0, segments[0], storage, None);
        h.header(b"HTTP/1.1 302 Found\r\n");
        h.header(b"Location: http://other/\r\n");
        assert_eq!(h.response_headers.len(), 2);
        h.header(b"HTTP/1.1 206 Partial Content\r\n");
        assert_eq!(h.response_headers.len(), 1, "headers cleared on new HTTP/ line");
        assert!(h.response_headers[0].contains("206"));
    }

    #[test]
    fn handler_write_rejects_non_206_with_zero() {
        let segments = plan_segments(1000, 1);
        let dir = tempfile::tempdir().unwrap();
        let tp = crate::storage::temp_path(&dir.path().join("out.bin"));
        let mut builder = crate::storage::StorageWriterBuilder::create(&tp).unwrap();
        builder.preallocate(1000).unwrap();
        let storage = builder.build();
        let mut h = SegmentHandler::new(0, segments[0], storage, None);
        h.header(b"HTTP/1.1 200 OK\r\n");
        h.header(b"Content-Length: 1000\r\n");
        let n = h.write(b"data").unwrap();
        assert_eq!(n, 0, "write should return 0 when not 206");
        assert_eq!(h.range_ok, Some(false));
        assert_eq!(h.bytes_written, 0);
    }

    #[test]
    fn handler_write_accepts_206_and_writes_at_offset() {
        let segments = plan_segments(1000, 4);
        let seg = segments[1];
        assert_eq!(seg.start, 250);
        assert_eq!(seg.end, 500);
        let dir = tempfile::tempdir().unwrap();
        let tp = crate::storage::temp_path(&dir.path().join("out.bin"));
        let mut builder = crate::storage::StorageWriterBuilder::create(&tp).unwrap();
        builder.preallocate(1000).unwrap();
        let storage = builder.build();
        let mut h = SegmentHandler::new(1, seg, storage, None);
        h.header(b"HTTP/1.1 206 Partial Content\r\n");
        h.header(b"Content-Range: bytes 250-499/1000\r\n");
        let n = h.write(b"abcd").unwrap();
        assert_eq!(n, 4);
        assert_eq!(h.range_ok, Some(true));
        assert_eq!(h.bytes_written, 4);
        let n2 = h.write(b"efgh").unwrap();
        assert_eq!(n2, 4);
        assert_eq!(h.bytes_written, 8);
    }
}
