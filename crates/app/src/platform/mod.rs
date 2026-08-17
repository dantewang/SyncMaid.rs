//! Windows-specific services, each behind a seam so the rest of the app never branches on
//! the platform.

pub mod autostart;
pub mod tray;
pub mod window_visibility;
