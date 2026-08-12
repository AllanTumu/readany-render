use crate::Format;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Colour {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Colour {
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FontId(pub u32);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PositionedGlyph {
    pub glyph_id: u32,
    pub x_advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
    pub cluster: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GlyphRun {
    pub font: FontId,
    pub family: String,
    pub size_px: f32,
    pub origin: Point,
    pub glyphs: Vec<PositionedGlyph>,
    pub text: String,
    pub colour: Colour,
    /// Clockwise rotation around `origin`, in degrees.
    #[serde(default)]
    pub rotation_deg: f32,
    pub source: Option<SourceRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Path {
    pub commands: Vec<PathCommand>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PathCommand {
    Move(Point),
    Line(Point),
    Quad(Point, Point),
    Cubic(Point, Point, Point),
    Close,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Paint {
    pub colour: Colour,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub paint: Paint,
    pub width: f32,
    pub dash: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PathItem {
    pub path: Path,
    pub fill: Option<Paint>,
    pub stroke: Option<Stroke>,
    pub source: Option<SourceRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageData {
    pub mime: String,
    pub bytes: Vec<u8>,
    pub pixel_size: Size,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageItem {
    pub data: ImageData,
    pub rect: Rect,
    pub source: Option<SourceRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub items: Vec<Item>,
    pub clip: Option<Rect>,
    pub source: Option<SourceRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
#[non_exhaustive]
pub enum Item {
    Glyphs(GlyphRun),
    Path(PathItem),
    Image(ImageItem),
    Group(Group),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenPanes {
    pub rows: u32,
    pub columns: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
#[non_exhaustive]
pub enum SourceRef {
    Cell {
        sheet: u32,
        row: u32,
        column: u32,
    },
    Text {
        paragraph: u32,
        start: u32,
        end: u32,
    },
    Shape {
        slide: u32,
        shape: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub size: Size,
    pub label: Option<String>,
    pub items: Vec<Item>,
    pub source: Option<SourceRef>,
    pub frozen: Option<FrozenPanes>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum Unrendered {
    Chart { page: u32, kind: String },
    PivotTable { page: u32 },
    ConditionalFormatting { page: u32, rules: u32 },
    HiddenSheet { name: String },
    FormulaWithoutCachedValue { sheet: u32, row: u32, column: u32 },
    ExternalReference { target: String },
    UnsupportedGlyphs { script: String, count: u32 },
    Ole { page: u32 },
    Macro,
    DelegateToHost { format: Format },
    Truncated { limit: String, of: u64 },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    pub title: Option<String>,
    pub creator: Option<String>,
    pub substituted_fonts: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rendered {
    pub pages: Vec<Page>,
    pub format: Format,
    pub unrendered: Vec<Unrendered>,
    pub meta: Meta,
}

impl Rendered {
    pub(crate) fn delegated(format: Format) -> Self {
        Self {
            pages: Vec::new(),
            format,
            unrendered: vec![Unrendered::DelegateToHost { format }],
            meta: Meta::default(),
        }
    }
}

pub(crate) fn rect_path(rect: Rect) -> Path {
    Path {
        commands: vec![
            PathCommand::Move(Point {
                x: rect.x,
                y: rect.y,
            }),
            PathCommand::Line(Point {
                x: rect.x + rect.width,
                y: rect.y,
            }),
            PathCommand::Line(Point {
                x: rect.x + rect.width,
                y: rect.y + rect.height,
            }),
            PathCommand::Line(Point {
                x: rect.x,
                y: rect.y + rect.height,
            }),
            PathCommand::Close,
        ],
    }
}
