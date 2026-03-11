//! # oracle-exp-reader
//!
//! High-performance Rust library and CLI tool for reading Oracle classic EXP dump files.
//!
//! ## Features
//! - Zero-copy memory-mapped file reading via `memmap2`
//! - Decodes Oracle NUMBER, DATE, TIMESTAMP to human-readable strings
//! - Multi-charset support via `encoding_rs`
//! - Outputs CSV, SQL INSERT statements, JSON Lines, or hex dumps
//!
//! ## Quick start
//!
//! ```no_run
//! use oracle_exp_reader::{DumpReader, reader::DumpEvent};
//!
//! let reader = DumpReader::open("dump.dmp").unwrap();
//! println!("Version: {}", reader.header().export_version);
//! println!("Charset: {}", reader.header().charset);
//!
//! for event in reader.events() {
//!     match event.unwrap() {
//!         DumpEvent::DdlStatement { sql, .. } => println!("DDL: {}", &sql[..sql.len().min(80)]),
//!         DumpEvent::Row { table, row_index, raw_values, .. } => {
//!             println!("  [{}] row {}: {} cols", table, row_index, raw_values.len());
//!         }
//!         DumpEvent::EndOfFile => break,
//!         _ => {}
//!     }
//! }
//! ```

pub mod error;
pub mod output;
pub mod parser;
pub mod reader;
pub mod types;

pub use reader::DumpReader;
pub use error::{DumpError, Result};
pub use types::*;
