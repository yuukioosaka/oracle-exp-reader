use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use oracle_exp_reader::{
    DumpReader,
    reader::DumpEvent,
    output::{CsvWriter, SqlInsertWriter, JsonLinesWriter, hex_dump},
};

#[derive(Parser)]
#[command(name = "oracle-exp-reader", about = "High-performance Oracle EXP dump file parser", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Print metadata from the dump file header
    Info { file: PathBuf },
    /// List all records (diagnostic)
    Records {
        file: PathBuf,
        #[arg(short = 'n', long, default_value = "100")]
        limit: usize,
        #[arg(long)]
        hex: bool,
    },
    /// Extract DDL statements
    Ddl { file: PathBuf },
    /// Export table data
    Export {
        file: PathBuf,
        #[arg(short, long, value_enum, default_value = "csv")]
        format: OutputFormat,
        #[arg(short, long)]
        table: Option<String>,
        #[arg(long, default_value = "0")]
        limit: u64,
        #[arg(long, default_value = "1000")]
        batch: usize,
    },
    /// Hex dump of raw file bytes
    Hexdump {
        file: PathBuf,
        #[arg(long, default_value = "0")]
        offset: String,
        #[arg(short = 'n', long, default_value = "256")]
        length: usize,
    },
}

#[derive(Clone, ValueEnum)]
enum OutputFormat { Csv, Sql, Json }

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    match cli.command {
        Commands::Info { file } => cmd_info(&file, &mut out)?,
        Commands::Records { file, limit, hex } => cmd_records(&file, limit, hex, &mut out)?,
        Commands::Ddl { file } => cmd_ddl(&file, &mut out)?,
        Commands::Export { file, format, table, limit, batch } => {
            cmd_export(&file, format, table.as_deref(), limit, batch, &mut out)?
        }
        Commands::Hexdump { file, offset, length } => {
            let offset = parse_offset(&offset)?;
            cmd_hexdump(&file, offset, length, &mut out)?;
        }
    }

    out.flush()?;
    Ok(())
}

fn cmd_info<W: Write>(path: &PathBuf, out: &mut W) -> anyhow::Result<()> {
    let reader = DumpReader::open(path)?;
    let h = reader.header();
    let size = reader.file_size();

    writeln!(out, "File          : {}", path.display())?;
    writeln!(out, "Size          : {} bytes ({:.2} MB)", size, size as f64 / 1_048_576.0)?;
    writeln!(out, "Export version: {}", h.export_version)?;
    writeln!(out, "Oracle version: {}", h.oracle_version)?;
    writeln!(out, "Character set : {}", h.charset)?;
    writeln!(out, "NChar charset : {}", h.ncharset)?;
    writeln!(out, "Export date   : {}", h.export_date)?;

    let mut tables = 0u64;
    let mut rows = 0u64;
    let mut ddl_count = 0u64;
    for event in reader.events() {
        match event? {
            DumpEvent::TableStart { .. } => tables += 1,
            DumpEvent::Row { .. } => rows += 1,
            DumpEvent::DdlStatement { .. } => ddl_count += 1,
            DumpEvent::EndOfFile => break,
            _ => {}
        }
    }
    writeln!(out, "\nTables found  : {}", tables)?;
    writeln!(out, "Total rows    : {}", rows)?;
    writeln!(out, "DDL statements: {}", ddl_count)?;
    Ok(())
}

fn cmd_records<W: Write>(path: &PathBuf, limit: usize, show_hex: bool, out: &mut W) -> anyhow::Result<()> {
    let reader = DumpReader::open(path)?;
    writeln!(out, "{:>10}  {:>25}  {:>10}  {}", "OFFSET", "TYPE", "LENGTH", "PREVIEW")?;
    writeln!(out, "{}", "-".repeat(70))?;

    for (i, rec) in reader.raw_records().enumerate() {
        if i >= limit {
            writeln!(out, "... (limit {} reached)", limit)?;
            break;
        }
        let rec = rec?;
        let preview: String = rec.data.iter()
            .take(40)
            .map(|&b| if b >= 0x20 && b < 0x7F { b as char } else { '.' })
            .collect();
        writeln!(out, "{:>10}  {:>25}  {:>10}  {}", rec.offset, format!("{}", rec.record_type), rec.data.len(), preview)?;
        if show_hex && !rec.data.is_empty() {
            hex_dump(out, &rec.data[..rec.data.len().min(64)], 0)?;
        }
    }
    Ok(())
}

fn cmd_ddl<W: Write>(path: &PathBuf, out: &mut W) -> anyhow::Result<()> {
    let reader = DumpReader::open(path)?;
    for event in reader.events() {
        match event? {
            DumpEvent::DdlStatement { sql, .. } => {
                let clean = sql.trim();
                if !clean.is_empty() {
                    writeln!(out, "{};", clean.trim_end_matches(';'))?;
                    writeln!(out)?;
                }
            }
            DumpEvent::EndOfFile => break,
            _ => {}
        }
    }
    Ok(())
}

fn cmd_export<W: Write>(path: &PathBuf, format: OutputFormat, table_filter: Option<&str>, row_limit: u64, batch: usize, out: &mut W) -> anyhow::Result<()> {
    let reader = DumpReader::open(path)?;
    match format {
        OutputFormat::Csv => {
            let mut w = CsvWriter::new(out);
            run_events(&reader, table_filter, row_limit, |ev| w.handle_event(ev).map_err(|e| anyhow::anyhow!("{}", e)))?;
        }
        OutputFormat::Sql => {
            let mut w = SqlInsertWriter::new(out, batch);
            run_events(&reader, table_filter, row_limit, |ev| w.handle_event(ev).map_err(|e| anyhow::anyhow!("{}", e)))?;
        }
        OutputFormat::Json => {
            let mut w = JsonLinesWriter::new(out);
            run_events(&reader, table_filter, row_limit, |ev| w.handle_event(ev).map_err(|e| anyhow::anyhow!("{}", e)))?;
        }
    }
    Ok(())
}

fn run_events<F>(reader: &DumpReader, table_filter: Option<&str>, row_limit: u64, mut handler: F) -> anyhow::Result<()>
where F: FnMut(&DumpEvent) -> anyhow::Result<()> {
    let filter_upper = table_filter.map(|s| s.to_uppercase());
    let mut in_matching_table = false;
    let mut rows_in_table: u64 = 0;

    for event in reader.events() {
        let event = event?;
        match &event {
            DumpEvent::TableStart { table_name, .. } => {
                in_matching_table = filter_upper.as_ref()
                    .map(|f| table_name.to_uppercase().contains(f.as_str()))
                    .unwrap_or(true);
                rows_in_table = 0;
                if in_matching_table { handler(&event)?; }
            }
            DumpEvent::Row { .. } => {
                if in_matching_table {
                    if row_limit > 0 && rows_in_table >= row_limit { continue; }
                    rows_in_table += 1;
                    handler(&event)?;
                }
            }
            DumpEvent::TableEnd { .. } => {
                if in_matching_table { handler(&event)?; }
                in_matching_table = false;
            }
            DumpEvent::DdlStatement { .. } if table_filter.is_none() => { handler(&event)?; }
            DumpEvent::EndOfFile => { handler(&event)?; break; }
            _ => { if table_filter.is_none() { handler(&event)?; } }
        }
    }
    Ok(())
}

fn cmd_hexdump<W: Write>(path: &PathBuf, offset: usize, length: usize, out: &mut W) -> anyhow::Result<()> {
    let data = std::fs::read(path)?;
    let end = (offset + length).min(data.len());
    if offset >= data.len() {
        eprintln!("Offset 0x{:X} is beyond file size {}", offset, data.len());
        return Ok(());
    }
    hex_dump(out, &data[offset..end], offset)?;
    Ok(())
}

fn parse_offset(s: &str) -> anyhow::Result<usize> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Ok(usize::from_str_radix(hex, 16)?)
    } else {
        Ok(s.parse()?)
    }
}
