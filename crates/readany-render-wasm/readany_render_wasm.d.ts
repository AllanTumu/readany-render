export interface Point { x: number; y: number }
export interface Size { width: number; height: number }
export interface Rect extends Point, Size {}
export interface Colour { r: number; g: number; b: number; a: number }
export interface FrozenPanes { rows: number; columns: number; width: number; height: number }
export interface ColumnSpan { index: number; x: number; width: number }
export interface RowSpan { index: number; y: number; height: number }
export interface SheetGrid { columns: ColumnSpan[]; rows: RowSpan[] }
export interface PositionedGlyph {
  glyph_id: number;
  x_advance: number;
  x_offset: number;
  y_offset: number;
  cluster: number;
}
export type SourceRef =
  | { kind: "Cell"; sheet: number; row: number; column: number }
  | { kind: "Text"; paragraph: number; start: number; end: number }
  | { kind: "Shape"; slide: number; shape: number }
  // `table` counts tables in document order from zero, per document and not per
  // page, so a table broken across a page boundary keeps one identity. `row`
  // and `column` are zero-based grid positions: a cell spanning columns 4 to 6
  // reports 4, and a vertically merged cell reports the row its merge began on.
  | { kind: "TableCell"; table: number; row: number; column: number };
export interface GlyphRun {
  font: number;
  family: string;
  size_px: number;
  origin: Point;
  glyphs: PositionedGlyph[];
  text: string;
  colour: Colour;
  rotation_deg: number;
  source: SourceRef | null;
}
export type PathCommand =
  | { Move: Point }
  | { Line: Point }
  | { Quad: [Point, Point] }
  | { Cubic: [Point, Point, Point] }
  | "Close";
export interface Path { commands: PathCommand[] }
export interface Paint { colour: Colour }
export interface Stroke { paint: Paint; width: number; dash: number[] }
export interface PathItem { path: Path; fill: Paint | null; stroke: Stroke | null; source: SourceRef | null }
export interface ImageData { mime: string; bytes: number[]; pixel_size: Size }
export interface ImageItem { data: ImageData; rect: Rect; source: SourceRef | null }
export interface Group { items: Item[]; clip: Rect | null; source: SourceRef | null }
export type Item =
  | { kind: "Glyphs"; value: GlyphRun }
  | { kind: "Path"; value: PathItem }
  | { kind: "Image"; value: ImageItem }
  | { kind: "Group"; value: Group };
export interface Page {
  size: Size;
  label: string | null;
  items: Item[];
  source: SourceRef | null;
  frozen: FrozenPanes | null;
}
export interface PageInfo {
  size: Size;
  label: string | null;
  source: SourceRef | null;
  frozen: FrozenPanes | null;
  grid: SheetGrid | null;
}
export type Format = "csv" | "tsv" | "xlsx" | "xlsm" | "ods" | "docx" | "odt" | "rtf" | "pptx" | "odp" | "pdf" | "png" | "jpeg" | "gif" | "bmp" | "webp" | "heic";
export type Unrendered =
  | { type: "Chart"; page: number; kind: string }
  | { type: "PivotTable"; page: number }
  | { type: "ConditionalFormatting"; page: number; rules: number }
  | { type: "HiddenSheet"; name: string }
  | { type: "FormulaWithoutCachedValue"; sheet: number; row: number; column: number }
  | { type: "ExternalReference"; target: string }
  | { type: "UnsupportedGlyphs"; script: string; count: number }
  | { type: "UnsupportedMedia"; page: number; kind: string; count: number }
  | { type: "Ole"; page: number }
  | { type: "Macro" }
  | { type: "DelegateToHost"; format: Format }
  | { type: "Truncated"; limit: string; of: number };
export interface Meta {
  title: string | null;
  creator: string | null;
  substituted_fonts: Record<string, string>;
}
export interface Rendered { pages: Page[]; format: Format; unrendered: Unrendered[]; meta: Meta }
export interface RenderOptions { filename?: string; strict?: boolean; sheetHeaders?: boolean; only?: { first: number; last: number } }
export interface RasterOptions { scale?: number }
export default function init(input?: RequestInfo | URL | Response | BufferSource | WebAssembly.Module): Promise<unknown>;
/**
 * Retains the display list inside WASM. Use this for large sheets so only a
 * viewport's items or pixels cross into JavaScript.
 */
export function open(bytes: Uint8Array, options?: RenderOptions): Document;
export class Document {
  free(): void;
  readonly pageCount: number;
  readonly format: Format;
  readonly unrendered: Unrendered[];
  readonly meta: Meta;
  pageInfo(page: number): PageInfo;
  itemsInRect(page: number, rect: Rect): Item[];
  setColumnWidth(page: number, column: number, widthPx: number): void;
  autoFitColumn(page: number, column: number): number;
  resetColumnWidths(page: number): void;
  renderRectRgba(page: number, rect: Rect, scale: number): Uint8Array;
  renderRectToCanvas(page: number, rect: Rect, canvas: HTMLCanvasElement, options?: RasterOptions): void;
}
export function render(bytes: Uint8Array, options?: RenderOptions): Rendered;
export function pageToSvg(document: Rendered, page: number): string;
export function renderPageRgba(document: Rendered, page: number, scale: number): Uint8Array;
export function itemsInRect(document: Rendered, page: number, rect: Rect): Item[];
export function renderRectToCanvas(document: Rendered, page: number, rect: Rect, canvas: HTMLCanvasElement, options?: RasterOptions): void;
export function renderToCanvas(document: Rendered, page: number, canvas: HTMLCanvasElement, options?: RasterOptions): void;
export function addFont(bytes: Uint8Array): void;
