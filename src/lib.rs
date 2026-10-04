//! Experimental single-authority local core; see docs/local-core.md.
mod adapters;
mod store;
mod types;
pub use adapters::*;
pub use store::Store;
pub use types::*;

pub mod processing;
