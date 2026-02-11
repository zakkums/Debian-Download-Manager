//! Build SegmentResult from completed Easy2 transfer (code + handler state).

use crate::retry::SegmentError;
use crate::segmenter::Segment;

use super::handler::SegmentHandler;
use super::super::SegmentResult;

/// Build result from response code and handler bytes_written.
pub(super) fn segment_result_from_easy(
    code: u32,
    segment: &Segment,
    handler: &SegmentHandler,
) -> SegmentResult {
    if code < 200 || code >= 300 {
        return Err(SegmentError::Http(code));
    }
    if code != 206 {
        return Err(SegmentError::InvalidRangeResponse(code));
    }
    let expected = segment.len();
    let received = handler.bytes_written;
    if received != expected {
        return Err(SegmentError::PartialTransfer { expected, received });
    }
    Ok(())
}
