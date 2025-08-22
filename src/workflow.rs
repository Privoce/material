use std::path::PathBuf;
use tokio::task;
use tracing::{error, info, warn};

use crate::{
    MODELS,
    ai_text_analyzer::AiTextAnalyzer,
    api::pdf::{WebhookRequest, convert_to_image},
    config::AiConfig,
    diff::{DiffResult, ModelJson, fmt_diff_result_to_md},
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
                self.send_response(&format!("❌ 分析失败: {}", error_msg))
                    .await;
            }
        }
    }

    /// 执行分析逻辑
    async fn perform_analysis(&self) -> Result<String, String> {
        // 1. 转换 PDF 为图片
        info!("📄 正在转换 PDF 为图片...");
        let output_path =
            convert_to_image(&self.pdf_path).map_err(|e| format!("PDF 转换失败: {}", e))?;

        // 2. 初始化 AI 分析器
        info!("🤖 正在初始化 AI 分析器...");
        let analyzer = AiTextAnalyzer::new(AiConfig::default());
        analyzer
            .verify_api_availability()
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

        let sorted_models = MODELS.clone();
        let mut diff_results = ModelJson::diff(sorted_models, model_json);
        DiffResult::sort(&mut diff_results);
        let response_text = fmt_diff_result_to_md(&diff_results);

        info!("✅ 分析完成");
        Ok(response_text)
    }

    /// 发送响应到 webhook
    async fn send_response(&self, content: &str) {
        let client = reqwest::Client::new();

        match client
            .post(&self.webhook_url)
            .header("content-type", "text/markdown")
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
pub fn create_pdf_analysis_workflow(
    pdf_path: PathBuf,
    req: &WebhookRequest,
) -> PdfAnalysisWorkflow {
    let webhook_url = format!(
        "https://huateng.voce.chat/api/bot/send_to_user/{}",
        req.from_uid
    );
    let api_key = "013b93273ce0dc707e4d55a214f0b54a63bde7fe7dc803b4eda52b3bc828975a7b22756964223a322c226e6f6e6365223a223661432f436558557032674141414141646e4b666f2f76412b64774b4b455465227d".to_string();
    PdfAnalysisWorkflow::new(pdf_path, webhook_url, api_key)
}
