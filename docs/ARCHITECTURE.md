# Architecture

The display list is the product boundary:

```text
bytes -> sniff -> defensive container -> semantic parser -> layout -> Page/Item
                                                               |-> raster PNG
                                                               |-> SVG
                                                               |-> WASM objects
```

Parsers never paint pixels. They convert native units to CSS pixels at 96 dpi
and attach `SourceRef` before handing items to the shared backends. `BTreeMap`
is used wherever iteration can affect serialized output; archive entry order is
normalized before format-level decisions.

`SourceRef` is how a rendered box is traced back to what it came from, and it
has four kinds. `Cell` is a spreadsheet cell, `Shape` a slide shape, `Text` a
paragraph and character range in flowing text, and `TableCell` a cell of a table
inside a flow document — a table, row and column, so a Word table row can be
selected the way a sheet row can. Table indices run in document order across the
whole document, including notes, headers and footers, so a table split across a
page boundary keeps one identity. Rows and columns are grid positions: a spanned
cell reports the first column it covers and a vertically merged cell the row its
merge began on, so every box of one cell answers to one address.

It is a public enum crossing the WASM boundary, and
`crates/readany-render-wasm/readany_render_wasm.d.ts` is hand-written and copied
over wasm-bindgen's own by `scripts/build-wasm.sh`. A variant added in Rust and
forgotten there is a runtime error in a browser rather than a compile error
here, so `every_source_ref_variant_is_declared_for_the_wasm_boundary` compares
the serialized fields of every variant against that file.

The library accepts byte slices only. ZIP entry names are logical package paths,
not filesystem paths. XML declarations that can introduce entities are rejected
before parsing, and archive expansion, XML depth, repeat counts, images, cells,
pages, and glyphs all have named finite ceilings.

The font-free WASM build has no bundled assets and requires `addFont`. Native
and bundled-WASM builds shape through HarfRust and rasterise the resulting
OpenType glyph IDs from the bundled face. Text remains in each `GlyphRun`; it is
never reconstructed from glyph IDs.
