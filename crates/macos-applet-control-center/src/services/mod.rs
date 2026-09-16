// SPDX-License-Identifier: GPL-3.0-only
//! One independent subscription per system service. A failing service only
//! disables its own tile; each one reconnects with exponential backoff.

pub mod audio;
pub mod bluetooth;
pub mod media;
pub mod network;
