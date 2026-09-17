// SPDX-License-Identifier: GPL-3.0-only
//! Apply, inspect and restore the macOS-style COSMIC top bar.
//!
//! Shared by the `cosmic-macos-setup` CLI and the `cosmic-macos-settings` app,
//! so both take the same backups and roll back the same way.

pub mod backup;
pub mod notifications;
pub mod panel_profile;
pub mod profile;
pub mod three_finger_drag;
pub mod user_service;
pub mod window_controls;

pub use notifications::Position;
pub use panel_profile::{Change, Options};
pub use profile::{Plan, RestoreTarget, Restored, Service};
