//! Writes the Swift and the Kotlin.
//!
//! A binary in this crate rather than an installed tool, so the generator that writes the
//! bindings is the version compiled into the library beside them. The two disagreeing about
//! their own wire format is a class of bug that shows up as a crash on a device, long after
//! the build that caused it.
//!
//! Run through `tools/bindings.sh`, which knows the output directories.

#![forbid(unsafe_code)]
#![allow(clippy::expect_used)]

fn main() {
    uniffi::uniffi_bindgen_main();
}
