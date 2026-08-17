//! The generated string tables and their typed accessors.
//!
//! Written by `build/strings.rs` from `lang/*.json`. A key that does not exist is a compile
//! error here, and so is calling one with the wrong number of arguments — which is the whole
//! reason this is generated rather than looked up by name.

// Only a handful of the 200-odd accessors have call sites so far; the rest are the same
// table and cost nothing.
#![allow(dead_code)]

include!(concat!(env!("OUT_DIR"), "/strings.rs"));
