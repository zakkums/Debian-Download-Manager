//! Minimal HAR 1.2 structures for resolving redirect chain and URL.

use serde::Deserialize;

/// Root HAR log (top-level wrapper).
#[derive(Debug, Deserialize)]
pub struct HarLog {
    pub log: HarRoot,
}

#[derive(Debug, Deserialize)]
pub struct HarRoot {
    pub entries: Vec<HarEntry>,
}

#[derive(Debug, Deserialize)]
pub struct HarEntry {
    pub request: HarRequest,
    pub response: HarResponse,
}

#[derive(Debug, Deserialize)]
pub struct HarRequest {
    pub url: String,
    #[serde(default)]
    pub headers: Vec<HarHeader>,
}

#[derive(Debug, Deserialize)]
pub struct HarResponse {
    #[serde(default)]
    pub status: u16,
    #[serde(default, rename = "redirectURL")]
    pub redirect_url: Option<String>,
    #[serde(default)]
    pub headers: Vec<HarHeader>,
}

#[derive(Debug, Deserialize)]
pub struct HarHeader {
    pub name: String,
    pub value: String,
}
