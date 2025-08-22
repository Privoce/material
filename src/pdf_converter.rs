use std::path::{Path, PathBuf};

use pdf2image::{DPI, PDF, Pages, RenderOptionsBuilder, image::ImageFormat};

use crate::{AnalyzerError, IResult};

/// 用于转化pdf为png图片的运行时
#[derive(Debug, Clone)]
pub struct PdfConverterRunner {
    /// pdf文件路径, 可以是文件或文件夹，如果是文件夹意味着需要转化所有的pdf文件
    pub path: PathBuf,
    /// 输出文件夹，默认为path的同级目录的/output文件夹，如果没有则创建
    pub output: PathBuf,
    pub is_dir: bool,
}

impl PdfConverterRunner {
    pub fn new<P1, P2>(path: P1, output: Option<P2>) -> Self
    where
        P1: AsRef<Path>,
        P2: AsRef<Path>,
    {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            panic!("Path {:?} does not exist", path);
        }

        let is_dir = path.is_dir();
        let output = if output.is_none() {
            if is_dir {
                path.join("output")
            } else {
                path.parent()
                    .map(|p| p.join("output"))
                    .unwrap_or_else(|| PathBuf::from("output"))
            }
        } else {
            output.unwrap().as_ref().to_path_buf()
        };

        if !output.exists() {
            std::fs::create_dir_all(&output).expect("Failed to create output directory");
        }

        Self {
            path,
            output,
            is_dir,
        }
    }
    /// 执行转换
    pub fn run(&self) -> IResult<()> {
        if self.is_dir {
            // 如果是目录，则遍历目录下的所有pdf文件
            for entry in std::fs::read_dir(&self.path)? {
                let entry = entry?;
                if entry.path().extension().and_then(|s| s.to_str()) == Some("pdf") {
                    let converter = PdfConverter::new(&entry.path(), &self.output);
                    // 这里可以调用转换方法
                    converter.run()?;
                }
            }
        } else {
            // 如果是单个文件，则直接转换
            let converter = PdfConverter::new(&self.path, &self.output);
            // 这里可以调用转换方法
            converter.run()?;
        }

        Ok(())
    }
}

/// 真正的Pdf转换器
pub struct PdfConverter {
    /// pdf文件路径
    pub path: PathBuf,
    pub output: PathBuf,
}

impl PdfConverter {
    pub fn new<P1, P2>(path: P1, output: P2) -> Self
    where
        P1: AsRef<Path>,
        P2: AsRef<Path>,
    {
        let path = path.as_ref().to_path_buf();
        let output = output.as_ref().to_path_buf();
        Self { path, output }
    }
    pub fn run(&self) -> IResult<()> {
        let name = self
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");
        dbg!(&self.path.display());
        let pdf = PDF::from_file(&self.path)
            .map_err(|e| AnalyzerError::PdfError(format!("Failed to load PDF: {}", e)))?;

        // 获取PDF页数
        let page_count = pdf.page_count();
        println!("PDF页数: {}", page_count);

        let option = RenderOptionsBuilder::default()
            .resolution(DPI::Uniform(300))
            .pdftocairo(true)
            .build()
            .map_err(|e| {
                AnalyzerError::PdfError(format!("Failed to build render options: {}", e))
            })?;

        // 根据实际页数渲染页面
        let pages = if page_count == 1 {
            pdf.render(Pages::Single(0), option)
                .map_err(|e| AnalyzerError::PdfError(format!("Failed to render PDF page: {}", e)))?
        } else {
            pdf.render(Pages::Range(0..=page_count - 1), option)
                .map_err(|e| {
                    AnalyzerError::PdfError(format!("Failed to render PDF pages: {}", e))
                })?
        };
        println!("实际渲染页数: {}", pages.len());

        for (index, page) in pages.iter().enumerate() {
            let filename = format!("page_{}.jpg", index);
            println!("保存图片: {}", filename);
            let output_dir = self.output.join(name);
            if !output_dir.exists() {
                std::fs::create_dir_all(&output_dir).map_err(|e| {
                    AnalyzerError::PdfError(format!("Failed to create output dir: {}", e))
                })?;
            }

            page.save_with_format(output_dir.join(&filename), ImageFormat::Jpeg)
                .map_err(|e| AnalyzerError::PdfError(format!("Failed to save image: {}", e)))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pdf_converter() {
        let runner = PdfConverterRunner::new(
            "D:\\work\\material_rs\\pdfs\\03-jz.pdf",
            Option::<String>::None,
        );
        // assert!(runner.run().is_ok());

        match runner.run() {
            Ok(_) => {
                dbg!(runner.output);
            }
            Err(e) => {
                dbg!(e.to_string());
            }
        }
    }

    // #[test]
    // fn test_pdf_converter_dir() {
    //     let runner = PdfConverterRunner::new("pdfs", None);
    //     assert!(runner.run().is_ok());
    // }
}
