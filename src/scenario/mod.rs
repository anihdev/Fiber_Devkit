//! Scenario parsing and execution modules.
//! This layer owns YAML schema validation and step orchestration; low-level FNN
//! HTTP calls live in `src/rpc`.

pub mod parser;
pub mod runner;
pub mod types;
