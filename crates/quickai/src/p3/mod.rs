//! The Phase 3 tests, written before the work they test (docs/phase3-test-plan.md).
//!
//! Every test is named `p3_<id>_<what>`, where `<id>` is its row in the test plan (`p3_z07_...`
//! is row Z07): `tests/phase3_plan.rs` checks that the plan and this tree agree. A test whose work
//! package has not landed is `#[ignore = "phase 3 WP3.x: ..."]` and fails when it is run
//! (`cargo test -p quickai -- --ignored p3_`), for the reason the message gives. When the work
//! package lands the `ignore` goes, and the test is a regression test from then on.

mod contract;
mod docs;
mod engine;
mod feed;
mod http_call;
mod install;
mod models;
#[cfg(unix)]
mod probe;
mod proxy;
#[cfg(unix)]
mod session;
