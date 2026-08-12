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

The workbooks are scored as 1,200 x 800 natural-sheet viewports against
LibreOffice HTML rendered by Chromium. The receipt is scored as two image
viewports. The PDF is a contract input: this crate deliberately returns
`DelegateToHost { format: Pdf }`, so it verifies that a statement never appears
as a successful-but-empty local render. PDF pixel fidelity belongs to the host
PDF renderer and is not manufactured here by comparing LibreOffice with itself.
