# SinglePDF Next Steps

Date: 2026-03-28

## Current Architecture

The project boundary is now:

- SingleFile for page capture
- Rust for PDF generation
- Browser extension as the one-click entry point
- Native messaging as the bridge between extension and Rust

That split is the right one. It avoids reimplementing browser capture logic in Rust and keeps the PDF-specific work on the Rust side where the project differentiates.

## Current State

Implemented:

- Rust HTML-to-PDF pipeline
- semantic extraction for headings, paragraphs, lists, images, blockquotes, code blocks, and tables
- heuristic pagination
- searchable PDF text output
- embedded image handling for:
  - `data:` images
  - local file images
  - inline `background-image`
  - simple stylesheet `background-image` selectors
- table rendering as PDF vector/text content
- browser helper for URL capture using Edge + vendored SingleFile
- browser extension alpha
- native messaging host mode in Rust
- Windows native host install/uninstall scripts

Current product flow:

1. Extension captures the page with SingleFile runtime code.
2. Extension sends frozen HTML to the Rust native host.
3. Rust converts frozen HTML into PDF.
4. Extension downloads the returned PDF.

## Most Important Remaining Work

### 1. Build And Packaging Workflow

This is the next most practical gap.

Needed:

- a single build script that:
  - builds the Rust binary
  - copies SingleFile runtime files into `extension/vendor/singlefile`
  - prepares the extension folder for loading
- a clear dev workflow for:
  - Firefox temporary load
  - Edge dev-mode load
- a documented native host install flow tied to the produced binary path

Suggested deliverables:

- `tools/build-singlepdf.ps1`
- `tools/build-singlepdf.mjs` or a Rust-side build helper if preferred
- updated `extension/README.md` with exact commands and browser steps

### 2. Extension Integration Hardening

The extension alpha works conceptually, but it needs hardening.

Needed:

- confirm the vendored SingleFile runtime files are sufficient for reliable capture
- verify behavior on:
  - dynamic pages
  - pages with frames
  - long pages
  - pages with many images
- add better error surfacing in the extension background script
- add a visible failure path if:
  - native host is missing
  - native host returns an error
  - SingleFile capture fails

Suggested improvements:

- transient browser notification on failure
- console logging with structured errors
- progress marker or badge state during capture/render

### 3. Native Host Installation UX

The Windows scripts exist, but the full setup story is not finished.

Needed:

- test install scripts outside the sandbox on a real user environment
- confirm Firefox registration path works end to end
- confirm Edge registration path works end to end
- document how to obtain the Edge extension ID for native messaging

Possible future improvement:

- a helper that rewrites the Edge manifest automatically once the extension ID is known

### 4. Capture Fidelity Validation

SingleFile is now the right capture mechanism, but the integrated extension path needs real-world validation.

Needed:

- test representative sites:
  - docs pages
  - news/article pages
  - dashboards
  - pages with deferred images
  - pages with background images
- compare:
  - extension-produced frozen HTML
  - Rust CLI URL capture helper output
- identify cases where extension capture and helper capture diverge

Focus questions:

- are all critical resources inlined?
- are frames preserved correctly?
- are CSS background assets represented in the final HTML in a way Rust can consume?

### 5. Layout And Pagination Refinement

The pipeline works, but pagination is still heuristic and narrow.

Needed:

- better heading keep-with-next tuning
- better handling of large images
- better handling of wide tables
- avoid awkward page breaks around blockquotes and code blocks
- refine row heights and column sizing for rendered tables

Good near-term targets:

- more stable table column width logic
- max-height handling for large images
- smarter spacing rules around headings and images

### 6. CSS Extraction Scope Expansion

Current CSS support is intentionally narrow.

Implemented:

- inline `background-image`
- `<style>` block `background-image` from simple selectors only

Still missing:

- descendant selectors
- multiple class selector combinations
- pseudo-classes and pseudo-elements
- external stylesheet parsing
- more CSS-driven decorative assets

Recommended approach:

- keep expansion narrow and incremental
- do not build a full CSS engine
- only implement selectors that materially improve capture fidelity for common web pages

### 7. HTTP/HTTPS Asset Resolution On Rust Side

Right now the Rust renderer handles:

- `data:` image sources
- local file sources

It does not fetch network assets directly, which is appropriate for privacy and determinism.

Preferred approach:

- rely on SingleFile capture to inline resources before Rust sees them

Only if needed later:

- add a fallback network fetch path, but only with strong justification

### 8. PDF Rendering Depth

The project now emits:

- text as text
- images as image XObjects
- tables as vector/text objects

Still missing or simplified:

- richer inline text styling
- links/bookmarks
- more accurate image sizing heuristics
- code block styling
- list indentation polish
- embedded fonts

Recommended order:

1. links and annotations
2. better image sizing
3. code block and list polish
4. embedded fonts if needed for fidelity

### 9. Extension Productization

The final target remains:

- one button click
- same result every time
- no user-facing options

To get there:

- remove any remaining debug/developer assumptions from the extension flow
- keep policy fixed in code
- make installation and updates predictable
- ensure failure modes are explicit and simple

Open product rule to preserve:

- ignore videos
- ignore audio
- ignore interactive controls
- preserve text as text wherever possible
- use PDF-native objects whenever practical

## Recommended Immediate Order

If picking this up later, continue in this order:

1. Finish build/packaging workflow for extension + native host.
2. Verify extension end-to-end on Firefox.
3. Verify extension end-to-end on Edge.
4. Harden error handling and install documentation.
5. Run real-world capture validation on representative sites.
6. Tune pagination and table/image layout based on those results.

## Files Most Relevant For Continuing

- `src/dom.rs`
- `src/paginate.rs`
- `src/pdf.rs`
- `src/native_host.rs`
- `src/main.rs`
- `extension/manifest.json`
- `extension/background.js`
- `extension/capture-content.js`
- `extension/README.md`
- `tools/build-extension.mjs`
- `tools/install-native-host.ps1`
- `tools/uninstall-native-host.ps1`

## Notes

- JPEG2000 is intentionally not part of the immediate plan.
- The current architecture split is good and should be preserved.
- Do not drift back toward “Rust captures the page itself” unless there is a very specific reason.
