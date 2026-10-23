//! Tracker core: presence engine, SQLite store, tracking Day math and the poll-thread runtime.
//! Depends on `tracker-api` only, so it builds and tests without any platform code.
//!
//! The shell uses `RuntimeHandle` (writes go through `Control`; `now` serves `get_now`) and
//! `Reader` (queries).
//! `Tracker` is the single writer the runtime drives; scenarios drive it directly.

pub mod days;
mod engine;
mod runtime;
mod store;

pub use runtime::{RuntimeHandle, Tracker};
pub use store::{Reader, StoreError};
