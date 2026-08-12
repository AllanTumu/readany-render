# readany-render

`readany-render` turns common office documents into deterministic, inspectable
display lists and rasterises those lists without uploading a document. Every
text run retains a source cell, paragraph range, or slide shape so applications
can connect visible evidence back to its origin.

```rust
use readany_render::{Options, Rect, rasterise_rect, render};

let bytes = std::fs::read("statement.xlsx")?;
let document = render(&bytes, &Options {
    filename: Some("statement.xlsx"),
    ..Options::default()
})?;
let viewport_png = rasterise_rect(
    &document.pages[0],
    Rect { x: 0.0, y: 0.0, width: 1_200.0, height: 800.0 },
    1.0,
)?.encode_png()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

```ts
import init, { open } from "readany-render";

await init();
const document = open(bytes, { filename: "statement.xlsx" });
document.renderRectToCanvas(
  0,
  { x: scrollLeft, y: scrollTop, width: canvas.clientWidth, height: canvas.clientHeight },
  canvas,
  { scale: devicePixelRatio },
);
const visibleItems = document.itemsInRect(0, {
  x: scrollLeft,
  y: scrollTop,
  width: canvas.clientWidth,
  height: canvas.clientHeight,
});
console.log(document.unrendered);
```

`open` retains the display list in WASM and is the intended browser API for
natural spreadsheet canvases. Only visible pixels or provenance-bearing items
cross the JavaScript boundary. The plain-object `render` API remains useful for
small documents and inspection. In Rust, use `rasterise_rect` and
`items_in_rect` for the same viewport workflow; `rasterise` deliberately
refuses full canvases above 100 million pixels.

## Measured format status

“Implemented” means the parser reaches a display list and names omissions. It
does not mean visual fidelity has been demonstrated. Every claim below names
the corpus behind it; results from one format are never generalized to another.

**Read the flow-format rows carefully.** The page-aligned scores in
`docs/FIDELITY.md` are computed page by page, so a pagination difference drags
them down even where every character was read correctly. On the UK IPO
agreement the text is character-identical to LibreOffice and the score is
0.148; both facts are true and only one of them is about reading. The table
below states which is which.

| Format | Status | Evidence |
| --- | --- | --- |
| XLSX | Proven on current corpus | Endo and OakPrism real workbooks: 100% exhaustive exact cell text; sampled geometry p95 0.27 px and 3.27 px |
| XLSM | Implemented; not real-corpus proven | Generated macro fixture and the XLSX path; macro omission is explicit |
| ODS | Implemented; not real-corpus proven | Generated ODS fixtures and golden/raster tests only |
| CSV | Implemented; parser-tested only | Generated quoted-field/newline fixtures only |
| TSV | Implemented; parser-tested only | Generated delimiter fixtures only |
| DOCX | Text complete, pagination not faithful | Real NIST chapter: character count within **0.2%** of reference and 89.7% document-wide sequence agreement, so the text is read. It paginates as 37 pages against 34, and the page-aligned score is 36.5% because of it. |
| ODT | Text **character-identical**, pagination not faithful | Real UK IPO agreement: 3,171 characters against 3,171, similarity **1.0000**. It paginates as three pages against two, and the page-aligned score is 14.8% entirely because of that. |
| RTF | Implemented; synthetic evidence only | `basic.rtf`: 100% exact text, 8.07 px p95; no real RTF corpus document |
| PPTX | Text complete, shape order not faithful | Real NASA deck: correct slide count and character count within **0.07%**, but 67.9% sequence agreement — the same text in a different order within a slide. Two EMF images are named as undrawn rather than omitted. |
| ODP | Implemented; synthetic evidence only | `basic.odp`: 100% exact text, 4.97 px p95; no real ODP corpus document |
| PNG | Implemented; parser/raster tested | Generated image fixtures |
| JPEG | Implemented; photographed evidence | Real photographed receipt plus EXIF-orientation fixtures |
| GIF | Implemented; parser/raster tested | Generated image fixtures |
| BMP | Implemented; parser/raster tested | Generated image fixtures |
| WebP | Implemented; parser/raster tested | Generated image fixtures |
| PDF | Delegated to host by design | Official CFPB statement pins `DelegateToHost { format: Pdf }` |
| HEIC | Delegated to host by design | Contract tests pin `DelegateToHost { format: Heic }` |

> **PDF is deliberately outside this renderer.** A PDF never becomes a partial
> local preview: `DelegateToHost` requires the caller to use its platform PDF
> viewer. Browsers, iOS, and Android already ship mature viewers, and this
> library does not compete with PDFium.

## Fonts and size

Native builds include regular, bold, italic, and bold-italic Carlito, Caladea,
and Liberation Sans/Serif/Mono faces plus DejaVu Sans, with their
OFL/Bitstream licence texts. They substitute Calibri, Cambria, Arial/Helvetica,
Times New Roman, and Courier New using compatible metrics.
Browser builds support either `--features fonts` or the core build plus
`addFont(bytes)`. Measured on 12 August 2026 on an Apple M4 Pro MacBook Pro (14
cores, 24 GB, macOS 26.5.1, Rust 1.97.1), the release WASM is **1,583,532 bytes
gzipped** without fonts (4 MiB budget) and **5,610,140 bytes gzipped** with
fonts (9 MiB budget).
The root npm export works with ESM bundlers; `readany-render/no-bundler` exposes
the same web initializer explicitly for direct browser imports. Both modes let
the caller pass a `wasmUrl` to `init`, and the npm tarball includes every
bundled font's licence text.

The library never opens a socket, never resolves an external relationship, and
never writes document data to disk. The CLI and fidelity harness are separate
native tools and are the only filesystem users.

## Measured performance

On the same machine and date, `./scripts/check-performance.sh` measured the
generated 400 x 350 sheet at **439 ms**, the real Endo workbook parse at **241
ms**, its 1,200 x 800 viewport raster at **41 ms**, the generated 100-page DOCX
at **1 ms**, and a small-page raster below the timer's 1 ms resolution. Budgets
are 500 ms for sheet parsing, 100 ms for the real viewport, 3,000 ms for the
100-page DOCX, and 100 ms for the small page.

## Development

```bash
python3 fixtures/generate.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
./scripts/check-performance.sh
./scripts/check-size.sh
./harness/run.sh
```

The measured release evidence and per-format result are recorded in
[`docs/IMPLEMENTATION_REPORT.md`](docs/IMPLEMENTATION_REPORT.md).

MSRV is Rust 1.85. The Rust crate and npm package are MIT licensed; bundled
font license text ships under `fonts/`.
