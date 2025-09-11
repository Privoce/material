use crate::{AnalyzerError, IResult, config::AiConfig};
use base64::{Engine as _, engine::general_purpose};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::time::{Duration, timeout};
use tracing::{debug, error, info, warn};

/// 文本提取结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextExtractionResult {
    pub image_path: PathBuf,
    pub model_type: Option<String>,
    pub materials: Vec<String>,
    pub project_name: Option<String>,
    pub error: Option<String>,
}

impl TextExtractionResult {
    pub fn new_error(image_path: PathBuf, error: String) -> Self {
        Self {
            image_path,
            model_type: None,
            materials: Vec::new(),
            project_name: None,
            error: Some(error),
        }
    }
    
    pub fn new_success(
        image_path: PathBuf,
        model_type: Option<String>,
        materials: Vec<String>,
        project_name: Option<String>,
    ) -> Self {
        Self {
            image_path,
            model_type,
            materials,
            project_name,
            error: None,
        }
    }
    
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }
}

/// AI文本分析器
pub struct AiTextAnalyzer {
    config: AiConfig,
    client: reqwest::Client,
}

impl AiTextAnalyzer {
    pub fn new(config: AiConfig) -> Self {
        let client = reqwest::Client::new();
        Self { config, client }
    }
    
    /// 检查API是否可用
    pub fn verify_api_availability(&self) -> IResult<()> {
        if self.config.api.is_none() {
            return Err(AnalyzerError::AiError(
                "API configuration not found. AiTextAnalyzer requires API configuration.".to_string()
            ));
        }
        info!("✅ AI文本分析器已初始化，使用远程API");
        Ok(())
    }
    
    /// 创建文本提取专用提示词
    fn create_text_extract_prompt(&self) -> String {
        r#"
请仔细分析这张模具图片，提取所有可见的文字信息，特别关注模具的基本信息。

**重点提取内容：**
1. 模具类型/零件类型 - 通常在标题栏或图纸名称处，通常名为名称, 详细类型如下:
```
["基座-H", "外基座", "防尘盖", "线轮 Bobbin", "上盖-037", "支架", "衔铁组件-026", "R53G 底板(60A)", "HAG12线架", "外壳-W", 
"HAT904G 基座", "基座-049", "罩壳", "防水塞", "HAT902-ET外壳 (C型)", "H157S护套", "HAG02动衔组件", "HAT905G底板", "Plug外壳", 
"HAT904G 外壳", "头外壳", "固定板", "外壳-H", "辅助开关底座", "基座-1A型", "ZC75N基座", "上盖-050", "HAGO2动衔连接件", "保 险丝盖板", 
"Plug盖板", "塞子", "基座盖板", "推动杆", "上盖", "推片", "ZC75N后盖", "线圈支架-032", "枪头后盖", "底座", "95316-3底板B模", "前基座", 
"安装板外壳", "线轮", "推板-034", "控制盒上壳", "C型基座", "固定板-025", "HAG12支撑座", "Y3F-外壳", " 底板", "拉带", "上基座", "座外壳", 
"线圈架-W", "ZC75N基座(60A-ASSLY带护针)", "NTC基座", "内基座", "线圈架", "基座-042", "底 座(1常开1常闭型)", "推杆", "上盖-048", "衔铁托板", 
"隔弧片", "骨架", "衔铁组件", "Y3F-顶面孔外壳", "底座(组常开型)", "HAT904G 骨架", "尾盖", "HAG12线圈架", "Header外壳", "动簧片组件", 
"SHG.SPRC2C.P03-1", "Y3F骨架", "外壳", "绝缘片", "基座", "基座-038", "基座-047", "上盖004", "Header盖板", "外盖"]
```
2. 材料/材质信息 - 可能标注为"材料"、"材质"、"Material"等，材料需要完整读出，包括后面跟着的型号, 详细材料有：
```
["PET FR530 BLACK BY DUPONT", "尼龙 PA66 K225-KS 黑色 (帝斯曼)", "MZCA-H", "UL746C", "PBT 543", "PBT RG301 BK", "PA66 RG301 黑色", "LCP-4008 (黑色)", 
"PA66 NPG30 黑色", "PBT", "PBT-RG301 黑色阻燃等级：V-0", "PET T102G30 TH3013", "PBT 4130", "PBT R212G30GT OG", "PC 3001-33201 黑色 沃特 UL94V-0 f1", 
"PA6 C0-FKGS6 黑色", "PET FR530", "RoHS UL94 V-0", "尼龙 PC FR7沙伯基础", "PA66-B30", "E202G30", "PBT RG301 BK165 UL94 V-0 RoHS 黑色 (金发)", "PBT 102G30 TH3013", 
"PAG K-FX56/B", "PBT RG530 黑色", "南亚 PBT 1403G6 (黑色)", "PET-FR530 黑色 (再生材35%)", "PA6 K-FKGS6/B 黑色 UL94-V0 DSM", "DT4E", "PBT 3316", "LCP E130i", 
"PET T102G30 TH3013 BK", "PBT FR530 黑色", "PBT E202G30(黑色)", "PBT RG301", "PET FR533NH 本色", "尼龙PA66 FR50 BK086", "PBT FR530 BK", "5010GN6-30MBX", "PET FR530 黑色", 
"尼龙 PA66 RG251 (F1) 黑色 UL94 V-0", "PBT RG301 (白色)", "PC PC3001-33201L 黑色 BK", "新光 PBT D202G30@ (黑色)", "尼龙 PC 121R", "UL94V-0", "衔铁 DT4E", "Lkh7.810.538", 
"UL746C f1 K-FK6G/B DSM", "PBT RG301 黑色", "PBT 201G20 BK", "UL746C F1 L/P/SS/D DSM", "PBT 5010GN6-30MBX", "PEI1000", "PA66 T303 G30 VO BK", "PET FR530 BLACK", "PTFE T1026M T18013", 
"阻燃等级：V-0", "PA66 HTNFR52G30NH或PA66-A3 GF25 VOX1", "PBT RG301 蓝色", "塑料 PBT RG301", "PBT 3316 (黑色阻燃)", "PPS R-7", "金发PBT RG301(白)", "东方PET", "UL94 V-0 RoHS (美国杜邦)", 
"HYZ01-2X3T", "PBT R212G30GT NC", "PET RG305 BLACK", "PET FRG30 BLACK BY DUPONT", "金发 PBT RG301(黑)", "PA4T", "尼龙 PA6-GF30", "C17410", "PET RG301 黑色", "尼龙 PA6 GF30 FR (17)", 
"PBT-RG301 黑色", "LCP E4008 BK", "Lk17.810.541", "PET RG305", "PBT RG301 (黑色)", "PPS B4200 G8 BK", "PA66 A3 GF25 VOX1(本色)", "PET FR530NH或PA4T TX-1", "PPA AFA6133 (本)", "PBT G30 白色", 
"金发 PBT RG301白", "LCP E4008", "PA46-GF30 TE250F6 黑色 UL94V-0", "PPS 6165 A6/A7 BLACK BY POLYPLASTIC", "PET FRF520", "PBT RG301 白色", "LCP E130i 黑色", "PBT 1403G6 黑色", "HY050-ZS1S-K", 
"PBT 3316 黑色 UL94 V-0", "PET-FR530", "尼龙 PA66 RG251 (f1) 黑色 UL94 V-0", "PA66+GF A26FM0 黑色", "尼龙 PA66 FR50", "PAG K-FXG56/B", "PA46", "PBT 5010GNG6-30M8X", "PA66 RPG25", "PBT RG301（黑）", 
"PBT T102G30 TH3013", "DSM尼龙 PA6 K-FKGS6/B 黑色 BK26037", "UL94 HB", "PA46 TE250F8", "南亚PBT 1403 G6(黑)", "PBT R0301", "磁钢 镍铁氧体", "PBT RG301+30GF 黑色", "PBT R212G30GT BK", "尼龙 PA66 EPR27", 
"PBT RG301 BLACK BY KINGFA", "PPS 4500 BK", "PET T102G30", "PC PC3001-33201L BK 黑色", "PBT 3316 黑色", "PBT 5010GN6-30 M6X黑色", "PBT 1403G6", "PBT 4130-104F", "尼龙 PA6-GF30 FR (17)", "PBT 4130(FNGW)", 
"PBT 5010G6N6-30 MBX", "PBT4130-104K", "PBT 4130-104K", "尼龙 PA6 K-FKGS6 绿色 PANTONE 7730C UL94 V-0 (DSM)", "PBT 4130 黑色 防紫外线", "PET EMC 130-20", "PBT4130-104F", "PET FR530 BK", "PPS R-4 黑色", 
"PBT E202630(黑色)", "PAA6+GF A26FM0 黑色", "PET-FR531", "PBT 4130 黑色", "PBT 5010GN6 BK", "PBT 3316 BK", "C18150-R540", "PA6 K-FKGS6 黑色 DSM UL94V-0", "PET FG550 BK", "PBT 1430", "PBT RG530 白色", 
"再生材 黑色", "UL94 V-0", "PBT FR530", "PET FR530 本色", "TPE EFT85B030MB-B 黑色", "PET FR830 BLACK", "尼龙 PA6-30GF, K-PESS6/B", "PBT 1430G6"]
```
3. 项目名称或称为型号

**注意事项：**
- 材料信息可能有多个，请全部提取
- 材料可能称为"材质"、"Material"、"原材料"等
- 保持原始文字的准确性，不要修改或简化

**输出JSON格式：**
```json
{
    "model_type": "模具类型或零件类型",
    "materials": ["材料1", "材料2"],
    "project_name": "项目名称或型号"
}
```

请确保：
1. 准确识别所有可见文字
2. 正确分类文字信息
3. 保持原始文字的准确性
4. 如果某些字段无法识别，设为null或空数组
5. 材料信息特别重要，请仔细提取
"#
        .to_string()
    }
    
    /// 为文字识别编码图像（保持高质量）
    async fn encode_image_for_text_extraction<P: AsRef<Path>>(&self, image_path: P) -> IResult<String> {
        let image_path = image_path.as_ref();
        
        let img = image::open(image_path)
            .map_err(|e| AnalyzerError::ImageError(format!("Failed to open image: {}", e)))?;
        
        info!("保持原始图像尺寸: {}x{}", img.width(), img.height());
        
        // 转换为RGB并编码为高质量JPEG
        let rgb_img = img.to_rgb8();
        let mut jpeg_data = Vec::new();
        
        {
            use image::codecs::jpeg::JpegEncoder;
            let mut encoder = JpegEncoder::new_with_quality(&mut jpeg_data, 95); // 高质量确保文字清晰
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
    
    /// 从文件夹中的多张图片提取文本信息并合并结果
    pub async fn extract_text_from_folder<P: AsRef<Path>>(
        &self,
        folder_path: P,
    ) -> IResult<TextExtractionResult> {
        let folder_path = folder_path.as_ref();
        info!("开始处理文件夹: {}", folder_path.display());
        
        // 读取文件夹中的所有图片文件
        let mut image_files = Vec::new();
        let entries = std::fs::read_dir(folder_path)
            .map_err(|e| AnalyzerError::ImageError(format!("无法读取文件夹: {}", e)))?;
        
        for entry in entries {
            let entry = entry.map_err(|e| AnalyzerError::ImageError(format!("读取文件项失败: {}", e)))?;
            let path = entry.path();
            
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    let ext = ext.to_string_lossy().to_lowercase();
                    if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "bmp" | "gif" | "tiff") {
                        image_files.push(path);
                    }
                }
            }
        }
        
        if image_files.is_empty() {
            return Ok(TextExtractionResult::new_error(
                folder_path.to_path_buf(),
                "文件夹中没有找到图片文件".to_string()
            ));
        }
        
        // 按文件名排序，确保处理顺序一致
        image_files.sort();
        info!("找到 {} 张图片，开始逐一处理", image_files.len());
        
        // 逐一处理每张图片
        let mut all_results = Vec::new();
        for (index, image_path) in image_files.iter().enumerate() {
            info!("处理第 {}/{} 张图片: {}", index + 1, image_files.len(), image_path.display());
            
            match self.extract_text_from_image(image_path).await {
                Ok(result) => {
                    if result.is_success() {
                        info!("✅ 第 {} 张图片处理成功", index + 1);
                    } else {
                        warn!("⚠️ 第 {} 张图片处理失败: {:?}", index + 1, result.error);
                    }
                    all_results.push(result);
                }
                Err(e) => {
                    error!("❌ 第 {} 张图片处理出错: {}", index + 1, e);
                    all_results.push(TextExtractionResult::new_error(
                        image_path.clone(),
                        format!("处理失败: {}", e)
                    ));
                }
            }
            
            // 在图片之间添加小延迟，避免API请求过于频繁
            if index < image_files.len() - 1 {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        
        // 合并所有结果
        self.merge_extraction_results(folder_path.to_path_buf(), all_results)
    }
    
    /// 从单张图片提取文本信息
    pub async fn extract_text_from_image<P: AsRef<Path>>(
        &self,
        image_path: P,
    ) -> IResult<TextExtractionResult> {
        let image_path = image_path.as_ref();
        info!("提取文字: {}", image_path.display());
        
        let api_config = self.config.api.as_ref()
            .ok_or_else(|| AnalyzerError::AiError("API configuration not found".to_string()))?;
        
        for attempt in 1..=self.config.max_retries {
            if attempt > 1 {
                info!("重试第 {} 次...", attempt);
            }
            
            match self.try_extract_text_api(image_path, attempt, api_config).await {
                Ok(result) => return Ok(result),
                Err(e) if attempt < self.config.max_retries => {
                    warn!("文本提取尝试 {} 失败: {}, 重试中...", attempt, e);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                Err(e) => {
                    error!("所有文本提取尝试都失败了 {}: {}", image_path.display(), e);
                    return Ok(TextExtractionResult::new_error(
                        image_path.to_path_buf(),
                        format!("所有提取尝试都失败: {}", e)
                    ));
                }
            }
        }
        
        unreachable!()
    }
    
    async fn try_extract_text_api<P: AsRef<Path>>(
        &self,
        image_path: P,
        _attempt: u32,
        api_config: &crate::config::ApiConfig,
    ) -> IResult<TextExtractionResult> {
        let image_path = image_path.as_ref();
        
        // 编码图像
        let image_base64 = self.encode_image_for_text_extraction(image_path).await?;
        
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
                                "text": self.create_text_extract_prompt()
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
                "max_tokens": 1024,
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
                                    "text": self.create_text_extract_prompt()
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
                    "max_tokens": 1024
                }
            });
            let url = format!("{}/services/aigc/text-generation/generation", 
                api_config.endpoint.replace("/compatible-mode/v1", ""));
            (payload, url)
        };
        
        debug!("发送文字提取请求到: {}", url);
        debug!("使用模型: {}", api_config.model_name);
        
        // 发送请求到远程API
        let response = timeout(
            Duration::from_secs(300), // 5分钟超时，文字提取可能需要更长时间
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
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
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
        
        debug!("Full API response: {}", serde_json::to_string_pretty(&response_json).unwrap_or_else(|_| "Failed to serialize response".to_string()));
        
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
        
        debug!("响应长度: {} 字符", content.len());
        debug!("原始响应前200字符: {}", &content[..content.len().min(200)]);
        
        // 解析提取结果
        let mut result = TextExtractionResult::new_success(
            image_path.to_path_buf(),
            None,
            Vec::new(),
            None,
        );
        
        // 尝试解析JSON响应
        match self.parse_text_extraction_response(content) {
            Ok(parsed_data) => {
                result.model_type = parsed_data.get("model_type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                
                result.materials = parsed_data.get("materials")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                
                result.project_name = parsed_data.get("project_name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                
                // 打印提取结果摘要
                self.print_extraction_summary(&result);
            }
            Err(e) => {
                warn!("JSON解析失败，尝试文本解析: {}", e);
                // 尝试从纯文本中解析信息
                if let Some(parsed_data) = self.parse_text_response(content) {
                    result.model_type = parsed_data.model_type;
                    result.materials = parsed_data.materials;
                    result.project_name = parsed_data.project_name;
                    info!("✅ 文本解析成功");
                    self.print_extraction_summary(&result);
                } else {
                    result.error = Some(format!("解析失败: {}", e));
                    warn!("❌ 所有解析方法都失败");
                }
            }
        }
        
        Ok(result)
    }
    
    /// 解析API返回的JSON响应
    fn parse_text_extraction_response(&self, content: &str) -> IResult<serde_json::Value> {
        let content = content.trim();
        
        // 尝试直接解析
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
            return Ok(parsed);
        }
        
        // 尝试提取JSON部分
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
    
    /// 从纯文本响应中解析信息
    fn parse_text_response(&self, content: &str) -> Option<TextExtractionResult> {
        let mut model_type = None;
        let mut materials = Vec::new();
        let mut project_name = None;
        
        let lines: Vec<&str> = content.split('\n').map(|l| l.trim()).collect();
        
        // 关键词匹配
        let material_keywords = ["材料", "材质", "material", "原材料", "钢材", "铝材", "塑料", "橡胶"];
        let type_keywords = ["类型", "型号", "模具", "零件", "type", "model"];
        let project_keywords = ["项目", "名称", "project", "name", "产品"];
        
        for line in &lines {
            if line.is_empty() {
                continue;
            }
            
            // 查找材料信息
            for keyword in &material_keywords {
                if line.to_lowercase().contains(&keyword.to_lowercase()) {
                    let material_text = line.replace(keyword, "");
                     let material_text = material_text.trim_matches(':').trim_matches('：').trim();
                    if !material_text.is_empty() {
                        materials.push(material_text.to_string());
                    }
                    break;
                }
            }
            
            // 查找类型信息
            if model_type.is_none() {
                for keyword in &type_keywords {
                    if line.to_lowercase().contains(&keyword.to_lowercase()) {
                        let type_text = line.replace(keyword, "");
                        let type_text = type_text.trim_matches(':').trim_matches('：').trim();
                        if !type_text.is_empty() {
                            model_type = Some(type_text.to_string());
                        }
                        break;
                    }
                }
            }
            
            // 查找项目名称
            if project_name.is_none() {
                for keyword in &project_keywords {
                    if line.to_lowercase().contains(&keyword.to_lowercase()) {
                        let project_text = line.replace(keyword, "");
                        let project_text = project_text.trim_matches(':').trim_matches('：').trim();
                        if !project_text.is_empty() {
                            project_name = Some(project_text.to_string());
                        }
                        break;
                    }
                }
            }
        }
        
        // 去重材料
        materials.dedup();
        
        // 如果没有找到结构化信息，至少保存一些文本
        if model_type.is_none() && materials.is_empty() && project_name.is_none() {
            let important_lines: Vec<&str> = lines.iter()
                .filter(|line| line.len() > 5)
                .take(10)
                .cloned()
                .collect();
            
            if !important_lines.is_empty() {
                model_type = Some(important_lines[0].to_string());
                if important_lines.len() > 1 {
                    project_name = Some(important_lines[1].to_string());
                }
            }
        }
        
        if model_type.is_some() || !materials.is_empty() || project_name.is_some() {
            Some(TextExtractionResult::new_success(
                PathBuf::new(),
                model_type,
                materials,
                project_name,
            ))
        } else {
            None
        }
    }
    
    /// 打印提取结果摘要
    fn print_extraction_summary(&self, result: &TextExtractionResult) {
        if let Some(error) = &result.error {
            error!("❌ 提取失败: {}", error);
            return;
        }
        
        info!("图片: {}", result.image_path.display());
        
        if let Some(model_type) = &result.model_type {
            info!("✅ 模具类型: {}", model_type);
        }
        
        if let Some(project_name) = &result.project_name {
            info!("📋 项目名称: {}", project_name);
        }
        
        if !result.materials.is_empty() {
            info!(" 材料信息: {}种", result.materials.len());
            for (i, material) in result.materials.iter().enumerate() {
                info!("    {}. {}", i + 1, material);
            }
        } else {
            warn!("⚠️ 未找到材料信息");
        }
        
        info!("---");
    }
    
    /// 合并多个提取结果
    fn merge_extraction_results(
        &self,
        folder_path: PathBuf,
        results: Vec<TextExtractionResult>,
    ) -> IResult<TextExtractionResult> {
        info!("开始合并 {} 个图片的提取结果", results.len());
        
        let mut merged_model_types = Vec::new();
        let mut merged_materials = Vec::new();
        let mut merged_project_names = Vec::new();
        let mut errors = Vec::new();
        
        let successful_results: Vec<_> = results.iter()
            .filter(|r| r.is_success())
            .collect();
        
        info!("成功处理: {}/{} 张图片", successful_results.len(), results.len());
        
        // 收集所有成功结果的信息
        for result in &successful_results {
            if let Some(model_type) = &result.model_type {
                if !model_type.trim().is_empty() {
                    merged_model_types.push(model_type.clone());
                }
            }
            
            for material in &result.materials {
                if !material.trim().is_empty() {
                    merged_materials.push(material.clone());
                }
            }
            
            if let Some(project_name) = &result.project_name {
                if !project_name.trim().is_empty() {
                    merged_project_names.push(project_name.clone());
                }
            }
        }
        
        // 收集错误信息
        for result in &results {
            if let Some(error) = &result.error {
                errors.push(format!("{}: {}", result.image_path.display(), error));
            }
        }
        
        // 去重和优化结果
        merged_model_types.sort();
        merged_model_types.dedup();
        
        merged_materials.sort();
        merged_materials.dedup();
        
        merged_project_names.sort();
        merged_project_names.dedup();
        
        // 选择最合适的模具类型（出现频率最高的）
        let final_model_type = if !merged_model_types.is_empty() {
            Some(merged_model_types[0].clone())
        } else {
            None
        };
        
        // 选择最合适的项目名称（出现频率最高的）
        let final_project_name = if !merged_project_names.is_empty() {
            Some(merged_project_names[0].clone())
        } else {
            None
        };
        
        // 创建合并结果
        let merged_result = if successful_results.is_empty() {
            TextExtractionResult::new_error(
                folder_path,
                format!("所有图片处理都失败: {}", errors.join("; "))
            )
        } else {
            TextExtractionResult::new_success(
                folder_path,
                final_model_type,
                merged_materials,
                final_project_name,
            )
        };
        
        // 打印合并结果摘要
        info!("📊 合并结果摘要:");
        self.print_extraction_summary(&merged_result);
        
        if !errors.is_empty() {
            warn!("⚠️ 处理失败的图片:");
            for error in &errors {
                warn!("  {}", error);
            }
        }
        
        Ok(merged_result)
    }
}
