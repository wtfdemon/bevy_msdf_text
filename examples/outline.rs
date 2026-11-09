//! A gallery of styles (plain, outline, thick outline, glow, drop shadow) at
//! several pixel sizes, all from the same atlas.

use bevy::{asset::AssetMetaCheck, prelude::*};
use bevy_msdf_text::{MsdfFont, MsdfShadow, MsdfText, MsdfTextMode, MsdfTextPlugin, TextAlign};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(AssetPlugin { meta_check: AssetMetaCheck::Never, ..default() }),
            MsdfTextPlugin,
        ))
        .insert_resource(ClearColor(Color::srgb(0.16, 0.18, 0.24)))
        .add_systems(Startup, setup)
        .run();
}

const INK: Color = Color::srgb(0.05, 0.05, 0.08);
const GOLD: Color = Color::srgb(1.0, 0.82, 0.30);

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn(Camera3d::default());
    let font: Handle<MsdfFont> = assets.load("fonts/FiraSans-Bold.ttf");

    let styles: [(&str, fn(MsdfText) -> MsdfText); 5] = [
        ("Plain", |t| t),
        ("Outline", |t| t.with_outline(0.08, Color::BLACK)),
        ("Thick", |t| t.with_color(GOLD).with_outline(0.2, INK)),
        ("Glow", |t| t.with_shadow(MsdfShadow::glow(Color::srgb(0.2, 0.8, 1.0), 0.2))),
        ("Shadow", |t| t.with_shadow(MsdfShadow::drop(Color::srgba(0.0, 0.0, 0.0, 0.8), Vec2::new(0.06, -0.08), 0.08))),
    ];
    let sizes = [12.0, 18.0, 28.0, 44.0, 64.0];

    let mut y = 24.0;
    for size in sizes {
        for (col, (name, style)) in styles.iter().enumerate() {
            commands.spawn(
                style(MsdfText::new(*name, font.clone()))
                    .with_mode(MsdfTextMode::Screen {
                        at: Vec2::new((col as f32 + 0.5) / styles.len() as f32, 0.0),
                        offset: Vec2::new(0.0, y),
                    })
                    .with_size(size)
                    .with_anchor(Vec2::new(0.0, 0.5)),
            );
        }
        y += size * 1.8 + 12.0;
    }
    commands.spawn(
        MsdfText::new("one 48 px atlas,\nevery size", font)
            .with_mode(MsdfTextMode::Screen { at: Vec2::new(0.5, 1.0), offset: Vec2::new(0.0, -24.0) })
            .with_size(40.0)
            .with_anchor(Vec2::new(0.0, -0.5))
            .with_align(TextAlign::Center)
            .with_color(GOLD)
            .with_outline(0.1, INK),
    );
}
