//! oh-key for Windows: a Text Services Framework input method (a "TIP").
//!
//! This DLL is loaded into every GUI process that uses the keyboard, so it
//! stays small and does no work in `DllMain`. The Vietnamese rules live in
//! `oh-key-engine`; this crate is only the COM glue around it.

// Without the exports module the test build never reaches the COM code.
#![cfg_attr(test, allow(dead_code))]
// rustc already exports the #[no_mangle] entry points; the linker only notes it.
#![allow(linker_messages)]

mod diagnostics;
mod display_attribute;
mod edit;
// The test executable must not export COM entry points of its own.
#[cfg(not(test))]
mod exports;
pub mod keys;
mod langbar;
mod registry;
mod service;
pub mod settings;

use std::sync::atomic::AtomicUsize;

/// Live text service objects; `DllCanUnloadNow` answers from this.
pub(crate) static OBJECT_COUNT: AtomicUsize = AtomicUsize::new(0);
