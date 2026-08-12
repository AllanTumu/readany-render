use crate::{Limits, RenderError};
use std::collections::BTreeMap;
use std::io::Read;

pub(crate) struct Archive {
    entries: BTreeMap<String, Vec<u8>>,
}

impl Archive {
    pub(crate) fn open(bytes: &[u8], limits: &Limits) -> Result<Self, RenderError> {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| {
            RenderError::malformed("the document container is damaged; obtain a fresh copy")
        })?;
        let count =
            u32::try_from(zip.len()).map_err(|_| RenderError::limit("zip_entries", u64::MAX))?;
        if count > limits.zip_entries {
            return Err(RenderError::limit("zip_entries", u64::from(count)));
        }
        let mut declared_total = 0_u64;
        let mut inflated_total = 0_u64;
        let mut entries = BTreeMap::new();
        for index in 0..zip.len() {
            let entry = zip.by_index(index).map_err(|_| {
                RenderError::malformed("a ZIP entry is unreadable; obtain a fresh copy")
            })?;
            let name = entry.name().to_owned();
            let size = entry.size();
            declared_total = declared_total
                .checked_add(size)
                .ok_or_else(|| RenderError::limit("decompressed_bytes", u64::MAX))?;
            if declared_total > limits.decompressed_bytes {
                return Err(RenderError::limit("decompressed_bytes", declared_total));
            }
            let compressed = entry.compressed_size().max(1);
            if size > compressed.saturating_mul(u64::from(limits.compression_ratio)) {
                return Err(RenderError::limit(
                    "compression_ratio",
                    size.div_ceil(compressed),
                ));
            }
            if entry.is_dir() {
                continue;
            }
            let mut data = Vec::new();
            let mut limited = entry.take(size.saturating_add(1));
            let mut buffer = [0_u8; 32 * 1024];
            loop {
                let read = limited.read(&mut buffer).map_err(|_| {
                    RenderError::malformed(
                        "a compressed document part is unreadable; obtain a fresh copy",
                    )
                })?;
                if read == 0 {
                    break;
                }
                inflated_total = inflated_total
                    .checked_add(read as u64)
                    .ok_or_else(|| RenderError::limit("decompressed_bytes", u64::MAX))?;
                if inflated_total > limits.decompressed_bytes {
                    return Err(RenderError::limit("decompressed_bytes", inflated_total));
                }
                let entry_inflated = data.len() as u64 + read as u64;
                if entry_inflated > compressed.saturating_mul(u64::from(limits.compression_ratio)) {
                    return Err(RenderError::limit(
                        "compression_ratio",
                        entry_inflated.div_ceil(compressed),
                    ));
                }
                data.extend_from_slice(&buffer[..read]);
            }
            if u64::try_from(data.len()).unwrap_or(u64::MAX) != size {
                return Err(RenderError::malformed(
                    "a document part ended before its declared size; obtain a fresh copy",
                ));
            }
            let name = normalize(&name)?;
            if entries.insert(name, data).is_some() {
                return Err(RenderError::malformed(
                    "the document container has duplicate logical paths; obtain a safe copy",
                ));
            }
        }
        Ok(Self { entries })
    }

    pub(crate) fn required(&self, name: &str) -> Result<&[u8], RenderError> {
        self.entries.get(name).map(Vec::as_slice).ok_or_else(|| {
            RenderError::malformed(format!(
                "required document part {name} is missing; obtain a fresh copy"
            ))
        })
    }

    pub(crate) fn get(&self, name: &str) -> Option<&[u8]> {
        self.entries.get(name).map(Vec::as_slice)
    }
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }
}

fn normalize(name: &str) -> Result<String, RenderError> {
    let mut output = Vec::new();
    let normalized = name.replace('\\', "/");
    for component in normalized.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                return Err(RenderError::malformed(
                    "a ZIP entry contains a parent path; obtain a safe copy",
                ));
            }
            value => output.push(value),
        }
    }
    Ok(output.join("/"))
}
