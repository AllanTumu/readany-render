# Format boundaries

- XLSX/XLSM sheet targets are resolved from workbook relationships. Cached
  values are authoritative; formula evaluation is intentionally absent. Cell
  fonts, fills, border edges, alignment, wrapping, shrink-to-fit and rotation
  are carried into the display list, and default-font metrics determine widths.
- ODF repeat attributes are expanded only after checking the cell ceiling.
- ODS named/automatic styles, column and row geometry, spans, frozen view
  positions, and cell paint are resolved before the shared sheet painter.
- DOCX resolves `docDefaults`, cycle-bounded `basedOn` chains, and direct
  formatting. Paragraph pagination, lists, table grids, repeating parts, and
  embedded-image relationships feed the shared flow layout. RTF is parsed as a
  scoped group/control-word stream with codepage-aware byte and hex decoding.
- ODT resolves named and automatic styles, page layout, list levels, spans,
  and embedded images before the shared flow layout.
- PPTX slide order and targets come from presentation relationships, and slide
  coordinates are converted from EMU at the parser boundary. Placeholder
  geometry is inherited through slide layouts and masters. ODP resolves master
  content and named graphic/text styles.
- PDF returns an empty page list and `DelegateToHost { Pdf }`.
- HEIC returns `DelegateToHost { Heic }`; no decoder is bundled.
- External relationships are named in `unrendered` and never fetched.

Unknown or damaged content produces a stable `RR-*` error. Strict mode turns
any otherwise inspectable incomplete result into `RR-0302`.
