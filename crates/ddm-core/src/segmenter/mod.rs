//! Range math and segment planning.
//!
//! Splits a download into N segments, computes HTTP Range header bounds,
//! and provides a completion bitmap for resume (serialized to DB BLOB).

mod bitmap;
mod range;

pub use bitmap::SegmentBitmap;
pub use range::{plan_segments, Segment};
