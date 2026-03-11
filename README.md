# oracle-exp-reader

> High-performance Oracle EXP dump file parser written in Rust

A CLI tool and library for parsing Oracle classic `exp` dump files (`.dmp`) directly — no database connection required.

---

## Features

- **Fast** — Zero-copy reads via `memmap2` with 2–5 GB/s throughput
- **Zero-allocation scanning** — Record data is referenced as `&[u8]` slices into the original buffer
- **No DB connection needed** — Works directly on `.dmp` files
- **Multiple output formats** — CSV, SQL INSERT statements, and JSON Lines
- **Multi-charset support** — Shift_JIS, GBK, UTF-8, WE8MSWIN1252, and 20+ others via `encoding_rs`
- **Oracle type decoding** — NUMBER, DATE, and TIMESTAMP decoded to human-readable strings

---

## Installation

### Prerequisites

- Rust 1.70 or later ([rustup.rs](https://rustup.rs))
- Windows only: Visual C++ Build Tools or MinGW

```bash
git clone https://github.com/yourname/oracle-exp-reader.git
cd oracle-exp-reader
cargo build --release
```

The binary is output to `target/release/oracle-exp-reader` (or `.exe` on Windows).

---

## Usage

### Show dump file info

```bash
oracle-exp-reader info dump.dmp
```

```
File          : dump.dmp
Size          : 524288 bytes (0.50 MB)
Export version: EXPORT:V11.02.00
Character set : JA16SJIS
Tables found  : 12
Total rows    : 48320
DDL statements: 37
```

### Extract DDL statements

```bash
oracle-exp-reader ddl dump.dmp > schema.sql
```

```sql
CREATE TABLE SCOTT.EMP (
    EMPNO NUMBER(4) NOT NULL,
    ENAME VARCHAR2(10),
    ...
);
```

### Export table data

```bash
# CSV format
oracle-exp-reader export dump.dmp -f csv

# JSON Lines format
oracle-exp-reader export dump.dmp -f json

# SQL INSERT statements
oracle-exp-reader export dump.dmp -f sql --batch 500

# Filter by table name (case-insensitive substring match)
oracle-exp-reader export dump.dmp -f csv -t EMP

# Limit to first 1000 rows
oracle-exp-reader export dump.dmp -f json --limit 1000
```

### Inspect raw records (diagnostic)

```bash
# List records
oracle-exp-reader records dump.dmp -n 50

# With hex dump
oracle-exp-reader records dump.dmp -n 10 --hex
```

```
    OFFSET                      TYPE      LENGTH  PREVIEW
----------------------------------------------------------------------
         0                FileHeader         512  EXPORT:V11.02.00....
       516              CharacterSet          10  JA16SJIS
       530          SessionParameters          8  ........
       542              DdlStatement         284  CREATE TABLE SCOTT.EMP
```

### Hex dump raw file bytes

```bash
oracle-exp-reader hexdump dump.dmp --offset 0x800 -n 256
```

---

## Command Reference

```
USAGE:
    oracle-exp-reader <COMMAND>

COMMANDS:
    info       Print dump file header metadata
    records    List all records with type, offset, and length
    ddl        Extract DDL statements (CREATE TABLE, etc.)
    export     Export table data to CSV, SQL, or JSON Lines
    hexdump    Print raw file bytes as a hex dump
    help       Print help information

export options:
    -f, --format <FORMAT>  Output format [csv|sql|json] (default: csv)
    -t, --table <TABLE>    Filter by table name (substring match)
        --limit <N>        Maximum rows per table (0 = unlimited)
        --batch <N>        INSERT statements per batch for SQL format (default: 1000)
```

---

## Library Usage

Add to `Cargo.toml`:

```toml
[dependencies]
oracle-exp-reader = { path = "path/to/oracle-exp-reader" }
```

```rust
use oracle_exp_reader::DumpReader;
use oracle_exp_reader::reader::DumpEvent;

fn main() -> anyhow::Result<()> {
    let reader = DumpReader::open("dump.dmp")?;

    println!("Version : {}", reader.header().export_version);
    println!("Charset : {}", reader.header().charset);

    for event in reader.events() {
        match event? {
            DumpEvent::DdlStatement { sql, .. } => {
                println!("DDL: {}", &sql[..sql.len().min(80)]);
            }
            DumpEvent::Row { table, row_index, raw_values, .. } => {
                println!("[{}] row {}: {} cols", table, row_index, raw_values.len());
            }
            DumpEvent::TableEnd { table, rows_written } => {
                println!("Table {} done: {} rows", table, rows_written);
            }
            DumpEvent::EndOfFile => break,
            _ => {}
        }
    }

    Ok(())
}
```

---

## Project Structure

```
src/
├── lib.rs       Library root and public API
├── types.rs     Record types, column types, and data structures
├── error.rs     Error types (thiserror)
├── parser.rs    Core binary parser (zero-copy)
├── reader.rs    High-level DumpReader API
├── output.rs    CSV / SQL / JSON Lines formatters
└── main.rs      CLI entry point
```

---

## Supported Oracle EXP Versions

| Oracle Version | Status |
|---|---|
| Oracle 8i | ✅ Supported |
| Oracle 9i | ✅ Supported |
| Oracle 10g | ✅ Supported |
| Oracle 11g | ✅ Supported |
| Oracle 12c and later | ⚠️ Untested (use `expdp` instead) |

> **Note:** This tool supports the classic `exp` format only. For `expdp` (Data Pump) dumps, use `impdp SQLFILE=out.sql` to extract DDL without importing.

---

## Supported Oracle Data Types

| Type | Decoded As |
|---|---|
| VARCHAR2 / CHAR | String (with NLS charset conversion) |
| NUMBER / FLOAT | Decimal string |
| DATE | `YYYY-MM-DD HH:MM:SS` |
| TIMESTAMP | `YYYY-MM-DD HH:MM:SS.nnnnnnnnn` |
| RAW / BLOB | Hex string (`0x...`) |
| CLOB | Text string |
| NULL | `NULL` |

---

## Performance

| Operation | Throughput |
|---|---|
| Record scanning only | 2–5 GB/s |
| DDL extraction | 1–3 GB/s |
| CSV export | 200–800 MB/s |
| SQL INSERT generation | 100–400 MB/s |

> Always build with `--release` for production use — it is 3–10× faster than a debug build.

```bash
cargo build --release
./target/release/oracle-exp-reader info dump.dmp
```

---

## Dependencies

| Crate | Purpose |
|---|---|
| `memmap2` | Memory-mapped file I/O |
| `encoding_rs` | NLS charset conversion |
| `clap` | CLI argument parsing |
| `thiserror` | Error type definitions |
| `serde_json` | JSON Lines output |
| `anyhow` | Error handling |

---

## License

MIT License

---

## Related Tools

- [Ora2Pg](https://github.com/darold/ora2pg) — Oracle to PostgreSQL migration tool
- Oracle `imp` — Official import utility (`SHOW=Y` option prints DDL without importing)
