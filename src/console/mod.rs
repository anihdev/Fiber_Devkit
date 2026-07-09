//! Local browser console modules.
//! This layer serves embedded static assets and read-only JSON endpoints over
//! existing DevKit data; it does not introduce mutation endpoints or streaming.

pub mod assets;
pub mod server;
