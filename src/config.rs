use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamConfig {
    /// sam python script path
    pub python_script_path: PathBuf,
    /// sam model path
    pub model_path: PathBuf,
    /// model type (vit_b, vit_l, vit_h)
    pub model_type: String,
}

impl Default for SamConfig {
    fn default() -> Self {
        let current_dir = std::env::current_dir().expect("Failed to get current directory");
        let python_script_path = current_dir.join("scripts").join("sam_split_png.py");
        let model_path = current_dir.join("scripts").join("sam_vit_b.pth");
        let model_type = "vit_b".to_string();

        Self {
            python_script_path,
            model_path,
            model_type,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    /// API key for cloud service
    pub api_key: String,
    /// API endpoint
    pub endpoint: String,
    /// Model name for API analysis
    pub model_name: String,
    /// Use compatible mode (OpenAI format) or native DashScope format
    pub use_compatible_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    /// Ollama base URL
    pub ollama_base: String,
    /// Model name for local analysis
    pub local_model: String,
    /// API configuration for cloud analysis
    pub api: Option<ApiConfig>,
    /// Enable fast mode
    pub fast_mode: bool,
    /// Maximum retries for analysis
    pub max_retries: u32,
    /// Request timeout in seconds
    pub timeout_seconds: u64,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            ollama_base: "http://localhost:11434".to_string(),
            local_model: "qwen2.5vl:7b".to_string(),
            api: Some(ApiConfig {
                api_key: "sk-c725815640934b548a829fc8be7a4ce5".to_string(),
                endpoint: "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string(),
                model_name: "qwen-vl-max".to_string(),
                use_compatible_mode: true,
            }),
            fast_mode: false,
            max_retries: 3,
            timeout_seconds: 300,
        }
    }
}
