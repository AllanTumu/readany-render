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

The library accepts byte slices only. ZIP entry names are logical package paths,
not filesystem paths. XML declarations that can introduce entities are rejected
before parsing, and archive expansion, XML depth, repeat counts, images, cells,
pages, and glyphs all have named finite ceilings.

The font-free WASM build has no bundled assets and requires `addFont`. Native
and bundled-WASM builds shape through HarfRust and rasterise the resulting
OpenType glyph IDs from the bundled face. Text remains in each `GlyphRun`; it is
never reconstructed from glyph IDs.
