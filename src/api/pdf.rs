use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use salvo::{Request, Response, handler, http::headers::ContentType, writing::Json};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    pdf_converter::PdfConverterRunner,
    workflow::{create_pdf_analysis_workflow, create_text_analysis_workflow},
};

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
const CONTENT_TYPE_TEXT_PLAIN: &str = "text/plain";
const CONTENT_TYPE_TEXT_MARKDOWN: &str = "text/markdown";
#[derive(Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum IdType {
    #[serde(rename = "uid")]
    UId,
    #[serde(rename = "gid")]
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
    pub properties: Option<HashMap<String, Value>>,
    #[serde(rename = "type")]
    pub ty: String,
}

impl WebhookReqDetail {
    pub fn is_pdf(&self) -> bool {
        self.content_type == CONTENT_TYPE_VOCECHAT
            && self
                .properties
                .as_ref()
                .and_then(|props| props.get("content_type"))
                .and_then(|v| v.as_str())
                .map_or(false, |ct| ct == "application/pdf")
    }

    pub fn is_text_message(&self) -> bool {
        self.content_type == CONTENT_TYPE_TEXT_PLAIN
            || self.content_type == CONTENT_TYPE_TEXT_MARKDOWN
    }

    pub fn get_text_content(&self) -> Option<String> {
        if self.is_text_message() {
            // content字段在文本消息中包含实际的文本内容
            self.content.to_str().map(|s| s.to_string())
        } else {
            None
        }
    }

    pub fn pdf_path(&self) -> Result<PathBuf, String> {
        // prefix: data/upload/file/${content}
        let current_exe = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .unwrap()
            .to_path_buf();
        // 对这个self.content进行处理，分割`/`或`\`转为PathBuf
        let content_path = self
            .content
            .components()
            .fold(PathBuf::new(), |mut acc, comp| {
                acc.push(comp);
                acc
            });

        let meta_file = current_exe
            .join("data")
            .join("upload")
            .join("file")
            .join(content_path);
        dbg!(&meta_file);
        // 复制这个meta_file并增加后缀
        let pdf_path = meta_file.with_extension("pdf");
        if meta_file.exists() {
            std::fs::copy(&meta_file, &pdf_path)
                .map_err(|e| format!("Failed to copy file: {}", e))?;
        } else {
            return Err("PDF file does not exist".to_string());
        }

        Ok(pdf_path)
    }
}

pub fn convert_to_image(path: &Path) -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .unwrap()
        .to_path_buf();
    let output_dir = current_exe.join("output");
    let name = path.file_stem().ok_or("Invalid PDF file name")?;
    let runner = PdfConverterRunner::new(path, Some(output_dir));
    match runner.run() {
        Ok(_) => Ok(runner.output.join(name)),
        Err(e) => Err(e.to_string()),
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
            x_api_key: "013b93273ce0dc707e4d55a214f0b54a63bde7fe7dc803b4eda52b3bc828975a7b22756964223a322c226e6f6e6365223a223661432f436558557032674141414141646e4b666f2f76412b64774b4b455465227d".to_string(),
            markdown_body: text.to_string(),
        }
    }

    pub async fn render(&self) -> () {
        // 使用异步reqwest客户端发送POST请求
        let client = reqwest::Client::new();
        let url = "https://api.vocechat.com/material/api/workhook";
        let content_type = self.content_type.to_string();
        let api_key = self.x_api_key.clone();
        let body = self.markdown_body.clone();

        let response = client
            .post(url)
            .header("content-type", content_type)
            .header("x-api-key", api_key)
            .body(body)
            .send()
            .await;

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
pub async fn workhook(req: &mut Request, res: &mut Response) -> Result<(), ()> {
    if let Ok(webhook_req) = req.parse_json::<WebhookRequest>().await {
        // 检查是否为PDF文件
        if webhook_req.detail.is_pdf() {
            // 处理pdf文件，立即返回"正在处理"响应，然后在后台处理
            match webhook_req.detail.pdf_path() {
                Ok(pdf_path) => {
                    // 立即返回响应，告知用户正在处理
                    res.render(Json(serde_json::json!({
                        "status": 200,
                        "message": "📄 收到PDF文件，正在分析中，请稍等..."
                    })));
                    // 启动后台分析工作流
                    let workflow = create_pdf_analysis_workflow(pdf_path, &webhook_req);
                    workflow.start_background_analysis();

                    return Ok(());
                }
                Err(e) => {
                    res.render(Json(serde_json::json!({
                        "status": 200,
                        "message": format!("❌ 无效的PDF文件路径: {}", e)
                    })));
                    return Err(());
                }
            }
        }
        // 检查是否为文本搜索请求
        else if webhook_req.detail.is_text_message() {
            if let Some(search_text) = webhook_req.detail.get_text_content() {
                // 立即返回响应，告知用户正在搜索
                res.render(Json(serde_json::json!({
                    "status": 200,
                    "message": "🔍 收到搜索请求，正在处理中..."
                })));

                // 启动后台搜索工作流
                let workflow = create_text_analysis_workflow(search_text, &webhook_req);
                workflow.start_background_search();

                return Ok(());
            } else {
                res.render(Json(serde_json::json!({
                    "status": 200,
                    "message": "❌ 无法解析文本内容"
                })));
            }
        } else {
            // 非PDF文件也非文本消息，返回提示信息
            res.render(Json(serde_json::json!({
                "status": 200,
                "message": "ℹ️ 请发送PDF文件进行分析，或发送文本进行搜索"
            })));
        }
    } else {
        res.render(Json(serde_json::json!({
            "status": 200,
            "message": "❌ 无效的请求格式"
        })));
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    #[test]
    fn componet_path() {
        let path = "2025/8/7/e034f8aa-55e5-4a4e-8c93-3fc2f4f45c72";
        let content_path =
            PathBuf::from(path)
                .components()
                .fold(PathBuf::new(), |mut acc, comp| {
                    acc.push(comp);
                    acc
                });
        dbg!(content_path.display());
    }
}
