use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context};

use crate::model::{SnapshotDocument, SnapshotMetadata, SnapshotSource};

pub struct HtmlSnapshot;

impl HtmlSnapshot {
    pub fn from_file(path: impl AsRef<Path>) -> anyhow::Result<SnapshotDocument> {
        let path = path.as_ref();
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("json"))
        {
            Some(true) => Self::from_snapshot_file(path),
            _ => Self::from_html_file(path),
        }
    }

    pub fn from_snapshot_file(path: impl AsRef<Path>) -> anyhow::Result<SnapshotDocument> {
        let path = path.as_ref();
        let json = fs::read_to_string(path)
            .with_context(|| format!("failed to read snapshot JSON from {}", path.display()))?;
        let mut snapshot: SnapshotDocument =
            serde_json::from_str(&json).with_context(|| {
                format!("failed to parse snapshot JSON from {}", path.display())
            })?;
        snapshot.source = SnapshotSource::File(path.to_path_buf());
        Ok(snapshot)
    }

    pub fn from_html_file(path: impl AsRef<Path>) -> anyhow::Result<SnapshotDocument> {
        let path = path.as_ref();
        let html = fs::read_to_string(path)
            .with_context(|| format!("failed to read HTML snapshot from {}", path.display()))?;
        Ok(SnapshotDocument {
            source: SnapshotSource::File(path.to_path_buf()),
            metadata: SnapshotMetadata::default(),
            html,
            hints: Vec::new(),
        })
    }

    pub fn from_html(html: impl Into<String>) -> SnapshotDocument {
        SnapshotDocument {
            source: SnapshotSource::Inline,
            metadata: SnapshotMetadata::default(),
            html: html.into(),
            hints: Vec::new(),
        }
    }

    pub fn capture_from_url(
        url: &str,
        browser_executable: Option<&Path>,
    ) -> anyhow::Result<SnapshotDocument> {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let script = manifest_dir
            .join("tools")
            .join("browser-helper")
            .join("capture-url.mjs");
        let mut command = Command::new("node");
        command.arg(script).arg("--url").arg(url);
        if let Some(browser_executable) = browser_executable {
            command.arg("--browser-executable").arg(browser_executable);
        }
        let output = command
            .output()
            .with_context(|| format!("failed to launch browser helper for {}", url))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("browser helper failed for {}: {}", url, stderr.trim());
        }

        let stdout = String::from_utf8(output.stdout)
            .context("browser helper returned non-UTF-8 snapshot output")?;
        let mut snapshot: SnapshotDocument = serde_json::from_str(&stdout)
            .context("browser helper returned invalid snapshot JSON")?;
        snapshot.source = SnapshotSource::Url(url.to_string());
        Ok(snapshot)
    }
}
