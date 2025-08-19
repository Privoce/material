use std::{collections::HashMap, env::current_exe, fs, io::Cursor, path::PathBuf};

use base64::{Engine, prelude::BASE64_STANDARD};
use image::ImageReader;
use serde::{Deserialize, Serialize};

use crate::ai_text_analyzer::TextExtractionResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelJson {
    pub model_type: Option<String>,
    pub materials: Vec<String>,
    pub project_name: Option<String>,
    pub source_directory: PathBuf,
    pub source_directory_name: String,
    pub extraction_timestamp: Option<String>,
}

impl From<TextExtractionResult> for ModelJson {
    fn from(value: TextExtractionResult) -> Self {
        let TextExtractionResult {
            image_path,
            model_type,
            materials,
            project_name,
            ..
        } = value;

        Self {
            model_type,
            materials,
            project_name,
            source_directory_name: image_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string(),
            source_directory: image_path,
            extraction_timestamp: None,
        }
    }
}

impl ModelJson {
    /// new from json use serde_json
    pub fn new(path: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path.as_path())?;
        let model_json: ModelJson = serde_json::from_str(&content)?;
        Ok(model_json)
    }

    pub fn patch_new(path: PathBuf) -> Result<Vec<Self>, Box<dyn std::error::Error>> {
        let mut result = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if entry.path().extension().and_then(|s| s.to_str()) == Some("json") {
                let model_json = ModelJson::new(entry.path())?;
                result.push(model_json);
            }
        }
        return Ok(result);
    }

    /// 将 Vec<ModelJson> 通过model_type进行分组
    pub fn sort(models: Vec<Self>) -> HashMap<String, Vec<Self>> {
        let mut map = HashMap::new();
        for model in models {
            map.entry(model.model_type.clone().unwrap_or("unknown".to_string()))
                .and_modify(|e: &mut Vec<ModelJson>| e.push(model.clone()))
                .or_insert(vec![model]);
        }
        map
    }

    pub fn diff(models: HashMap<String, Vec<Self>>, model: Self) -> Vec<DiffResult> {
        let mut results = Vec::new();

        for (model_type, model_info) in models {
            // 进行模具类型比较，优先全词匹配
            let model_type_diff = improved_diff_text(
                &model_type,
                &(model.model_type.clone().unwrap_or("unknown".to_string())),
            );

            // 如果模具类型相似度太低，直接跳过
            if model_type_diff < 0.1 {
                continue;
            }

            // 比较材料
            for cmodel in model_info {
                // 比较source_directory_name，去除一样的
                if cmodel.source_directory_name == model.source_directory_name {
                    continue;
                }

                let cm_len = cmodel.materials.len();
                let m_len = model.materials.len();

                if cm_len == 0 || m_len == 0 {
                    continue;
                }

                // 计算材料相似度
                let material_similarity =
                    calculate_material_similarity(&cmodel.materials, &model.materials);

                // 综合相似度：模具类型相似度权重0.3，材料相似度权重0.7
                let final_percentage = model_type_diff * 0.3 + material_similarity * 0.7;

                // 只有相似度超过阈值才加入结果
                if final_percentage > 0.1 {
                    results.push(DiffResult {
                        source_directory: cmodel.source_directory.clone(),
                        source_name: cmodel.source_directory_name.clone(),
                        percentage: final_percentage,
                    });
                }
            }
        }

        results
    }
}

/// 改进的文本相似度计算，优先全词匹配
pub fn improved_diff_text(text1: &str, text2: &str) -> f32 {
    let text1_clean = text1.trim();
    let text2_clean = text2.trim();

    // 完全相同
    if text1_clean == text2_clean {
        return 1.0;
    }

    // 如果任一为空
    if text1_clean.is_empty() || text2_clean.is_empty() {
        return 0.0;
    }

    // 检查是否有一个是另一个的子串
    if text1_clean.contains(text2_clean) || text2_clean.contains(text1_clean) {
        let shorter_len = text1_clean.len().min(text2_clean.len()) as f32;
        let longer_len = text1_clean.len().max(text2_clean.len()) as f32;
        return shorter_len / longer_len;
    }

    // 分词匹配作为后备方案
    let tokens1 = split_text_improved(text1_clean);
    let tokens2 = split_text_improved(text2_clean);

    diff_text_tokens(tokens1, tokens2)
}

/// 改进的分词，保留更多语义单元
pub fn split_text_improved(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current_token = String::new();

    for ch in text.chars() {
        if ch.is_whitespace() {
            if !current_token.is_empty() {
                tokens.push(current_token.clone());
                current_token.clear();
            }
        } else if ch == '-' || ch == '_' || ch == '.' {
            // 保留分隔符作为独立的token
            if !current_token.is_empty() {
                tokens.push(current_token.clone());
                current_token.clear();
            }
            tokens.push(ch.to_string());
        } else {
            current_token.push(ch);
        }
    }

    if !current_token.is_empty() {
        tokens.push(current_token);
    }

    tokens
}

/// 基于token的差异计算
pub fn diff_text_tokens(tokens1: Vec<String>, tokens2: Vec<String>) -> f32 {
    if tokens1.is_empty() && tokens2.is_empty() {
        return 1.0;
    }

    if tokens1.is_empty() || tokens2.is_empty() {
        return 0.0;
    }

    let mut matched_count = 0;
    let mut used_indices = Vec::new();

    // 首先尝试完全匹配
    for token1 in &tokens1 {
        for (idx, token2) in tokens2.iter().enumerate() {
            if !used_indices.contains(&idx) && token1 == token2 {
                matched_count += 1;
                used_indices.push(idx);
                break;
            }
        }
    }

    // 如果完全匹配数量较少，尝试字符级别匹配
    if matched_count < tokens1.len().min(tokens2.len()) / 2 {
        let char_similarity = diff_text(
            tokens1.join("").chars().map(|c| c.to_string()).collect(),
            tokens2.join("").chars().map(|c| c.to_string()).collect(),
        );

        // 取较高的相似度
        let token_similarity = matched_count as f32 / tokens1.len().max(tokens2.len()) as f32;
        return token_similarity.max(char_similarity * 0.8); // 字符匹配权重稍低
    }

    matched_count as f32 / tokens1.len().max(tokens2.len()) as f32
}

/// 计算材料列表的相似度
pub fn calculate_material_similarity(materials1: &[String], materials2: &[String]) -> f32 {
    if materials1.is_empty() || materials2.is_empty() {
        return 0.0;
    }

    // 过滤无效材料
    let valid_materials1: Vec<&String> = materials1
        .iter()
        .filter(|m| !is_invalid_material(m))
        .collect();

    let valid_materials2: Vec<&String> = materials2
        .iter()
        .filter(|m| !is_invalid_material(m))
        .collect();

    if valid_materials1.is_empty() || valid_materials2.is_empty() {
        return 0.0;
    }

    let mut total_similarity = 0.0;
    let mut match_count = 0;

    // 为每个材料找到最佳匹配
    for material1 in &valid_materials1 {
        let mut best_similarity = 0.0f32;

        for material2 in &valid_materials2 {
            let similarity = improved_diff_text(material1, material2);
            best_similarity = best_similarity.max(similarity);
        }

        // 只有相似度超过阈值才计入
        if best_similarity > 0.2 {
            total_similarity += best_similarity;
            match_count += 1;
        }
    }

    if match_count == 0 {
        return 0.0;
    }

    // 平均相似度，但要考虑匹配比例
    let avg_similarity = total_similarity / match_count as f32;
    let match_ratio =
        match_count as f32 / valid_materials1.len().max(valid_materials2.len()) as f32;

    avg_similarity * match_ratio
}

/// 判断是否为无效材料
pub fn is_invalid_material(material: &str) -> bool {
    let material_lower = material.trim().to_lowercase();
    material_lower.is_empty()
        || material_lower == "附"
        || material_lower == "附件"
        || material_lower == "附表"
        || material_lower == "见附件"
        || material_lower == "见附表"
        || material_lower.len() < 2 // 太短的材料名称可能无效
}

pub fn split_text(text: &str) -> Vec<String> {
    text.chars()
        .filter(|c| !c.is_whitespace()) // 过滤掉空白字符
        .map(|c| c.to_string())
        .collect()
}

pub fn diff_text(text1: Vec<String>, text2: Vec<String>) -> f32 {
    if text1.is_empty() && text2.is_empty() {
        return 1.0; // 两个空文本完全相似
    }

    if text1.is_empty() || text2.is_empty() {
        return 0.0; // 一个为空一个不为空，相似度为0
    }

    // 选择更长的文本作为基数
    let base_length = text1.len().max(text2.len()) as f32;

    // 创建较短文本的副本用于标记已匹配的字符
    let shorter_text = if text1.len() <= text2.len() {
        text1.clone()
    } else {
        text2.clone()
    };

    let longer_text = if text1.len() > text2.len() {
        &text1
    } else {
        &text2
    };

    let mut matched_count = 0;
    let mut used_indices = Vec::new(); // 记录已使用的索引

    // 遍历较长的文本，对每个字符在较短文本中寻找匹配
    for char_in_longer in longer_text {
        // 在较短文本中寻找未使用的匹配字符
        for (index, char_in_shorter) in shorter_text.iter().enumerate() {
            if !used_indices.contains(&index) && char_in_longer == char_in_shorter {
                matched_count += 1;
                used_indices.push(index); // 标记该索引已使用
                break; // 找到匹配后跳出内层循环
            }
        }
    }

    // 计算相似度：匹配数量 / 基数长度
    matched_count as f32 / base_length
}

/// 进行DIff之后的结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffResult {
    pub source_directory: PathBuf,
    pub source_name: String,
    /// 相似度
    pub percentage: f32,
}

impl DiffResult {
    pub fn sort(res: &mut Vec<Self>) -> () {
        res.sort_by(|a, b| {
            b.percentage // 注意这里改为降序排列，相似度高的在前面
                .partial_cmp(&a.percentage)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

impl PartialEq for DiffResult {
    fn eq(&self, other: &Self) -> bool {
        self.source_directory == other.source_directory
            && (self.percentage - other.percentage).abs() < f32::EPSILON
    }
}

impl Eq for DiffResult {}

impl PartialOrd for DiffResult {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.percentage.partial_cmp(&other.percentage)
    }
}

impl Ord for DiffResult {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap_or(std::cmp::Ordering::Equal)
    }
}

const MD_TEXT: &str = r#"
❗注意: 相似度是基于模具类型和材料的综合计算结果，值越高表示越相似。我们会返回相似度最高10个结果。
${result_table}
❗若遇到来源文件为`unknown`，说明该文件名称出错，请报告提交该错误
"#;

const MD_TABLE: &str = r#"
| 来源文件 | 相似度 |
| --- | --- |
| {$source} | {$percentage}% |
<img src="data:image/jpeg;base64,${base64_image}" height="400px" />
<a href="${href}">查看模型</a>
"#;

/// 将最后的结果转为markdown格式
pub fn fmt_diff_result_to_md(results: &Vec<DiffResult>) -> String {
    let mut md = String::new();
    md.push_str("对该pdf文件进行相似度比较的结果如下:\n");
    let img_dir = current_exe()
        .map_err(|e| format!("获取执行目录失败: {}", e))
        .unwrap()
        .parent()
        .ok_or("无法获取执行目录的父目录")
        .unwrap()
        .join("data")
        .join("upload")
        .join("file")
        .join("models")
        .join("imgs");
    // 处理表格
    // 如果相似度低于50%没有必要处理
    let result_table: String = results
        .iter()
        .take(if results.len() >= 10 {
            10
        } else {
            results.len()
        })
        .filter_map(|res| {
            let img_path = img_dir
                .join(&res.source_name)
                .join(format!("{}_page_001.png", res.source_name));
            if !img_path.exists() {
                return None;
            }

            let img = ImageReader::open(img_path).ok()?.decode().ok()?;
            let mut img_bytes = Vec::new();
            img.write_to(&mut Cursor::new(&mut img_bytes), image::ImageFormat::Png)
                .ok()?;
            let base64_image = BASE64_STANDARD.encode(img_bytes);
            // chatdm: format!("https://huateng.voce.chat/#/chat/dm/{}", cmodel.)
            Some(
                MD_TABLE
                    .replace("{$source}", &res.source_name)
                    .replace("{$percentage}", &format!("{:.2}", res.percentage * 100.0))
                    .replace("${base64_image}", &base64_image)
                    .replace(
                        "${href}",
                        &format!(
                            "https://huateng.voce.chat/#/compare?file_path={}",
                            res.source_name
                        ),
                    ),
            )
        })
        .collect();

    md.push_str(
        &MD_TEXT
            .to_string()
            .replace("${result_table}", &result_table),
    );

    md
}

fn fmt_diff_test(results: &Vec<DiffResult>) -> String {
    let mut md = String::new();
    md.push_str("对该pdf文件进行相似度比较的结果如下:\n");
    let img_dir =
        PathBuf::from("D:\\work\\material_rs\\target\\debug\\data\\upload\\file\\models\\imgs");
    // 处理表格
    // 如果相似度低于50%没有必要处理
    let result_table: String = results
        .iter()
        .take(if results.len() >= 10 {
            10
        } else {
            results.len()
        })
        .filter_map(|res| {
            let img_path = img_dir
                .join(&res.source_name)
                .join(format!("{}_page_001.png", res.source_name));
            if !img_path.exists() {
                return None;
            }
            let img = ImageReader::open(img_path).ok()?.decode().ok()?;
            let mut img_bytes = Vec::new();
            img.write_to(&mut Cursor::new(&mut img_bytes), image::ImageFormat::Png)
                .ok()?;
            let base64_image = BASE64_STANDARD.encode(img_bytes);

            Some(
                MD_TABLE
                    .replace("{$source}", &res.source_name)
                    .replace("{$percentage}", &format!("{:.2}", res.percentage * 100.0))
                    .replace("${base64_image}", &base64_image),
            )
        })
        .collect();

    md.push_str(
        &MD_TEXT
            .to_string()
            .replace("${result_table}", &result_table),
    );

    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff() {
        // D:\work\material\output\json\208T-03_A基座-A3_Model_1_text_data.json
        let model = ModelJson::new(PathBuf::from(
            "D:\\work\\material\\jsons\\SARN外壳_text_data.json",
        ))
        .unwrap();
        let models = ModelJson::patch_new(PathBuf::from("D:\\work\\material\\jsons")).unwrap();
        dbg!(models.len());
        let sorted_models = ModelJson::sort(models);
        let mut res = ModelJson::diff(sorted_models, model);
        DiffResult::sort(&mut res);
        let res = fmt_diff_result_to_md(&res);
        let md_file = "D:\\work\\material_rs\\test.md";
        fs::write(md_file, res).expect("Failed to write markdown file");
    }

    #[test]
    fn copy_split_text() {
        let text1 = "89(89";
        let result1 = split_text(text1);
        let text2 = "8989外壳";
        let result2 = split_text(text2);
        let res = diff_text(result1, result2);
        dbg!(res);
    }

    #[test]
    fn test_split_text() {
        let text1 = "*89基座";
        let result1 = split_text(text1);
        assert_eq!(result1, vec!["*", "8", "9", "基", "座"]);

        let text2 = "SEL4基座";
        let result2 = split_text(text2);
        assert_eq!(result2, vec!["S", "E", "L", "4", "基", "座"]);

        // 测试带空格的文本
        let text3 = "测试 文本";
        let result3 = split_text(text3);
        assert_eq!(result3, vec!["测", "试", "文", "本"]);
    }

    #[test]
    fn test_diff_text() {
        let text1 = split_text("*89基座");
        let text2 = split_text("SEL4基座");

        let similarity = diff_text(text1, text2);
        // 基数是较长的文本长度，这里都是6个字符
        // 匹配的字符：基、座（2个）
        // 相似度应该是 2/6 ≈ 0.333
        assert!((similarity - 0.333).abs() < 0.01);

        // 测试完全相同的文本
        let text3 = split_text("基座");
        let text4 = split_text("基座");
        let similarity2 = diff_text(text3, text4);
        assert_eq!(similarity2, 1.0);

        // 测试完全不同的文本
        let text5 = split_text("abc");
        let text6 = split_text("def");
        let similarity3 = diff_text(text5, text6);
        assert_eq!(similarity3, 0.0);

        // 测试空文本
        let empty1 = split_text("");
        let empty2 = split_text("");
        let similarity4 = diff_text(empty1, empty2);
        assert_eq!(similarity4, 1.0);

        let empty3 = split_text("");
        let text7 = split_text("测试");
        let similarity5 = diff_text(empty3, text7);
        assert_eq!(similarity5, 0.0);
    }

    #[test]
    fn test_improved_diff_text() {
        // 测试完全相同
        assert_eq!(improved_diff_text("PBT-RG301", "PBT-RG301"), 1.0);

        // 测试子串匹配
        let similarity1 = improved_diff_text("PBT", "PBT-RG301");
        assert!(similarity1 > 0.3 && similarity1 < 1.0);

        // 测试相似材料代码
        let similarity2 = improved_diff_text("PBT-RG301", "PBT-RG302");
        assert!(similarity2 > 0.8);

        // 测试完全不同
        let similarity3 = improved_diff_text("PBT", "ABS");
        assert!(similarity3 < 0.5);

        println!("PBT vs PBT-RG301: {}", similarity1);
        println!("PBT-RG301 vs PBT-RG302: {}", similarity2);
        println!("PBT vs ABS: {}", similarity3);
    }

    #[test]
    fn test_material_similarity() {
        let materials1 = vec!["PBT-RG301".to_string(), "ABS".to_string()];

        let materials2 = vec!["PBT-RG301".to_string(), "ABS-V0".to_string()];

        let similarity = calculate_material_similarity(&materials1, &materials2);
        println!("材料相似度: {}", similarity);
        assert!(similarity > 0.7); // 应该有较高的相似度

        // 测试完全不同的材料
        let materials3 = vec!["Steel".to_string(), "Aluminum".to_string()];

        let similarity2 = calculate_material_similarity(&materials1, &materials3);
        println!("不同材料相似度: {}", similarity2);
        assert!(similarity2 < 0.3); // 应该有较低的相似度
    }

    #[test]
    fn test_split_text_improved() {
        let result = split_text_improved("PBT-RG301");
        assert_eq!(result, vec!["PBT", "-", "RG301"]);

        let result2 = split_text_improved("ABS_V0.5");
        assert_eq!(result2, vec!["ABS", "_", "V0", ".", "5"]);

        println!("分词结果1: {:?}", result);
        println!("分词结果2: {:?}", result2);
    }
}
