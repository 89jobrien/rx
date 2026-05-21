pub mod repo;
pub mod script;

// Re-export script types at crate root for ergonomic access from existing
// consumers (rx-install, rxx, rx-registry-json).
pub use script::*;
