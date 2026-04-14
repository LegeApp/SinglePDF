# SingleFile-to-PDF Rust Architecture

## Goal

Build a local-first pipeline that turns a fully captured web page into a paginated, searchable PDF without embedding a full browser engine into the Rust renderer.

The target product is not a toolkit with many knobs. It is a deterministic browser extension with one action:

1. Click the toolbar button.
2. Capture the page like SingleFile.
3. Convert the frozen page into a PDF using fixed heuristics.
4. Download the result.

There should be no user-visible rendering or compression options in the final product.

The intended architecture is:

1. Snapshot the live page into a frozen, self-contained HTML document.
2. Parse that HTML into semantic flow blocks.
3. Paginate those blocks using document-aware heuristics.
4. Render pages through a narrow display-list core.
5. Serialize the result to PDF through `lopdf`, keeping text searchable.

This repository currently contains:

- `SingleFile/`: upstream JavaScript source used as the reference implementation for page capture.
- New Rust code under `src/*.rs`: the in-progress native pipeline.
- Existing JS entrypoints retained for reference and potential interop.

## Design Principles

### Narrow rendering core

The Rust renderer should not become a mini browser. It should accept an already frozen document model and focus on:

- block measurement
- pagination
- simple painting
- image placeholder or raster embedding
- PDF-friendly text placement

It should not own:

- JavaScript execution
- network fetching of page subresources during layout
- full CSS cascade and selector matching
- browser-grade layout for grid/flex/positioned content

### Fixed behavior

The production extension should use a single built-in profile:

- ignore video and audio content
- ignore interactive form state and controls
- preserve text as text wherever possible
- emit PDF-native objects for text, rules, fills, and images
- apply the same pagination and compression policy on every run

### Frozen input boundary

The browser capture problem and the PDF generation problem are separate.

The browser side is best handled by SingleFile because it already knows how to:

- wait for page execution
- inline resources
- flatten frames
- preserve the final DOM as a single HTML payload

The Rust side begins after that capture boundary.

### Display-list first

Pagination and rendering should be built around a display list instead of direct drawing calls. That gives us:

- replayable page output
- backend independence
- easier debugging
- future raster and SVG paths
- deterministic PDF assembly

### Top-left layout coordinates

All internal layout uses top-left coordinates in CSS-like page space. The PDF backend performs the Y-axis inversion when emitting content streams.

## System Architecture

## 1. Snapshot Layer

### Responsibility

Produce a `SnapshotDocument` containing:

- source URL metadata when available
- the frozen HTML string
- optional capture metadata

### Inputs

- live URL via a future browser bridge
- existing `.html` file
- raw HTML string

### Planned implementations

- `HtmlSnapshot::from_file`: alpha path, implemented now
- `HtmlSnapshot::from_html`: alpha path, implemented now
- `HtmlSnapshot::capture_from_url`: alpha helper path, implemented now through Node + Edge + vendored SingleFile

### SingleFile integration plan

The first practical integration should shell out to a small browser-driven helper instead of porting SingleFile JS into Rust.

Current sequence:

1. Launch Edge headlessly through `playwright-core`.
2. Navigate to the requested URL and wait for load plus a short idle window.
3. Inject vendored `SingleFile/lib/single-file-bootstrap.js`.
4. Inject vendored `SingleFile/lib/single-file.js`.
5. Call `globalThis.singlefile.getPageData(...)`.
6. Return the generated HTML bytes to Rust over stdout.

That keeps the hard browser work in a browser while preserving a native Rust pipeline after capture.

## 2. DOM Processing Layer

### Responsibility

Parse frozen HTML into a simplified semantic document model.

### Output model

The parser emits `FlowBlock` values such as:

- headings
- paragraphs
- list items
- preformatted/code blocks
- blockquotes
- images
- tables
- spacers/dividers

Each block carries:

- extracted text or asset metadata
- style hints
- estimated importance
- page-break preferences

### Alpha scope

The alpha parser intentionally handles a reduced subset:

- `h1` to `h6`
- `p`
- `li`
- `pre`
- `blockquote`
- `img`
- `table`
- fallback text blocks from `div`, `section`, `article`, `main`

Ignored for now:

- floats
- absolute positioning
- CSS transforms
- multi-column layout
- flex and grid fidelity

### Heuristics

The processor derives rough style from tag semantics:

- heading font size by level
- monospace treatment for `pre`
- spacing before and after blocks
- keep-with-next for headings
- avoid-split preferences for tables and images

## 3. Pagination Layer

### Responsibility

Convert flow blocks into `LogicalPage`s using readable and stable page breaks.

### Inputs

- page size
- margins
- target text width
- list of flow blocks

### Strategy

The paginator uses estimated line wrapping instead of true browser layout. This is acceptable because the alpha renderer is text-first and semantic-first, not pixel-identical.

Core rules:

- never split headings from the next block when avoidable
- keep images and tables atomic in alpha
- prefer breaking before large blocks instead of leaving tiny tails
- preserve explicit hard breaks when surfaced later

### Future improvements

- widow/orphan control
- nested list indentation
- table row splitting
- footnotes
- CSS `break-before`, `break-after`, `break-inside`

## 4. Rendering Core

### Responsibility

Turn `LogicalPage`s into `RenderPage`s made of `DrawOp`s.

### Display-list types

The rendering core should stay close to this shape:

- `Rect`
- `Color`
- `TextStyle`
- `TextRun`
- `DrawOp`
- `RenderPage`

Alpha operations are:

- `FillRect`
- `StrokeRect`
- `DrawText`

Planned operations:

- `DrawImage`
- clipping
- layered painting
- link annotations

## 5. PDF Backend

### Responsibility

Serialize display-list pages into a PDF document using `lopdf`.

### Why `lopdf`

It matches the project direction:

- low-level control
- streaming-friendly object construction
- explicit page/resource management
- good fit for later invisible text overlays and image XObjects

### Alpha behavior

The alpha backend emits:

- A4 pages by default
- built-in Helvetica fonts
- searchable text
- simple borders and shaded boxes for tables/images
- text placeholders for unsupported rich elements

### Planned upgrades

- embedded fonts
- raster image XObjects
- JPEG image recompression for photo-like raster regions
- JPEG2000 support investigation for PDFs that target compatible readers
- invisible OCR/text overlay mode
- link annotations
- incremental/streamed page flushing

## Compression Strategy

Compression is one of the main areas where this project should improve on a naive browser-print pipeline.

### Compression layers

There are three separate compression decisions:

1. Snapshot compression:
   SingleFile already reduces HTML/CSS and deduplicates some resources.

2. Layout/render compression:
   The Rust side decides which regions stay as text/vector content and which regions must become raster images.

3. PDF asset compression:
   Raster regions should be encoded with a PDF-compatible format chosen per content type.

### Intended policy

- Body text stays as text objects, not raster.
- Line art, diagrams, and UI chrome should prefer lossless or near-lossless handling.
- Photo-heavy regions should use lossy compression.
- Large repeated assets should be shared across pages and object references.

### Output format roadmap

- Initial implementation target: JPEG image XObjects for photo-like raster regions.
- Follow-up target: lossless image path for diagrams and screenshots.
- Research target: JPEG2000 support behind a feature gate, because reader compatibility and encoder availability need to be validated carefully.

The important design point is that compression belongs in the PDF asset layer, not in the semantic paginator.

## JPEG2000 Plan

JPEG2000 is a reasonable long-term option for photo-heavy pages, but it should be introduced carefully and only after the JPEG path is stable.

### Why defer it

- It adds a native dependency boundary through `openjp2`.
- Reader support is generally good in modern desktop PDF viewers, but interoperability still needs validation.
- It is a poor first step while the project still lacks real raster image XObject emission.

### Planned inclusion path

1. Implement image XObject embedding with a stable JPEG path first.
2. Add a raster asset abstraction that separates:
   - decoded pixels
   - compression decision
   - encoded bytes
   - emitted PDF filter and metadata
3. Add an optional `openjp2` encoder backend behind a feature gate.
4. Restrict JPEG2000 use initially to large photo-like regions.
5. Keep screenshots, diagrams, and UI-heavy regions on a lossless path until measured otherwise.

### `toojpeg-rust` role

The intended first encoder integration is `toojpeg-rust` for baseline JPEG generation of photo-like raster regions before they are embedded into PDF image objects.

## Module Layout

Rust code is organized as:

- `src/lib.rs`: crate exports
- `src/model.rs`: flow, layout, and display-list types
- `src/snapshot.rs`: HTML snapshot inputs
- `src/dom.rs`: semantic extraction from frozen HTML
- `src/paginate.rs`: measurement and page splitting
- `src/pdf.rs`: `lopdf` backend
- `src/main.rs`: CLI entrypoint

## Data Flow

```text
URL or HTML file
  -> SnapshotDocument
  -> ParsedDocument
  -> Vec<FlowBlock>
  -> Vec<LogicalPage>
  -> Vec<RenderPage>
  -> PDF bytes
```

## Alpha Definition

The first workable alpha should satisfy these constraints:

- Accept a local HTML file as input.
- Extract readable body content without browser execution.
- Paginate headings and paragraphs sensibly.
- Render searchable text into a valid PDF.
- Represent tables and images as labeled placeholders rather than dropping them.
- Be structured so a real SingleFile capture step can slot in later.

This alpha is not expected to be visually identical to browser print output.

## Planned Iteration Order

### Phase 1: Working alpha

- HTML file input
- semantic extraction
- heuristic pagination
- searchable text PDF output

### Phase 2: Capture integration

- invoke SingleFile through a browser helper
- capture live URLs into frozen HTML
- preserve metadata such as title and source URL

### Phase 3: Browser extension integration

- package the capture flow as an Edge/Firefox extension
- keep a single toolbar action and no user-facing save options
- run the same pipeline for every capture
- ignore videos, audio, and interactive controls by policy

### Phase 4: Rich rendering

- raster images
- background fills and borders
- basic inline emphasis
- list indentation and bullets
- code block styling

### Phase 5: Fidelity improvements

- CSS-aware measurements
- explicit page-break directives
- links/bookmarks
- embedded fonts
- better table handling

## Risks And Constraints

### Fidelity risk

A pure Rust semantic renderer will not match browser layout for complex web apps. The mitigation is to:

- keep the renderer intentionally narrow
- rasterize only hard cases later
- preserve text as text whenever possible

### Integration risk

SingleFile is JavaScript-first. A realistic Rust system should treat it as an external capture engine, not something to directly translate wholesale into Rust.

### Performance risk

Large frozen HTML documents can be substantial. The pipeline should evolve toward:

- page-by-page rendering
- shared image bytes
- limited intermediate duplication

## Immediate Deliverables In This Repository

1. This architecture document.
2. A Rust crate scaffold.
3. A CLI that converts local HTML into an alpha PDF.
4. A modular foundation for later SingleFile-driven URL capture.
