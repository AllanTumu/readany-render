use crate::model::*;
use crate::{Format, Options, RenderError};
use image::GenericImageView;
use std::io::Cursor;

pub(crate) fn render(
    bytes: &[u8],
    format: Format,
    options: &Options<'_>,
) -> Result<Rendered, RenderError> {
    // Dimensions are read before pixel allocation so a compressed image cannot
    // cross the pixel ceiling merely by reaching the decoder.
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| RenderError::malformed("the image header is damaged; obtain a fresh copy"))?;
    let (encoded_width, encoded_height) = reader.into_dimensions().map_err(|_| {
        RenderError::malformed("the image dimensions are damaged; obtain a fresh copy")
    })?;
    let pixels = u64::from(encoded_width)
        .checked_mul(u64::from(encoded_height))
        .ok_or_else(|| RenderError::limit("image_pixels", u64::MAX))?;
    if pixels > options.limits.image_pixels {
        return Err(RenderError::limit("image_pixels", pixels));
    }
    let image = image::load_from_memory(bytes)
        .map_err(|_| RenderError::malformed("the image data is damaged; obtain a fresh copy"))?;
    let image = apply_orientation(image, bytes, format);
    let (width, height) = image.dimensions();
    let mut normalized = Cursor::new(Vec::new());
    image
        .write_to(&mut normalized, image::ImageFormat::Png)
        .map_err(|_| {
            RenderError::malformed("the decoded image could not be normalized; obtain a fresh copy")
        })?;
    let size = Size {
        width: width as f32,
        height: height as f32,
    };
    Ok(Rendered {
        pages: vec![Page {
            size,
            label: options.filename.map(str::to_owned),
            items: vec![Item::Image(ImageItem {
                data: ImageData {
                    mime: "image/png".into(),
                    bytes: normalized.into_inner(),
                    pixel_size: size,
                },
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: size.width,
                    height: size.height,
                },
                source: None,
            })],
            source: None,
            frozen: None,
        }],
        format,
        unrendered: Vec::new(),
        meta: Meta::default(),
    })
}

fn apply_orientation(
    image: image::DynamicImage,
    bytes: &[u8],
    format: Format,
) -> image::DynamicImage {
    if format != Format::Jpeg {
        return image;
    }
    let orientation = exif::Reader::new()
        .read_from_container(&mut Cursor::new(bytes))
        .ok()
        .and_then(|fields| {
            fields
                .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .cloned()
        })
        .and_then(|field| field.value.get_uint(0))
        .unwrap_or(1);
    orient(image, orientation)
}

fn orient(image: image::DynamicImage, orientation: u32) -> image::DynamicImage {
    match orientation {
        2 => image.fliph(),
        3 => image.rotate180(),
        4 => image.flipv(),
        5 => image.fliph().rotate90(),
        6 => image.rotate90(),
        7 => image.fliph().rotate270(),
        8 => image.rotate270(),
        _ => image,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    #[test]
    fn exif_orientation_six_swaps_the_page_axes() {
        let image = image::DynamicImage::ImageRgba8(ImageBuffer::from_fn(3, 2, |x, y| {
            Rgba([x as u8, y as u8, 0, 255])
        }));
        let oriented = orient(image, 6);
        assert_eq!((oriented.width(), oriented.height()), (2, 3));
    }

    #[test]
    fn every_exif_orientation_preserves_pixel_count() {
        for orientation in 1..=8 {
            let image = image::DynamicImage::ImageRgba8(ImageBuffer::new(7, 3));
            let oriented = orient(image, orientation);
            assert_eq!(oriented.width() * oriented.height(), 21);
        }
    }
}
