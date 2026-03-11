//! High-level `DumpReader` that wraps a memory-mapped Oracle EXP dump file.

use std::fs::File;
use std::path::Path;

use encoding_rs::Encoding;
use memmap2::Mmap;

use crate::error::Result;
use crate::parser::{self, RecordIter};
use crate::types::*;

// ─────────────────────────────────────────────────────────────
// DumpReader
// ─────────────────────────────────────────────────────────────

/// Memory-mapped Oracle EXP dump file reader.
///
/// # Example
/// ```no_run
/// use oracle_exp_reader::DumpReader;
/// use oracle_exp_reader::reader::DumpEvent;
///
/// let reader = DumpReader::open("dump.dmp").unwrap();
/// println!("{:?}", reader.header());
///
/// for event in reader.events() {
///     match event.unwrap() {
///         DumpEvent::DdlStatement { sql, .. } => println!("DDL: {}", sql),
///         DumpEvent::Row { table, raw_values, .. } => {
///             println!("ROW in {}: {} cols", table, raw_values.len());
///         }
///         _ => {}
///     }
/// }
/// ```
pub struct DumpReader {
    mmap: Mmap,
    records_start: usize,
    header: DumpHeader,
    charset_enc: Option<&'static Encoding>,
}

impl DumpReader {
    /// Open and memory-map an Oracle EXP dump file.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        // SAFETY: the mmap is read-only and we hold the File open for its lifetime
        let mmap = unsafe { Mmap::map(&file)? };

        let (records_start, version) = parser::find_records_start(&mmap)?;

        // Parse dump header from the first few records
        let mut header = DumpHeader::default();
        header.export_version = version;

        // Quick-scan the first records to get charset / header info
        for rec in RecordIter::new(&mmap, records_start).take(20) {
            match rec? {
                r if r.record_type == RecordType::FileHeader => {
                    header = parser::parse_header_record(&r);
                }
                r if r.record_type == RecordType::CharacterSet => {
                    if let Ok(s) = std::str::from_utf8(r.data) {
                        header.charset = s.trim_matches('\0').to_string();
                    }
                }
                r if r.record_type == RecordType::EndOfFile => break,
                _ => {}
            }
        }

        let charset_enc = parser::charset_to_encoding(&header.charset);

        Ok(Self { mmap, records_start, header, charset_enc })
    }

    /// Access the parsed dump header metadata.
    pub fn header(&self) -> &DumpHeader {
        &self.header
    }

    /// Iterate over high-level dump events.
    pub fn events(&self) -> EventIter<'_> {
        EventIter {
            raw: RecordIter::new(&self.mmap, self.records_start),
            charset_enc: self.charset_enc,
            current_table: None,
            current_owner: String::new(),
            row_count: 0,
        }
    }

    /// Iterate over raw binary records (for advanced / diagnostic use).
    pub fn raw_records(&self) -> RecordIter<'_> {
        RecordIter::new(&self.mmap, self.records_start)
    }

    /// Return the total file size in bytes.
    pub fn file_size(&self) -> usize {
        self.mmap.len()
    }
}

// ─────────────────────────────────────────────────────────────
// High-level events
// ─────────────────────────────────────────────────────────────

/// A high-level parsed event from the dump file.
#[derive(Debug)]
pub enum DumpEvent {
    /// File header / metadata
    Header(DumpHeader),
    /// A DDL statement (CREATE TABLE, CREATE INDEX, GRANT, etc.)
    DdlStatement {
        sql: String,
        byte_offset: usize,
    },
    /// Beginning of row data for a table
    TableStart {
        owner: String,
        table_name: String,
    },
    /// A single row of data
    Row {
        table: String,
        owner: String,
        row_index: u64,
        /// Raw column byte vectors (None = NULL)
        raw_values: Vec<Option<Vec<u8>>>,
        byte_offset: usize,
    },
    /// End of row data for a table
    TableEnd {
        table: String,
        rows_written: u64,
    },
    /// End of dump file
    EndOfFile,
    /// Record type not specifically handled — raw bytes available
    Unknown {
        record_type: u16,
        byte_offset: usize,
        length: usize,
    },
}

// ─────────────────────────────────────────────────────────────
// EventIter
// ─────────────────────────────────────────────────────────────

pub struct EventIter<'a> {
    raw: RecordIter<'a>,
    charset_enc: Option<&'static Encoding>,
    current_table: Option<String>,
    current_owner: String,
    row_count: u64,
}

impl<'a> Iterator for EventIter<'a> {
    type Item = Result<DumpEvent>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let rec = match self.raw.next()? {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };

            match rec.record_type {
                RecordType::FileHeader => {
                    let header = parser::parse_header_record(&rec);
                    return Some(Ok(DumpEvent::Header(header)));
                }

                RecordType::DdlStatement | RecordType::ViewDefinition
                | RecordType::TriggerDefinition | RecordType::ProcedureBody
                | RecordType::SequenceDefinition | RecordType::SynonymDefinition => {
                    let sql = decode_text(rec.data, self.charset_enc);
                    if !sql.trim().is_empty() {
                        return Some(Ok(DumpEvent::DdlStatement {
                            sql,
                            byte_offset: rec.offset,
                        }));
                    }
                }

                RecordType::TableDataStart => {
                    // Best effort: extract table name from surrounding DDL context.
                    // The table name is typically in a nearby TableDefinition record.
                    // For now we emit TableStart with what we have.
                    let name = extract_table_name_from_context(rec.data);
                    self.current_table = Some(name.clone());
                    self.row_count = 0;
                    return Some(Ok(DumpEvent::TableStart {
                        owner: self.current_owner.clone(),
                        table_name: name,
                    }));
                }

                RecordType::RowData => {
                    let offset = rec.offset;
                    let raw_cols: Vec<Option<&[u8]>> = match parser::parse_row_data(rec.data) {
                        Ok(cols) => cols,
                        Err(e) => return Some(Err(e)),
                    };

                    let raw_values: Vec<Option<Vec<u8>>> = raw_cols
                        .into_iter()
                        .map(|opt: Option<&[u8]>| opt.map(|s| s.to_vec()))
                        .collect();

                    let idx = self.row_count;
                    self.row_count += 1;

                    return Some(Ok(DumpEvent::Row {
                        table: self.current_table.clone().unwrap_or_default(),
                        owner: self.current_owner.clone(),
                        row_index: idx,
                        raw_values,
                        byte_offset: offset,
                    }));
                }

                RecordType::TableDataEnd => {
                    let table = self.current_table.take().unwrap_or_default();
                    let rows = self.row_count;
                    self.row_count = 0;
                    return Some(Ok(DumpEvent::TableEnd {
                        table,
                        rows_written: rows,
                    }));
                }

                RecordType::SchemaInfo => {
                    self.current_owner = decode_text(rec.data, self.charset_enc)
                        .trim_matches('\0')
                        .to_string();
                }

                RecordType::EndOfFile => {
                    return Some(Ok(DumpEvent::EndOfFile));
                }

                RecordType::Unknown(code) => {
                    return Some(Ok(DumpEvent::Unknown {
                        record_type: code,
                        byte_offset: rec.offset,
                        length: rec.data.len(),
                    }));
                }

                _ => {
                    // Skip other known-but-unhandled record types
                    continue;
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────

fn decode_text(data: &[u8], enc: Option<&'static Encoding>) -> String {
    if let Some(encoding) = enc {
        let (decoded, _, _) = encoding.decode(data);
        decoded.into_owned()
    } else {
        String::from_utf8_lossy(data).into_owned()
    }
    // Strip null terminators
    .trim_matches('\0')
    .to_string()
}

fn extract_table_name_from_context(data: &[u8]) -> String {
    // Table name may be null-terminated at the start of the data block
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    let raw = &data[..end.min(64)];
    // Keep only printable ASCII
    let s: String = raw.iter().filter(|&&b| b >= 0x20 && b < 0x7F).map(|&b| b as char).collect();
    if s.is_empty() { "UNKNOWN".to_string() } else { s }
}
