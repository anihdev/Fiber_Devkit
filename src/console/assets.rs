//! Embedded console frontend assets.
//! The server reads these at compile time with `include_str!` so `fiber console`
//! has no runtime dependency on asset files being present on disk.

/// Embedded console HTML shell.
pub const INDEX_HTML: &str = include_str!("../../console/index.html");
/// Embedded console JavaScript.
pub const APP_JS: &str = include_str!("../../console/app.js");
/// Embedded console stylesheet.
pub const STYLE_CSS: &str = include_str!("../../console/style.css");
/// Embedded adaptive SVG favicon.
pub const FAVICON_SVG: &str = include_str!("../../console/favicon.svg");
