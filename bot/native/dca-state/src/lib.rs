//! Persistent state for the DCA bot: Firebase (Firestore / Realtime Database) with a local JSON fallback.

pub mod config;
pub mod firebase;
pub mod models;
pub mod state_store;
pub mod stores;
pub mod util;

pub use state_store::StateStore;
