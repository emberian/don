//! Platform-independent half of donscan, split out so the vtable-map logic can be
//! unit-tested on the arm64 Mac (`cargo test -p donscan --lib`) without linking
//! kernel32. The scanner itself lives in `main.rs` and is Windows-only.

pub mod vtables;
