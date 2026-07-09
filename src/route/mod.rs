//! Route intelligence modules for Fiber DevKit.
//! This layer owns native Fiber route scoring and CCH availability comparison;
//! it never executes payments or creates live CCH orders.

pub mod analyzer;
pub mod cch;
