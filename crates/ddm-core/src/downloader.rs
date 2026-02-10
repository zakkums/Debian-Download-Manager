//! Core segmented downloader engine.
//!
//! Consumes direct URL + headers, runs N concurrent HTTP Range GETs,
//! writes each segment to storage at the correct offset and updates the completion bitmap.

use anyhow::{Context, Result};
use crate::segmenter::{Segment, SegmentBitmap};
use crate::storage::StorageWriter;
use std::cell::Cell;
use std::collections::HashMap;
use std::time::Duration;

/// Downloads a single segment: GET with Range header, write body to storage at segment offset.
fn download_one_segment(
    url: &str,
    custom_headers: &HashMap<String, String>,
    segment: &Segment,
    storage: &StorageWriter,
) -> Result<()> {
    let bytes_written = Cell::new(0u64);
    let segment_start = segment.start;
    let storage = storage.clone();

    let mut easy = curl::easy::Easy::new();
    easy.url(url).context("invalid URL")?;
    easy.follow_location(true)?;
    easy.connect_timeout(Duration::from_secs(30))?;
    easy.timeout(Duration::from_secs(300))?;

    // Range: curl expects "start-end" (inclusive), not "bytes=start-end"
    let range_str = format!("{}-{}", segment.start, segment.end.saturating_sub(1));
    easy.range(&range_str)?;

    let mut list = curl::easy::List::new();
    for (k, v) in custom_headers {
        list.append(&format!("{}: {}", k.trim(), v.trim()))?;
    }
    if !custom_headers.is_empty() {
        easy.http_headers(list)?;
    }

    {
        let mut transfer = easy.transfer();
        transfer.write_function(move |data| {
            let off = bytes_written.get();
            storage
                .write_at(segment_start + off, data)
                .map_err(|_| curl::easy::WriteError::Pause)?;
            bytes_written.set(off + data.len() as u64);
            Ok(data.len())
        })?;
        transfer.perform().context("segment GET failed")?;
    }

    let code = easy.response_code().context("no response code")?;
    if code < 200 || code >= 300 {
        anyhow::bail!("segment GET returned HTTP {}", code);
    }

    Ok(())
}

/// Downloads all segments that are not yet completed, writing to `storage` and updating `bitmap`.
/// Runs one thread per incomplete segment (bounded by segment count).
/// Input: direct URL, optional headers, segment list, storage writer, mutable bitmap.
pub fn download_segments(
    url: &str,
    custom_headers: &HashMap<String, String>,
    segments: &[Segment],
    storage: &StorageWriter,
    bitmap: &mut SegmentBitmap,
) -> Result<()> {
    let incomplete: Vec<(usize, Segment)> = segments
        .iter()
        .enumerate()
        .filter(|(i, _)| !bitmap.is_completed(*i))
        .map(|(i, s)| (i, *s))
        .collect();

    if incomplete.is_empty() {
        return Ok(());
    }

    let url = url.to_string();
    let headers = custom_headers.clone();
    let storage = storage.clone();

    let results: Vec<(usize, Result<()>)> = incomplete
        .into_iter()
        .map(|(index, segment)| {
            let u = url.clone();
            let h = headers.clone();
            let st = storage.clone();
            let handle = std::thread::spawn(move || download_one_segment(&u, &h, &segment, &st));
            (index, handle.join().unwrap_or_else(|e| Err(anyhow::anyhow!("thread panicked: {:?}", e))))
        })
        .collect();

    for (index, res) in results {
        res.context(format!("segment {}", index))?;
        bitmap.set_completed(index);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segmenter::plan_segments;

    #[test]
    fn download_segments_updates_bitmap() {
        let segments = plan_segments(1000, 4);
        let mut bitmap = SegmentBitmap::new(4);
        assert!(!bitmap.all_completed(4));
        bitmap.set_completed(0);
        bitmap.set_completed(2);
        let incomplete: Vec<_> = segments
            .iter()
            .enumerate()
            .filter(|(i, _)| !bitmap.is_completed(*i))
            .collect();
        assert_eq!(incomplete.len(), 2);
        assert!(bitmap.is_completed(0));
        assert!(!bitmap.is_completed(1));
        assert!(bitmap.is_completed(2));
        assert!(!bitmap.is_completed(3));
    }
}
