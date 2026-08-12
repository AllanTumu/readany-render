#!/usr/bin/env python3
"""Generate every committed fixture from readable source."""

from pathlib import Path
from zipfile import ZipFile, ZipInfo, ZIP_STORED, ZIP_DEFLATED
import struct
import zlib

ROOT = Path(__file__).resolve().parent
FIXED_ZIP_TIME = (1980, 1, 1, 0, 0, 0)


def write_entry(archive: ZipFile, path: str, value: str | bytes, compression: int = ZIP_STORED) -> None:
    info = ZipInfo(path, date_time=FIXED_ZIP_TIME)
    info.compress_type = compression
    info.external_attr = 0o600 << 16
    archive.writestr(info, value.encode() if isinstance(value, str) else value)


def package(name: str, parts: dict[str, str | bytes]) -> None:
    with ZipFile(ROOT / name, "w", ZIP_STORED) as archive:
        for path, value in parts.items():
            write_entry(archive, path, value)


def odf_manifest(mime: str) -> str:
    return f'''<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2">
<manifest:file-entry manifest:full-path="/" manifest:version="1.2" manifest:media-type="{mime}"/>
<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>
<manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/>
</manifest:manifest>'''


ODF_STYLES = '''<?xml version="1.0" encoding="UTF-8"?>
<office:document-styles xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" office:version="1.2"><office:styles/></office:document-styles>'''


ROOT.mkdir(parents=True, exist_ok=True)
(ROOT / "basic.csv").write_text('Name,Amount,Note\r\nAlice,1234.50,"line one\nline two"\r\nBob,-42,done\r\n', encoding="utf-8")
(ROOT / "basic.tsv").write_text("Name\tAmount\nAlice\t12\n", encoding="utf-8")
(ROOT / "basic.rtf").write_text(r"{\rtf1\ansi First paragraph\par Unicode \u8364? and \b bold\b0.}", encoding="ascii")
(ROOT / "delegate.pdf").write_bytes(b"%PDF-1.7\n% delegate fixture\n")
(ROOT / "delegate.heic").write_bytes(b"\x00\x00\x00\x18ftypheic" + b"\x00" * 20)
def png_chunk(kind: bytes, data: bytes) -> bytes:
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)


(ROOT / "pixel.png").write_bytes(
    b"\x89PNG\r\n\x1a\n"
    + png_chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 6, 0, 0, 0))
    + png_chunk(b"IDAT", zlib.compress(b"\x00\x16\x65\x34\xff"))
    + png_chunk(b"IEND", b"")
)


def xlsx_parts(rows: int = 3, columns: int = 3, features: bool = False) -> dict[str, str | bytes]:
    row_xml = []
    for row in range(1, rows + 1):
        cell_xml = []
        for column in range(columns):
            letters = ""
            value = column + 1
            while value:
                value, rem = divmod(value - 1, 26)
                letters = chr(65 + rem) + letters
            ref = f"{letters}{row}"
            if row == 1:
                cell_xml.append(f'<c r="{ref}" t="inlineStr"><is><t>Column {column + 1}</t></is></c>')
            else:
                cell_xml.append(f'<c r="{ref}" s="1"><v>{row * (column + 1)}</v></c>')
        row_xml.append(f'<row r="{row}">{"".join(cell_xml)}</row>')
    if features:
        # Tangut has no glyph in the committed last-resort DejaVu face. This
        # deliberately proves UnsupportedGlyphs instead of depending on an
        # emoji font installed on the test machine.
        row_xml.append('<row r="4"><c r="A4"><f>SUM(A2:A3)</f></c><c r="B4"><f>SUM(B2:B3)</f><v>10</v></c><c r="C4" t="inlineStr"><is><t>𗀀</t></is></c></row>')
    conditional = '<conditionalFormatting sqref="A1:A3"><cfRule type="cellIs" priority="1" operator="greaterThan"><formula>1</formula></cfRule></conditionalFormatting>' if features else ''
    merges = '<mergeCells count="1"><mergeCell ref="A1:B1"/></mergeCells>' if features else ''
    sheet = f'''<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetViews><sheetView workbookViewId="0"><pane xSplit="1" ySplit="1" topLeftCell="B2" state="frozen"/></sheetView></sheetViews><cols><col min="1" max="1" width="12" customWidth="1"/></cols><sheetData>{''.join(row_xml)}</sheetData>{merges}{conditional}</worksheet>'''
    hidden = '<sheet name="Hidden evidence" sheetId="2" state="hidden" r:id="rId2"/>' if features else ''
    workbook = f'''<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><workbookPr date1904="0"/><sheets><sheet name="Data" sheetId="1" r:id="rId1"/>{hidden}</sheets></workbook>'''
    relationships = ['<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>']
    if features:
        relationships.append('<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/>')
    parts: dict[str, str | bytes] = {
        "[Content_Types].xml": '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>',
        "_rels/.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>',
        "xl/workbook.xml": workbook,
        "xl/_rels/workbook.xml.rels": f'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{"".join(relationships)}</Relationships>',
        "xl/worksheets/sheet1.xml": sheet,
        "xl/styles.xml": '<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts><fills count="1"><fill><patternFill patternType="none"/></fill></fills><borders count="1"><border/></borders><cellStyleXfs count="1"><xf/></cellStyleXfs><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="4"/></cellXfs></styleSheet>',
    }
    if features:
        parts.update({
            "xl/worksheets/sheet2.xml": '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>',
            "xl/charts/chart1.xml": '<chartSpace xmlns="http://schemas.openxmlformats.org/drawingml/2006/chart"/>',
            "xl/pivotTables/pivotTable1.xml": '<pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"/>',
            "xl/externalLinks/externalLink1.xml": '<externalLink xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"/>',
            "xl/vbaProject.bin": b"macro fixture",
        })
    return parts


package("basic.xlsx", xlsx_parts())
package("features.xlsm", xlsx_parts(features=True))
package("wide.xlsx", xlsx_parts(350, 400))
package("frozen-scroll.xlsx", xlsx_parts(120, 80))

package("basic.ods", {
    "mimetype": "application/vnd.oasis.opendocument.spreadsheet",
    "META-INF/manifest.xml": odf_manifest("application/vnd.oasis.opendocument.spreadsheet"),
    "styles.xml": ODF_STYLES,
    "settings.xml": '<office:document-settings xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:config="urn:oasis:names:tc:opendocument:xmlns:config:1.0"><office:settings><config:config-item config:name="ShowGrid" config:type="boolean">true</config:config-item><config:config-item config:name="HorizontalSplitPosition" config:type="int">1</config:config-item><config:config-item config:name="VerticalSplitPosition" config:type="int">1</config:config-item></office:settings></office:document-settings>',
    "content.xml": '<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.2"><office:body><office:spreadsheet><table:table table:name="Sheet 1"><table:table-row><table:table-cell table:number-columns-repeated="2" office:value-type="string"><text:p>Hello</text:p></table:table-cell></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>',
})
package("repeat-bomb.ods", {
    "mimetype": "application/vnd.oasis.opendocument.spreadsheet",
    "content.xml": '<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"><office:body><office:spreadsheet><table:table table:name="Bomb"><table:table-row><table:table-cell table:number-columns-repeated="1000000000"/></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>',
})

package("basic.docx", {
    "[Content_Types].xml": '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>',
    "_rels/.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>',
    "word/document.xml": '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Page-faithful paragraph with provenance.</w:t></w:r></w:p><w:p><w:r><w:t>Second paragraph.</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr></w:body></w:document>',
    "word/_rels/document.xml.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Target="https://example.invalid/pixel.png" TargetMode="External"/></Relationships>',
    "word/embeddings/object1.bin": b"OLE fixture",
})

package("flow-features.docx", {
    "[Content_Types].xml": '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>',
    "_rels/.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>',
    "word/styles.xml": '<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri"/><w:sz w:val="22"/></w:rPr></w:rPrDefault></w:docDefaults><w:style w:type="paragraph" w:styleId="Base"><w:pPr><w:spacing w:after="120"/><w:jc w:val="center"/></w:pPr><w:rPr><w:b/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:basedOn w:val="Base"/><w:pPr><w:keepNext/></w:pPr><w:rPr><w:sz w:val="32"/></w:rPr></w:style></w:styles>',
    "word/numbering.xml": '<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl></w:abstractNum><w:num w:numId="7"><w:abstractNumId w:val="0"/></w:num></w:numbering>',
    "word/document.xml": '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Styled heading</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="7"/></w:numPr></w:pPr><w:r><w:rPr><w:i/></w:rPr><w:t>Numbered italic item</w:t></w:r></w:p><w:tbl><w:tblGrid><w:gridCol w:w="2160"/><w:gridCol w:w="2880"/></w:tblGrid><w:tr><w:tc><w:tcPr><w:tcW w:w="2160"/><w:gridSpan w:val="1"/></w:tcPr><w:p><w:r><w:t>Left cell</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2880"/></w:tcPr><w:p><w:r><w:t>Right cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="914400"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rIdImage"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p><w:p><w:r><w:br w:type="page"/></w:r><w:r><w:t>Second page</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rIdHeader"/><w:footerReference w:type="default" r:id="rIdFooter"/><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr></w:body></w:document>',
    "word/_rels/document.xml.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/pixel.png"/><Relationship Id="rIdHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="rIdFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/></Relationships>',
    "word/header1.xml": '<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>Repeated header</w:t></w:r></w:p></w:hdr>',
    "word/footer1.xml": '<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>Repeated footer</w:t></w:r></w:p></w:ftr>',
    "word/media/pixel.png": (ROOT / "pixel.png").read_bytes(),
})

hundred_page_body = []
for page in range(1, 101):
    page_break = '<w:r><w:br w:type="page"/></w:r>' if page < 100 else ''
    hundred_page_body.append(
        f'<w:p><w:r><w:t>Performance page {page}</w:t></w:r>{page_break}</w:p>'
    )
package("hundred-pages.docx", {
    "[Content_Types].xml": '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>',
    "_rels/.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>',
    "word/document.xml": '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>' + ''.join(hundred_page_body) + '<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr></w:body></w:document>',
})
package("gridspan-bomb.docx", {
    "[Content_Types].xml": '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>',
    "_rels/.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>',
    "word/document.xml": '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:tbl><w:tr><w:tc><w:tcPr><w:gridSpan w:val="1000000000"/></w:tcPr><w:p/></w:tc></w:tr></w:tbl></w:body></w:document>',
})
package("basic.odt", {
    "mimetype": "application/vnd.oasis.opendocument.text",
    "META-INF/manifest.xml": odf_manifest("application/vnd.oasis.opendocument.text"),
    "styles.xml": ODF_STYLES,
    "content.xml": '<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.2"><office:body><office:text><text:h text:outline-level="1">Heading</text:h><text:p>OpenDocument paragraph.</text:p></office:text></office:body></office:document-content>',
})
package("basic.pptx", {
    "[Content_Types].xml": '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/></Types>',
    "_rels/.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>',
    "ppt/presentation.xml": '<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst><p:sldSz cx="12192000" cy="6858000"/><p:notesSz cx="6858000" cy="9144000"/><p:defaultTextStyle/></p:presentation>',
    "ppt/_rels/presentation.xml.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>',
    "ppt/slides/slide1.xml": '<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="914400" y="914400"/><a:ext cx="4572000" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US" sz="2400"/><a:t>Slide title</a:t></a:r><a:endParaRPr lang="en-US"/></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>',
})
package("slide-features.pptx", {
    "[Content_Types].xml": '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/></Types>',
    "_rels/.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>',
    "ppt/presentation.xml": '<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst><p:sldSz cx="12192000" cy="6858000"/></p:presentation>',
    "ppt/_rels/presentation.xml.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>',
    "ppt/slides/slide1.xml": '<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="Inherited title"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:p><a:r><a:rPr sz="2400"/><a:t>Inherited title</a:t></a:r></a:p></p:txBody></p:sp><p:pic><p:nvPicPr><p:cNvPr id="3" name="Pixel"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rIdImage"/></p:blipFill><p:spPr><a:xfrm><a:off x="914400" y="2286000"/><a:ext cx="914400" cy="914400"/></a:xfrm></p:spPr></p:pic><p:cxnSp><p:nvCxnSpPr><p:cNvPr id="4" name="Connector"/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr><p:spPr><a:xfrm><a:off x="2286000" y="2286000"/><a:ext cx="1828800" cy="914400"/></a:xfrm><a:prstGeom prst="line"/></p:spPr></p:cxnSp></p:spTree></p:cSld></p:sld>',
    "ppt/slides/_rels/slide1.xml.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdLayout" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rIdImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/pixel.png"/></Relationships>',
    "ppt/slideLayouts/slideLayout1.xml": '<p:sldLayout xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="914400" y="457200"/><a:ext cx="9144000" cy="914400"/></a:xfrm></p:spPr></p:sp></p:spTree></p:cSld></p:sldLayout>',
    "ppt/slideLayouts/_rels/slideLayout1.xml.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdMaster" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>',
    "ppt/slideMasters/slideMaster1.xml": '<p:sldMaster xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="9144000" cy="914400"/></a:xfrm></p:spPr></p:sp></p:spTree></p:cSld></p:sldMaster>',
    "ppt/media/pixel.png": (ROOT / "pixel.png").read_bytes(),
})
package("unsupported-media.pptx", {
    "[Content_Types].xml": '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="svg" ContentType="image/svg+xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/></Types>',
    "_rels/.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>',
    "ppt/presentation.xml": '<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst><p:sldSz cx="12192000" cy="6858000"/></p:presentation>',
    "ppt/_rels/presentation.xml.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>',
    "ppt/slides/slide1.xml": '<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:cSld><p:spTree><p:pic><p:nvPicPr><p:cNvPr id="2" name="Unsupported vector"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rIdImage"/></p:blipFill><p:spPr><a:xfrm><a:off x="914400" y="914400"/><a:ext cx="914400" cy="914400"/></a:xfrm></p:spPr></p:pic></p:spTree></p:cSld></p:sld>',
    "ppt/slides/_rels/slide1.xml.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/vector.svg"/></Relationships>',
    "ppt/media/vector.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>',
})
package("basic.odp", {
    "mimetype": "application/vnd.oasis.opendocument.presentation",
    "META-INF/manifest.xml": odf_manifest("application/vnd.oasis.opendocument.presentation"),
    "styles.xml": ODF_STYLES,
    "content.xml": '<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" office:version="1.2"><office:body><office:presentation><draw:page draw:name="page1"><draw:frame svg:x="1in" svg:y="1in" svg:width="6in" svg:height="1in"><draw:text-box><text:p>ODP title</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>',
})

# A real XML entity declaration inside an otherwise recognisable workbook.
bomb = xlsx_parts()
bomb["xl/workbook.xml"] = '<!DOCTYPE workbook [<!ENTITY x "boom">]><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheets/></workbook>'
package("entity.xlsx", bomb)

# Hostile containers are readable source too: each crosses exactly one named
# ceiling before the parser can allocate based on attacker-controlled counts.
with ZipFile(ROOT / "zip-bomb.xlsx", "w", ZIP_DEFLATED) as archive:
    write_entry(archive, "xl/workbook.xml", "A" * 1_000_000, ZIP_DEFLATED)

with ZipFile(ROOT / "zip-slip.xlsx", "w", ZIP_STORED) as archive:
    write_entry(archive, "xl/workbook.xml", "<workbook/>")
    write_entry(archive, "../outside", "never written")

with ZipFile(ROOT / "many-entries.xlsx", "w", ZIP_STORED) as archive:
    write_entry(archive, "xl/workbook.xml", "<workbook/>")
    for index in range(10_000):
        write_entry(archive, f"parts/{index}", "")

deep = xlsx_parts()
deep["xl/workbook.xml"] = "<a>" * 300 + "</a>" * 300
package("deep-xml.xlsx", deep)

(ROOT / "huge.png").write_bytes(
    b"\x89PNG\r\n\x1a\n"
    + png_chunk(b"IHDR", struct.pack(">IIBBBBB", 100_001, 1_001, 8, 6, 0, 0, 0))
    + png_chunk(b"IEND", b"")
)
(ROOT / "huge.bmp").write_bytes(
    struct.pack("<2sIHHI", b"BM", 54, 0, 0, 54)
    + struct.pack("<IIIHHIIIIII", 40, 10_001, 10_001, 1, 24, 0, 0, 2_835, 2_835, 0, 0)
)
