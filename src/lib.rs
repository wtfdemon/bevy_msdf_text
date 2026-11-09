#![doc = include_str!("../README.md")]

mod atlas;
mod layout;
mod material;

pub use atlas::{ASCII, Glyph, GlyphQuad, MsdfAtlasSettings, MsdfFont, MsdfFontLoader};
pub use material::{MsdfMaterial, MsdfParams};

use bevy::{
    asset::{AssetEventSystems, embedded_asset},
    camera::visibility::NoFrustumCulling,
    platform::collections::HashSet,
    prelude::*,
    window::PrimaryWindow,
};

/// Adds MSDF text: the [`MsdfFont`] asset + loader, [`MsdfMaterial`], and the
/// system that (re)builds a [`MsdfText`]'s mesh when it changes.
pub struct MsdfTextPlugin;

/// The PostUpdate set that turns changed [`MsdfText`]s into meshes and
/// materials. Animate texts before it. It runs before bevy's
/// [`AssetEventSystems`], so a mesh or material it inserts reaches the render
/// world the same frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MsdfTextSystems;

impl Plugin for MsdfTextPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "msdf_text.wgsl");
        app.add_plugins(MaterialPlugin::<MsdfMaterial>::default())
            .init_asset::<MsdfFont>()
            .init_asset_loader::<MsdfFontLoader>()
            // Before the frame's asset events go out. Left unordered, the
            // schedule may run it after them: each insert's event then waits
            // a frame, so a text that changes two frames running has its
            // newer mesh taken to the render world with the older event, and
            // the newer event finds it gone ("RenderMesh with
            // RenderAssetUsages == RENDER_WORLD cannot be extracted: The
            // asset has already been extracted"), and every change shows a
            // frame late.
            .configure_sets(PostUpdate, MsdfTextSystems.before(AssetEventSystems))
            .add_systems(PostUpdate, update_texts.in_set(MsdfTextSystems));
    }
}

/// Where and how big a text is drawn. All modes share one shader; only the
/// vertex stage differs.
#[derive(Debug, Clone, Copy, PartialEq, Default, Reflect)]
pub enum MsdfTextMode {
    /// Billboard at the entity's position, facing the camera, scaling with
    /// distance like any object. `size` = world units per em.
    #[default]
    World,
    /// Anchored to the entity's position but a constant size on screen, like
    /// a game's name tag. `size` = logical pixels per em.
    WorldFixedPx,
    /// HUD text. `at` is a viewport point (0..1, top-left origin), `offset`
    /// logical pixels from it (+y down). `size` = logical pixels per em.
    /// The entity's transform is ignored.
    Screen { at: Vec2, offset: Vec2 },
    /// Flat in the entity's local XY plane (a sign on a wall), oriented by its
    /// transform. `size` = world units per em.
    Plane,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Reflect)]
pub enum TextAlign {
    Left,
    #[default]
    Center,
    Right,
}

/// A soft copy of the (outlined) glyphs drawn behind them: a drop shadow
/// with an offset, a glow without one. Lengths in em.
#[derive(Debug, Clone, Copy, PartialEq, Reflect)]
pub struct MsdfShadow {
    pub color: Color,
    /// +y up.
    pub offset: Vec2,
    pub softness: f32,
}

impl MsdfShadow {
    pub fn drop(color: impl Into<Color>, offset: Vec2, softness: f32) -> Self {
        Self { color: color.into(), offset, softness }
    }

    pub fn glow(color: impl Into<Color>, softness: f32) -> Self {
        Self { color: color.into(), offset: Vec2::ZERO, softness }
    }
}

/// A run of MSDF text. Lengths (outline, shadow, spacing) are em, so a style
/// looks the same at every size. Effects reach at most
/// [`MsdfFont::range_em`]` / 2` outside the glyphs (0.25 em by default):
/// keep `outline_width + shadow.softness` (plus any shadow offset) under it.
#[derive(Component, Debug, Clone, Reflect)]
#[require(Transform, Visibility, NoFrustumCulling)]
pub struct MsdfText {
    pub text: String,
    pub font: Handle<MsdfFont>,
    pub mode: MsdfTextMode,
    /// World units or logical pixels per em, see [`MsdfTextMode`].
    pub size: f32,
    pub color: Color,
    pub outline_width: f32,
    pub outline_color: Color,
    pub shadow: Option<MsdfShadow>,
    /// Which point of the text block sits on the entity: (0, 0) center,
    /// (-0.5, -0.5) bottom-left, (0, -0.5) bottom-center (above a head).
    pub anchor: Vec2,
    pub align: TextAlign,
    /// Multiplies the font's line height.
    pub line_height: f32,
    /// Extra advance per glyph, em.
    pub letter_spacing: f32,
}

impl Default for MsdfText {
    fn default() -> Self {
        Self {
            text: String::new(),
            font: Handle::default(),
            mode: MsdfTextMode::World,
            size: 1.0,
            color: Color::WHITE,
            outline_width: 0.0,
            outline_color: Color::BLACK,
            shadow: None,
            anchor: Vec2::ZERO,
            align: TextAlign::Center,
            line_height: 1.0,
            letter_spacing: 0.0,
        }
    }
}

impl MsdfText {
    pub fn new(text: impl Into<String>, font: Handle<MsdfFont>) -> Self {
        Self { text: text.into(), font, ..default() }
    }

    pub fn with_mode(self, mode: MsdfTextMode) -> Self {
        Self { mode, ..self }
    }

    pub fn with_size(self, size: f32) -> Self {
        Self { size, ..self }
    }

    pub fn with_color(self, color: impl Into<Color>) -> Self {
        Self { color: color.into(), ..self }
    }

    pub fn with_outline(self, width: f32, color: impl Into<Color>) -> Self {
        Self { outline_width: width, outline_color: color.into(), ..self }
    }

    pub fn with_shadow(self, shadow: MsdfShadow) -> Self {
        Self { shadow: Some(shadow), ..self }
    }

    pub fn with_anchor(self, anchor: Vec2) -> Self {
        Self { anchor, ..self }
    }

    pub fn with_align(self, align: TextAlign) -> Self {
        Self { align, ..self }
    }

    fn params(&self, font: &MsdfFont, scale_factor: f32) -> MsdfParams {
        let (mode, size, screen) = match self.mode {
            MsdfTextMode::World => (0, self.size, Vec4::ZERO),
            MsdfTextMode::WorldFixedPx => (1, self.size * scale_factor, Vec4::ZERO),
            MsdfTextMode::Screen { at, offset } => {
                let offset = offset * scale_factor;
                (2, self.size * scale_factor, Vec4::new(at.x, at.y, offset.x, offset.y))
            }
            MsdfTextMode::Plane => (3, self.size, Vec4::ZERO),
        };
        let shadow = self.shadow.map_or(Color::NONE, |s| s.color);
        MsdfParams {
            fill: self.color.to_linear().to_vec4(),
            outline: self.outline_color.to_linear().to_vec4(),
            shadow: shadow.to_linear().to_vec4(),
            screen,
            mode,
            size,
            outline_width: self.outline_width,
            shadow_softness: self.shadow.map_or(0.0, |s| s.softness),
            range_em: font.range_em,
            range_px: font.range_px,
        }
    }
}

/// Builds the mesh + material of every new or changed [`MsdfText`] whose font
/// is loaded, and of every text whose font or window scale changed.
fn update_texts(
    mut commands: Commands,
    fonts: Res<Assets<MsdfFont>>,
    mut font_events: MessageReader<AssetEvent<MsdfFont>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<MsdfMaterial>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut last_scale: Local<f32>,
    texts: Query<(
        Entity,
        Ref<MsdfText>,
        Option<&Mesh3d>,
        Option<&MeshMaterial3d<MsdfMaterial>>,
    )>,
) {
    let reloaded: HashSet<AssetId<MsdfFont>> = font_events
        .read()
        .filter_map(|e| match *e {
            AssetEvent::LoadedWithDependencies { id } | AssetEvent::Modified { id } => Some(id),
            _ => None,
        })
        .collect();
    let scale = window.single().map_or(1.0, |w| w.scale_factor());
    let rescaled = *last_scale != scale;
    *last_scale = scale;

    for (entity, text, mesh, material) in &texts {
        let built = mesh.is_some() && material.is_some();
        if built && !text.is_changed() && !rescaled && !reloaded.contains(&text.font.id()) {
            continue;
        }
        let Some(font) = fonts.get(&text.font) else { continue };
        let new_mesh = layout::build_mesh(font, &text);
        let new_material = MsdfMaterial { params: text.params(font, scale), atlas: font.atlas.clone() };
        match (mesh, material) {
            (Some(mesh), Some(material)) => {
                let _ = meshes.insert(mesh, new_mesh);
                let _ = materials.insert(material, new_material);
            }
            _ => {
                commands.entity(entity).insert((
                    Mesh3d(meshes.add(new_mesh)),
                    MeshMaterial3d(materials.add(new_material)),
                ));
            }
        }
    }
}
