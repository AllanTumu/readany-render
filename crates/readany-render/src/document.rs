use crate::RenderError;
use crate::model::{
    ColumnSpan, FrozenPanes, GlyphRun, Item, Page, PathCommand, Rect, Rendered, SheetGrid,
    SourceRef,
};

/// A single digit plus a usable drag target remains readable at this floor.
pub const MIN_COLUMN_WIDTH_PX: f32 = 24.0;
/// One desktop viewport prevents an accidental drag from creating an enormous raster.
pub const MAX_COLUMN_WIDTH_PX: f32 = 800.0;
/// XLSX cells use four display pixels on each horizontal edge.
pub const AUTO_FIT_PADDING_PX: f32 = 8.0;

/// Owns caller-only layout overrides alongside, but outside, canonical render identity.
pub struct Document {
    rendered: Rendered,
    base_grids: Vec<Option<SheetGrid>>,
}

impl Document {
    pub fn new(rendered: Rendered) -> Self {
        let base_grids = rendered
            .pages
            .iter()
            .map(|page| page.grid.clone())
            .collect();
        Self {
            rendered,
            base_grids,
        }
    }

    pub fn rendered(&self) -> &Rendered {
        &self.rendered
    }

    pub fn set_column_width(
        &mut self,
        page: usize,
        column: u32,
        width_px: f32,
    ) -> Result<(), RenderError> {
        if !width_px.is_finite() {
            return Err(RenderError::invalid_options(
                "column width is not finite; choose a width between 24 and 800 pixels",
            ));
        }
        let page = self.rendered.pages.get_mut(page).ok_or_else(|| {
            RenderError::invalid_options("page index is out of range; choose a visible sheet")
        })?;
        let old_grid = page.grid.clone().ok_or_else(|| {
            RenderError::invalid_options(
                "the selected page is not a sheet; choose a spreadsheet page",
            )
        })?;
        let mut new_grid = old_grid.clone();
        let position = new_grid
            .columns
            .iter()
            .position(|span| span.index == column)
            .ok_or_else(|| {
                RenderError::invalid_options(
                    "column index is out of range; choose a visible sheet column",
                )
            })?;
        let width = width_px.clamp(MIN_COLUMN_WIDTH_PX, MAX_COLUMN_WIDTH_PX);
        let delta = width - new_grid.columns[position].width;
        if delta.abs() <= f32::EPSILON {
            return Ok(());
        }
        new_grid.columns[position].width = width;
        for span in new_grid.columns.iter_mut().skip(position + 1) {
            span.x += delta;
        }
        relayout_page(page, &old_grid, new_grid);
        Ok(())
    }

    pub fn auto_fit_column(&mut self, page: usize, column: u32) -> Result<f32, RenderError> {
        let page_ref = self.rendered.pages.get(page).ok_or_else(|| {
            RenderError::invalid_options("page index is out of range; choose a visible sheet")
        })?;
        if page_ref
            .grid
            .as_ref()
            .is_none_or(|grid| !grid.columns.iter().any(|span| span.index == column))
        {
            return Err(RenderError::invalid_options(
                "column index is out of range; choose a visible sheet column",
            ));
        }
        let shaped_width = widest_run(&page_ref.items, column);
        let width =
            (shaped_width + AUTO_FIT_PADDING_PX).clamp(MIN_COLUMN_WIDTH_PX, MAX_COLUMN_WIDTH_PX);
        self.set_column_width(page, column, width)?;
        Ok(width)
    }

    pub fn reset_column_widths(&mut self, page: usize) -> Result<(), RenderError> {
        let target = self
            .base_grids
            .get(page)
            .ok_or_else(|| {
                RenderError::invalid_options("page index is out of range; choose a visible sheet")
            })?
            .clone()
            .ok_or_else(|| {
                RenderError::invalid_options(
                    "the selected page is not a sheet; choose a spreadsheet page",
                )
            })?;
        let page = self.rendered.pages.get_mut(page).ok_or_else(|| {
            RenderError::invalid_options("page index is out of range; choose a visible sheet")
        })?;
        let old = page.grid.clone().ok_or_else(|| {
            RenderError::invalid_options(
                "the selected page is not a sheet; choose a spreadsheet page",
            )
        })?;
        relayout_page(page, &old, target);
        Ok(())
    }
}

fn widest_run(items: &[Item], column: u32) -> f32 {
    items
        .iter()
        .map(|item| match item {
            Item::Glyphs(run) => {
                if cell_column(run.source.as_ref()) == Some(column) {
                    run.glyphs.iter().map(|glyph| glyph.x_advance).sum()
                } else {
                    0.0
                }
            }
            Item::Path(_) | Item::Image(_) => 0.0,
            Item::Group(group) => widest_run(&group.items, column),
        })
        .fold(0.0, f32::max)
}

fn cell_column(source: Option<&SourceRef>) -> Option<u32> {
    match source {
        Some(SourceRef::Cell { column, .. }) => Some(*column),
        Some(SourceRef::Text { .. }) | Some(SourceRef::Shape { .. }) | None => None,
    }
}

fn relayout_page(page: &mut Page, old_grid: &SheetGrid, new_grid: SheetGrid) {
    for item in &mut page.items {
        relayout_item(item, old_grid, &new_grid, None);
    }
    page.size.width = new_grid
        .columns
        .last()
        .map(|span| span.x + span.width)
        .unwrap_or(page.size.width);
    if let Some(frozen) = &mut page.frozen {
        resize_frozen(frozen, &new_grid);
    }
    page.grid = Some(new_grid);
}

fn resize_frozen(frozen: &mut FrozenPanes, grid: &SheetGrid) {
    if frozen.columns > 0 {
        frozen.width = grid
            .columns
            .get(frozen.columns.saturating_sub(1) as usize)
            .map(|span| span.x + span.width)
            .unwrap_or(frozen.width);
    }
    if frozen.rows > 0 {
        frozen.height = grid
            .rows
            .get(frozen.rows.saturating_sub(1) as usize)
            .map(|span| span.y + span.height)
            .unwrap_or(frozen.height);
    }
}

fn relayout_item(
    item: &mut Item,
    old_grid: &SheetGrid,
    new_grid: &SheetGrid,
    cell_clip: Option<(Rect, Rect)>,
) {
    match item {
        Item::Glyphs(run) => relayout_run(run, old_grid, new_grid, cell_clip),
        Item::Path(path) => {
            for command in &mut path.path.commands {
                match command {
                    PathCommand::Move(point) | PathCommand::Line(point) => {
                        point.x = remap_x(point.x, old_grid, new_grid);
                    }
                    PathCommand::Quad(control, point) => {
                        control.x = remap_x(control.x, old_grid, new_grid);
                        point.x = remap_x(point.x, old_grid, new_grid);
                    }
                    PathCommand::Cubic(first, second, point) => {
                        first.x = remap_x(first.x, old_grid, new_grid);
                        second.x = remap_x(second.x, old_grid, new_grid);
                        point.x = remap_x(point.x, old_grid, new_grid);
                    }
                    PathCommand::Close => {}
                }
            }
        }
        Item::Image(image) => {
            let right = remap_x(image.rect.x + image.rect.width, old_grid, new_grid);
            image.rect.x = remap_x(image.rect.x, old_grid, new_grid);
            image.rect.width = right - image.rect.x;
        }
        Item::Group(group) => {
            let clips = group.clip.map(|old| {
                let right = remap_x(old.x + old.width, old_grid, new_grid);
                let new = Rect {
                    x: remap_x(old.x, old_grid, new_grid),
                    width: right - remap_x(old.x, old_grid, new_grid),
                    ..old
                };
                group.clip = Some(new);
                (old, new)
            });
            for child in &mut group.items {
                relayout_item(child, old_grid, new_grid, clips.or(cell_clip));
            }
        }
    }
}

fn relayout_run(
    run: &mut GlyphRun,
    old_grid: &SheetGrid,
    new_grid: &SheetGrid,
    cell_clip: Option<(Rect, Rect)>,
) {
    if let Some((old, new)) = cell_clip {
        let width: f32 = run.glyphs.iter().map(|glyph| glyph.x_advance).sum();
        let left_gap = run.origin.x - old.x;
        let right_gap = old.x + old.width - run.origin.x - width;
        run.origin.x = if (left_gap - right_gap).abs() <= 1.0 {
            new.x + (new.width - width) / 2.0
        } else if right_gap < left_gap {
            new.x + new.width - width - right_gap
        } else {
            new.x + left_gap
        };
        return;
    }
    if let Some(column) = cell_column(run.source.as_ref()) {
        if let (Some(old), Some(new)) =
            (column_span(old_grid, column), column_span(new_grid, column))
        {
            let is_header = old_grid
                .rows
                .first()
                .is_some_and(|row| run.origin.y < row.y);
            if is_header {
                let width: f32 = run.glyphs.iter().map(|glyph| glyph.x_advance).sum();
                run.origin.x = new.x + (new.width - width) / 2.0;
            } else {
                run.origin.x = new.x + (run.origin.x - old.x);
            }
            return;
        }
    }
    run.origin.x = remap_x(run.origin.x, old_grid, new_grid);
}

fn column_span(grid: &SheetGrid, column: u32) -> Option<&ColumnSpan> {
    grid.columns.iter().find(|span| span.index == column)
}

fn remap_x(x: f32, old_grid: &SheetGrid, new_grid: &SheetGrid) -> f32 {
    for (old, new) in old_grid.columns.iter().zip(&new_grid.columns) {
        if (x - old.x).abs() <= 0.01 {
            return new.x;
        }
        let old_right = old.x + old.width;
        if (x - old_right).abs() <= 0.01 {
            return new.x + new.width;
        }
        if x > old.x && x < old_right {
            return new.x + (x - old.x);
        }
    }
    let old_right = old_grid.columns.last().map(|span| span.x + span.width);
    let new_right = new_grid.columns.last().map(|span| span.x + span.width);
    match (old_right, new_right) {
        (Some(old), Some(new)) if x >= old => x + new - old,
        _ => x,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FontSource, Options, render};

    #[test]
    fn a_width_override_moves_following_columns_without_changing_sources() {
        let bytes = b"short,a value much wider than its cell\nnext,row";
        let rendered = render(
            bytes,
            &Options {
                filename: Some("claim.csv"),
                fonts: FontSource::default(),
                ..Options::default()
            },
        )
        .unwrap_or_else(|error| panic!("the CSV fixture renders: {error}"));
        let mut document = Document::new(rendered);
        let before = document.rendered().pages[0].clone();
        let before_source = before.items[0].clone();
        let before_width = before.size.width;
        document
            .set_column_width(0, 0, 300.0)
            .unwrap_or_else(|error| panic!("the first column resizes: {error}"));
        let after = &document.rendered().pages[0];
        assert!(after.size.width > before_width);
        assert_eq!(source_of(&before_source), source_of(&after.items[0]));
        assert_eq!(
            after.grid.as_ref().map(|grid| grid.columns[0].width),
            Some(300.0)
        );
    }

    #[test]
    fn auto_fit_uses_the_shaped_advance_plus_excel_padding() {
        let rendered = render(
            b"a value much wider than its cell,x",
            &Options {
                filename: Some("claim.csv"),
                fonts: FontSource::default(),
                ..Options::default()
            },
        )
        .unwrap_or_else(|error| panic!("the CSV fixture renders: {error}"));
        let shaped = widest_run(&rendered.pages[0].items, 0);
        let mut document = Document::new(rendered);
        let chosen = document
            .auto_fit_column(0, 0)
            .unwrap_or_else(|error| panic!("the first column auto-fits: {error}"));
        assert!(chosen >= shaped + AUTO_FIT_PADDING_PX);
    }

    #[test]
    fn reset_restores_the_default_render_exactly() {
        let rendered = render(
            b"one,two\nthree,four",
            &Options {
                filename: Some("claim.csv"),
                fonts: FontSource::default(),
                ..Options::default()
            },
        )
        .unwrap_or_else(|error| panic!("the CSV fixture renders: {error}"));
        let default = serde_json::to_vec(&rendered)
            .unwrap_or_else(|error| panic!("the default serialises: {error}"));
        let mut document = Document::new(rendered);
        document
            .set_column_width(0, 0, 350.0)
            .unwrap_or_else(|error| panic!("the first column resizes: {error}"));
        document
            .reset_column_widths(0)
            .unwrap_or_else(|error| panic!("the widths reset: {error}"));
        let reset = serde_json::to_vec(document.rendered())
            .unwrap_or_else(|error| panic!("the reset serialises: {error}"));
        assert_eq!(reset, default);
    }

    fn source_of(item: &Item) -> Option<&SourceRef> {
        match item {
            Item::Glyphs(run) => run.source.as_ref(),
            Item::Path(path) => path.source.as_ref(),
            Item::Image(image) => image.source.as_ref(),
            Item::Group(group) => group.source.as_ref(),
        }
    }
}
