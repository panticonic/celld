// Copyright 2026 Deno Land Inc. Apache-2.0 license.

//! The deterministic execution core for Dolt cells (docs/fork/design.md
//! section 3a): `execute(conn, program, ctx) -> Outcome`, the consensus
//! authorizer, the engine digest.
//!
//! It lives in its own crate, `celld-dolt-exec`, so that it and its
//! determinism and conformance tests build against only rusqlite and the
//! patched DoltLite amalgamation. This module is the path callers use.

pub use celld_dolt_exec::*;
