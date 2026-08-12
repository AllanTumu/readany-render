# Fidelity

LibreOffice 26.2 is the committed office reference, not truth. Page documents
are exported to PDF and inspected with `pdftotext -bbox`. Natural spreadsheets
are exported to HTML; Chromium supplies word boxes and pixels for two 1,200 x
800 viewports. The harness measures at 96 and 192 dpi.

## Release evidence

The release gate is based on text and geometry, not blank raster area. Each
display-list `GlyphRun` is split into positioned words. Page words are matched
to the PDF bbox text. Sheet words are matched by normalized text and their
`SourceRef::Cell`, so repeated values in different cells cannot be paired by
accident.

`exact` is the F1 score `2 * matched / (ours + reference)`. The median global
translation is removed before geometry is measured. Each matched word then
contributes `exp(-error / 12)`, where error includes centre drift and box-size
drift. `text fidelity` is `exact * geometry`. The report includes the recovered
translation, mean error, p95 error, and the largest drifts with their source
cell or paragraph.

Measured 12 August 2026:

| Corpus document | Exact | Geometry | Text fidelity | Mean / p95 error |
| --- | ---: | ---: | ---: | ---: |
| `basic.docx` | 1.000000 | 0.853182 | 0.853182 | 2.24 / 6.36 px |
| `basic.odp` | 1.000000 | 0.715689 | 0.715689 | 4.05 / 4.97 px |
| `basic.odt` | 1.000000 | 0.607502 | 0.607502 | 6.90 / 12.35 px |
| `basic.pptx` | 1.000000 | 0.682948 | 0.682948 | 4.61 / 5.48 px |
| `basic.rtf` | 0.769231 | 0.727558 | 0.559660 | 4.19 / 7.99 px |
| `endo-prem-2023.xlsx` | 0.961005 | 0.292757 | 0.281341 | 24.11 / 48.88 px |
| `oakprism-stress-v3.xlsx` | 0.801871 | 0.384682 | 0.308466 | 22.21 / 40.63 px |

The page-document text-fidelity mean is **0.683796** and the real-sheet mean is
**0.294903**. These are honest regression baselines, not absolute quality
claims. The small synthetic page corpus is not sufficient to justify an
absolute release floor. CI rejects per-document or corpus text-geometry
regression beyond 0.001 and requires baseline keys to match the corpus exactly.
Baseline updates require the explicit `--update` flag.

## Pixel diagnostics

Pixels remain useful for the contact sheet and investigation, but do not gate a
release. The harness reports the best local 8 x 8-window SSIM over every integer
translation in a plus or minus 3 px search and reports ink density for both
images. A dimension difference above 2% remains a hard failure before any
comparison.

| Corpus document | Aligned SSIM | Ink, ours / reference |
| --- | ---: | ---: |
| `basic.docx` | 0.995943 | 0.1878% / 0.1944% |
| `basic.odp` | 0.985421 | 8.7667% / 8.8467% |
| `basic.odt` | 0.993403 | 0.1425% / 0.1992% |
| `basic.pptx` | 0.996485 | 0.1082% / 0.1360% |
| `basic.rtf` | 0.995884 | 0.1133% / 0.1370% |
| `endo-prem-2023.xlsx` | 0.577407 | 10.6424% / 10.9472% |
| `oakprism-stress-v3.xlsx` | 0.405837 | 13.9865% / 16.5430% |
| `receipt.jpg` | 1.000000 | 95.6258% / 95.6258% |

The old XLSX/ODS scores of 0.987544 and 0.995099 are withdrawn because white
padding dominated them. The later unaligned sheet mean of 0.473395 is also no
longer treated as fidelity evidence. Likewise, the approximately 0.99 scores on
sparse page fixtures are retained only as pixel diagnostics: their very low ink
density makes them incapable of proving layout fidelity.
