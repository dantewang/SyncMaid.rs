//! Windows-specific services, each behind a seam so the rest of the app never branches on
//! the platform.

pub mod autostart;
pub mod shell_open;
pub mod single_instance;
pub mod tray;
pub mod window_visibility;
