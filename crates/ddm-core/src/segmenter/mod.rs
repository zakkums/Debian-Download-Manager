//! Range math and segment planning.
//!
//! Splits a download into N segments, computes HTTP Range header bounds,
//! and provides a completion bitmap for resume (serialized to DB BLOB).

mod range;
mod bitmap;

pub use range::{Segment, plan_segments};
pub use bitmap::SegmentBitmap;

