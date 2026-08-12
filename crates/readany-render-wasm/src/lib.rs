#![forbid(unsafe_code)]

use readany_render::{FontSource, Options, OwnedFont, PageRange, Rect, SvgOptions};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

thread_local! {
    static FONTS: RefCell<Vec<OwnedFont>> = const { RefCell::new(Vec::new()) };
}

#[derive(Default, Deserialize)]
struct WasmOptions {
    filename: Option<String>,
    strict: Option<bool>,
    only: Option<WasmPageRange>,
}

#[derive(Deserialize)]
struct WasmPageRange {
    first: u32,
    last: u32,
}

#[derive(Default, Deserialize)]
struct WasmRasterOptions {
    scale: Option<f32>,
}

#[wasm_bindgen(js_name = render)]
pub fn render_document(bytes: &[u8], options: JsValue) -> Result<JsValue, JsValue> {
    let js_options = serde_wasm_bindgen::from_value::<WasmOptions>(options).unwrap_or_default();
    let rendered = render_native(bytes, &js_options)?;
    serde_wasm_bindgen::to_value(&rendered).map_err(|error| JsValue::from_str(&error.to_string()))
}

/// Keeps a large display list inside WASM so JavaScript only requests visible
/// items or pixels instead of materialising hundreds of megabytes of objects.
#[wasm_bindgen]
pub struct Document {
    rendered: readany_render::Rendered,
}

#[wasm_bindgen(js_name = open)]
pub fn open_document(bytes: &[u8], options: JsValue) -> Result<Document, JsValue> {
    let js_options = serde_wasm_bindgen::from_value::<WasmOptions>(options).unwrap_or_default();
    Ok(Document {
        rendered: render_native(bytes, &js_options)?,
    })
}

#[derive(Serialize)]
struct PageInfo<'a> {
    size: readany_render::Size,
    label: &'a Option<String>,
    source: &'a Option<readany_render::SourceRef>,
    frozen: &'a Option<readany_render::FrozenPanes>,
}

#[wasm_bindgen]
impl Document {
    #[wasm_bindgen(getter, js_name = pageCount)]
    pub fn page_count(&self) -> usize {
        self.rendered.pages.len()
    }

    #[wasm_bindgen(getter)]
    pub fn format(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.rendered.format)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(getter)]
    pub fn unrendered(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.rendered.unrendered)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(getter)]
    pub fn meta(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.rendered.meta)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = pageInfo)]
    pub fn page_info(&self, page: usize) -> Result<JsValue, JsValue> {
        let page = page_at(&self.rendered, page)?;
        serde_wasm_bindgen::to_value(&PageInfo {
            size: page.size,
            label: &page.label,
            source: &page.source,
            frozen: &page.frozen,
        })
        .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = itemsInRect)]
    pub fn items_in_rect(&self, page: usize, rect: JsValue) -> Result<JsValue, JsValue> {
        let page = page_at(&self.rendered, page)?;
        let rect = js_rect(rect)?;
        let items = readany_render::items_in_rect(page, rect).map_err(js_error)?;
        serde_wasm_bindgen::to_value(&items).map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = renderRectRgba)]
    pub fn render_rect_rgba(
        &self,
        page: usize,
        rect: JsValue,
        scale: f32,
    ) -> Result<Vec<u8>, JsValue> {
        let page = page_at(&self.rendered, page)?;
        Ok(readany_render::rasterise_rect(page, js_rect(rect)?, scale)
            .map_err(js_error)?
            .data)
    }

    #[wasm_bindgen(js_name = renderRectToCanvas)]
    pub fn render_rect_to_canvas(
        &self,
        page: usize,
        rect: JsValue,
        canvas: web_sys::HtmlCanvasElement,
        options: JsValue,
    ) -> Result<(), JsValue> {
        let page = page_at(&self.rendered, page)?;
        paint_rect_to_canvas(page, js_rect(rect)?, canvas, options)
    }
}

fn render_native(
    bytes: &[u8],
    js_options: &WasmOptions,
) -> Result<readany_render::Rendered, JsValue> {
    FONTS.with(|stored| {
        let stored = stored.borrow();
        let fonts = if stored.is_empty() {
            FontSource::default()
        } else {
            FontSource::Borrowed(&stored)
        };
        let options = Options {
            filename: js_options.filename.as_deref(),
            fonts,
            only: js_options.only.as_ref().map(|range| PageRange {
                first: range.first,
                last: range.last,
            }),
            strict: js_options.strict.unwrap_or(false),
            ..Options::default()
        };
        readany_render::render(bytes, &options).map_err(js_error)
    })
}

#[wasm_bindgen(js_name = pageToSvg)]
pub fn page_to_svg(document: JsValue, page: usize) -> Result<String, JsValue> {
    let rendered: readany_render::Rendered = serde_wasm_bindgen::from_value(document)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let page = rendered
        .pages
        .get(page)
        .ok_or_else(|| JsValue::from_str("page index is out of range"))?;
    readany_render::to_svg(page, &SvgOptions::default()).map_err(js_error)
}

#[wasm_bindgen(js_name = renderPageRgba)]
pub fn render_page_rgba(document: JsValue, page: usize, scale: f32) -> Result<Vec<u8>, JsValue> {
    let rendered: readany_render::Rendered = serde_wasm_bindgen::from_value(document)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let page = rendered
        .pages
        .get(page)
        .ok_or_else(|| JsValue::from_str("page index is out of range"))?;
    Ok(readany_render::rasterise(page, scale)
        .map_err(js_error)?
        .data)
}

#[wasm_bindgen(js_name = itemsInRect)]
pub fn items_in_rect(document: JsValue, page: usize, rect: JsValue) -> Result<JsValue, JsValue> {
    let rendered: readany_render::Rendered = serde_wasm_bindgen::from_value(document)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let items = readany_render::items_in_rect(page_at(&rendered, page)?, js_rect(rect)?)
        .map_err(js_error)?;
    serde_wasm_bindgen::to_value(&items).map_err(|error| JsValue::from_str(&error.to_string()))
}

#[wasm_bindgen(js_name = renderRectToCanvas)]
pub fn render_rect_to_canvas(
    document: JsValue,
    page: usize,
    rect: JsValue,
    canvas: web_sys::HtmlCanvasElement,
    options: JsValue,
) -> Result<(), JsValue> {
    let rendered: readany_render::Rendered = serde_wasm_bindgen::from_value(document)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    paint_rect_to_canvas(page_at(&rendered, page)?, js_rect(rect)?, canvas, options)
}

#[wasm_bindgen(js_name = renderToCanvas)]
pub fn render_to_canvas(
    document: JsValue,
    page: usize,
    canvas: web_sys::HtmlCanvasElement,
    options: JsValue,
) -> Result<(), JsValue> {
    let scale = serde_wasm_bindgen::from_value::<WasmRasterOptions>(options)
        .unwrap_or_default()
        .scale
        .unwrap_or(1.0);
    let rendered: readany_render::Rendered = serde_wasm_bindgen::from_value(document)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let page = page_at(&rendered, page)?;
    let pixmap = readany_render::rasterise(page, scale).map_err(js_error)?;
    paint_pixmap(canvas, &pixmap)
}

fn paint_rect_to_canvas(
    page: &readany_render::Page,
    rect: Rect,
    canvas: web_sys::HtmlCanvasElement,
    options: JsValue,
) -> Result<(), JsValue> {
    let scale = serde_wasm_bindgen::from_value::<WasmRasterOptions>(options)
        .unwrap_or_default()
        .scale
        .unwrap_or(1.0);
    let pixmap = readany_render::rasterise_rect(page, rect, scale).map_err(js_error)?;
    paint_pixmap(canvas, &pixmap)
}

fn paint_pixmap(
    canvas: web_sys::HtmlCanvasElement,
    pixmap: &readany_render::Pixmap,
) -> Result<(), JsValue> {
    canvas.set_width(pixmap.width);
    canvas.set_height(pixmap.height);
    let context = canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("the canvas has no 2D context"))?
        .dyn_into::<web_sys::CanvasRenderingContext2d>()?;
    let data = web_sys::ImageData::new_with_u8_clamped_array_and_sh(
        wasm_bindgen::Clamped(&pixmap.data),
        pixmap.width,
        pixmap.height,
    )?;
    context.put_image_data(&data, 0.0, 0.0)
}

fn page_at(
    rendered: &readany_render::Rendered,
    page: usize,
) -> Result<&readany_render::Page, JsValue> {
    rendered
        .pages
        .get(page)
        .ok_or_else(|| JsValue::from_str("page index is out of range"))
}

fn js_rect(value: JsValue) -> Result<Rect, JsValue> {
    serde_wasm_bindgen::from_value(value).map_err(|error| JsValue::from_str(&error.to_string()))
}

#[wasm_bindgen(js_name = addFont)]
pub fn add_font(bytes: &[u8]) -> Result<(), JsValue> {
    use skrifa::{MetadataProvider, string::StringId};
    let face = skrifa::FontRef::new(bytes)
        .map_err(|_| JsValue::from_str("the supplied font is not a valid OpenType font"))?;
    let family = face
        .localized_strings(StringId::FAMILY_NAME)
        .next()
        .map(|name| name.to_string())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| JsValue::from_str("the supplied font has no family name"))?;
    FONTS.with(|stored| {
        stored.borrow_mut().push(OwnedFont {
            family,
            bytes: bytes.to_vec(),
        })
    });
    Ok(())
}

fn js_error(error: readany_render::RenderError) -> JsValue {
    JsValue::from_str(&format!("{}: {}", error.code.stable(), error.message))
}
