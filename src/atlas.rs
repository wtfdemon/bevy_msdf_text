//! Font loading: a TTF/OTF becomes one MTSDF atlas plus glyph metrics.
//!
//! Every glyph of the charset is turned into a multi-channel signed distance
//! field with a true SDF in alpha (MTSDF) by `fdsm`, packed into a single
//! linear `Rgba8Unorm` texture. All metrics are stored in em units, so the
//! same atlas serves every text size.

use bevy::{
    asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader},
    image::{ImageSampler, ImageSamplerDescriptor},
    platform::{collections::HashMap, time::Instant},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use fdsm::{
    bezier::scanline::FillRule,
    correct_error::{ErrorCorrectionConfig, correct_error_mtsdf},
    generate::generate_mtsdf,
    render::correct_sign_mtsdf,
    shape::Shape,
    transform::Transform as _,
};
use fdsm_ttf_parser::ttf_parser::{self, Face, GlyphId};
use image::Rgba32FImage;
use nalgebra::{Affine2, Matrix3};
use serde::{Deserialize, Serialize};

/// Printable ASCII, the default charset.
pub const ASCII: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";

/// A font baked into an MTSDF atlas. Load it with
/// `asset_server.load::<MsdfFont>("fonts/MyFont.ttf")`.
#[derive(Asset, TypePath, Debug)]
pub struct MsdfFont {
    /// RGB = MSDF, A = true SDF. Linear, never sRGB.
    pub atlas: Handle<Image>,
    pub glyphs: HashMap<char, Glyph>,
    /// Extra advance (em) between a pair, from GPOS pair adjustment or `kern`.
    pub kerning: HashMap<(char, char), f32>,
    /// Baseline to top of the tallest glyphs, em.
    pub ascender: f32,
    /// Baseline to bottom (negative), em.
    pub descender: f32,
    /// Baseline-to-baseline distance, em.
    pub line_height: f32,
    /// The distance field covers `±range_em / 2` around every outline.
    /// Outline width + shadow softness should stay inside that.
    pub range_em: f32,
    /// The same range in atlas texels (msdfgen's `pxRange`).
    pub range_px: f32,
}

/// One glyph's metrics, all in em units with +y up from the baseline.
#[derive(Debug, Clone, Copy)]
pub struct Glyph {
    pub advance: f32,
    /// The quad to draw relative to the pen position, padded by `range_em / 2`
    /// so effects outside the outline have room. `None` for blank glyphs.
    pub quad: Option<GlyphQuad>,
}

#[derive(Debug, Clone, Copy)]
pub struct GlyphQuad {
    /// em, relative to the pen on the baseline.
    pub plane: Rect,
    /// Normalized atlas coordinates (y down, like the texture).
    pub uv: Rect,
}

/// How the atlas is generated. Set it with `load_with_settings`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsdfAtlasSettings {
    /// Texels per em in the atlas. 48 holds sharp corners from tiny to huge.
    pub em_px: f32,
    /// Distance-field range in texels (msdfgen's `pxRange`). Bigger means
    /// thicker possible outlines and softer shadows, at the cost of atlas size.
    pub range_px: f32,
    /// Characters to bake. Anything else renders as `?` (or is skipped).
    pub charset: String,
}

impl Default for MsdfAtlasSettings {
    fn default() -> Self {
        Self { em_px: 48.0, range_px: 24.0, charset: ASCII.into() }
    }
}

#[derive(Default, TypePath)]
pub struct MsdfFontLoader;

impl AssetLoader for MsdfFontLoader {
    type Asset = MsdfFont;
    type Settings = MsdfAtlasSettings;
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &MsdfAtlasSettings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<MsdfFont, BevyError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let start = Instant::now();
        let baked = bake(&bytes, settings)?;
        info!(
            "msdf atlas {}: {} glyphs, {}x{}, {} kerning pairs, {:.0} ms",
            load_context.path(),
            baked.font.glyphs.len(),
            baked.image.width(),
            baked.image.height(),
            baked.font.kerning.len(),
            start.elapsed().as_secs_f64() * 1e3,
        );
        let atlas = load_context.add_labeled_asset("atlas", baked.image);
        Ok(MsdfFont { atlas, ..baked.font })
    }

    // No extensions: `.ttf` stays Bevy's `Font`. A typed
    // `load::<MsdfFont>(..)` picks this loader by asset type.
}

struct Baked {
    font: MsdfFont,
    image: Image,
}

/// One glyph's distance field before packing.
struct Cell {
    c: char,
    field: Rgba32FImage,
    plane: Rect,
    pos: UVec2,
}

fn bake(bytes: &[u8], settings: &MsdfAtlasSettings) -> Result<Baked, BevyError> {
    let face = Face::parse(bytes, 0)?;
    let upem = face.units_per_em() as f64;
    let scale = settings.em_px as f64 / upem; // texels per font unit
    let range = settings.range_px as f64;
    let pad = range / 2.0;
    let em = |units: f64| (units / upem) as f32;

    let mut glyphs = HashMap::default();
    let mut cells = Vec::new();
    for c in settings.charset.chars() {
        let Some(id) = face.glyph_index(c) else { continue };
        let advance = em(face.glyph_hor_advance(id).unwrap_or(0) as f64);
        glyphs.insert(c, Glyph { advance, quad: None });
        let (Some(bbox), Some(mut shape)) =
            (face.glyph_bounding_box(id), fdsm_ttf_parser::load_shape_from_face(&face, id))
        else {
            continue; // blank, e.g. space
        };
        let (x0, y0, x1, y1) = (bbox.x_min as f64, bbox.y_min as f64, bbox.x_max as f64, bbox.y_max as f64);
        let w = ((x1 - x0) * scale + range).ceil() as u32;
        let h = ((y1 - y0) * scale + range).ceil() as u32;
        // Font units (y up) -> texels of this cell (y down), `pad` texels of margin.
        shape.transform(&Affine2::from_matrix_unchecked(Matrix3::new(
            scale, 0.0, pad - x0 * scale,
            0.0, -scale, pad + y1 * scale,
            0.0, 0.0, 1.0,
        )));
        let colored = Shape::edge_coloring_simple(shape, 3.0f64.sin(), 0x5eed);
        let prepared = colored.prepare();
        let mut field = Rgba32FImage::new(w, h);
        generate_mtsdf(&prepared, range, &mut field);
        correct_error_mtsdf(&mut field, &colored, &prepared, range, &ErrorCorrectionConfig::default());
        correct_sign_mtsdf(&mut field, &prepared, FillRule::Nonzero);
        // The cell's corners in em: texel (0, 0) is font (x0 - pad, y1 + pad).
        let left = x0 - pad / scale;
        let top = y1 + pad / scale;
        let plane = Rect::new(
            em(left),
            em(top - h as f64 / scale),
            em(left + w as f64 / scale),
            em(top),
        );
        cells.push(Cell { c, field, plane, pos: UVec2::ZERO });
    }

    let size = pack(&mut cells);
    let mut data = vec![0u8; (size.x * size.y * 4) as usize];
    for cell in &cells {
        for (x, y, px) in cell.field.enumerate_pixels() {
            let i = (((cell.pos.y + y) * size.x + cell.pos.x + x) * 4) as usize;
            for (k, v) in px.0.iter().enumerate() {
                data[i + k] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
        let (w, h) = (cell.field.width() as f32, cell.field.height() as f32);
        let p = cell.pos.as_vec2();
        let uv = Rect::from_corners(p / size.as_vec2(), (p + Vec2::new(w, h)) / size.as_vec2());
        glyphs.get_mut(&cell.c).unwrap().quad = Some(GlyphQuad { plane: cell.plane, uv });
    }

    let mut image = Image::new(
        Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());

    let kerning = kerning(&face, &settings.charset)
        .into_iter()
        .map(|(pair, units)| (pair, em(units as f64)))
        .collect();
    let line_gap = face.line_gap() as f64;
    Ok(Baked {
        font: MsdfFont {
            atlas: Handle::default(),
            glyphs,
            kerning,
            ascender: em(face.ascender() as f64),
            descender: em(face.descender() as f64),
            line_height: em(face.ascender() as f64 - face.descender() as f64 + line_gap),
            range_em: settings.range_px / settings.em_px,
            range_px: settings.range_px,
        },
        image,
    })
}

/// Shelf packing, tallest first, with a 1 texel gutter. Returns the atlas size.
fn pack(cells: &mut [Cell]) -> UVec2 {
    const GUTTER: u32 = 1;
    cells.sort_by_key(|c| std::cmp::Reverse(c.field.height()));
    let area: u32 = cells
        .iter()
        .map(|c| (c.field.width() + GUTTER) * (c.field.height() + GUTTER))
        .sum();
    let widest = cells.iter().map(|c| c.field.width() + GUTTER).max().unwrap_or(1);
    let width = ((area as f32 * 1.15).sqrt() as u32).max(widest).next_multiple_of(64);
    let (mut x, mut y, mut shelf) = (GUTTER, GUTTER, 0);
    for cell in cells.iter_mut() {
        let (w, h) = (cell.field.width(), cell.field.height());
        if x + w + GUTTER > width {
            (x, y, shelf) = (GUTTER, y + shelf + GUTTER, 0);
        }
        cell.pos = UVec2::new(x, y);
        x += w + GUTTER;
        shelf = shelf.max(h);
    }
    UVec2::new(width, (y + shelf + GUTTER).next_multiple_of(4))
}

/// Horizontal pair kerning for the charset, in font units: GPOS pair
/// adjustment where the font has it (all modern fonts), else the old `kern`.
fn kerning(face: &Face, charset: &str) -> HashMap<(char, char), i32> {
    use ttf_parser::gpos::{PairAdjustment, PositioningSubtable};
    let ids: Vec<(char, GlyphId)> =
        charset.chars().filter_map(|c| Some((c, face.glyph_index(c)?))).collect();
    let mut out = HashMap::default();
    let mut add = |pair, v: i32| {
        if v != 0 {
            *out.entry(pair).or_insert(0) += v;
        }
    };
    if let Some(gpos) = face.tables().gpos {
        for lookup in gpos.lookups {
            let pairs: Vec<PairAdjustment> = lookup
                .subtables
                .into_iter::<PositioningSubtable>()
                .filter_map(|s| match s {
                    PositioningSubtable::Pair(p) => Some(p),
                    _ => None,
                })
                .collect();
            if pairs.is_empty() {
                continue;
            }
            for &(a, ga) in &ids {
                for &(b, gb) in &ids {
                    // Within one lookup the first subtable that covers the pair wins.
                    let hit = pairs.iter().find_map(|p| match p {
                        PairAdjustment::Format1 { coverage, sets } => {
                            sets.get(coverage.get(ga)?)?.get(gb).map(|(v, _)| v.x_advance)
                        }
                        PairAdjustment::Format2 { coverage, classes, matrix } => {
                            coverage.get(ga)?;
                            matrix.get((classes.0.get(ga), classes.1.get(gb))).map(|(v, _)| v.x_advance)
                        }
                    });
                    if let Some(v) = hit {
                        add((a, b), v as i32);
                    }
                }
            }
        }
    } else if let Some(kern) = face.tables().kern {
        for sub in kern.subtables {
            if !sub.horizontal || sub.variable {
                continue;
            }
            for &(a, ga) in &ids {
                for &(b, gb) in &ids {
                    if let Some(v) = sub.glyphs_kerning(ga, gb) {
                        add((a, b), v as i32);
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bakes_ascii() {
        let bytes = std::fs::read("assets/fonts/FiraSans-Bold.ttf").unwrap();
        let start = std::time::Instant::now();
        let baked = bake(&bytes, &MsdfAtlasSettings::default()).unwrap();
        println!(
            "{} glyphs, {}x{}, {} kerning pairs, {:?}",
            baked.font.glyphs.len(),
            baked.image.width(),
            baked.image.height(),
            baked.font.kerning.len(),
            start.elapsed()
        );
        assert_eq!(baked.font.glyphs.len(), ASCII.chars().count());
        assert!(baked.font.glyphs[&' '].quad.is_none());
        let a = baked.font.glyphs[&'A'].quad.unwrap();
        assert!(a.plane.width() > 0.5 && a.uv.max.x <= 1.0 && a.uv.max.y <= 1.0);
        // The texel at the center of 'I' is inside: all channels above 0.5.
        let i = baked.font.glyphs[&'I'].quad.unwrap();
        let size = UVec2::new(baked.image.width(), baked.image.height()).as_vec2();
        let c = (i.uv.center() * size).as_uvec2();
        let data = baked.image.data.as_ref().unwrap();
        let px = &data[((c.y * size.x as u32 + c.x) * 4) as usize..][..4];
        assert!(px.iter().all(|&v| v > 128), "{px:?}");
        if std::env::var_os("MSDF_DUMP").is_some() {
            image::save_buffer("/tmp/msdf_atlas.png", data, size.x as u32, size.y as u32, image::ExtendedColorType::Rgba8).unwrap();
        }
    }
}
