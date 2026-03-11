/// Oracle EXP dump file record types (reverse-engineered from Oracle dump format)
///
/// Oracle exp creates a binary dump with fixed-size records:
///   [2 bytes: record type (big-endian)] [2 bytes: data length (big-endian)] [N bytes: data]
///
/// Special case: if length == 0xFFFF, the actual length is in the next 4 bytes (big-endian u32).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum RecordType {
    /// File header — first record, contains "EXPORT:Vxx.xx.xx" version string
    FileHeader = 1,
    /// Database character set / NLS info
    CharacterSet = 2,
    /// Export session parameters (NLS_LANGUAGE etc.)
    SessionParameters = 3,
    /// DDL statement (CREATE TABLE, CREATE INDEX, GRANT, etc.)
    DdlStatement = 6,
    /// Table data start marker — signals beginning of table row data
    TableDataStart = 7,
    /// Row data chunk — contains one or more serialized column values
    RowData = 8,
    /// Table data end marker
    TableDataEnd = 9,
    /// Schema / user info
    SchemaInfo = 11,
    /// Table definition metadata (columns, constraints)
    TableDefinition = 12,
    /// Column definition within a table
    ColumnDefinition = 13,
    /// End of dump file
    EndOfFile = 14,
    /// LOB data segment
    LobData = 15,
    /// Cluster definition
    ClusterDefinition = 16,
    /// Index definition
    IndexDefinition = 17,
    /// View definition
    ViewDefinition = 18,
    /// Trigger definition
    TriggerDefinition = 19,
    /// Procedure / function body
    ProcedureBody = 20,
    /// Sequence definition
    SequenceDefinition = 21,
    /// Synonym definition
    SynonymDefinition = 22,
    /// Database link
    DatabaseLink = 23,
    /// Snapshot / materialized view definition
    SnapshotDefinition = 24,
    /// User/role definitions
    UserDefinition = 25,
    /// Tablespace info
    TablespaceInfo = 26,
    /// Comment on object
    Comment = 27,
    /// Audit settings
    AuditSettings = 28,
    /// Unknown / unsupported record
    Unknown(u16),
}

impl From<u16> for RecordType {
    fn from(v: u16) -> Self {
        match v {
            1 => Self::FileHeader,
            2 => Self::CharacterSet,
            3 => Self::SessionParameters,
            6 => Self::DdlStatement,
            7 => Self::TableDataStart,
            8 => Self::RowData,
            9 => Self::TableDataEnd,
            11 => Self::SchemaInfo,
            12 => Self::TableDefinition,
            13 => Self::ColumnDefinition,
            14 => Self::EndOfFile,
            15 => Self::LobData,
            16 => Self::ClusterDefinition,
            17 => Self::IndexDefinition,
            18 => Self::ViewDefinition,
            19 => Self::TriggerDefinition,
            20 => Self::ProcedureBody,
            21 => Self::SequenceDefinition,
            22 => Self::SynonymDefinition,
            23 => Self::DatabaseLink,
            24 => Self::SnapshotDefinition,
            25 => Self::UserDefinition,
            26 => Self::TablespaceInfo,
            27 => Self::Comment,
            28 => Self::AuditSettings,
            other => Self::Unknown(other),
        }
    }
}

impl std::fmt::Display for RecordType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::FileHeader => "FileHeader",
            Self::CharacterSet => "CharacterSet",
            Self::SessionParameters => "SessionParameters",
            Self::DdlStatement => "DdlStatement",
            Self::TableDataStart => "TableDataStart",
            Self::RowData => "RowData",
            Self::TableDataEnd => "TableDataEnd",
            Self::SchemaInfo => "SchemaInfo",
            Self::TableDefinition => "TableDefinition",
            Self::ColumnDefinition => "ColumnDefinition",
            Self::EndOfFile => "EndOfFile",
            Self::LobData => "LobData",
            Self::ClusterDefinition => "ClusterDefinition",
            Self::IndexDefinition => "IndexDefinition",
            Self::ViewDefinition => "ViewDefinition",
            Self::TriggerDefinition => "TriggerDefinition",
            Self::ProcedureBody => "ProcedureBody",
            Self::SequenceDefinition => "SequenceDefinition",
            Self::SynonymDefinition => "SynonymDefinition",
            Self::DatabaseLink => "DatabaseLink",
            Self::SnapshotDefinition => "SnapshotDefinition",
            Self::UserDefinition => "UserDefinition",
            Self::TablespaceInfo => "TablespaceInfo",
            Self::Comment => "Comment",
            Self::AuditSettings => "AuditSettings",
            Self::Unknown(n) => return write!(f, "Unknown({})", n),
        };
        write!(f, "{}", name)
    }
}

/// A raw record read from the dump file
#[derive(Debug, Clone)]
pub struct RawRecord<'a> {
    pub record_type: RecordType,
    /// Byte offset within the dump file
    pub offset: usize,
    /// Raw data payload (zero-copy slice into the memory-mapped file)
    pub data: &'a [u8],
}

/// Oracle column data types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OracleType {
    Varchar2,
    Nvarchar2,
    Number,
    Long,
    Date,
    Raw,
    LongRaw,
    Char,
    Nchar,
    Clob,
    Nclob,
    Blob,
    Bfile,
    Timestamp,
    TimestampWithTZ,
    TimestampWithLocalTZ,
    IntervalYearToMonth,
    IntervalDayToSecond,
    Float,
    RowId,
    URowId,
    XmlType,
    Other(u8),
}

impl From<u8> for OracleType {
    fn from(code: u8) -> Self {
        match code {
            1 => Self::Varchar2,
            2 => Self::Nvarchar2,
            3 => Self::Number,
            8 => Self::Long,
            12 => Self::Date,
            23 => Self::Raw,
            24 => Self::LongRaw,
            96 => Self::Char,
            100 => Self::Float,
            101 => Self::Float,
            112 => Self::Clob,
            113 => Self::Blob,
            114 => Self::Bfile,
            180 => Self::Timestamp,
            181 => Self::TimestampWithTZ,
            182 => Self::IntervalYearToMonth,
            183 => Self::IntervalDayToSecond,
            186 => Self::TimestampWithLocalTZ,
            208 => Self::URowId,
            other => Self::Other(other),
        }
    }
}

impl std::fmt::Display for OracleType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Varchar2 => "VARCHAR2",
            Self::Nvarchar2 => "NVARCHAR2",
            Self::Number => "NUMBER",
            Self::Long => "LONG",
            Self::Date => "DATE",
            Self::Raw => "RAW",
            Self::LongRaw => "LONG RAW",
            Self::Char => "CHAR",
            Self::Nchar => "NCHAR",
            Self::Clob => "CLOB",
            Self::Nclob => "NCLOB",
            Self::Blob => "BLOB",
            Self::Bfile => "BFILE",
            Self::Timestamp => "TIMESTAMP",
            Self::TimestampWithTZ => "TIMESTAMP WITH TIME ZONE",
            Self::TimestampWithLocalTZ => "TIMESTAMP WITH LOCAL TIME ZONE",
            Self::IntervalYearToMonth => "INTERVAL YEAR TO MONTH",
            Self::IntervalDayToSecond => "INTERVAL DAY TO SECOND",
            Self::Float => "FLOAT",
            Self::RowId => "ROWID",
            Self::URowId => "UROWID",
            Self::XmlType => "XMLTYPE",
            Self::Other(n) => return write!(f, "UNKNOWN({})", n),
        };
        write!(f, "{}", s)
    }
}

/// Parsed column value
#[derive(Debug, Clone)]
pub enum ColumnValue {
    Null,
    Text(String),
    Bytes(Vec<u8>),
    Number(String),
}

impl std::fmt::Display for ColumnValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => write!(f, "NULL"),
            Self::Text(s) => write!(f, "{}", s),
            Self::Number(n) => write!(f, "{}", n),
            Self::Bytes(b) => {
                write!(f, "0x")?;
                for byte in b {
                    write!(f, "{:02X}", byte)?;
                }
                Ok(())
            }
        }
    }
}

/// Metadata for a single column
#[derive(Debug, Clone)]
pub struct ColumnMeta {
    pub name: String,
    pub col_type: OracleType,
    pub length: u16,
    pub precision: u8,
    pub scale: u8,
    pub nullable: bool,
}

/// Metadata for a table
#[derive(Debug, Clone)]
pub struct TableMeta {
    pub owner: String,
    pub name: String,
    pub columns: Vec<ColumnMeta>,
}

/// Header info parsed from the dump file
#[derive(Debug, Clone, Default)]
pub struct DumpHeader {
    pub export_version: String,
    pub oracle_version: String,
    pub export_date: String,
    pub charset: String,
    pub ncharset: String,
    pub platform: String,
    pub export_mode: String, // FULL, OWNER, TABLE
}

/// Parsed row from a table
#[derive(Debug, Clone)]
pub struct TableRow {
    pub values: Vec<ColumnValue>,
}
