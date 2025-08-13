#[allow(dead_code)]
mod ai_analyzer;
mod ai_text_analyzer;
pub mod api;
pub mod config;
#[allow(dead_code)]
pub mod diff;
mod pdf_converter;
pub mod router;
#[allow(dead_code)]
mod sam;
mod workflow;

use std::{collections::HashMap, env::current_exe, sync::LazyLock};

use thiserror::Error;

use crate::diff::ModelJson;

pub type IResult<T> = std::result::Result<T, AnalyzerError>;
// 初始化一个排序好的模具比较数据
pub static MODELS: LazyLock<HashMap<String, Vec<ModelJson>>> = LazyLock::new(|| {
    let models_dir = current_exe()
        .map_err(|e| format!("获取执行目录失败: {}", e))
        .unwrap()
        .parent()
        .ok_or("无法获取执行目录的父目录")
        .unwrap()
        .join("models")
        .join("jsons");
    let models = ModelJson::patch_new(models_dir).unwrap();
    ModelJson::sort(models)
});


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
