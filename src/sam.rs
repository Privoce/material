//! ffi调用python处理图片进行分割
// use crate::{ config::SamConfig, AnalyzerError, IResult};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, error, info};

use crate::{AnalyzerError, IResult, config::SamConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamSplitResult {
    pub input_image: PathBuf,
    pub output_dir: PathBuf,
    pub view_files: Vec<PathBuf>,
    pub visualization_file: Option<PathBuf>,
    pub info_file: Option<PathBuf>,
}

pub struct SamInterface {
    config: SamConfig,
}

impl SamInterface {
    pub fn new(config: SamConfig) -> Self {
        Self { config }
    }

    /// Initialize Python and verify the SAM script exists
    pub fn verify_setup(&self) -> IResult<()> {
        // 检查Python脚本是否存在
        if !self.config.python_script_path.exists() {
            return Err(AnalyzerError::SamError(format!(
                "SAM Python script not found: {}",
                self.config.python_script_path.display()
            )));
        }

        // 检查SAM模型文件是否存在
        if !self.config.model_path.exists() {
            return Err(AnalyzerError::SamError(format!(
                "SAM model file not found: {}",
                self.config.model_path.display()
            )));
        }

        info!("SAM setup verified successfully");
        Ok(())
    }

    /// Split a single image using SAM via Python FFI
    pub async fn split_image<P1: AsRef<Path>, P2: AsRef<Path>>(
        &self,
        image_path: P1,
        output_dir: Option<P2>,
    ) -> IResult<SamSplitResult> {
        let image_path = image_path.as_ref();

        if !image_path.exists() {
            return Err(AnalyzerError::SamError(format!(
                "Input image does not exist: {}",
                image_path.display()
            )));
        }

        let output_dir = if let Some(dir) = output_dir {
            dir.as_ref().to_path_buf()
        } else {
            let stem = image_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            image_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(format!("{}_improved_views", stem))
        };

        info!("Splitting image: {}", image_path.display());
        info!("Output directory: {}", output_dir.display());

        // 使用spawn_blocking来运行Python代码，避免阻塞异步运行时
        let image_path = image_path.to_path_buf();
        let output_dir = output_dir.clone();
        let script_path = self.config.python_script_path.clone();
        let model_path = self.config.model_path.clone();
        let model_type = self.config.model_type.clone();

        let result = tokio::task::spawn_blocking(move || -> IResult<SamSplitResult> {
            let py_result = Python::with_gil(|py| -> PyResult<SamSplitResult> {
                // 添加脚本目录到Python路径
                let sys = py.import("sys")?;
                let path = sys.getattr("path")?;
                let script_dir = script_path.parent().ok_or_else(|| {
                    pyo3::exceptions::PyValueError::new_err("Invalid script path")
                })?;
                path.call_method1("insert", (0, script_dir.to_str().unwrap()))?;

                // 导入SAM分割模块
                let sam_module = py.import("sam_split_png")?;
                let splitter_class = sam_module.getattr("ImprovedSAMDrawingSplitter")?;

                // 创建分割器实例
                let kwargs = PyDict::new(py);
                kwargs.set_item("model_type", &model_type)?;
                kwargs.set_item("checkpoint_path", model_path.to_str().unwrap())?;

                let splitter = splitter_class.call((), Some(&kwargs))?;

                // 调用split_image方法
                let split_kwargs = PyDict::new(py);
                split_kwargs.set_item("image_path", image_path.to_str().unwrap())?;
                split_kwargs.set_item("output_dir", output_dir.to_str().unwrap())?;
                split_kwargs.set_item("visualize", true)?;

                debug!("Calling Python SAM split_image method...");
                let py_result = splitter.call_method("split_image", (), Some(&split_kwargs))?;

                // 解析Python返回的结果
                let saved_files: Vec<String> = py_result.extract()?;

                let view_files: Vec<PathBuf> = saved_files.into_iter().map(PathBuf::from).collect();

                // 查找可视化文件和信息文件
                let visualization_file = output_dir
                    .parent()
                    .map(|p| {
                        p.join(format!(
                            "{}_segmentation_results.png",
                            image_path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("unknown")
                        ))
                    })
                    .filter(|p| p.exists());

                let info_file = output_dir
                    .join("improved_views_info.json")
                    .exists()
                    .then(|| output_dir.join("improved_views_info.json"));

                info!(
                    "SAM splitting completed. Generated {} views",
                    view_files.len()
                );

                Ok(SamSplitResult {
                    input_image: image_path,
                    output_dir,
                    view_files,
                    visualization_file,
                    info_file,
                })
            });

            // 将PyErr转换为AnalyzerError
            py_result
                .map_err(|e| AnalyzerError::PythonError(format!("Python execution error: {}", e)))
        })
        .await
        .map_err(|e| AnalyzerError::SamError(format!("Blocking task failed: {}", e)))??;

        Ok(result)
    }

    /// Split multiple images in parallel
    pub async fn split_images_batch<P1: AsRef<Path>, P2: AsRef<Path>>(
        &self,
        image_paths: &[P1],
        output_base_dir: Option<P2>,
    ) -> IResult<Vec<SamSplitResult>> {
        if image_paths.is_empty() {
            return Ok(Vec::new());
        }

        info!(
            "Starting batch SAM splitting for {} images",
            image_paths.len()
        );

        let mut results = Vec::new();

        // 顺序处理以避免Python GIL问题和内存压力
        for (i, image_path) in image_paths.iter().enumerate() {
            let image_path = image_path.as_ref();
            info!(
                "Processing image {}/{}: {}",
                i + 1,
                image_paths.len(),
                image_path.display()
            );

            let output_dir = if let Some(base_dir) = output_base_dir.as_ref() {
                let stem = image_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                Some(base_dir.as_ref().join(stem))
            } else {
                None
            };

            match self.split_image(image_path, output_dir).await {
                Ok(result) => {
                    results.push(result);
                    info!("Successfully processed: {}", image_path.display());
                }
                Err(e) => {
                    error!("Failed to process {}: {}", image_path.display(), e);
                    // 继续处理其他图像
                }
            }
        }

        info!(
            "Batch SAM splitting completed. Processed {}/{} images successfully",
            results.len(),
            image_paths.len()
        );

        Ok(results)
    }

    /// Process all PNG files in a directory
    pub async fn split_directory<P: AsRef<Path>>(
        &self,
        png_directory: P,
        output_base_dir: Option<P>,
    ) -> IResult<Vec<SamSplitResult>> {
        let png_dir = png_directory.as_ref();

        if !png_dir.exists() || !png_dir.is_dir() {
            return Err(AnalyzerError::SamError(format!(
                "PNG directory does not exist or is not a directory: {}",
                png_dir.display()
            )));
        }

        // 收集所有PNG文件
        let mut png_files = Vec::new();
        let mut entries = tokio::fs::read_dir(png_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.to_lowercase() == "png")
                    .unwrap_or(false)
            {
                png_files.push(path);
            }
        }

        if png_files.is_empty() {
            return Err(AnalyzerError::SamError(format!(
                "No PNG files found in directory: {}",
                png_dir.display()
            )));
        }

        self.split_images_batch(&png_files, output_base_dir).await
    }
}

/// Utility function to check if Python and required modules are available
pub fn check_python_dependencies() -> IResult<()> {
    Python::with_gil(|py| {
        // 检查必要的Python模块
        let required_modules = vec![
            "torch",
            "torchvision",
            "cv2",
            "numpy",
            "PIL",
            "segment_anything",
            "sklearn",
        ];

        for module_name in required_modules {
            match py.import(module_name) {
                Ok(_) => debug!("✅ Python module '{}' available", module_name),
                Err(_) => {
                    return Err(AnalyzerError::PythonError(format!(
                        "Required Python module '{}' not found. Please install it.",
                        module_name
                    )));
                }
            }
        }

        info!("All Python dependencies are available");
        Ok(())
    })
}

