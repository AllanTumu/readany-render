# Real-world corpus

These documents are committed separately from `fixtures/` because they exercise
real-world or production-shaped inputs rather than the generator's basic parser
fixtures. Their provenance is stated precisely; a public sample is not described
as a private transaction.

| File | Origin | SHA-256 |
| --- | --- | --- |
| `endo-prem-2023.xlsx` | OakPrism sample data, the 393 x 328 motivating workbook identified during acceptance testing | `ecbc893cb09a3e525dafa41d9708e95963ead9008b27d38ebcf2fef4e0dc96fb` |
| `oakprism-stress-v3.xlsx` | OakPrism real-world spreadsheet stress sample | `2504695f79e52661cfb31fdc15a52f84cc89b5e4bb85e04ae5c5d646e6ff799e` |
| `receipt.jpg` | Existing `sk-bench` photographed receipt fixture; production-shaped, not asserted to record a real purchase | `74e33dd80d34786b6f26209e8ac3ba398a5188c2ff426ad29d34466829d040ce` |
| `cfpb-sample-credit-card-statement.pdf` | U.S. Consumer Financial Protection Bureau public sample credit-card statement | `755073d4d13732eb9bd9b340c6ff28325741835c369a6c8b58b3f4650ffed52d` |
| `nist-hb133-2026-chapter-2.docx` | NIST Handbook 133 (2026), Chapter 2, downloaded from the official [NIST edition page](https://www.nist.gov/pml/owm/nist-handbook-133-current-edition) on 2026-08-12 | `a4f3dde16357dc79cc206e122e6bf7edd2b7f8548e4a3ae764e4030d7751742d` |
| `nasa-agency-report-2022.pptx` | NASA Agency Report, NTRS document 20220013802, downloaded from its official [NASA record](https://ntrs.nasa.gov/citations/20220013802) on 2026-08-12 | `80e6c3a1d879cbaaf38235378787ab8612391a6da8b0e2ecc23a69c01da357d2` |
| `uk-ipo-one-way-nda.odt` | UK Intellectual Property Office example one-way NDA, downloaded from the official [GOV.UK publication](https://www.gov.uk/government/publications/non-disclosure-agreements) on 2026-08-12 | `79da48f2a99a41f2d5434480cbd664f53fcda8d5793b3228c18979676774018e` |

The three public office documents stand in for a private owner-supplied DOCX,
which was not available in the workspace. Redistribution is permitted: NIST
states that unmarked information on its site may be copied and that its federal
publications are generally public domain in the United States; the NTRS record
explicitly says "Public Use Permitted"; and the GOV.UK page publishes the ODT
under the [Open Government Licence 3.0](https://www.nationalarchives.gov.uk/doc/open-government-licence/version/3/).
Source credit is retained here and no agency endorsement is implied.

These are structurally meaningful inputs, not renamed basic fixtures:

- The NIST DOCX is 34 LibreOffice reference pages (37 renderer pages), with 211
  styles, 288 numbering levels, six tables, four floating drawings, three inline
  images, and separate header and footer parts.
- The NASA PPTX has 11 slides, two masters, 15 layouts, two tables, and 102
  pictures. Two EMF assets that this renderer cannot paint are explicitly named
  in `Document::unrendered`; they are not silently dropped.
- The UK IPO ODT is a real two-page reference agreement (three renderer pages),
  rather than a generated parser fixture.

The workbooks are scored as 1,200 x 800 natural-sheet viewports against
LibreOffice HTML rendered by Chromium. The receipt is scored as two image
viewports. Page documents are compared on every page shared by the renderer and
LibreOffice, with page-count agreement scored independently. The PDF is a
contract input: this crate deliberately returns
`DelegateToHost { format: Pdf }`, so it verifies that a statement never appears
as a successful-but-empty local render. PDF pixel fidelity belongs to the host
PDF renderer and is not manufactured here by comparing LibreOffice with itself.
