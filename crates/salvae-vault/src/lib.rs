//! Salvaê vault: encrypted, versioned save storage over a message channel.
//!
//! The vault logic is transport-agnostic (see [`channel::Channel`]); the live
//! Discord REST transport lives in a separate crate/plan and reuses the pure
//! helpers in [`multipart`] and [`ratelimit`].
