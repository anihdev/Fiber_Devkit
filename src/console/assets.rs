//! Embedded console frontend assets.
//! The server reads these at compile time so `fiber console` has no runtime
//! dependency on asset files being present on disk.

/// Embedded console HTML shell.
pub const INDEX_HTML: &str = include_str!("../../console/index.html");
/// Embedded console JavaScript.
pub const APP_JS: &str = include_str!("../../console/app.js");
/// Embedded console stylesheet.
pub const STYLE_CSS: &str = include_str!("../../console/style.css");
/// Embedded 16px PNG favicon.
pub const FAVICON_16_PNG: &[u8] = include_bytes!("../../console/favicon-16x16.png");
/// Embedded 32px PNG favicon and browser fallback icon.
pub const FAVICON_32_PNG: &[u8] = include_bytes!("../../console/favicon-32x32.png");
/// Embedded 180px touch icon.
pub const APPLE_TOUCH_ICON_PNG: &[u8] = include_bytes!("../../console/apple-touch-icon.png");
