use std::{
    env,
    ffi::OsString,
    fs,
    io::{stdin, stdout, IsTerminal},
    path::PathBuf,
};

use anyhow::{bail, Context};
use clap::Parser;
use singlepdf::{native_host, DomProcessor, HtmlSnapshot, PageLayout, Paginator, PdfRenderer, RenderReport};

#[derive(Parser, Debug)]
#[command(name = "singlepdf")]
#[command(about = "Convert frozen HTML into a paginated PDF or run as a native host")]
struct Cli {
    #[arg(long)]
    snapshot: Option<PathBuf>,

    #[arg(long)]
    html: Option<PathBuf>,

    #[arg(long)]
    url: Option<String>,

    #[arg(long)]
    out: Option<PathBuf>,

    #[arg(long)]
    browser_executable: Option<PathBuf>,

    #[arg(long)]
    native_host: bool,
}

fn main() -> anyhow::Result<()> {
    let stdin_is_terminal = stdin().is_terminal();
    let stdout_is_terminal = stdout().is_terminal();
    let raw_args = env::args_os().collect::<Vec<_>>();

    if should_run_native_host_early(&raw_args, stdin_is_terminal, stdout_is_terminal) {
        return native_host::run_native_host();
    }

    let cli = Cli::parse();
    if cli.native_host {
        return native_host::run_native_host();
    }

    let out = cli
        .out
        .as_ref()
        .context("`--out` is required unless native host mode is used")?;
    let snapshot = match (&cli.snapshot, &cli.html, &cli.url) {
        (Some(snapshot), None, None) => HtmlSnapshot::from_snapshot_file(snapshot)?,
        (None, Some(html), None) => HtmlSnapshot::from_html_file(html)?,
        (None, None, Some(url)) => {
            HtmlSnapshot::capture_from_url(url, cli.browser_executable.as_deref())?
        }
        (None, None, None) => bail!("one of `--snapshot`, `--html`, or `--url` is required"),
        _ => bail!("use only one of `--snapshot`, `--html`, or `--url`"),
    };
    let parsed = DomProcessor::parse(&snapshot);
    let title = parsed
        .title
        .clone()
        .unwrap_or_else(|| "Untitled document".to_string());
    let paginator = Paginator::new(PageLayout::default());
    let pages = paginator.paginate(&parsed.blocks);
    let (pdf, report) = PdfRenderer::bytes_and_report_from_pages(&pages)
        .with_context(|| format!("failed to render PDF for {}", title))?;
    fs::write(out, &pdf).with_context(|| format!("failed to write {}", out.display()))?;
    let report_path = write_report_file(out, &report)
        .with_context(|| format!("failed to write render report for {}", out.display()))?;
    println!(
        "Rendered '{}' to {} ({} pages, {} blocks) [report: {}]",
        title,
        out.display(),
        pages.len(),
        parsed.blocks.len(),
        report_path.display()
    );
    Ok(())
}

fn write_report_file(pdf_path: &std::path::Path, report: &RenderReport) -> anyhow::Result<PathBuf> {
    let mut report_path = pdf_path.to_path_buf();
    report_path.set_extension("report.json");
    let bytes = serde_json::to_vec_pretty(report).context("failed to serialize render report JSON")?;
    fs::write(&report_path, bytes)
        .with_context(|| format!("failed to write {}", report_path.display()))?;
    Ok(report_path)
}

fn should_run_native_host_early(
    args: &[OsString],
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
) -> bool {
    if stdin_is_terminal || stdout_is_terminal {
        return false;
    }

    let has_explicit_cli_mode = args.iter().skip(1).any(|arg| {
        matches!(
            arg.to_str(),
            Some("--snapshot")
                | Some("--html")
                | Some("--url")
                | Some("--out")
                | Some("--browser-executable")
                | Some("--help")
                | Some("-h")
        )
    });

    !has_explicit_cli_mode
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_native_host_mode_for_stdio_launch_without_args() {
        let args = vec![OsString::from("singlepdf")];
        assert!(should_run_native_host_early(&args, false, false));
    }

    #[test]
    fn does_not_infer_native_host_mode_for_interactive_shell() {
        let args = vec![OsString::from("singlepdf")];
        assert!(!should_run_native_host_early(&args, true, true));
    }

    #[test]
    fn explicit_render_args_take_precedence_over_stdio_inference() {
        let args = vec![
            OsString::from("singlepdf"),
            OsString::from("--snapshot"),
            OsString::from("page.json"),
            OsString::from("--out"),
            OsString::from("page.pdf"),
        ];
        assert!(!should_run_native_host_early(&args, false, false));
    }

    #[test]
    fn explicit_render_args_take_precedence_over_stdio_inference_for_html() {
        let args = vec![
            OsString::from("singlepdf"),
            OsString::from("--html"),
            OsString::from("page.html"),
            OsString::from("--out"),
            OsString::from("page.pdf"),
        ];
        assert!(!should_run_native_host_early(&args, false, false));
    }

    #[test]
    fn firefox_native_host_arguments_do_not_block_host_mode() {
        let args = vec![
            OsString::from("singlepdf"),
            OsString::from("manifest.json"),
            OsString::from("singlepdf@example.local"),
        ];
        assert!(should_run_native_host_early(&args, false, false));
    }
}
