use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use salvo::{
    Request, Response, handler,
    http::headers::ContentType,
    writing::Json,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    ai_analyzer::AiAnalyzer, config::{AiConfig, SamConfig}, pdf_converter::PdfConverterRunner, sam::SamInterface, workflow::create_pdf_analysis_workflow
};

#[derive(Deserialize, Debug)]
pub struct PdfPathRequest {
    pub path: String,
    pub output: Option<String>,
}

impl From<PdfPathRequest> for PdfConverterRunner {
    fn from(value: PdfPathRequest) -> Self {
        let PdfPathRequest { path, output } = value;
        PdfConverterRunner::new(path, output)
    }
}

/// 传入本机的pdf文件路径/文件夹路径，转为png图片并返回处理后的路径
/// POST /api/pdf_to_png
/// ```
/// {
///   "path": "C:/path/to/pdf/or/folder",
///   "output": "C:/path/to/output/folder" // 可选参数
/// }
/// ```
#[handler]
pub async fn from_path(req: &mut Request, res: &mut Response) -> Result<(), ()> {
    let pdf_request: PdfPathRequest = req.parse_json().await.unwrap();
    let runner: PdfConverterRunner = pdf_request.into();
    if let Err(e) = runner.run() {
        res.render(Json(serde_json::json!({
            "status": "error",
            "message": e.to_string(),
        })));
        return Err(());
    }
    res.render(Json(serde_json::json!({
        "status": "success",
        "output": runner.output,
    })));
    Ok(())
}

/// 对某个pdf文件进行SAM分割
#[handler]
pub async fn split(_req: &mut Request, res: &mut Response) -> Result<(), ()> {
    let sam_interface = SamInterface::new(SamConfig::default());
    sam_interface
        .verify_setup()
        .expect("SAM setup verification failed");
    match sam_interface
        .split_image(
            "D:\\work\\material_rs\\pdfs\\output\\03-jz\\page_0.jpg",
            Some("D:\\work\\material_rs\\pdfs\\output\\03-jz\\split"),
        )
        .await
    {
        Ok(result) => {
            res.render(Json(serde_json::json!({
                "status": "success",
                "output": result.output_dir,
            })));
        }
        Err(e) => {
            res.render(Json(serde_json::json!({
                "status": "error",
                "message": format!("SAM splitting failed: {}", e),
            })));
        }
    }

    Ok(())
}

#[handler]
pub async fn ai_analysis(_req: &mut Request, res: &mut Response) -> Result<(), ()> {
    // 创建一个analyzer实例
    let analyzer = AiAnalyzer::new(AiConfig::default());
    // "D:\\work\\material_rs\\pdfs\\output\\03-jz\\split\\improved_view_01.png"
    match analyzer
        .analyze_single_view(
            "D:\\work\\material_rs\\pdfs\\output\\03-jz\\page_0.jpg",
            true,
        )
        .await
    {
        Ok(result) => {
            res.render(Json(serde_json::json!({
                "status": "success",
                "output": result,
            })));
        }
        Err(e) => {
            res.render(Json(serde_json::json!({
                "status": "error",
                "message": format!("AI analysis failed: {}", e),
            })));
            return Err(());
        }
    }

    Ok(())
}

// 示例请求体
// {
//   "created_at": 1754560852630,
//   "detail": {
//     "content": "2025/8/7/e034f8aa-55e5-4a4e-8c93-3fc2f4f45c72",
//     "content_type": "vocechat/file",
//     "expires_in": null,
//     "properties": {
//       "content_type": "application/pdf",
//       "local_id": 1754560852515,
//       "name": "03骨架 .pdf",
//       "size": 102003
//     },
//     "type": "normal"
//   },
//   "domain": null,
//   "from_uid": 1,
//   "mid": 1,
//   "target": { "uid": 2 },
//   "type": "chat",
//   "widget_id": null
// }

const CONTENT_TYPE_VOCECHAT: &str = "vocechat/file";
#[derive(Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum IdType {
    UId,
    GId,
}

#[derive(Deserialize, Debug, Clone)]
pub struct WebhookRequest {
    pub created_at: i64,
    pub detail: WebhookReqDetail,
    pub from_uid: u64,
    pub mid: u64,
    pub target: HashMap<IdType, u64>,
    pub r#type: String,
    pub widget_id: Option<String>,
    pub domain: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct WebhookReqDetail {
    pub content: PathBuf,
    pub content_type: String,
    pub expires_in: Option<i64>,
    pub properties: HashMap<String, Value>,
    #[serde(rename = "type")]
    pub ty: String,
}

impl WebhookReqDetail {
    pub fn is_pdf(&self) -> bool {
        self.content_type == CONTENT_TYPE_VOCECHAT
            && self
                .properties
                .get("content_type")
                .and_then(|v| v.as_str())
                .map_or(false, |ct| ct == "application/pdf")
    }
    pub fn pdf_path(&self) -> Result<PathBuf, Box<dyn std::error::Error>> {
        // prefix: data/upload/file/${content}
        let current_exe = std::env::current_exe()?;
        Ok(current_exe
            .join("data")
            .join("upload")
            .join("file")
            .join(&self.content)
            .with_extension("pdf"))
    }
}

pub fn convert_to_image(path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let current_exe = std::env::current_exe()?;
    let output_dir = current_exe.join("output");
    let name = path.file_stem().ok_or("Invalid PDF file name")?;
    let runner = PdfConverterRunner::new(path, Some(output_dir));
    match runner.run() {
        Ok(_) => Ok(runner.output.join(name)),
        Err(e) => Err(Box::new(e)),
    }
}

pub struct WebhookResponse {
    pub content_type: ContentType,
    pub x_api_key: String,
    pub markdown_body: String,
}

impl WebhookResponse {
    pub fn new(text: &str) -> Self {
        Self {
            content_type: ContentType::text(),
            x_api_key: "e655422b1150390aa9421f534d256e906b685c0d383bcd2a6d43a6510212e07a7b22756964223a322c226e6f6e6365223a2230596b4b4e6e4d336b32674141414141623835646a68562b6b46724542513855227d".to_string(),
            markdown_body: text.to_string(),
        }
    }
    pub fn render(&self) -> () {
        // 使用reqwest创建一个Client发送POST请求
        let client = reqwest::blocking::Client::new();
        let response = client
            .post("https://api.vocechat.com/material/api/workhook")
            .header("content-type", self.content_type.to_string())
            .header("x-api-key", &self.x_api_key)
            .body(self.markdown_body.clone())
            .send();
        if let Err(e) = response {
            eprintln!("Failed to send webhook: {}", e);
        } else {
            println!("Webhook sent successfully");
        }
    }
}

/// 对接vocechat的机器人的webhook
/// POST /material/api/workhook
#[handler]
pub async fn workhook(req: &mut Request, _res: &mut Response) -> Result<(), ()> {
    if let Ok(webhook_req) = req.parse_json::<WebhookRequest>().await {
        // 获取到 webhook 请求体之后判断是否为pdf文件
        if webhook_req.detail.is_pdf() {
            // 处理pdf文件，立即返回"正在处理"响应，然后在后台处理
            match webhook_req.detail.pdf_path() {
                Ok(pdf_path) => {
                    // 立即返回响应，告知用户正在处理
                    WebhookResponse::new("📄 收到PDF文件，正在分析中，请稍等...").render();
                    
                    // 启动后台分析工作流
                    let workflow = create_pdf_analysis_workflow(pdf_path);
                    workflow.start_background_analysis();
                    
                    return Ok(());
                }
                Err(_) => {
                    WebhookResponse::new("❌ 无效的PDF文件路径").render();
                    return Err(());
                }
            }
        } else {
            // 非PDF文件，可以返回提示信息
            WebhookResponse::new("ℹ️ 请发送PDF文件进行分析").render();
        }
    } else {
        WebhookResponse::new("❌ 无效的请求格式").render();
        return Err(());
    }

    Ok(())
}

#[handler]
pub async fn workhook_check(_req: &mut Request, res: &mut Response) -> Result<(), ()> {
    res.render(Json(serde_json::json!({
        "status": 200,
        "message": "Webhook is active"
    })));
    Ok(())
}
