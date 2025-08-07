use crate::{AnalyzerError, IResult, config::AiConfig};
use base64::{Engine as _, engine::general_purpose};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::time::{Duration, timeout};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ViewAnalysis {
    Model(ModelAnalysis),
    Info(InfoAnalysis),
    Error(ErrAnalysis),
}

impl From<ErrAnalysis> for ViewAnalysis {
    fn from(err: ErrAnalysis) -> Self {
        ViewAnalysis::Error(err)
    }
}

impl From<ModelAnalysis> for ViewAnalysis {
    fn from(model: ModelAnalysis) -> Self {
        ViewAnalysis::Model(model)
    }
}

impl From<InfoAnalysis> for ViewAnalysis {
    fn from(info: InfoAnalysis) -> Self {
        ViewAnalysis::Info(info)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelAnalysis {
    pub image_path: PathBuf,
    pub view_category: String,
    pub view_type: String,
    pub x_max: Option<f64>,
    pub y_max: Option<f64>,
    pub x_tolerance: Option<String>,
    pub y_tolerance: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrAnalysis {
    pub image_path: PathBuf,
    pub error_message: String,
    pub attempt_number: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfoAnalysis {
    pub image_path: PathBuf,
    pub part_info: Option<PartInfo>,
    pub company: Option<String>,
    pub text_content: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartInfo {
    pub name: Option<String>,
    pub material: Option<String>,
    pub scale: Option<String>,
    pub drawing_number: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisIResult {
    pub successful_analyses: u32,
    pub failed_analyses: u32,
    pub total_views: u32,
    pub engineering_views: u32,
    pub info_views: u32,
    pub views: Vec<ViewAnalysis>,
    pub dimensions: DimensionSummary,
    pub anomalies: AnomalyReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionSummary {
    pub x_max: Option<f64>,
    pub y_max: Option<f64>,
    pub x_values: Vec<f64>,
    pub y_values: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyReport {
    pub x_mistake_value: Option<f64>,
    pub y_mistake_value: Option<f64>,
    pub corrected_x_max: Option<f64>,
    pub corrected_y_max: Option<f64>,
    pub gap_analysis: Option<GapAnalysis>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapAnalysis {
    pub total_values: usize,
    pub top_3_values: Vec<f64>,
    pub gap1: f64,
    pub gap2: f64,
    pub gap_ratio: f64,
    pub method: String,
}

pub struct AiAnalyzer {
    config: AiConfig,
    client: reqwest::Client,
}

impl AiAnalyzer {
    pub fn new(config: AiConfig) -> Self {
        let client = reqwest::Client::new();
        Self { config, client }
    }

    /// Create analysis prompt for vision model
    fn create_view_prompt(&self) -> String {
        r#"
分析这个工程图纸视图，判断类型并提取关键信息。

**核心任务：**
1. 如果是信息视图（标题栏等），提取零件信息
2. 如果是工程视图，找到X轴和Y轴方向的最大尺寸值

**关键理解：**
- X轴 = 水平方向的最大尺寸
- Y轴 = 垂直方向的最大尺寸
请专注识别尺寸线方向，并且文字的方向和x轴，y轴方向一致。
- 公差一般以"±"符号表示，跟在尺寸值后面。

**JSON格式：**

对于信息视图（标题栏、材料清单、技术要求等）：
```json
{
    "view_category": "info",
    "view_type": "标题栏|材料清单|技术要求|尺寸表",
    "part_info": {
        "name": "零件名称",
        "material": "材料代码",
        "scale": "图纸比例",
        "drawing_number": "图纸编号"
    },
    "company": "公司名称，如果有，否则为null",
    "text_content": ["所有可见的重要文字"]
}
```

工程视图：
```json
{
    "view_category": "engineering",
    "view_type": "主视图|俯视图|剖视图|详细视图",
    "x_max": 水平方向最大尺寸值,
    "y_max": 垂直方向最大尺寸值,
    "x_tolerance": "公差或null",
    "y_tolerance": "公差或null"
}
```
"#
        .to_string()
    }

    /// Encode image to base64 for AI analysis
    async fn encode_image_for_analysis<P: AsRef<Path>>(&self, image_path: P) -> IResult<String> {
        let image_path = image_path.as_ref();

        // 读取并处理图像
        let img = image::open(image_path)
            .map_err(|e| AnalyzerError::ImageError(format!("Failed to open image: {}", e)))?;

        // 根据fast_mode调整图像大小和质量
        let (max_size, quality) = if self.config.fast_mode {
            (1024, 75)
        } else {
            (2048, 85)
        };

        let img = if img.width().max(img.height()) > max_size {
            info!(
                "Resizing image from {}x{} to max size {}",
                img.width(),
                img.height(),
                max_size
            );
            img.resize(max_size, max_size, image::imageops::FilterType::Lanczos3)
        } else {
            img
        };

        // 转换为RGB并编码为JPEG
        let rgb_img = img.to_rgb8();
        let mut jpeg_data = Vec::new();

        {
            use image::codecs::jpeg::JpegEncoder;
            let mut encoder = JpegEncoder::new_with_quality(&mut jpeg_data, quality);
            encoder
                .encode(
                    rgb_img.as_raw(),
                    rgb_img.width(),
                    rgb_img.height(),
                    image::ColorType::Rgb8.into(),
                )
                .map_err(|e| AnalyzerError::ImageError(format!("Failed to encode JPEG: {}", e)))?;
        }

        Ok(general_purpose::STANDARD.encode(&jpeg_data))
    }

    /// Analyze single view using Ollama local model
    pub async fn analyze_single_view_local<P: AsRef<Path>>(
        &self,
        image_path: P,
    ) -> IResult<ViewAnalysis> {
        let image_path = image_path.as_ref();
        info!("Analyzing view: {}", image_path.display());

        for attempt in 1..=self.config.max_retries {
            if attempt > 1 {
                info!("Retry attempt {} for {}", attempt, image_path.display());
            }

            match self.try_analyze_local(image_path, attempt).await {
                Ok(analysis) => return Ok(analysis),
                Err(e) if attempt < self.config.max_retries => {
                    warn!("Analysis attempt {} failed: {}, retrying...", attempt, e);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                Err(e) => {
                    error!(
                        "All analysis attempts failed for {}: {}",
                        image_path.display(),
                        e
                    );
                    return Ok(ErrAnalysis {
                        image_path: image_path.to_path_buf(),
                        error_message: e.to_string(),
                        attempt_number: attempt,
                    }
                    .into());
                }
            }
        }

        unreachable!()
    }

    /// Analyze single view using remote API (DashScope)
    pub async fn analyze_single_view_api<P: AsRef<Path>>(
        &self,
        image_path: P,
    ) -> IResult<ViewAnalysis> {
        let image_path = image_path.as_ref();
        info!("Analyzing view with remote API: {}", image_path.display());

        let api_config = self
            .config
            .api
            .as_ref()
            .ok_or_else(|| AnalyzerError::AiError("API configuration not found".to_string()))?;

        for attempt in 1..=self.config.max_retries {
            if attempt > 1 {
                info!("Retry attempt {} for {}", attempt, image_path.display());
            }

            match self.try_analyze_api(image_path, attempt, api_config).await {
                Ok(analysis) => return Ok(analysis),
                Err(e) if attempt < self.config.max_retries => {
                    warn!(
                        "API analysis attempt {} failed: {}, retrying...",
                        attempt, e
                    );
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                Err(e) => {
                    error!(
                        "All API analysis attempts failed for {}: {}",
                        image_path.display(),
                        e
                    );
                    return Ok(ErrAnalysis {
                        image_path: image_path.to_path_buf(),
                        error_message: e.to_string(),
                        attempt_number: attempt,
                    }
                    .into());
                }
            }
        }

        unreachable!()
    }

    async fn try_analyze_local<P: AsRef<Path>>(
        &self,
        image_path: P,
        attempt: u32,
    ) -> IResult<ViewAnalysis> {
        let image_path = image_path.as_ref();

        // 编码图像
        let image_base64 = self.encode_image_for_analysis(image_path).await?;

        // 准备请求数据
        let payload = serde_json::json!({
            "model": self.config.local_model,
            "prompt": self.create_view_prompt(),
            "images": [image_base64],
            "stream": false,
            "options": {
                "temperature": 0.1,
                "num_ctx": 4096,
                "num_predict": 512,
                "num_thread": 8,
                "repeat_penalty": 1.1
            }
        });

        debug!("Sending request to Ollama...");

        // 发送请求到Ollama
        let url = format!("{}/api/generate", self.config.ollama_base);
        let response = timeout(
            Duration::from_secs(self.config.timeout_seconds),
            self.client.post(&url).json(&payload).send(),
        )
        .await
        .map_err(|_| AnalyzerError::AiError("Request timeout".to_string()))?
        .map_err(|e| AnalyzerError::AiError(format!("HTTP request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(AnalyzerError::AiError(format!(
                "API request failed with status: {}",
                response.status()
            )));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AnalyzerError::AiError(format!("Failed to parse response: {}", e)))?;

        let content = response_json
            .get("response")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AnalyzerError::AiError("No response content".to_string()))?;

        debug!("Response length: {} characters", content.len());

        // 解析JSON响应
        let parsed_result = self.parse_ai_response(content)?;

        // 清理占位符值
        let cleaned_result = self.clean_extracted_values(parsed_result);

        // 构建ViewAnalysis
        let view_category = cleaned_result
            .get("view_category")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let analysis = match view_category {
            "engineering" => {
                let model_analysis = ModelAnalysis {
                    image_path: image_path.to_path_buf(),
                    view_category: cleaned_result
                        .get("view_category")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    view_type: cleaned_result
                        .get("view_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    x_max: cleaned_result.get("x_max").and_then(|v| v.as_f64()),
                    y_max: cleaned_result.get("y_max").and_then(|v| v.as_f64()),
                    x_tolerance: cleaned_result
                        .get("x_tolerance")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    y_tolerance: cleaned_result
                        .get("y_tolerance")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                };
                ViewAnalysis::Model(model_analysis)
            }
            "info" => {
                let mut part_info = None;

                // 处理part_info
                if let Some(part_info_val) = cleaned_result.get("part_info") {
                    if let Some(part_obj) = part_info_val.as_object() {
                        part_info = Some(PartInfo {
                            name: part_obj
                                .get("name")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            material: part_obj
                                .get("material")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            scale: part_obj
                                .get("scale")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            drawing_number: part_obj
                                .get("drawing_number")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                        });
                    }
                }

                let info_analysis = InfoAnalysis {
                    image_path: image_path.to_path_buf(),
                    part_info,
                    company: cleaned_result
                        .get("company")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    text_content: cleaned_result
                        .get("text_content")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str())
                                .map(|s| s.to_string())
                                .collect()
                        }),
                };
                ViewAnalysis::Info(info_analysis)
            }
            _ => {
                // 未知类型，创建错误分析
                let err_analysis = ErrAnalysis {
                    image_path: image_path.to_path_buf(),
                    error_message: format!("Unknown view category: {}", view_category),
                    attempt_number: attempt,
                };
                ViewAnalysis::Error(err_analysis)
            }
        };

        // 打印分析结果
        match &analysis {
            ViewAnalysis::Model(model) => {
                info!("✅ Engineering view: {}", model.view_type);
                if let Some(x_max) = model.x_max {
                    info!(
                        "   X-axis max: {}mm{}",
                        x_max,
                        model.x_tolerance.as_deref().unwrap_or("")
                    );
                }
                if let Some(y_max) = model.y_max {
                    info!(
                        "   Y-axis max: {}mm{}",
                        y_max,
                        model.y_tolerance.as_deref().unwrap_or("")
                    );
                }
            }
            ViewAnalysis::Info(info) => {
                info!("✅ Info view");
                if let Some(part_info) = &info.part_info {
                    if let Some(name) = &part_info.name {
                        info!("   Part name: {}", name);
                    }
                    if let Some(material) = &part_info.material {
                        info!("   Material: {}", material);
                    }
                    if let Some(drawing_number) = &part_info.drawing_number {
                        info!("   Drawing number: {}", drawing_number);
                    }
                }
                if let Some(company) = &info.company {
                    info!("   Company: {}", company);
                }
            }
            ViewAnalysis::Error(err) => {
                error!("❌ Analysis error: {}", err.error_message);
            }
        }

        Ok(analysis)
    }

    async fn try_analyze_api<P: AsRef<Path>>(
        &self,
        image_path: P,
        attempt: u32,
        api_config: &crate::config::ApiConfig,
    ) -> IResult<ViewAnalysis> {
        let image_path = image_path.as_ref();

        // 编码图像
        let image_base64 = self.encode_image_for_analysis(image_path).await?;

        // 根据配置选择API格式
        let (payload, url) = if api_config.use_compatible_mode {
            // OpenAI兼容格式
            let payload = serde_json::json!({
                "model": api_config.model_name,
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": self.create_view_prompt()
                            },
                            {
                                "type": "image_url",
                                "image_url": {
                                    "url": format!("data:image/jpeg;base64,{}", image_base64)
                                }
                            }
                        ]
                    }
                ],
                "temperature": 0.1,
                "max_tokens": 1000,
                "stream": false
            });
            let url = format!("{}/chat/completions", api_config.endpoint);
            (payload, url)
        } else {
            // DashScope原生格式
            let payload = serde_json::json!({
                "model": api_config.model_name,
                "input": {
                    "messages": [
                        {
                            "role": "user",
                            "content": [
                                {
                                    "text": self.create_view_prompt()
                                },
                                {
                                    "image": format!("data:image/jpeg;base64,{}", image_base64)
                                }
                            ]
                        }
                    ]
                },
                "parameters": {
                    "result_format": "message",
                    "temperature": 0.1,
                    "max_tokens": 1000
                }
            });
            let url = format!(
                "{}/services/aigc/text-generation/generation",
                api_config.endpoint.replace("/compatible-mode/v1", "")
            );
            (payload, url)
        };

        debug!("Sending request to remote API...");
        debug!("API URL: {}", url);
        debug!("Model: {}", api_config.model_name);
        debug!("Compatible mode: {}", api_config.use_compatible_mode);
        debug!(
            "Request payload: {}",
            serde_json::to_string_pretty(&payload)
                .unwrap_or_else(|_| "Failed to serialize payload".to_string())
        );

        // 发送请求到远程API
        let response = timeout(
            Duration::from_secs(self.config.timeout_seconds),
            self.client
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_config.api_key))
                .header("Content-Type", "application/json")
                .json(&payload)
                .send(),
        )
        .await
        .map_err(|_| AnalyzerError::AiError("API request timeout".to_string()))?
        .map_err(|e| AnalyzerError::AiError(format!("API HTTP request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            debug!("Full API error response: {}", error_text);
            return Err(AnalyzerError::AiError(format!(
                "API request failed with status {}: {}",
                status, error_text
            )));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AnalyzerError::AiError(format!("Failed to parse API response: {}", e)))?;

        debug!(
            "Full API response: {}",
            serde_json::to_string_pretty(&response_json)
                .unwrap_or_else(|_| "Failed to serialize response".to_string())
        );

        // 根据API格式解析响应
        let content = if api_config.use_compatible_mode {
            // OpenAI兼容格式
            response_json
                .get("choices")
                .and_then(|choices| choices.as_array())
                .and_then(|arr| arr.first())
                .and_then(|choice| choice.get("message"))
                .and_then(|message| message.get("content"))
                .and_then(|content| content.as_str())
        } else {
            // DashScope原生格式
            response_json
                .get("output")
                .and_then(|output| output.get("text"))
                .and_then(|text| text.as_str())
        }
        .ok_or_else(|| AnalyzerError::AiError("No content in API response".to_string()))?;

        debug!("API response length: {} characters", content.len());

        // 解析JSON响应
        let parsed_result = self.parse_ai_response(content)?;

        // 清理占位符值
        let cleaned_result = self.clean_extracted_values(parsed_result);

        // 构建ViewAnalysis (使用与本地分析相同的逻辑)
        let view_category = cleaned_result
            .get("view_category")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let analysis = match view_category {
            "engineering" => {
                let model_analysis = ModelAnalysis {
                    image_path: image_path.to_path_buf(),
                    view_category: cleaned_result
                        .get("view_category")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    view_type: cleaned_result
                        .get("view_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    x_max: cleaned_result.get("x_max").and_then(|v| v.as_f64()),
                    y_max: cleaned_result.get("y_max").and_then(|v| v.as_f64()),
                    x_tolerance: cleaned_result
                        .get("x_tolerance")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    y_tolerance: cleaned_result
                        .get("y_tolerance")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                };
                ViewAnalysis::Model(model_analysis)
            }
            "info" => {
                let mut part_info = None;

                // 处理part_info
                if let Some(part_info_val) = cleaned_result.get("part_info") {
                    if let Some(part_obj) = part_info_val.as_object() {
                        part_info = Some(PartInfo {
                            name: part_obj
                                .get("name")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            material: part_obj
                                .get("material")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            scale: part_obj
                                .get("scale")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            drawing_number: part_obj
                                .get("drawing_number")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                        });
                    }
                }

                let info_analysis = InfoAnalysis {
                    image_path: image_path.to_path_buf(),
                    part_info,
                    company: cleaned_result
                        .get("company")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    text_content: cleaned_result
                        .get("text_content")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str())
                                .map(|s| s.to_string())
                                .collect()
                        }),
                };
                ViewAnalysis::Info(info_analysis)
            }
            _ => {
                // 未知类型，创建错误分析
                let err_analysis = ErrAnalysis {
                    image_path: image_path.to_path_buf(),
                    error_message: format!("Unknown view category: {}", view_category),
                    attempt_number: attempt,
                };
                ViewAnalysis::Error(err_analysis)
            }
        };

        // 打印分析结果 (与本地分析相同的逻辑)
        match &analysis {
            ViewAnalysis::Model(model) => {
                info!("✅ Engineering view (API): {}", model.view_type);
                if let Some(x_max) = model.x_max {
                    info!(
                        "   X-axis max: {}mm{}",
                        x_max,
                        model.x_tolerance.as_deref().unwrap_or("")
                    );
                }
                if let Some(y_max) = model.y_max {
                    info!(
                        "   Y-axis max: {}mm{}",
                        y_max,
                        model.y_tolerance.as_deref().unwrap_or("")
                    );
                }
            }
            ViewAnalysis::Info(info) => {
                info!("✅ Info view (API)");
                if let Some(part_info) = &info.part_info {
                    if let Some(name) = &part_info.name {
                        info!("   Part name: {}", name);
                    }
                    if let Some(material) = &part_info.material {
                        info!("   Material: {}", material);
                    }
                    if let Some(drawing_number) = &part_info.drawing_number {
                        info!("   Drawing number: {}", drawing_number);
                    }
                }
                if let Some(company) = &info.company {
                    info!("   Company: {}", company);
                }
            }
            ViewAnalysis::Error(err) => {
                error!("❌ API Analysis error: {}", err.error_message);
            }
        }

        Ok(analysis)
    }

    /// Analyze single view - automatically choose between local and API based on configuration
    pub async fn analyze_single_view<P: AsRef<Path>>(
        &self,
        image_path: P,
        use_api: bool,
    ) -> IResult<ViewAnalysis> {
        if use_api && self.config.api.is_some() {
            self.analyze_single_view_api(image_path).await
        } else {
            self.analyze_single_view_local(image_path).await
        }
    }

    fn parse_ai_response(&self, content: &str) -> IResult<serde_json::Value> {
        // 尝试直接解析
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
            return Ok(parsed);
        }

        // 尝试提取JSON部分
        let content = content.trim();
        let json_content = if let Some(start) = content.find("```json") {
            let start = start + 7;
            if let Some(end) = content[start..].find("```") {
                &content[start..start + end]
            } else {
                content
            }
        } else if let Some(start) = content.find('{') {
            if let Some(end) = content.rfind('}') {
                &content[start..=end]
            } else {
                content
            }
        } else {
            content
        };

        serde_json::from_str(json_content.trim())
            .map_err(|e| AnalyzerError::AiError(format!("JSON parse error: {}", e)))
    }

    fn clean_extracted_values(&self, mut data: serde_json::Value) -> serde_json::Value {
        if let Some(obj) = data.as_object_mut() {
            for (key, value) in obj.iter_mut() {
                if (key == "x_max" || key == "y_max") && self.is_placeholder_value(value) {
                    warn!(
                        "Detected suspicious placeholder value {}: {:?}, setting to null",
                        key, value
                    );
                    *value = serde_json::Value::Null;
                }
            }
        }
        data
    }

    fn is_placeholder_value(&self, value: &serde_json::Value) -> bool {
        if let Some(num) = value.as_f64() {
            let common_placeholders = [10.0, 20.0, 30.0, 50.0, 80.0, 100.0, 120.0, 150.0, 200.0];
            common_placeholders.contains(&num)
        } else {
            false
        }
    }

    /// Analyze all views in a directory
    pub async fn analyze_view_directory<P: AsRef<Path>>(
        &self,
        views_dir: P,
        use_api: bool,
    ) -> IResult<AnalysisIResult> {
        let views_dir = views_dir.as_ref();

        if !views_dir.exists() || !views_dir.is_dir() {
            return Err(AnalyzerError::AiError(format!(
                "Views directory does not exist: {}",
                views_dir.display()
            )));
        }

        info!(
            "Starting analysis using {} for directory: {}",
            if use_api && self.config.api.is_some() {
                "remote API"
            } else {
                "local model"
            },
            views_dir.display()
        );

        // 收集所有PNG文件
        let mut view_files = Vec::new();
        let mut entries = tokio::fs::read_dir(views_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.to_lowercase() == "png")
                    .unwrap_or(false)
            {
                view_files.push(path);
            }
        }

        if view_files.is_empty() {
            return Err(AnalyzerError::AiError(format!(
                "No PNG files found in directory: {}",
                views_dir.display()
            )));
        }

        info!("Found {} view files for analysis", view_files.len());

        // 分析所有视图
        let mut analyses = Vec::new();
        let mut successful_analyses = 0;
        let mut failed_analyses = 0;
        let mut engineering_views = 0;
        let mut info_views = 0;

        for (i, view_file) in view_files.iter().enumerate() {
            info!(
                "Analyzing view {}/{}: {}",
                i + 1,
                view_files.len(),
                view_file.display()
            );

            let analysis = self.analyze_single_view(view_file, use_api).await?;

            match &analysis {
                ViewAnalysis::Model(_) => {
                    successful_analyses += 1;
                    engineering_views += 1;
                }
                ViewAnalysis::Info(_) => {
                    successful_analyses += 1;
                    info_views += 1;
                }
                ViewAnalysis::Error(_) => {
                    failed_analyses += 1;
                }
            }

            analyses.push(analysis);
        }

        // 计算维度汇总和异常检测
        let dimensions = self.calculate_dimension_summary(&analyses);
        let anomalies = self.detect_anomalies(&analyses);

        let result = AnalysisIResult {
            successful_analyses,
            failed_analyses,
            total_views: analyses.len() as u32,
            engineering_views,
            info_views,
            views: analyses,
            dimensions,
            anomalies,
        };

        info!(
            "Analysis completed: {}/{} successful",
            successful_analyses, result.total_views
        );

        Ok(result)
    }

    fn calculate_dimension_summary(&self, analyses: &[ViewAnalysis]) -> DimensionSummary {
        let mut x_values = Vec::new();
        let mut y_values = Vec::new();

        for analysis in analyses {
            if let ViewAnalysis::Model(model) = analysis {
                if let Some(x_max) = model.x_max {
                    x_values.push(x_max);
                }
                if let Some(y_max) = model.y_max {
                    y_values.push(y_max);
                }
            }
        }

        x_values.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        y_values.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

        DimensionSummary {
            x_max: x_values.first().copied(),
            y_max: y_values.first().copied(),
            x_values,
            y_values,
        }
    }

    fn detect_anomalies(&self, analyses: &[ViewAnalysis]) -> AnomalyReport {
        let dimensions = self.calculate_dimension_summary(analyses);

        let (corrected_x_max, x_mistake_value, x_gap_analysis) =
            self.detect_anomaly_with_gaps(&dimensions.x_values);
        let (corrected_y_max, y_mistake_value, y_gap_analysis) =
            self.detect_anomaly_with_gaps(&dimensions.y_values);

        AnomalyReport {
            x_mistake_value,
            y_mistake_value,
            corrected_x_max,
            corrected_y_max,
            gap_analysis: x_gap_analysis.or(y_gap_analysis),
        }
    }

    fn detect_anomaly_with_gaps(
        &self,
        values: &[f64],
    ) -> (Option<f64>, Option<f64>, Option<GapAnalysis>) {
        if values.len() < 3 {
            return (values.first().copied(), None, None);
        }

        let a = values[0];
        let b = values[1];
        let c = values[2];

        let gap1 = a - b;
        let gap2 = b - c;

        let gap_analysis = GapAnalysis {
            total_values: values.len(),
            top_3_values: vec![a, b, c],
            gap1,
            gap2,
            gap_ratio: if gap2 > 0.0 { gap1 / gap2 } else { 999.0 },
            method: "3_value_gap_analysis".to_string(),
        };

        // 异常检测逻辑
        if gap2 == 0.0 {
            if gap1 > 30.0 {
                (Some(b), Some(a), Some(gap_analysis))
            } else {
                (Some(a), None, Some(gap_analysis))
            }
        } else if gap1 > gap2 * 2.0 && gap1 > 30.0 {
            (Some(b), Some(a), Some(gap_analysis))
        } else {
            (Some(a), None, Some(gap_analysis))
        }
    }

    /// Check if API is available and configured
    pub fn is_api_available(&self) -> bool {
        self.config.api.is_some()
    }

    /// Get recommended analysis mode based on configuration
    pub fn get_recommended_mode(&self) -> bool {
        // 如果配置了API且启用了fast_mode，推荐使用API
        self.config.fast_mode && self.is_api_available()
    }

    /// Analyze view directory with automatic mode selection
    pub async fn analyze_view_directory_auto<P: AsRef<Path>>(
        &self,
        views_dir: P,
    ) -> IResult<AnalysisIResult> {
        let use_api = self.get_recommended_mode();
        self.analyze_view_directory(views_dir, use_api).await
    }
}
