pub mod api;
mod pdf_converter;
pub mod router;
#[allow(dead_code)]
mod sam;
pub mod config;
#[allow(dead_code)]
mod ai_analyzer;
mod ai_text_analyzer;
#[allow(dead_code)]
mod diff;
mod workflow;

pub type IResult<T> = std::result::Result<T, AnalyzerError>;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AnalyzerError {
    #[error("PDF processing error: {0}")]
    PdfError(String),

    #[error("Image processing error: {0}")]
    ImageError(String),

    #[error("AI analysis error: {0}")]
    AiError(String),

    #[error("SAM interface error: {0}")]
    SamError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Python FFI error: {0}")]
    PythonError(String),

    #[error("Workflow error: {0}")]
    WorkflowError(String),
}
