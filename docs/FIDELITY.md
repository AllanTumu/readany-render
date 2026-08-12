# Fidelity

LibreOffice 26.2 is the committed office reference, not truth. Page documents
are exported to PDF and inspected with `pdftotext -bbox`. Natural spreadsheets
are exported to HTML; Chromium supplies cell text, word boxes, and pixels. The
harness measures five distributed 1,200 x 800 sheet viewports at 96 and 192 dpi.

## Release evidence

The release gate is based on semantic text and geometry, not blank raster area.
For spreadsheets, exact text is exhaustive: every non-empty cell in each full
sheet is compared by `SourceRef::Cell`. Geometry is sampled at the four sheet
corners and centre. This covers about 1.08% of Endo and 2.05% of OakPrism, and
the report states that boundary rather than presenting it as full-sheet layout
coverage. Page display-list `GlyphRun`s are split into positioned words and
matched against PDF bounding-box text.

`exact` is the F1 score `2 * matched / (ours + reference)`. The median global
translation is removed before local geometry is measured. Each matched word
then contributes `exp(-error / 12)`, where error includes centre and box-size
drift. `text fidelity` is `exact * geometry`. The report includes translation,
mean error, p95 error, and source-aware largest drifts.

The publish bars are executable before the `--update` branch: updating a
baseline cannot waive them. Every real sheet must have at least **99%
exhaustive exact cell text** and sampled text-box **p95 error no greater than 4
px**. Page-document floors are document-specific because the evidence is not
uniform; they preserve the measured starting point but do not turn a poor
starting point into a fidelity claim.

Measured 12 August 2026:

| Corpus document | Exact | Geometry | Text fidelity | Mean / p95 error |
| --- | ---: | ---: | ---: | ---: |
| `basic.docx` | 1.000000 | 0.853182 | 0.853182 | 2.24 / 6.36 px |
| `basic.odp` | 1.000000 | 0.715689 | 0.715689 | 4.05 / 4.97 px |
| `basic.odt` | 1.000000 | 0.607502 | 0.607502 | 6.90 / 12.35 px |
| `basic.pptx` | 1.000000 | 0.682948 | 0.682948 | 4.61 / 5.48 px |
| `basic.rtf` | 1.000000 | 0.717480 | 0.717480 | 4.30 / 8.07 px |
| `endo-prem-2023.xlsx` | 1.000000 | 0.978372 | 0.978372 | 0.26 / 0.27 px |
| `nasa-agency-report-2022.pptx` | 0.778898 | 0.098206 | 0.076492 | 162.40 / 373.57 px |
| `nist-hb133-2026-chapter-2.docx` | 0.365007 | 0.017430 | 0.006362 | 226.55 / 610.30 px |
| `oakprism-stress-v3.xlsx` | 1.000000 | 0.967207 | 0.967207 | 0.45 / 3.27 px |
| `uk-ipo-one-way-nda.odt` | 0.148148 | 0.008732 | 0.001294 | 347.64 / 864.96 px |

The page-document text-fidelity mean is **0.457618** and the real-sheet mean is
**0.972789**. These remain regression evidence, not proof of universal format
fidelity. CI rejects per-document or corpus regression beyond 0.001, requires
baseline keys to match the corpus, and enforces both sets of hard floors.

### What the page-aligned figures do and do not say

**Measured 12 August 2026, and they correct a misleading impression.** The
`exact` column above is computed **page by page**. When pagination diverges,
every later page is compared against a reference page holding different content,
so one early page break drags the whole score down. Read alone, `0.148` on the
UK IPO agreement suggests the reader recovered a seventh of the text. It did
not. It recovered all of it.

Comparing the same documents **document-wide** — whitespace collapsed, Symbol
private-use bullet glyphs dropped, Unicode NFKC-normalised, page boundaries
ignored — against the same LibreOffice reference:

| Corpus document | Page-aligned `exact` | Characters ours / reference | Document-wide similarity |
| --- | ---: | ---: | ---: |
| `uk-ipo-one-way-nda.odt` | 0.148148 → **0.9634** (fixed) | 3,171 / 3,171 | **1.000000** |
| `nist-hb133-2026-chapter-2.docx` | 0.365007 | 65,997 / 65,843 | **0.897400** |
| `nasa-agency-report-2022.pptx` | 0.778898 | 4,034 / 4,037 | 0.679200 |

Three different facts, and the single number hid all three:

* **ODT is character-identical.** Every character, in order. Its defect was
  pagination alone — three pages where LibreOffice makes two — and it is now
  **fixed**. The agreement carries `fo:break-before="page"` on its very first
  paragraph; honoured literally that opens the document with a blank page and
  shifts every page after it. Word and LibreOffice both suppress a break before
  the first block. Page-aligned fidelity went from **0.1481 to 0.9634** on that
  one condition, which is the clearest possible demonstration that the score was
  measuring placement rather than reading.
* **DOCX reads within 0.2% of the reference character count** and agrees on
  ~90% of the sequence. Its defect is pagination — 37 pages against 34, with
  content distributed differently from the first page onward, where ours holds
  1,076 characters and the reference holds 4,188.
* **PPTX reads within 0.07% of the character count** but agrees on only 68% of
  the *sequence*. The same characters in a different order: shape ordering
  within a slide, not missing content. Its slide count is already correct.

So two questions were being answered by one number, and they have different
answers:

| Question | ODT | DOCX | PPTX |
| --- | --- | --- | --- |
| Was the text read? | completely | essentially completely | completely |
| Was it placed correctly? | no — pagination | no — pagination | no — shape order |

**Neither figure is retracted and neither is promoted.** The page-aligned score
remains the release gate, because placement is part of fidelity and a renderer
that puts the right words on the wrong page has not rendered the document. What
changes is that it is no longer reported as though it measured reading. A README
that said "36.5% exact text" and nothing else would have understated this
library to the point of dishonesty in the other direction.

### Page-document evidence floors

Each row shows the measurement followed by the rounded hard floor derived from
it. The floor is checked for exact text, p95 geometry error, and page-count
agreement. `pagination` is `min(ours, reference) / max(ours, reference)`.

| Corpus document | Measured exact / minimum | Measured p95 / maximum | Pages ours / reference | Pagination / minimum |
| --- | ---: | ---: | ---: | ---: |
| `basic.docx` | 1.000000 / 0.99 | 6.36 / 6.50 px | 1 / 1 | 1.000000 / 1.00 |
| `basic.odt` | 1.000000 / 0.99 | 12.35 / 12.50 px | 1 / 1 | 1.000000 / 1.00 |
| `basic.rtf` | 1.000000 / 0.99 | 8.07 / 8.25 px | 1 / 1 | 1.000000 / 1.00 |
| `basic.pptx` | 1.000000 / 0.99 | 5.48 / 5.60 px | 1 / 1 | 1.000000 / 1.00 |
| `basic.odp` | 1.000000 / 0.99 | 4.97 / 5.10 px | 1 / 1 | 1.000000 / 1.00 |
| `nist-hb133-2026-chapter-2.docx` | 0.365007 / 0.36 | 610.30 / 611.00 px | 37 / 34 | 0.918919 / 0.91 |
| `nasa-agency-report-2022.pptx` | 0.778898 / 0.77 | 373.57 / 374.00 px | 11 / 11 | 1.000000 / 1.00 |
| `uk-ipo-one-way-nda.odt` | 0.148148 / 0.14 | 864.96 / 866.00 px | 3 / 2 | 0.666667 / 0.66 |

The real corpus answers the `basic.odt` question decisively. Its 0.608 geometry
score was not evidence that ODT layout was nearly acceptable; the six-word,
one-page fixture was too small to exercise pagination. The real UK agreement
has only 14.8% exact text matching, 864.96 px p95 drift, and an extra page. DOCX
has the same class of defect: the NIST chapter becomes 37 pages instead of 34,
with major text loss/reordering and local drift. PPTX keeps the correct 11-slide
count, but its 77.9% exact match, 373.57 px p95 drift, and explicit unsupported
EMF evidence show that complex master/shape layout is not yet faithful.

Only pages present on both sides contribute text and pixel measurements; extra
or missing pages are measured separately by the pagination score. This avoids
pretending an unmatched page has usable geometry while making the pagination
defect impossible to hide.

## Pixel diagnostics

Pixels remain useful for the contact sheet and investigation, but do not gate a
release. The harness reports the best local 8 x 8-window SSIM over every integer
translation in a plus or minus 3 px search and reports ink density for both
images. A dimension difference above 2% is a hard failure before comparison.

| Corpus document | Aligned SSIM | Ink, ours / reference |
| --- | ---: | ---: |
| `basic.docx` | 0.995943 | 0.1878% / 0.1944% |
| `basic.odp` | 0.985421 | 8.7667% / 8.8467% |
| `basic.odt` | 0.993403 | 0.1425% / 0.1992% |
| `basic.pptx` | 0.996485 | 0.1082% / 0.1360% |
| `basic.rtf` | 0.995884 | 0.1133% / 0.1370% |
| `endo-prem-2023.xlsx` | 0.733993 | 14.8375% / 6.7182% |
| `nasa-agency-report-2022.pptx` | 0.752658 | 37.6333% / 78.3645% |
| `nist-hb133-2026-chapter-2.docx` | 0.664051 | 6.5803% / 7.0554% |
| `oakprism-stress-v3.xlsx` | 0.677122 | 21.6146% / 13.9774% |
| `receipt.jpg` | 1.000000 | 95.6258% / 95.6258% |
| `uk-ipo-one-way-nda.odt` | 0.710659 | 4.3147% / 6.7372% |

The old XLSX/ODS scores of 0.987544 and 0.995099 are withdrawn because white
padding dominated them. The later unaligned sheet mean of 0.473395 is also not
fidelity evidence. Likewise, approximately 0.99 scores on sparse page fixtures
are pixel diagnostics only: their low ink density cannot prove layout fidelity.

### Gridline before/after diagnostic

The missing-gridline hypothesis was measured rather than assumed. Before sheet
gridlines were implemented, our/reference ink was 5.9187%/6.7182% on Endo and
12.0678%/13.9774% on OakPrism: relative gaps of 11.9% and 13.7%. With the
workbook's default-on gridlines painted, the same figures became
14.8375%/6.7182% and 21.6146%/13.9774%. That deliberately recorded result shows
the reference HTML export does not preserve Calc's interactive grid furniture;
its old ink gap was not a valid way to tune gridline darkness. Grid visibility,
indexed colour, z-order, and raster output are therefore pinned structurally
and visually, while pixel ink remains a diagnostic rather than a release gate.
