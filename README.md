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

## Format support

| Family | Formats | Boundary |
| --- | --- | --- |
| Spreadsheets | XLSX, XLSM, ODS, CSV, TSV | Cached formula values, widths/heights, merges, frozen panes, fonts, fills, borders, alignment, and number formats are displayed. Charts, pivots, conditional formatting, hidden sheets, macros, and external links are reported. |
| Flow documents | DOCX, ODT, RTF | Style cascades, shaped runs, bidi and line breaking, indents, alignment, spacing, pagination controls, lists, tables, repeating parts, and embedded images produce positioned content. Embedded OLE and external links are reported. |
| Slides | PPTX, ODP | Explicit geometry, styled text and shapes, embedded images, and layout/master inheritance produce one display-list page per slide. |
| Images | PNG, JPEG, GIF, BMP, WebP | One image item on one page, subject to the pixel ceiling. |
| Delegated | PDF, HEIC | No partial preview: `DelegateToHost` tells the caller to use its platform viewer. |

PDF delegation is deliberate. Browsers, iOS, and Android already ship mature
PDF viewers, and this library does not compete with PDFium.

## Fonts and size

Native builds include regular, bold, italic, and bold-italic Carlito, Caladea,
and Liberation Sans/Serif/Mono faces plus DejaVu Sans, with their
OFL/Bitstream licence texts. They substitute Calibri, Cambria, Arial/Helvetica,
Times New Roman, and Courier New using compatible metrics.
Browser builds support either `--features fonts` or the core build plus
`addFont(bytes)`. The measured release WASM sizes are 1,578,147 bytes gzipped
without fonts (4 MiB budget) and 5,605,777 bytes with fonts (9 MiB budget).
The root npm export works with ESM bundlers; `readany-render/no-bundler` exposes
the same web initializer explicitly for direct browser imports. Both modes let
the caller pass a `wasmUrl` to `init`, and the npm tarball includes every
bundled font's licence text.

The library never opens a socket, never resolves an external relationship, and
never writes document data to disk. The CLI and fidelity harness are separate
native tools and are the only filesystem users.

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
