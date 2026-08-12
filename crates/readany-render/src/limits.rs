use crate::RenderError;

/// Named allocation and work ceilings applied before or during expansion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// 100 MiB admits large office files while bounding the first allocation.
    pub input_bytes: u64,
    /// 500 MiB covers the 400 x 350 reference workbook with ample headroom.
    pub decompressed_bytes: u64,
    /// 200:1 rejects the generated million-byte deflate bomb during inflation.
    pub compression_ratio: u32,
    /// 10,000 parts exceeds the largest corpus package; the 10,001-part fixture fails.
    pub zip_entries: u32,
    /// 2,000 pages is above practical reader use and bounds pagination work.
    pub pages: u32,
    /// Five million cells is over 35 times the 140,000-cell performance corpus.
    pub cells: u64,
    /// 100 megapixels admits modern photos without permitting unbounded decoding.
    pub image_pixels: u64,
    /// Depth 256 admits ordinary OOXML while the 300-level fixture proves the bound.
    pub xml_depth: u32,
    /// Entity expansion remains zero because document entities are never resolved.
    pub xml_entity_expansions: u32,
    /// Two million glyphs per page is far above the measured office corpus.
    pub glyphs_per_page: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            input_bytes: 100 * 1024 * 1024,
            decompressed_bytes: 500 * 1024 * 1024,
            compression_ratio: 200,
            zip_entries: 10_000,
            pages: 2_000,
            cells: 5_000_000,
            image_pixels: 100_000_000,
            xml_depth: 256,
            xml_entity_expansions: 0,
            glyphs_per_page: 2_000_000,
        }
    }
}

impl Limits {
    pub(crate) fn check_input(&self, actual: usize) -> Result<(), RenderError> {
        let actual =
            u64::try_from(actual).map_err(|_| RenderError::limit("input_bytes", u64::MAX))?;
        if actual > self.input_bytes {
            return Err(RenderError::limit("input_bytes", actual));
        }
        Ok(())
    }
}
