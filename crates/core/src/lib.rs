//! Crunchyroll Desktop core: API access, data shaping and caches. No UI code.

pub mod api;
pub mod cards;
pub mod format;
pub mod images;
pub mod locale;
pub mod manifest;
pub mod models;
pub mod session;

pub use api::{DrmSystem, Playback};
pub use session::Session;
