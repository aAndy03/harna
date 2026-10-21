//! The per-file passes validate fans out over threads: the chunking every
//! pass shares, and the TypeScript/JavaScript pass (1.1.3).

pub mod parallel;
pub mod web;
