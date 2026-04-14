# SinglePDF

SinglePDF is a local-first browser-extension workflow that converts a page captured by SingleFile into a searchable PDF using a Rust renderer.

The current design boundary is intentional:

- **SingleFile** handles page capture/freeze.
- **Rust (`singlepdf.exe`)** handles parsing, pagination, rendering, and PDF writing.
- **WebExtension + native messaging** bridges browser capture to Rust PDF generation.

## Status

Active alpha. The pipeline is usable and currently optimized for article/news/Wikipedia-style pages and common ecommerce/product pages.

## High-Level Pipeline

1. Extension invokes SingleFile runtime on the active tab.
2. Extension sends the frozen snapshot to `singlepdf.exe` via native messaging.
3. Rust parses snapshot HTML into semantic flow blocks.
4. Blocks are paginated and rendered to PDF objects (text stays live/selectable).
5. PDF is saved to `Downloads`, and a render report (`.report.json`) is written next to it.

## Repository Layout

- `src/` Rust core (`dom`, `paginate`, `pdf`, `compression`, `native_host`, etc.)
- `extension/` Firefox/Chromium extension sources
- `tools/` build/install scripts and browser helper tooling
- `tests/` unit/integration tests
- `fixtures/` sample HTML inputs

## Requirements

- Windows (current native-host scripts target Windows)
- Rust toolchain (stable)
- Node.js (for extension build helper/capture tooling)
- Firefox Developer Edition/Nightly (recommended for temporary extension loading)

## Build

From this directory:

```powershell
.\tools\build-singlepdf.ps1
```

This builds:

- Rust binary: `build\release\singlepdf.exe`
- Loadable extension folder: `build\extension`

## Install Native Host (Firefox)

```powershell
.\tools\install-native-host.ps1 -ExePath .\build\release\singlepdf.exe -FirefoxOnly
```

To remove:

```powershell
.\tools\uninstall-native-host.ps1 -FirefoxOnly
```

## Load Extension (Firefox)

1. Open `about:debugging#/runtime/this-firefox`
2. Click **Load Temporary Add-on**
3. Select `build\extension\manifest.json`
4. Click the toolbar action on a normal web page

## CLI Usage

Render from saved SingleFile HTML:

```powershell
.\build\release\singlepdf.exe --html "C:\path\page.html" --out ".\build\page.pdf"
```

Render from structured snapshot JSON:

```powershell
.\build\release\singlepdf.exe --snapshot ".\build\snapshot.json" --out ".\build\page.pdf"
```

When using CLI, a report file is written next to the PDF (`*.report.json`).

## Tests

Run standard tests:

```powershell
cargo test -q
```

Run local fixture regression harness (uses local saved HTML paths from manifest):

```powershell
cargo test --test regression_fixtures -- --ignored --nocapture
```

Fixture manifest:

- `tests/fixtures/regression/manifest.json`

## Current Behavior Notes

- Text is kept as PDF text objects when possible.
- Images are currently encoded through the configured legacy image strategy (JPEG/JPEG2000 path).
- Extension/native-host responses include a structured render report; skipped images and fallbacks are reported.
- MRC scaffolding exists in codebase but is not the default active path in current extension flow.

## Development Notes

- Main architecture details: `ARCHITECTURE.md`
- Near-term roadmap: `NEXT_STEPS_2026-03-28.md`

