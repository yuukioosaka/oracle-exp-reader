//! Output formatters: CSV, INSERT SQL, JSON Lines, and hex dump.

use crate::reader::DumpEvent;
use std::io::{self, Write};

// ─────────────────────────────────────────────────────────────
// CSV writer
// ─────────────────────────────────────────────────────────────

/// Write dump events to CSV (one file per table or stdout).
pub struct CsvWriter<W: Write> {
    out: W,
    delimiter: u8,
    in_table: bool,
    col_count: usize,
}

impl<W: Write> CsvWriter<W> {
    pub fn new(out: W) -> Self {
        Self {
            out,
            delimiter: b',',
            in_table: false,
            col_count: 0,
        }
    }

    pub fn with_delimiter(mut self, delim: u8) -> Self {
        self.delimiter = delim;
        self
    }

    pub fn handle_event(&mut self, event: &DumpEvent) -> io::Result<()> {
        match event {
            DumpEvent::TableStart { owner, table_name } => {
                writeln!(self.out, "-- Table: {}.{}", owner, table_name)?;
                self.in_table = true;
                self.col_count = 0;
            }
            DumpEvent::Row { raw_values, .. } => {
                if self.in_table {
                    self.col_count = raw_values.len();
                    let row: Vec<String> = raw_values
                        .iter()
                        .map(|v| match v {
                            None => String::new(),
                            Some(b) => csv_escape(&String::from_utf8_lossy(b)),
                        })
                        .collect();
                    writeln!(
                        self.out,
                        "{}",
                        row.join(&(self.delimiter as char).to_string())
                    )?;
                }
            }
            DumpEvent::TableEnd {
                table,
                rows_written,
            } => {
                writeln!(self.out, "-- End of {}: {} rows", table, rows_written)?;
                self.in_table = false;
            }
            DumpEvent::DdlStatement { sql, .. } => {
                writeln!(self.out, "-- DDL: {}", sql.lines().next().unwrap_or(""))?;
            }
            _ => {}
        }
        Ok(())
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// ─────────────────────────────────────────────────────────────
// SQL INSERT writer
// ─────────────────────────────────────────────────────────────

pub struct SqlInsertWriter<W: Write> {
    out: W,
    current_table: String,
    current_owner: String,
    batch_size: usize,
    row_buffer: Vec<String>,
    total_rows: u64,
}

impl<W: Write> SqlInsertWriter<W> {
    pub fn new(out: W, batch_size: usize) -> Self {
        Self {
            out,
            current_table: String::new(),
            current_owner: String::new(),
            batch_size: batch_size.max(1),
            row_buffer: Vec::new(),
            total_rows: 0,
        }
    }

    pub fn handle_event(&mut self, event: &DumpEvent) -> io::Result<()> {
        match event {
            DumpEvent::DdlStatement { sql, .. } => {
                writeln!(self.out, "{};", sql.trim_end_matches(';'))?;
                writeln!(self.out)?;
            }
            DumpEvent::TableStart { owner, table_name } => {
                self.current_owner = owner.clone();
                self.current_table = table_name.clone();
                self.total_rows = 0;
                writeln!(self.out, "-- Loading table {}.{}", owner, table_name)?;
            }
            DumpEvent::Row {
                raw_values,
                owner,
                table,
                ..
            } => {
                let values: Vec<String> = raw_values
                    .iter()
                    .map(|v| match v {
                        None => "NULL".to_string(),
                        Some(b) => sql_quote_value(b),
                    })
                    .collect();

                let qualified = if owner.is_empty() {
                    table.clone()
                } else {
                    format!("{}.{}", owner, table)
                };

                self.row_buffer.push(format!(
                    "INSERT INTO {} VALUES ({});",
                    qualified,
                    values.join(", ")
                ));
                self.total_rows += 1;

                if self.row_buffer.len() >= self.batch_size {
                    self.flush_buffer()?;
                }
            }
            DumpEvent::TableEnd {
                table,
                rows_written,
            } => {
                self.flush_buffer()?;
                writeln!(self.out, "-- End {}: {} rows", table, rows_written)?;
                writeln!(self.out, "COMMIT;")?;
                writeln!(self.out)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn flush_buffer(&mut self) -> io::Result<()> {
        for line in self.row_buffer.drain(..) {
            writeln!(self.out, "{}", line)?;
        }
        Ok(())
    }
}

fn sql_quote_value(raw: &[u8]) -> String {
    // Try UTF-8; if it fails, hex-encode
    match std::str::from_utf8(raw) {
        Ok(s) => {
            // If it looks like a number, don't quote it
            if is_numeric_str(s) {
                s.to_string()
            } else {
                format!("'{}'", s.replace('\'', "''"))
            }
        }
        Err(_) => {
            // Binary: use Oracle hex literal
            let hex: String = raw.iter().map(|b| format!("{:02X}", b)).collect();
            format!("HEXTORAW('{}')", hex)
        }
    }
}

fn is_numeric_str(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == '+')
}

// ─────────────────────────────────────────────────────────────
// JSON Lines writer
// ─────────────────────────────────────────────────────────────

pub struct JsonLinesWriter<W: Write> {
    out: W,
}

impl<W: Write> JsonLinesWriter<W> {
    pub fn new(out: W) -> Self {
        Self { out }
    }

    pub fn handle_event(&mut self, event: &DumpEvent) -> io::Result<()> {
        match event {
            DumpEvent::DdlStatement { sql, byte_offset } => {
                let obj = serde_json::json!({
                    "type": "ddl",
                    "offset": byte_offset,
                    "sql": sql.trim()
                });
                writeln!(self.out, "{}", obj)?;
            }
            DumpEvent::TableStart { owner, table_name } => {
                let obj = serde_json::json!({
                    "type": "table_start",
                    "owner": owner,
                    "table": table_name
                });
                writeln!(self.out, "{}", obj)?;
            }
            DumpEvent::Row {
                table,
                owner,
                row_index,
                raw_values,
                byte_offset,
            } => {
                let cols: Vec<serde_json::Value> = raw_values
                    .iter()
                    .map(|v| match v {
                        None => serde_json::Value::Null,
                        Some(b) => match std::str::from_utf8(b) {
                            Ok(s) => serde_json::Value::String(s.to_string()),
                            Err(_) => {
                                let hex: String = b.iter().map(|x| format!("{:02X}", x)).collect();
                                serde_json::Value::String(format!("0x{}", hex))
                            }
                        },
                    })
                    .collect();

                let obj = serde_json::json!({
                    "type": "row",
                    "table": table,
                    "owner": owner,
                    "row": row_index,
                    "offset": byte_offset,
                    "values": cols
                });
                writeln!(self.out, "{}", obj)?;
            }
            DumpEvent::TableEnd {
                table,
                rows_written,
            } => {
                let obj = serde_json::json!({
                    "type": "table_end",
                    "table": table,
                    "rows": rows_written
                });
                writeln!(self.out, "{}", obj)?;
            }
            DumpEvent::EndOfFile => {
                let obj = serde_json::json!({ "type": "eof" });
                writeln!(self.out, "{}", obj)?;
            }
            _ => {}
        }
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────
// Hex dump (diagnostic)
// ─────────────────────────────────────────────────────────────

/// Print a hex+ASCII dump of a byte slice, like `xxd`.
pub fn hex_dump<W: Write>(out: &mut W, data: &[u8], start_offset: usize) -> io::Result<()> {
    for (chunk_idx, chunk) in data.chunks(16).enumerate() {
        let addr = start_offset + chunk_idx * 16;
        write!(out, "{:08X}  ", addr)?;

        for (i, b) in chunk.iter().enumerate() {
            write!(out, "{:02X} ", b)?;
            if i == 7 {
                write!(out, " ")?;
            }
        }

        // Pad short lines
        let padding = 16 - chunk.len();
        for i in 0..padding {
            write!(out, "   ")?;
            if chunk.len() + i == 7 {
                write!(out, " ")?;
            }
        }

        write!(out, " |")?;
        for &b in chunk {
            let c = if b >= 0x20 && b < 0x7F {
                b as char
            } else {
                '.'
            };
            write!(out, "{}", c)?;
        }
        writeln!(out, "|")?;
    }
    Ok(())
}
