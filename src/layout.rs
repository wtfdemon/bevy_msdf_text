//! Text layout: a string becomes one mesh of glyph quads in em units.
//!
//! Positions are em, +y up, relative to the text's anchor point; the vertex
//! shader scales and places them per [`crate::MsdfTextMode`]. `position.z`
//! carries the layer: 0 = fill, 1 = outline, 2 = shadow. Layers are emitted
//! back to front, and triangles draw in order, so no sorting is needed.

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

use crate::{MsdfFont, MsdfText, TextAlign};

const FILL: f32 = 0.0;
const OUTLINE: f32 = 1.0;
const SHADOW: f32 = 2.0;

/// A line of text: its first glyph in `placed` and its advance width (em).
#[derive(Debug, Clone, Copy)]
struct Line {
    start: usize,
    width: f32,
}

pub(crate) fn build_mesh(font: &MsdfFont, text: &MsdfText) -> Mesh {
    // Pass 1: pen positions per glyph, line widths.
    let mut placed: Vec<(char, Vec2)> = Vec::with_capacity(text.text.len());
    let mut lines = vec![Line { start: 0, width: 0.0 }];
    let (mut x, mut y) = (0.0f32, 0.0f32);
    let mut prev = None;
    let line_height = font.line_height * text.line_height;
    for c in text.text.chars() {
        if c == '\n' {
            lines.last_mut().unwrap().width = x;
            lines.push(Line { start: placed.len(), width: 0.0 });
            (x, y, prev) = (0.0, y - line_height, None);
            continue;
        }
        let c = if font.glyphs.contains_key(&c) { c } else { '?' };
        let Some(glyph) = font.glyphs.get(&c) else { continue };
        if let Some(p) = prev {
            x += font.kerning.get(&(p, c)).copied().unwrap_or(0.0);
        }
        placed.push((c, Vec2::new(x, y)));
        x += glyph.advance + text.letter_spacing;
        prev = Some(c);
    }
    lines.last_mut().unwrap().width = x;

    // The block: x in [0, width], y from the first line's ascender down to
    // the last line's descender. The anchor picks a point in it.
    let width = lines.iter().map(|l| l.width).fold(0.0, f32::max);
    let top = font.ascender;
    let bottom = y + font.descender;
    let origin = Vec2::new(
        width * (text.anchor.x + 0.5),
        bottom + (top - bottom) * (text.anchor.y + 0.5),
    );

    // Back to front, each a full pass over the glyphs so that no glyph's
    // outline or shadow lands on its neighbour's fill.
    let mut layers = Vec::with_capacity(3);
    if let Some(s) = text.shadow.filter(|s| s.color.alpha() > 0.0) {
        layers.push((SHADOW, s.offset));
    }
    if text.outline_width > 0.0 && text.outline_color.alpha() > 0.0 {
        layers.push((OUTLINE, Vec2::ZERO));
    }
    layers.push((FILL, Vec2::ZERO));
    let quads = placed.len() * layers.len();
    let mut positions = Vec::with_capacity(quads * 4);
    let mut uvs = Vec::with_capacity(quads * 4);
    let mut push = |plane: Rect, uv: Rect, layer: f32| {
        positions.extend([
            [plane.min.x, plane.min.y, layer],
            [plane.max.x, plane.min.y, layer],
            [plane.max.x, plane.max.y, layer],
            [plane.min.x, plane.max.y, layer],
        ]);
        // Atlas v grows downward, em y upward.
        uvs.extend([
            [uv.min.x, uv.max.y],
            [uv.max.x, uv.max.y],
            [uv.max.x, uv.min.y],
            [uv.min.x, uv.min.y],
        ]);
    };
    for &(layer, offset) in &layers {
        for (i, line) in lines.iter().enumerate() {
            let end = lines.get(i + 1).map_or(placed.len(), |l| l.start);
            let align = match text.align {
                TextAlign::Left => 0.0,
                TextAlign::Center => (width - line.width) * 0.5,
                TextAlign::Right => width - line.width,
            };
            for &(c, pen) in &placed[line.start..end] {
                let Some(q) = font.glyphs[&c].quad else { continue };
                let shift = pen + Vec2::new(align, 0.0) - origin + offset;
                push(Rect { min: q.plane.min + shift, max: q.plane.max + shift }, q.uv, layer);
            }
        }
    }

    if positions.is_empty() {
        // Nothing visible: one empty quad keeps the mesh valid for the GPU.
        positions = vec![[0.0; 3]; 4];
        uvs = vec![[0.0; 2]; 4];
    }
    let indices = (0..positions.len() as u32 / 4)
        .flat_map(|q| [0, 1, 2, 0, 2, 3].map(|i| q * 4 + i))
        .collect();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}
