use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};

use crate::{DomProcessor, PageLayout, Paginator, PdfRenderer, RenderReport, SnapshotDocument};

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum NativeRequest {
    #[serde(rename = "render_snapshot")]
    RenderSnapshot { snapshot: SnapshotDocument },
}

#[derive(Debug, Serialize)]
pub struct NativeResponse {
    pub ok: bool,
    pub filename: Option<String>,
    pub saved_path: Option<String>,
    pub report_path: Option<String>,
    pub page_count: Option<usize>,
    pub report: Option<RenderReport>,
    pub error: Option<String>,
}

pub fn run_native_host() -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    loop {
        let Some(message) = read_message(&mut reader)? else {
            break;
        };
        let response = match serde_json::from_slice::<NativeRequest>(&message) {
            Ok(request) => handle_request(request),
            Err(error) => NativeResponse {
                ok: false,
                filename: None,
                saved_path: None,
                report_path: None,
                page_count: None,
                report: None,
                error: Some(format!("invalid request JSON: {error}")),
            },
        };
        write_message(&mut writer, &serde_json::to_vec(&response)?)?;
    }

    Ok(())
}

fn handle_request(request: NativeRequest) -> NativeResponse {
    match render_request(request) {
        Ok((filename, saved_path, report_path, page_count, report)) => NativeResponse {
            ok: true,
            filename: Some(filename),
            saved_path: Some(saved_path.display().to_string()),
            report_path: report_path.map(|path| path.display().to_string()),
            page_count: Some(page_count),
            report: Some(report),
            error: None,
        },
        Err(error) => NativeResponse {
            ok: false,
            filename: None,
            saved_path: None,
            report_path: None,
            page_count: None,
            report: None,
            error: Some(error.to_string()),
        },
    }
}

fn render_request(
    request: NativeRequest,
) -> anyhow::Result<(String, PathBuf, Option<PathBuf>, usize, RenderReport)> {
    match request {
        NativeRequest::RenderSnapshot { snapshot } => {
            let parsed = DomProcessor::parse(&snapshot);
            let effective_title = snapshot
                .metadata
                .title
                .or(parsed.title.clone())
                .or(snapshot.metadata.url.map(|url| sanitize_filename(&url)))
                .unwrap_or_else(|| "singlepdf-document".to_string());
            let paginator = Paginator::new(PageLayout::default());
            let pages = paginator.paginate(&parsed.blocks);
            let (pdf, report) = PdfRenderer::bytes_and_report_from_pages(&pages)
                .with_context(|| format!("failed to render PDF for {}", effective_title))?;
            let filename = format!("{}.pdf", sanitize_filename(&effective_title));
            let saved_path = write_pdf_to_downloads(&filename, &pdf)?;
            let report_path = write_report_next_to_pdf(&saved_path, &report).ok();
            Ok((filename, saved_path, report_path, pages.len(), report))
        }
    }
}

fn read_message(reader: &mut impl Read) -> anyhow::Result<Option<Vec<u8>>> {
    let mut len_bytes = [0u8; 4];
    match reader.read_exact(&mut len_bytes) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error).context("failed to read native message length"),
    }
    let len = u32::from_le_bytes(len_bytes) as usize;
    if len == 0 {
        bail!("received empty native message");
    }
    let mut body = vec![0u8; len];
    reader
        .read_exact(&mut body)
        .context("failed to read native message body")?;
    Ok(Some(body))
}

fn write_message(writer: &mut impl Write, body: &[u8]) -> anyhow::Result<()> {
    let len = u32::try_from(body.len()).context("native response too large")?;
    writer
        .write_all(&len.to_le_bytes())
        .context("failed to write native response length")?;
    writer
        .write_all(body)
        .context("failed to write native response body")?;
    writer.flush().context("failed to flush native response")?;
    Ok(())
}

fn sanitize_filename(input: &str) -> String {
    let mut out = input
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string();
    if out.is_empty() {
        out = "singlepdf-document".to_string();
    }
    out
}

fn write_pdf_to_downloads(filename: &str, pdf: &[u8]) -> anyhow::Result<PathBuf> {
    let downloads_dir = downloads_dir().context("could not resolve the Downloads directory")?;
    fs::create_dir_all(&downloads_dir)
        .with_context(|| format!("failed to create {}", downloads_dir.display()))?;
    let output_path = unique_output_path(&downloads_dir, filename);
    fs::write(&output_path, pdf)
        .with_context(|| format!("failed to write PDF to {}", output_path.display()))?;
    Ok(output_path)
}

fn write_report_next_to_pdf(pdf_path: &Path, report: &RenderReport) -> anyhow::Result<PathBuf> {
    let mut report_path = pdf_path.to_path_buf();
    report_path.set_extension("report.json");
    let bytes =
        serde_json::to_vec_pretty(report).context("failed to serialize render report JSON")?;
    fs::write(&report_path, bytes)
        .with_context(|| format!("failed to write render report to {}", report_path.display()))?;
    Ok(report_path)
}

fn downloads_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Some(user_profile) = env::var_os("USERPROFILE") {
            return Some(PathBuf::from(user_profile).join("Downloads"));
        }
        let home_drive = env::var_os("HOMEDRIVE")?;
        let home_path = env::var_os("HOMEPATH")?;
        return Some(PathBuf::from(home_drive).join(home_path).join("Downloads"));
    }

    #[cfg(not(target_os = "windows"))]
    {
        env::var_os("HOME").map(|home| PathBuf::from(home).join("Downloads"))
    }
}

fn unique_output_path(directory: &Path, filename: &str) -> PathBuf {
    let candidate = directory.join(filename);
    if !candidate.exists() {
        return candidate;
    }

    let stem = Path::new(filename)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("singlepdf-document");
    let extension = Path::new(filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("pdf");

    for index in 2.. {
        let candidate = directory.join(format!("{stem} ({index}).{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!("the integer sequence for output filenames should not terminate")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_output_path_preserves_original_name_when_unused() {
        let base = PathBuf::from(r"C:\Temp\singlepdf-tests");
        let candidate = unique_output_path(&base, "article.pdf");
        assert_eq!(candidate, base.join("article.pdf"));
    }

    #[test]
    fn sanitize_filename_removes_reserved_characters() {
        assert_eq!(sanitize_filename(r#"Fox:News/Story*?"#), "Fox_News_Story__");
    }
}
