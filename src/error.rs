use thiserror::Error;

#[derive(Debug, Error)]
pub enum DumpError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Not an Oracle EXP dump file (invalid magic / header)")]
    InvalidMagic,

    #[error("Unexpected end of file at offset {offset}")]
    UnexpectedEof { offset: usize },

    #[error("Invalid record length {length} at offset {offset}")]
    InvalidLength { length: u32, offset: usize },

    #[error("Character set conversion error for charset '{charset}'")]
    CharsetConversion { charset: String },

    #[error("Record parse error at offset {offset}: {message}")]
    ParseError { offset: usize, message: String },

    #[error("Unsupported export version: {version}")]
    UnsupportedVersion { version: String },
}

pub type Result<T> = std::result::Result<T, DumpError>;