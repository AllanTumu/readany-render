use crate::{RenderError, RenderErrorCode};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
    Csv,
    Tsv,
    Xlsx,
    Xlsm,
    Ods,
    Docx,
    Odt,
    Rtf,
    Pptx,
    Odp,
    Pdf,
    Png,
    Jpeg,
    Gif,
    Bmp,
    Webp,
    Heic,
}

pub(crate) fn sniff(bytes: &[u8], filename: Option<&str>) -> Result<Format, RenderError> {
    if bytes.starts_with(b"%PDF-") {
        return Ok(Format::Pdf);
    }
    if bytes.starts_with(b"{\\rtf") {
        return Ok(Format::Rtf);
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(Format::Png);
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Ok(Format::Jpeg);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Ok(Format::Gif);
    }
    if bytes.starts_with(b"BM") {
        return Ok(Format::Bmp);
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Ok(Format::Webp);
    }
    if bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && matches!(&bytes[8..12], b"heic" | b"heix" | b"mif1")
    {
        return Ok(Format::Heic);
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return sniff_zip(bytes);
    }
    let extension = filename.and_then(|name| {
        name.rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_lowercase())
    });
    match extension.as_deref() {
        Some("csv") => Ok(Format::Csv),
        Some("tsv") => Ok(Format::Tsv),
        _ if looks_like_text(bytes) => Ok(if bytes.contains(&b'\t') {
            Format::Tsv
        } else {
            Format::Csv
        }),
        _ => Err(RenderError::new(
            RenderErrorCode::UnknownFormat,
            "the bytes do not match a supported document; choose a common office, PDF, or image file",
        )),
    }
}

fn sniff_zip(bytes: &[u8]) -> Result<Format, RenderError> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|_| {
        RenderError::malformed("the ZIP container is damaged; obtain a fresh copy of the document")
    })?;
    let mut names = Vec::new();
    for index in 0..archive.len().min(128) {
        let entry = archive.by_index(index).map_err(|_| {
            RenderError::malformed("a ZIP entry could not be read; obtain a fresh copy")
        })?;
        names.push(entry.name().to_owned());
    }
    if names.iter().any(|n| n == "xl/workbook.xml") {
        return Ok(if names.iter().any(|n| n.ends_with("vbaProject.bin")) {
            Format::Xlsm
        } else {
            Format::Xlsx
        });
    }
    if names.iter().any(|n| n == "word/document.xml") {
        return Ok(Format::Docx);
    }
    if names.iter().any(|n| n == "ppt/presentation.xml") {
        return Ok(Format::Pptx);
    }
    let mime = archive.by_name("mimetype").ok().and_then(|mut entry| {
        let mut value = String::new();
        std::io::Read::read_to_string(&mut entry, &mut value)
            .ok()
            .map(|_| value)
    });
    match mime.as_deref() {
        Some("application/vnd.oasis.opendocument.spreadsheet") => Ok(Format::Ods),
        Some("application/vnd.oasis.opendocument.text") => Ok(Format::Odt),
        Some("application/vnd.oasis.opendocument.presentation") => Ok(Format::Odp),
        _ => Err(RenderError::new(
            RenderErrorCode::UnknownFormat,
            "the ZIP is not a supported office document; choose an XLSX, DOCX, PPTX, or OpenDocument file",
        )),
    }
}

fn looks_like_text(bytes: &[u8]) -> bool {
    let sample = &bytes[..bytes.len().min(4096)];
    !sample.contains(&0)
        && (std::str::from_utf8(sample).is_ok()
            || sample.starts_with(&[0xff, 0xfe])
            || sample.starts_with(&[0xfe, 0xff]))
}
