use std::{collections::HashMap, env::current_exe, fs, path::PathBuf};
use serde::{Deserialize, Serialize};

use crate::ai_text_analyzer::TextExtractionResult;

/// 用户查询结构
#[derive(Debug, Clone)]
pub struct UserQuery {
    pub model_type: Option<String>,
    pub materials: Vec<String>,
}

impl UserQuery {
    /// 解析用户查询字符串
    /// 支持格式：
    /// - model_type: 夹板;
    /// - material: PBT RG301;
    /// - model_type: 夹板; - material: PBT RG301;
    /// - material: PBT RG301, PA66 GF30;
    pub fn parse(input: &str) -> Self {
        let mut model_type = None;
        let mut materials = Vec::new();
        
        // 按分号分割不同的查询条件
        for part in input.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            
            // 检查是否是 model_type 查询
            if part.starts_with("- model_type:") || part.starts_with("-model_type:") || part.starts_with("model_type:") {
                let value_part = part.split(':').nth(1).unwrap_or("").trim();
                if !value_part.is_empty() {
                    model_type = Some(value_part.to_string());
                }
            }
            // 检查是否是 material 查询
            else if part.starts_with("- material:") || part.starts_with("-material:") || part.starts_with("material:") {
                let value_part = part.split(':').nth(1).unwrap_or("").trim();
                if !value_part.is_empty() {
                    // 按逗号分割多个材料
                    for material in value_part.split(',') {
                        let material = material.trim();
                        if !material.is_empty() {
                            materials.push(material.to_string());
                        }
                    }
                }
            }
        }
        
        Self { model_type, materials }
    }
}

/// 模具类型分组定义
const MODEL_TYPE_GROUPS: &[&[&str]] = &[
    &["夹板", "底板", "绝缘隔板", "绝缘板", "衔铁托板", "外板", "定位件", "磁体盖", "固定板"],
    &["衔铁组件", "组件", "动组件", "动簧片组件"],
    &["基座", "上基座", "外基座", "内基座", "底座"],
    &["推杆", "推片", "推动片"],
    &["骨架", "线圈架"],
];

/// 材料分组定义 - 按主要材料类型分组
const MATERIAL_GROUPS: &[&[&str]] = &[
    // PBT 系列
    &["PBT", "PBT RG301", "PBT RG530", "PBT 3316", "PBT 4130", "PBT 543", "PBT 1403G6", "PBT 1430", "PBT FR530", "PBT 102G30", "PBT E202G30", "PBT 201G20", "PBT 5010GN6", "PBT R212G30GT", "PBT T102G30"],
    // PET 系列  
    &["PET", "PET FR530", "PET RG301", "PET RG305", "PET T102G30", "PET FRG30", "PET EMC", "PET FG550", "PET FR830", "PET-FR530", "PET-FR531", "PET FR533NH", "PET FRF520"],
    // PA66 系列 (尼龙66)
    &["PA66", "尼龙 PA66", "PA66 RG301", "PA66 NPG30", "PA66 K225-KS", "PA66 FR50", "PA66 RG251", "PA66 EPR27", "PA66 T303", "PA66 A3", "PA66+GF", "PA66-B30"],
    // PA6 系列 (尼龙6)
    &["PA6", "尼龙 PA6", "PA6 C0-FKGS6", "PA6 K-FKGS6", "PA6-GF30", "PA6 GF30"],
    // PA46 系列
    &["PA46", "PA46-GF30", "PA46 TE250F6", "PA46 TE250F8"],
    // PC 系列 (聚碳酸酯)
    &["PC", "PC 3001-33201", "PC PC3001-33201L", "PC 121R", "PC FR7"],
    // LCP 系列 (液晶聚合物)
    &["LCP", "LCP-4008", "LCP E4008", "LCP E130i"],
    // PPS 系列 (聚苯硫醚)
    &["PPS", "PPS R-7", "PPS B4200", "PPS 6165", "PPS 4500", "PPS R-4"],
    // 其他特殊材料
    &["PPA", "PEI", "PTFE", "PA4T", "TPE", "磁钢", "衔铁", "再生材"],
];

/// 标准化材料名称，提取主要材料类型和型号
fn normalize_material_name(material: &str) -> (String, String) {
    let material = material.trim();
    let material_upper = material.to_uppercase();
    
    // 移除常见的颜色和属性描述
    let color_keywords = ["黑色", "白色", "本色", "蓝色", "绿色", "BLACK", "WHITE", "BK", "BL", "WH"];
    let property_keywords = ["阻燃", "FR", "UL94", "V-0", "ROHS", "防紫外线", "GF", "BY", "DUPONT", "DSM", "帝斯曼", "金发", "南亚", "东方", "沙伯基础", "美国杜邦"];
    
    let mut base_material = material_upper.clone();
    
    // 移除颜色描述
    for color in &color_keywords {
        base_material = base_material.replace(color, "").trim().to_string();
    }
    
    // 移除属性描述
    for prop in &property_keywords {
        base_material = base_material.replace(prop, "").trim().to_string();
    }
    
    // 移除括号内容
    if let Some(paren_pos) = base_material.find('(') {
        base_material = base_material[..paren_pos].trim().to_string();
    }
    
    // 提取主要材料类型
    let main_type = if base_material.starts_with("PBT") {
        "PBT".to_string()
    } else if base_material.starts_with("PET") {
        "PET".to_string()
    } else if base_material.contains("PA66") || base_material.contains("尼龙 PA66") {
        "PA66".to_string()
    } else if base_material.contains("PA6") || base_material.contains("尼龙 PA6") {
        "PA6".to_string()
    } else if base_material.starts_with("PA46") {
        "PA46".to_string()
    } else if base_material.starts_with("PC") {
        "PC".to_string()
    } else if base_material.starts_with("LCP") {
        "LCP".to_string()
    } else if base_material.starts_with("PPS") {
        "PPS".to_string()
    } else {
        // 对于其他材料，取第一个单词作为主类型
        base_material.split_whitespace().next().unwrap_or("UNKNOWN").to_string()
    };
    
    // 清理并返回完整的标准化名称和主类型
    let normalized_full = base_material.replace("  ", " ").trim().to_string();
    (main_type, normalized_full)
}

/// 标准化模具类型名称，去除前缀后缀
fn normalize_model_type(model_type: &str) -> String {
    let model_type = model_type.trim();
    
    // 去除常见的前缀和后缀模式
    let mut normalized = model_type.to_string();
    
    // 去除型号后缀（如 -047, -H, -W 等）
    if let Some(dash_pos) = normalized.rfind('-') {
        let after_dash = &normalized[dash_pos + 1..];
        // 如果破折号后面是数字、字母组合或单个字母，则去除
        // 扩大范围以处理像 "1A型" 这样的情况
        if after_dash.chars().all(|c| c.is_alphanumeric() || c == '型') && after_dash.len() <= 6 {
            normalized = normalized[..dash_pos].to_string();
        }
    }
    
    // 去除括号内容（如 (60A), (C型) 等）
    if let Some(paren_pos) = normalized.find('(') {
        normalized = normalized[..paren_pos].trim().to_string();
    }
    
    // 去除常见前缀（如 HAT904G, HAG12 等产品代码）
    let prefixes_to_remove = ["HAT904G", "HAT902", "HAT905G", "HAG02", "HAG12", "ZC75N", "Y3F"];
    for prefix in &prefixes_to_remove {
        if normalized.starts_with(prefix) {
            normalized = normalized[prefix.len()..].trim().to_string();
            break;
        }
    }
    
    // 去除其他常见词汇
    let words_to_remove = ["护套", "外壳", "盖板", "支架", "组件"];
    for word in &words_to_remove {
        if normalized.ends_with(word) && normalized.len() > word.len() {
            normalized = normalized[..normalized.len() - word.len()].trim().to_string();
        }
    }
    
    normalized.trim().to_string()
}

/// 获取模具类型所属的分组
fn get_model_type_group(model_type: &str) -> Option<usize> {
    let normalized = normalize_model_type(model_type);
    
    for (group_index, group) in MODEL_TYPE_GROUPS.iter().enumerate() {
        for &standard_type in *group {
            // 检查标准化后的类型是否包含分组中的标准类型
            if normalized.contains(standard_type) || standard_type.contains(&normalized) {
                return Some(group_index);
            }
        }
    }
    None
}

/// 计算模具类型相似度，考虑分组和标准化
fn calculate_model_type_similarity(type1: &str, type2: &str) -> f32 {
    // 如果完全相同，返回最高相似度
    if type1 == type2 {
        return 1.0;
    }
    
    let normalized1 = normalize_model_type(type1);
    let normalized2 = normalize_model_type(type2);
    
    // 标准化后完全相同，相似度很高但稍低于完全匹配
    if normalized1 == normalized2 {
        return 0.95;
    }
    
    // 检查是否属于同一分组
    let group1 = get_model_type_group(type1);
    let group2 = get_model_type_group(type2);
    
    match (group1, group2) {
        (Some(g1), Some(g2)) if g1 == g2 => {
            // 同一分组内，根据标准化后的文本相似度计算
            let text_similarity = improved_diff_text(&normalized1, &normalized2);
            // 同组内相似度基础分 0.6，加上文本相似度的 40%
            0.6 + text_similarity * 0.4
        }
        (Some(_), Some(_)) => {
            // 不同分组，但都是已知类型，相似度为0
            0.0
        }
        _ => {
            // 至少有一个不在已知分组中，使用原始文本比较
            improved_diff_text(type1, type2) * 0.5 // 未知类型降权
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelJson {
    pub model_type: Option<String>,
    pub materials: Vec<String>,
    pub project_name: Option<String>,
    pub source_directory: PathBuf,
    pub source_directory_name: String,
    pub extraction_timestamp: Option<String>,
}

impl ToString for ModelJson {
    fn to_string(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
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
            // 使用改进的模具类型相似度计算
            let model_type_diff = calculate_model_type_similarity(
                &model_type,
                &(model.model_type.clone().unwrap_or("unknown".to_string())),
            );

            // 如果模具类型相似度太低，直接跳过
            if model_type_diff < 0.15 {
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

    /// 根据模具类型搜索
    pub fn search_model_type<'a>(models: &'a [Self], query_type: &str) -> Vec<&'a Self> {
        models
            .iter()
            .filter(|model| {
                if let Some(model_type) = &model.model_type {
                    // 使用改进的相似度计算
                    let similarity = calculate_model_type_similarity(model_type, query_type);
                    similarity > 0.5 // 相似度阈值
                } else {
                    false
                }
            })
            .collect()
    }

    /// 根据材料搜索
    pub fn search_materials<'a>(models: &'a [Self], query_materials: &[String]) -> Vec<&'a Self> {
        models
            .iter()
            .filter(|model| {
                if model.materials.is_empty() || query_materials.is_empty() {
                    return false;
                }
                
                // 计算材料相似度
                let similarity = calculate_material_similarity(&model.materials, query_materials);
                similarity > 0.3 // 材料相似度阈值
            })
            .collect()
    }

    /// 综合搜索（模具类型和材料）
    pub fn search_combined<'a>(models: &'a [Self], query: &UserQuery) -> Vec<&'a Self> {
        models
            .iter()
            .filter(|model| {
                let mut type_match = true;
                let mut material_match = true;
                
                // 检查模具类型匹配
                if let Some(query_type) = &query.model_type {
                    if let Some(model_type) = &model.model_type {
                        let similarity = calculate_model_type_similarity(model_type, query_type);
                        type_match = similarity > 0.5;
                    } else {
                        type_match = false;
                    }
                }
                
                // 检查材料匹配
                if !query.materials.is_empty() {
                    if !model.materials.is_empty() {
                        let similarity = calculate_material_similarity(&model.materials, &query.materials);
                        material_match = similarity > 0.3;
                    } else {
                        material_match = false;
                    }
                }
                
                type_match && material_match
            })
            .collect()
    }
}

/// 格式化搜索结果为 Markdown 表格
pub fn format_search_results_to_md(results: &[&ModelJson]) -> String {
    if results.is_empty() {
        return "未找到匹配的模具数据。".to_string();
    }
    
    let mut md = String::new();
    md.push_str("| 名称 | 模具类型 | 材料 |\n");
    md.push_str("|--|--|--|\n");
    
    for model in results.iter().take(20) { // 限制最多显示20条结果
        let name = &model.source_directory_name;
        let model_type = model.model_type.as_deref().unwrap_or("未知");
        let materials = if model.materials.is_empty() {
            "无".to_string()
        } else {
            model.materials.join(", ")
        };
        
        md.push_str(&format!("| {} | {} | {} |\n", name, model_type, materials));
    }
    
    if results.len() > 20 {
        md.push_str(&format!("\n*注: 仅显示前20条结果，共找到{}条记录*\n", results.len()));
    }
    
    md
}

/// 处理用户查询并返回 Markdown 格式结果
pub fn handle_user_search(models: &[ModelJson], query_text: &str) -> String {
    let query = UserQuery::parse(query_text);
    
    let results = if query.model_type.is_some() && !query.materials.is_empty() {
        // 综合搜索
        ModelJson::search_combined(models, &query)
    } else if let Some(model_type) = &query.model_type {
        // 仅搜索模具类型
        ModelJson::search_model_type(models, model_type)
    } else if !query.materials.is_empty() {
        // 仅搜索材料
        ModelJson::search_materials(models, &query.materials)
    } else {
        // 无效查询
        return "查询格式错误，请参考以下格式：\n\
1. 查找类型：`- model_type: 夹板;`\n\
2. 查找材料：`- material: PBT RG301;`\n\
3. 查找类型和材料：`- model_type: 夹板; - material: PBT RG301;`\n\
4. 多个材料：`- material: PBT RG301, PA66 GF30;`".to_string();
    };
    
    format_search_results_to_md(&results)
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

/// 计算材料列表的相似度 - 改进版本，基于材料分组和精确匹配
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

    // 标准化材料
    let normalized1: Vec<(String, String)> = valid_materials1
        .iter()
        .map(|m| normalize_material_name(m))
        .collect();
    
    let normalized2: Vec<(String, String)> = valid_materials2
        .iter()
        .map(|m| normalize_material_name(m))
        .collect();

    let mut total_similarity = 0.0;
    let mut exact_matches = 0;
    let mut type_matches = 0;

    // 为每个材料在第一个列表中找到最佳匹配
    for (main_type1, full_name1) in &normalized1 {
        let mut best_similarity = 0.0f32;
        let mut found_exact = false;
        let mut found_type = false;

        for (main_type2, full_name2) in &normalized2 {
            // 1. 精确匹配 (去除颜色后的完整名称相同)
            if full_name1 == full_name2 {
                best_similarity = 1.0;
                found_exact = true;
                break;
            }
            
            // 2. 主材料类型匹配且型号相似
            if main_type1 == main_type2 {
                found_type = true;
                // 对于同类型材料，比较具体型号
                let similarity = calculate_material_type_similarity(full_name1, full_name2);
                best_similarity = best_similarity.max(similarity);
            }
        }

        if found_exact {
            exact_matches += 1;
            total_similarity += 1.0;
        } else if found_type && best_similarity > 0.7 {
            // 同类型且高相似度才算匹配
            type_matches += 1;
            total_similarity += best_similarity;
        }
        // 如果连主材料类型都不匹配，则不计入相似度
    }

    let total_matches = exact_matches + type_matches;
    if total_matches == 0 {
        return 0.0;
    }

    // 计算平均相似度，并考虑匹配比例
    let avg_similarity = total_similarity / total_matches as f32;
    let match_ratio = total_matches as f32 / normalized1.len().max(normalized2.len()) as f32;

    // 精确匹配权重更高
    let exact_weight = exact_matches as f32 / total_matches as f32;
    let final_similarity = avg_similarity * match_ratio;
    
    // 如果有精确匹配，提升整体相似度
    if exact_matches > 0 {
        final_similarity * (0.8 + 0.2 * exact_weight)
    } else {
        final_similarity * 0.8 // 仅类型匹配的降权
    }
}

/// 计算同类型材料的具体相似度
fn calculate_material_type_similarity(material1: &str, material2: &str) -> f32 {
    // 对于同主类型的材料，进行更细致的比较
    
    // 1. 完全相同
    if material1 == material2 {
        return 1.0;
    }
    
    // 2. 提取材料型号进行比较
    let tokens1: Vec<&str> = material1.split_whitespace().collect();
    let tokens2: Vec<&str> = material2.split_whitespace().collect();
    
    if tokens1.is_empty() || tokens2.is_empty() {
        return 0.0;
    }
    
    // 如果第一个token（主材料类型）相同
    if tokens1[0] == tokens2[0] {
        // 比较后续的型号部分
        let remaining1: Vec<&str> = tokens1.iter().skip(1).cloned().collect();
        let remaining2: Vec<&str> = tokens2.iter().skip(1).cloned().collect();
        
        if remaining1.is_empty() && remaining2.is_empty() {
            return 1.0; // 都只有主类型，完全匹配
        }
        
        if remaining1.is_empty() || remaining2.is_empty() {
            return 0.8; // 一个有型号一个没有，部分匹配
        }
        
        // 比较型号的相似度
        let type_similarity = improved_diff_text(&remaining1.join(" "), &remaining2.join(" "));
        
        // 对于同主类型，型号相似度要求更高
        if type_similarity > 0.8 {
            return 0.9 + type_similarity * 0.1;
        } else if type_similarity > 0.5 {
            return 0.7 + type_similarity * 0.2;
        } else {
            return type_similarity * 0.5; // 型号差异太大，降权
        }
    }
    
    // 不同主类型，相似度很低
    0.1
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
// <img src="data:image/jpeg;base64,${base64_image}" height="400px" />
const MD_TABLE: &str = r#"
| 来源文件 | 相似度 |
| --- | --- |
| {$source} | {$percentage}% |
<img src="${img_path}" width="400px" />
<a href="${href}">查看模型</a>
"#;

const NO_RESULT_TEXT: &str = r#"
未找到相似的模具文件，可能是该模具类型或材料较为特殊或AI识别有误，建议手动检查。
```
${model_data}
```
您可以直接使用以下方式让AI BOT帮您进行搜索：
1. 查找类型：`- model_type: 夹板;`
2. 查找材料：`- material: PBT RG301;`
3. 查找类型和材料：`- model_type: 夹板; - material: PBT RG301;`
4. 多个材料：`- material: PBT RG301, PA66 GF30;`
"#;

/// 将最后的结果转为markdown格式
pub fn fmt_diff_result_to_md(results: &Vec<DiffResult>, model_data: Option<String>) -> String {
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
                .join(format!("{}_page_001", res.source_name));

            if !img_path.exists() {
                return None;
            }
            
            Some(
                MD_TABLE
                    .replace("{$source}", &res.source_name)
                    .replace("{$percentage}", &format!("{:.2}", res.percentage * 100.0))
                    .replace(
                        "${img_path}",
                        &format!(
                            "https://huateng.voce.chat/api/resource/file?file_path=models/imgs/{}/{}_page_001",
                            &res.source_name, &res.source_name
                        ),
                    )
                    .replace(
                        "${href}",
                        &format!(
                            "http://45.76.31.59:3009/#/compare?file_path={}",
                            res.source_name
                        ),
                    ),
            )
        })
        .collect();

    if result_table.is_empty() {
        // 如果没有有效结果，返回提示信息
        md.push_str(
            &NO_RESULT_TEXT
                .to_string()
                .replace("${model_data}", model_data.as_deref().unwrap_or("无")),
        );

    }else{
        md.push_str(
        &MD_TEXT
            .to_string()
            .replace("${result_table}", &result_table),
    );
    }

    md
}

fn fmt_diff_test(results: &Vec<DiffResult>) -> String {
    let mut md = String::new();
    md.push_str("对该pdf文件进行相似度比较的结果如下:\n");
    let img_dir =
        PathBuf::from("D:\\work\\material_rs\\target\\debug\\data\\upload\\file\\models\\imgs");
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
            
            Some(
                MD_TABLE
                    .replace("{$source}", &res.source_name)
                    .replace("{$percentage}", &format!("{:.2}", res.percentage * 100.0))
                    .replace(
                        "${img_path}",
                        &format!(
                            "https://huateng.voce.chat/api/resource/file?file_path=models/imgs/{}/{}_page_001",
                            &res.source_name, &res.source_name
                        ),
                    )
                    .replace(
                        "${href}",
                        &format!(
                            "http://45.76.31.59:3009/#/compare?file_path={}",
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn all_models() {
        let models = ModelJson::patch_new(PathBuf::from(
            "D:\\work\\material_rs\\target\\debug\\data\\upload\\file\\models\\jsons",
        ))
        .unwrap();

        let mut set = HashSet::new();

        for m in models.iter() {
            for material in &m.materials {
                set.insert(material.to_string());
            }
        }

        println!("{:?}", set);
    }

    #[test]
    fn diff() {
        // D:\work\material\output\json\208T-03_A基座-A3_Model_1_text_data.json
        let model = ModelJson::new(PathBuf::from(
            "D:\\work\\material_rs\\target\\debug\\data\\upload\\file\\models\\jsons\\HJ034-2PZL内基座_text_data.json",
        ))
        .unwrap();
        let models = ModelJson::patch_new(PathBuf::from(
            "D:\\work\\material_rs\\target\\debug\\data\\upload\\file\\models\\jsons",
        ))
        .unwrap();
        let sorted_models = ModelJson::sort(models);
        let mut res = ModelJson::diff(sorted_models, model);
        DiffResult::sort(&mut res);
        
        let res = fmt_diff_test(&res);
        dbg!(&res);
        // let md_file = "D:\\work\\material_rs\\test.md";
        // fs::write(md_file, res).expect("Failed to write markdown file");
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
    fn test_normalize_model_type() {
        // 测试去除型号后缀
        assert_eq!(normalize_model_type("基座-047"), "基座");
        assert_eq!(normalize_model_type("外壳-H"), "外壳");
        assert_eq!(normalize_model_type("上盖-050"), "上盖");
        
        // 测试去除括号内容
        assert_eq!(normalize_model_type("底座(1常开1常闭型)"), "底座");
        assert_eq!(normalize_model_type("ZC75N基座(60A-ASSLY带护针)"), "基座");
        
        // 测试去除产品代码前缀
        assert_eq!(normalize_model_type("HAT904G 基座"), "基座");
        assert_eq!(normalize_model_type("HAG12线圈架"), "线圈架");
        assert_eq!(normalize_model_type("Y3F骨架"), "骨架");
        
        // 测试复杂情况
        assert_eq!(normalize_model_type("HAT904G 外壳"), "外壳");
        assert_eq!(normalize_model_type("基座-1A型"), "基座");
    }

    #[test]
    fn test_get_model_type_group() {
        // 测试基座分组
        assert_eq!(get_model_type_group("基座"), Some(2));
        assert_eq!(get_model_type_group("上基座"), Some(2));
        assert_eq!(get_model_type_group("HAT904G 基座"), Some(2));
        
        // 测试骨架分组
        assert_eq!(get_model_type_group("骨架"), Some(4));
        assert_eq!(get_model_type_group("线圈架"), Some(4));
        assert_eq!(get_model_type_group("HAG12线圈架"), Some(4));
        
        // 测试组件分组
        assert_eq!(get_model_type_group("衔铁组件"), Some(1));
        assert_eq!(get_model_type_group("动簧片组件"), Some(1));
        
        // 测试未知类型
        assert_eq!(get_model_type_group("未知类型"), None);
    }

    #[test]
    fn test_calculate_model_type_similarity() {
        // 测试完全相同
        assert_eq!(calculate_model_type_similarity("基座", "基座"), 1.0);
        
        // 测试标准化后相同
        let sim1 = calculate_model_type_similarity("基座-047", "基座");
        assert!((sim1 - 0.95).abs() < 0.01);
        
        // 测试同组内相似
        let sim2 = calculate_model_type_similarity("基座", "上基座");
        assert!(sim2 > 0.6 && sim2 < 1.0);
        
        // 测试不同组 - 现在应该是0.0
        let sim3 = calculate_model_type_similarity("基座", "骨架");
        assert_eq!(sim3, 0.0);
        
        // 测试复杂情况
        let sim4 = calculate_model_type_similarity("HAT904G 基座", "基座-047");
        assert!(sim4 > 0.9);
        
        // 测试更多不同分组的情况
        let sim5 = calculate_model_type_similarity("衔铁组件", "推杆");
        assert_eq!(sim5, 0.0);
        
        let sim6 = calculate_model_type_similarity("底板", "线圈架");
        assert_eq!(sim6, 0.0);
        
        println!("基座-047 vs 基座: {}", sim1);
        println!("基座 vs 上基座: {}", sim2);
        println!("基座 vs 骨架: {}", sim3);
        println!("HAT904G 基座 vs 基座-047: {}", sim4);
        println!("衔铁组件 vs 推杆: {}", sim5);
        println!("底板 vs 线圈架: {}", sim6);
    }

    #[test]
    fn test_user_query_parse() {
        // 测试解析模具类型查询
        let query1 = UserQuery::parse("- model_type: 夹板;");
        assert_eq!(query1.model_type, Some("夹板".to_string()));
        assert!(query1.materials.is_empty());
        
        // 测试解析材料查询
        let query2 = UserQuery::parse("- material: PBT RG301;");
        assert!(query2.model_type.is_none());
        assert_eq!(query2.materials, vec!["PBT RG301"]);
        
        // 测试解析综合查询
        let query3 = UserQuery::parse("- model_type: 夹板; - material: PBT RG301;");
        assert_eq!(query3.model_type, Some("夹板".to_string()));
        assert_eq!(query3.materials, vec!["PBT RG301"]);
        
        // 测试解析多个材料
        let query4 = UserQuery::parse("- material: PBT RG301, PA66 GF30;");
        assert!(query4.model_type.is_none());
        assert_eq!(query4.materials, vec!["PBT RG301", "PA66 GF30"]);
        
        // 测试不同格式
        let query5 = UserQuery::parse("model_type: 基座; material: ABS;");
        assert_eq!(query5.model_type, Some("基座".to_string()));
        assert_eq!(query5.materials, vec!["ABS"]);
    }

    #[test]
    fn test_search_functionality() {
        // 创建测试数据
        let models = vec![
            ModelJson {
                model_type: Some("基座".to_string()),
                materials: vec!["PBT-RG301".to_string(), "ABS".to_string()],
                project_name: Some("项目A".to_string()),
                source_directory: PathBuf::from("test1"),
                source_directory_name: "test1".to_string(),
                extraction_timestamp: None,
            },
            ModelJson {
                model_type: Some("外壳".to_string()),
                materials: vec!["PA66".to_string()],
                project_name: Some("项目B".to_string()),
                source_directory: PathBuf::from("test2"),
                source_directory_name: "test2".to_string(),
                extraction_timestamp: None,
            },
            ModelJson {
                model_type: Some("上基座".to_string()),
                materials: vec!["PBT-RG302".to_string()],
                project_name: Some("项目C".to_string()),
                source_directory: PathBuf::from("test3"),
                source_directory_name: "test3".to_string(),
                extraction_timestamp: None,
            },
        ];
        
        // 测试按类型搜索
        let type_results = ModelJson::search_model_type(&models, "基座");
        assert!(type_results.len() >= 1); // 应该找到基座和上基座
        
        // 测试按材料搜索
        let material_results = ModelJson::search_materials(&models, &vec!["PBT".to_string()]);
        assert!(material_results.len() >= 1); // 应该找到包含PBT的材料
        
        // 测试综合搜索
        let query = UserQuery {
            model_type: Some("基座".to_string()),
            materials: vec!["PBT".to_string()],
        };
        let combined_results = ModelJson::search_combined(&models, &query);
        assert!(!combined_results.is_empty());
        
        println!("类型搜索结果: {}", type_results.len());
        println!("材料搜索结果: {}", material_results.len());
        println!("综合搜索结果: {}", combined_results.len());
    }

    #[test]
    fn test_format_search_results() {
        let models = vec![
            ModelJson {
                model_type: Some("基座".to_string()),
                materials: vec!["PBT-RG301".to_string()],
                project_name: None,
                source_directory: PathBuf::from("test1"),
                source_directory_name: "test1".to_string(),
                extraction_timestamp: None,
            },
        ];
        
        let model_refs: Vec<&ModelJson> = models.iter().collect();
        let md_result = format_search_results_to_md(&model_refs);
        
        assert!(md_result.contains("| 名称 | 模具类型 | 材料 |"));
        assert!(md_result.contains("test1"));
        assert!(md_result.contains("基座"));
        assert!(md_result.contains("PBT-RG301"));
        
        println!("格式化结果:\n{}", md_result);
    }

    #[test]
    fn test_text_search_combine() {
        let models = ModelJson::patch_new(PathBuf::from(
            "D:\\work\\material_rs\\target\\debug\\data\\upload\\file\\models\\jsons",
        ))
        .unwrap();
        // let sorted_models = ModelJson::sort(models);
        let query_text = "- model_type: 基座; - material: PBT RG301;";
        let UserQuery { model_type, materials } = UserQuery::parse(query_text);
        let res = ModelJson::search_combined(models.as_ref(), &UserQuery { model_type, materials });
        let md_res = format_search_results_to_md(&res);
        dbg!(&md_res);
    }

    #[test]
    fn test_text_search_material() {
        let models = ModelJson::patch_new(PathBuf::from(
            "D:\\work\\material_rs\\target\\debug\\data\\upload\\file\\models\\jsons",
        ))
        .unwrap();
        // let sorted_models = ModelJson::sort(models);
        let query_text = "- material: PBT RG301;";
        let UserQuery {  materials, .. } = UserQuery::parse(query_text);
        let res = ModelJson::search_materials(models.as_ref(), &materials);
        let md_res = format_search_results_to_md(&res);
        dbg!(&md_res);
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

    #[test]
    fn copy_meta_to_png() {
        // let path = "D:\\work\\material_rs\\target\\debug\\data\\upload\\file\\models\\imgs";
        // 将path下所有文件夹下的图片元数据进行复制并增加后缀名.png
    }
}
