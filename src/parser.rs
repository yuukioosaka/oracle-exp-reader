//! Core parser for Oracle EXP (classic export) dump files.
//!
//! ## File Format (reverse-engineered)
//!
//! ```text
//! ┌──────────────────────────────────────────────────────┐
//! │  HEADER MAGIC (variable)                             │
//! │  Usually starts with 3 bytes then "EXPORT:Vxx.xx.xx"│
//! ├──────────────────────────────────────────────────────┤
//! │  RECORD 1..N                                         │
//! │  ┌─────────────┬──────────────┬──────────────────┐  │
//! │  │ type: u16BE │ len: u16BE   │ data: [u8; len]  │  │
//! │  └─────────────┴──────────────┴──────────────────┘  │
//! │  If len == 0xFFFF → next 4 bytes = actual len (u32BE)│
//! └──────────────────────────────────────────────────────┘
//! ```
//!
//! Row data encoding (per column):
//! ```text
//! ┌──────────────────────────────────────────────────────┐
//! │ col_len: u8   — 0xFF = NULL, 0xFE = long val follows │
//! │ data: [u8; col_len]                                  │
//! └──────────────────────────────────────────────────────┘
//! ```

use crate::error::{DumpError, Result};
use crate::types::*;
use encoding_rs::Encoding;

/// Minimum bytes needed to identify a file as an Oracle EXP dump
const MIN_HEADER_LEN: usize = 20;

/// Magic string present in every Oracle EXP dump header
const EXPORT_MAGIC: &[u8] = b"EXPORT:V";

/// NULL column indicator
const COL_NULL: u8 = 0xFF;
/// Long-value-follows indicator (actual length in next 4 bytes)
const COL_LONG: u8 = 0xFE;

// ─────────────────────────────────────────────────────────────
// Low-level byte helpers
// ─────────────────────────────────────────────────────────────

#[inline]
fn read_u8(data: &[u8], pos: usize) -> Result<u8> {
    data.get(pos)
        .copied()
        .ok_or(DumpError::UnexpectedEof { offset: pos })
}

#[inline]
fn read_u16be(data: &[u8], pos: usize) -> Result<u16> {
    if pos + 2 > data.len() {
        return Err(DumpError::UnexpectedEof { offset: pos });
    }
    Ok(u16::from_be_bytes([data[pos], data[pos + 1]]))
}

#[inline]
fn read_u32be(data: &[u8], pos: usize) -> Result<u32> {
    if pos + 4 > data.len() {
        return Err(DumpError::UnexpectedEof { offset: pos });
    }
    Ok(u32::from_be_bytes([
        data[pos],
        data[pos + 1],
        data[pos + 2],
        data[pos + 3],
    ]))
}

// ─────────────────────────────────────────────────────────────
// Record iterator
// ─────────────────────────────────────────────────────────────

/// Iterator over raw records in an Oracle EXP dump file.
/// Zero-copy: each record's data field is a slice into the original buffer.
pub struct RecordIter<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> RecordIter<'a> {
    /// Create a new record iterator positioned after the file header.
    pub fn new(data: &'a [u8], start: usize) -> Self {
        Self { data, pos: start }
    }

    /// Current byte offset
    pub fn offset(&self) -> usize {
        self.pos
    }
}

impl<'a> Iterator for RecordIter<'a> {
    type Item = Result<RawRecord<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.data.len() {
            return None;
        }

        let offset = self.pos;

        // Read record type (2 bytes)
        let rtype_raw = match read_u16be(self.data, self.pos) {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        };
        self.pos += 2;

        // Read length (2 bytes); 0xFFFF means "long record" with 4-byte actual length
        let len_short = match read_u16be(self.data, self.pos) {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        };
        self.pos += 2;

        let data_len: usize = if len_short == 0xFFFF {
            let long_len = match read_u32be(self.data, self.pos) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            self.pos += 4;
            long_len as usize
        } else {
            len_short as usize
        };

        // Bounds check
        if self.pos + data_len > self.data.len() {
            return Some(Err(DumpError::UnexpectedEof { offset: self.pos }));
        }

        let payload = &self.data[self.pos..self.pos + data_len];
        self.pos += data_len;

        Some(Ok(RawRecord {
            record_type: RecordType::from(rtype_raw),
            offset,
            data: payload,
        }))
    }
}

// ─────────────────────────────────────────────────────────────
// Header detection
// ─────────────────────────────────────────────────────────────

/// Detect the start offset of record data after the variable-length file header.
/// Returns (start_offset, export_version_string).
pub fn find_records_start(data: &[u8]) -> Result<(usize, String)> {
    if data.len() < MIN_HEADER_LEN {
        return Err(DumpError::InvalidMagic);
    }

    // Search for "EXPORT:V" within the first 64 bytes
    let search_window = &data[..data.len().min(64)];
    let magic_pos = search_window
        .windows(EXPORT_MAGIC.len())
        .position(|w| w == EXPORT_MAGIC)
        .ok_or(DumpError::InvalidMagic)?;

    // Read until null byte or non-printable to extract version string
    let ver_start = magic_pos;
    let ver_end = data[ver_start..]
        .iter()
        .position(|&b| b == 0 || b < 0x20)
        .map(|p| ver_start + p)
        .unwrap_or(ver_start + 20);

    let version = String::from_utf8_lossy(&data[ver_start..ver_end]).to_string();

    // Records begin on the next 4-byte-aligned boundary after the version string,
    // or immediately after the null terminator — typically at a fixed offset.
    // In practice, Oracle pads the initial block to 2048 bytes.
    // We scan forward from the end of the version string to find the first valid
    // record-type marker.
    let scan_start = ver_end;
    let records_start = find_first_record(data, scan_start)?;

    Ok((records_start, version))
}

/// Scan forward from `from` to find what looks like the first real record header.
/// A valid record starts with a u16BE type (1–28) followed by a plausible length.
fn find_first_record(data: &[u8], from: usize) -> Result<usize> {
    let mut pos = from;

    // Skip NUL padding
    while pos < data.len() && data[pos] == 0 {
        pos += 1;
    }

    // Try to validate a record at this position; if not valid, advance 1 byte
    let end = data.len().saturating_sub(4);
    while pos < end {
        if let Ok(rtype) = read_u16be(data, pos) {
            if rtype >= 1 && rtype <= 28 {
                // Looks plausible — consider this the start of records
                return Ok(pos);
            }
        }
        pos += 1;
    }

    Err(DumpError::ParseError {
        offset: from,
        message: "Could not find first record".to_string(),
    })
}

// ─────────────────────────────────────────────────────────────
// Header record parsing
// ─────────────────────────────────────────────────────────────

/// Parse the file header record (type 1) to extract metadata.
pub fn parse_header_record(record: &RawRecord<'_>) -> DumpHeader {
    let text = String::from_utf8_lossy(record.data);
    let mut header = DumpHeader::default();

    // Version is in the parent context; parse NLS / date from the record body
    for line in text.split('\x00').filter(|s| !s.is_empty()) {
        if let Some(val) = line.strip_prefix("EXPORT:V") {
            header.export_version = format!("EXPORT:V{}", val.trim_end());
        } else if line.starts_with("NLS_CHARACTERSET=") {
            header.charset = line["NLS_CHARACTERSET=".len()..].to_string();
        } else if line.starts_with("NLS_NCHAR_CHARACTERSET=") {
            header.ncharset = line["NLS_NCHAR_CHARACTERSET=".len()..].to_string();
        } else if line.contains("EXPORT:") || line.contains("Export:") {
            // Try to grab date from lines like "Export done in WE8MSWIN1252 character set..."
        }
    }

    // Also scan raw bytes for printable strings
    extract_header_strings(record.data, &mut header);

    header
}

fn extract_header_strings(data: &[u8], header: &mut DumpHeader) {
    // Extract all NUL-delimited printable strings
    let mut start = 0;
    let mut strings: Vec<String> = Vec::new();

    for (i, &b) in data.iter().enumerate() {
        if b == 0 {
            if i > start {
                let chunk = &data[start..i];
                if chunk.iter().all(|&c| c >= 0x20 && c < 0x7F) {
                    strings.push(String::from_utf8_lossy(chunk).to_string());
                }
            }
            start = i + 1;
        }
    }

    for s in &strings {
        if s.starts_with("EXPORT:V") && header.export_version.is_empty() {
            header.export_version = s.clone();
        } else if s.len() > 5 && s.contains('/') && header.export_date.is_empty() {
            // Date-like pattern
            header.export_date = s.clone();
        } else if (s.contains("WE8") || s.contains("UTF8") || s.contains("AL32"))
            && header.charset.is_empty()
        {
            header.charset = s.clone();
        }
    }
}

// ─────────────────────────────────────────────────────────────
// Row data parsing
// ─────────────────────────────────────────────────────────────

/// Parse a RowData record into individual column byte slices.
///
/// Each column is encoded as:
///   - `0xFF` → NULL
///   - `0xFE` → long value: next 4 bytes = length, then data
///   - `0x00` → empty string (zero-length, not NULL)
///   - other  → length byte, then that many bytes of data
pub fn parse_row_data<'a>(data: &'a [u8]) -> Result<Vec<Option<&'a [u8]>>> {
    let mut pos = 0;
    let mut cols: Vec<Option<&'a [u8]>> = Vec::new();

    while pos < data.len() {
        let indicator = read_u8(data, pos)?;
        pos += 1;

        match indicator {
            COL_NULL => {
                cols.push(None);
            }
            COL_LONG => {
                // 4-byte length follows
                let len = read_u32be(data, pos)? as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(DumpError::UnexpectedEof { offset: pos });
                }
                cols.push(Some(&data[pos..pos + len]));
                pos += len;
            }
            0x00 => {
                // Zero-length (empty string)
                cols.push(Some(&data[pos..pos]));
            }
            len => {
                let len = len as usize;
                if pos + len > data.len() {
                    return Err(DumpError::UnexpectedEof { offset: pos });
                }
                cols.push(Some(&data[pos..pos + len]));
                pos += len;
            }
        }
    }

    Ok(cols)
}

// ─────────────────────────────────────────────────────────────
// Column value decoding
// ─────────────────────────────────────────────────────────────

/// Decode a raw column byte slice into a typed `ColumnValue`.
///
/// Encoding depends on the Oracle data type:
/// - VARCHAR2 / CHAR / LONG → text (charset-dependent)
/// - NUMBER → Oracle internal BCD-like number format
/// - DATE → 7-byte date encoding
/// - RAW / BLOB → hex bytes
pub fn decode_column_value(
    raw: &[u8],
    col_type: &OracleType,
    charset_enc: Option<&'static Encoding>,
) -> ColumnValue {
    if raw.is_empty() {
        return ColumnValue::Text(String::new());
    }

    match col_type {
        OracleType::Varchar2
        | OracleType::Nvarchar2
        | OracleType::Char
        | OracleType::Nchar
        | OracleType::Long
        | OracleType::XmlType => {
            let text = if let Some(enc) = charset_enc {
                let (decoded, _encoding_used, _had_errors) = enc.decode(raw);
                decoded.into_owned()
            } else {
                String::from_utf8_lossy(raw).into_owned()
            };
            ColumnValue::Text(text)
        }

        OracleType::Number | OracleType::Float => ColumnValue::Number(decode_oracle_number(raw)),

        OracleType::Date => ColumnValue::Text(decode_oracle_date(raw)),

        OracleType::Timestamp | OracleType::TimestampWithTZ | OracleType::TimestampWithLocalTZ => {
            ColumnValue::Text(decode_oracle_timestamp(raw))
        }

        OracleType::Raw | OracleType::LongRaw | OracleType::Blob | OracleType::Bfile => {
            ColumnValue::Bytes(raw.to_vec())
        }

        OracleType::Clob | OracleType::Nclob => {
            let text = if let Some(enc) = charset_enc {
                let (decoded, _, _) = enc.decode(raw);
                decoded.into_owned()
            } else {
                String::from_utf8_lossy(raw).into_owned()
            };
            ColumnValue::Text(text)
        }

        _ => ColumnValue::Bytes(raw.to_vec()),
    }
}

// ─────────────────────────────────────────────────────────────
// Oracle NUMBER format decoder
// ─────────────────────────────────────────────────────────────

/// Decode Oracle's internal NUMBER representation to a decimal string.
///
/// Oracle NUMBER uses a proprietary base-100 encoding:
/// ```text
/// byte[0]: exponent biased by 65 (0xC1 = positive, 0x3E = negative)
/// byte[1..]: mantissa digits in base 100, biased by 1
/// Negative numbers have complement encoding with 0x66 terminator
/// ```
pub fn decode_oracle_number(data: &[u8]) -> String {
    if data.is_empty() {
        return "NULL".to_string();
    }

    let exp_byte = data[0];

    // Special: 0x80 = positive zero
    if exp_byte == 0x80 {
        return "0".to_string();
    }

    let negative = exp_byte < 0x80;

    let (exponent, digits): (i32, Vec<u8>) = if negative {
        // Negative: complement the exponent byte, stop at 0x66 terminator
        let exp = ((!exp_byte) as i32) - 65;
        let raw_digits: Vec<u8> = data[1..]
            .iter()
            .take_while(|&&b| b != 0x66)
            .map(|&b| 101 - b) // un-complement each mantissa byte
            .collect();
        (exp, raw_digits)
    } else {
        let exp = (exp_byte as i32) - 65;
        let raw_digits: Vec<u8> = data[1..].iter().map(|&b| b - 1).collect();
        (exp, raw_digits)
    };

    if digits.is_empty() {
        return if negative { "-0" } else { "0" }.to_string();
    }

    // Build decimal string from base-100 digits
    // Each digit is 0-99 and represents two decimal places
    let mut s = String::new();
    if negative {
        s.push('-');
    }

    // The decimal point position is after (exponent + 1) * 2 digits from the left
    let decimal_pos = (exponent + 1) * 2; // number of integer digits

    let mut all_digits = String::new();
    for &d in &digits {
        all_digits.push_str(&format!("{:02}", d));
    }

    // Remove leading zeros from all_digits
    let trimmed = all_digits.trim_start_matches('0');
    if trimmed.is_empty() {
        return "0".to_string();
    }

    if decimal_pos <= 0 {
        s.push_str("0.");
        for _ in 0..(-decimal_pos) {
            s.push('0');
        }
        s.push_str(trimmed);
    } else if decimal_pos as usize >= trimmed.len() {
        s.push_str(trimmed);
        for _ in trimmed.len()..(decimal_pos as usize) {
            s.push('0');
        }
    } else {
        let (int_part, frac_part) = trimmed.split_at(decimal_pos as usize);
        s.push_str(int_part);
        let frac_trimmed = frac_part.trim_end_matches('0');
        if !frac_trimmed.is_empty() {
            s.push('.');
            s.push_str(frac_trimmed);
        }
    }

    s
}

// ─────────────────────────────────────────────────────────────
// Oracle DATE format decoder
// ─────────────────────────────────────────────────────────────

/// Decode Oracle's 7-byte DATE format.
///
/// ```text
/// byte[0]: century + 100  (e.g. 120 = 20th century, 121 = 21st century)
/// byte[1]: year within century + 100
/// byte[2]: month (1-12)
/// byte[3]: day   (1-31)
/// byte[4]: hour  + 1
/// byte[5]: minute + 1
/// byte[6]: second + 1
/// ```
pub fn decode_oracle_date(data: &[u8]) -> String {
    if data.len() < 7 {
        return format!("DATE({})", hex_str(data));
    }

    let century = data[0] as i32 - 100;
    let year_in_cent = data[1] as i32 - 100;
    let year = century * 100 + year_in_cent;
    let month = data[2];
    let day = data[3];
    let hour = data[4] as i32 - 1;
    let minute = data[5] as i32 - 1;
    let second = data[6] as i32 - 1;

    if hour == 0 && minute == 0 && second == 0 {
        format!("{:04}-{:02}-{:02}", year, month, day)
    } else {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            year, month, day, hour, minute, second
        )
    }
}

/// Decode Oracle TIMESTAMP (11 bytes = DATE bytes + nanoseconds in 4 bytes)
pub fn decode_oracle_timestamp(data: &[u8]) -> String {
    if data.len() < 7 {
        return format!("TIMESTAMP({})", hex_str(data));
    }

    let date_part = decode_oracle_date(&data[..7]);

    let nanos = if data.len() >= 11 {
        let ns = u32::from_be_bytes([data[7], data[8], data[9], data[10]]);
        if ns > 0 {
            format!(".{:09}", ns)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    format!("{}{}", date_part, nanos)
}

// ─────────────────────────────────────────────────────────────
// Utilities
// ─────────────────────────────────────────────────────────────

fn hex_str(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join("")
}

/// Resolve an Oracle NLS charset name to an encoding_rs `Encoding`.
pub fn charset_to_encoding(charset: &str) -> Option<&'static Encoding> {
    let upper = charset.to_uppercase();
    match upper.as_str() {
        "AL32UTF8" | "UTF8" | "UTF-8" => Some(encoding_rs::UTF_8),
        "WE8MSWIN1252" | "WE8ISO8859P1" => Some(encoding_rs::WINDOWS_1252),
        "EE8MSWIN1250" | "EE8ISO8859P2" => Some(encoding_rs::ISO_8859_2),
        "CL8MSWIN1251" | "CL8ISO8859P5" => Some(encoding_rs::WINDOWS_1251),
        "AL16UTF16" => Some(encoding_rs::UTF_16BE),
        "JA16SJIS" => Some(encoding_rs::SHIFT_JIS),
        "JA16EUC" => Some(encoding_rs::EUC_JP),
        "ZHS16GBK" => Some(encoding_rs::GBK),
        "ZHT16BIG5" => Some(encoding_rs::BIG5),
        "KO16MSWIN949" | "KO16KSC5601" => Some(encoding_rs::EUC_KR),
        "AR8MSWIN1256" => Some(encoding_rs::WINDOWS_1256),
        "TR8MSWIN1254" => Some(encoding_rs::WINDOWS_1254),
        "EL8MSWIN1253" => Some(encoding_rs::WINDOWS_1253),
        "IW8MSWIN1255" => Some(encoding_rs::WINDOWS_1255),
        _ => None,
    }
}

/// Extract a null-terminated string from a byte slice at the given offset.
pub fn read_nul_string(data: &[u8], pos: usize) -> (String, usize) {
    let end = data[pos..]
        .iter()
        .position(|&b| b == 0)
        .map(|p| pos + p)
        .unwrap_or(data.len());
    let s = String::from_utf8_lossy(&data[pos..end]).into_owned();
    (s, end + 1)
}

/// Extract a length-prefixed string (1-byte length then data).
pub fn read_len1_string(data: &[u8], pos: usize) -> Result<(String, usize)> {
    let len = read_u8(data, pos)? as usize;
    if pos + 1 + len > data.len() {
        return Err(DumpError::UnexpectedEof { offset: pos });
    }
    let s = String::from_utf8_lossy(&data[pos + 1..pos + 1 + len]).into_owned();
    Ok((s, pos + 1 + len))
}

/// Extract a length-prefixed string (2-byte BE length then data).
pub fn read_len2_string(data: &[u8], pos: usize) -> Result<(String, usize)> {
    let len = read_u16be(data, pos)? as usize;
    if pos + 2 + len > data.len() {
        return Err(DumpError::UnexpectedEof { offset: pos });
    }
    let s = String::from_utf8_lossy(&data[pos + 2..pos + 2 + len]).into_owned();
    Ok((s, pos + 2 + len))
}
