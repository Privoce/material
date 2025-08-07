use std::{env::current_exe, path::PathBuf};
use tokio::task;
use tracing::{error, info, warn};

use crate::{
    ai_text_analyzer::AiTextAnalyzer,
    api::pdf::{convert_to_image, WebhookResponse},
    config::AiConfig,
    diff::{fmt_diff_result_to_md, ModelJson},
};

/// PDF 分析工作流
pub struct PdfAnalysisWorkflow {
    pdf_path: PathBuf,
    webhook_url: String,
    api_key: String,
}

impl PdfAnalysisWorkflow {
    pub fn new(pdf_path: PathBuf, webhook_url: String, api_key: String) -> Self {
        Self {
            pdf_path,
            webhook_url,
            api_key,
        }
    }

    /// 启动后台分析任务
    pub fn start_background_analysis(self) {
        task::spawn(async move {
            self.run_analysis().await;
        });
    }

    /// 执行完整的分析流程
    async fn run_analysis(self) {
        info!("开始后台分析 PDF: {}", self.pdf_path.display());

        let result = self.perform_analysis().await;
        
        match result {
            Ok(response_text) => {
                info!("✅ 分析完成，发送结果");
                self.send_response(&response_text).await;
            }
            Err(error_msg) => {
                error!("❌ 分析失败: {}", error_msg);
                self.send_response(&format!("❌ 分析失败: {}", error_msg)).await;
            }
        }
    }

    /// 执行分析逻辑
    async fn perform_analysis(&self) -> Result<String, String> {
        // 1. 转换 PDF 为图片
        info!("📄 正在转换 PDF 为图片...");
        let output_path = convert_to_image(&self.pdf_path)
            .map_err(|e| format!("PDF 转换失败: {}", e))?;

        // 2. 初始化 AI 分析器
        info!("🤖 正在初始化 AI 分析器...");
        let analyzer = AiTextAnalyzer::new(AiConfig::default());
        analyzer.verify_api_availability()
            .map_err(|e| format!("AI 分析器初始化失败: {}", e))?;

        // 3. 提取文本信息
        info!("🔍 正在提取文本信息...");
        let extraction_result = analyzer
            .extract_text_from_folder(&output_path)
            .await
            .map_err(|e| format!("文本提取失败: {}", e))?;

        // 4. 检查提取结果
        if let Some(error) = &extraction_result.error {
            return Err(format!("文本提取错误: {}", error));
        }

        // 5. 转换为 ModelJson 并进行相似度比较
        info!("📊 正在进行相似度比较...");
        let model_json = ModelJson::from(extraction_result);
        
        let models_dir = current_exe()
            .map_err(|e| format!("获取执行目录失败: {}", e))?
            .join("models")
            .join("jsons");

        let models = ModelJson::patch_new(models_dir)
            .map_err(|e| format!("加载模型数据失败: {}", e))?;

        let sorted_models = ModelJson::sort(models);
        let diff_results = ModelJson::diff(sorted_models, model_json);
        let response_text = fmt_diff_result_to_md(&diff_results);

        info!("✅ 分析完成");
        Ok(response_text)
    }

    /// 发送响应到 webhook
    async fn send_response(&self, content: &str) {
        let client = reqwest::Client::new();
        
        match client
            .post(&self.webhook_url)
            .header("content-type", "text/plain")
            .header("x-api-key", &self.api_key)
            .body(content.to_string())
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    info!("✅ 结果已成功发送到 webhook");
                } else {
                    warn!("⚠️ Webhook 响应状态: {}", response.status());
                }
            }
            Err(e) => {
                error!("❌ 发送 webhook 失败: {}", e);
            }
        }
    }
}

/// 创建并启动 PDF 分析工作流
pub fn create_pdf_analysis_workflow(pdf_path: PathBuf) -> PdfAnalysisWorkflow {
    let webhook_url = "https://api.vocechat.com/material/api/workhook".to_string();
    let api_key = "e655422b1150390aa9421f534d256e906b685c0d383bcd2a6d43a6510212e07a7b22756964223a322c226e6f6e6365223a2230596b4b4e6e4d336b32674141414141623835646a68562b6b46724542513855227d".to_string();
    
    PdfAnalysisWorkflow::new(pdf_path, webhook_url, api_key)
}
