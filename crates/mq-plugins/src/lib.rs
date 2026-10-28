//! The plugin engine (plans/plugins-and-marketplace.md section 6).
//!
//! The store-build check (6.1 item 2) knows a core carries this engine by the
//! symbols `JS_NewRuntime` and `JS_Eval` before stripping, and by these
//! QuickJS-NG error messages, which every stripped library with the engine
//! keeps (bench/plugins/d-spike.jsonl):
//!
//! - `invalid opcode: pc=%u opcode=0x%02x`
//! - `bytecode function expected`
//! - `out of memory in regexp execution`

pub mod cheerio;
pub mod dom;
pub mod engine;
pub mod natives;
pub mod streams;
