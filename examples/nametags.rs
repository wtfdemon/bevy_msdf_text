//! Two wanderers with name tags: one keeps its pixel size at any distance,
//! the other scales like a real object. Plus HUD text and a giant sign to fly
//! up to, all from one 512 px atlas.
//!
//! Click to look (pointer lock), WASD to move, Q/E down/up, shift = fast,
//! Esc to let go of the mouse.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::{
    asset::AssetMetaCheck,
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use bevy_msdf_text::{
    ASCII, MsdfAtlasSettings, MsdfFont, MsdfShadow, MsdfText, MsdfTextMode, MsdfTextPlugin,
    TextAlign,
};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "bevy_msdf_text: nametags".into(),
                        // Web: fill the page's canvas.
                        canvas: Some("#bevy".into()),
                        fit_canvas_to_parent: true,
                        prevent_default_event_handling: true,
                        ..default()
                    }),
                    ..default()
                })
                // No `.meta` files: don't make the browser ask for them.
                .set(AssetPlugin { meta_check: AssetMetaCheck::Never, ..default() }),
            MsdfTextPlugin,
        ))
        .insert_resource(ClearColor(Color::srgb(0.53, 0.70, 0.86)))
        .add_systems(Startup, setup)
        .add_systems(Update, (wander, fly_camera, grab_cursor))
        .run();
}

/// Walks a slow loop around `center`.
#[derive(Component)]
struct Wanderer {
    center: Vec3,
    radius: Vec2,
    speed: f32,
    phase: f32,
}

#[derive(Component, Default)]
struct FlyCamera {
    yaw: f32,
    pitch: f32,
}

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // The default charset is printable ASCII; the hint wants a middle dot too.
    let font: Handle<MsdfFont> = assets
        .load_builder()
        .with_settings(|s: &mut MsdfAtlasSettings| s.charset = format!("{ASCII}·"))
        .load("fonts/FiraSans-Bold.ttf");

    // Camera, looking at the yard from the south-east.
    let eye = Vec3::new(9.0, 5.0, 16.0);
    let look = Transform::from_translation(eye).looking_at(Vec3::new(0.0, 1.5, 0.0), Vec3::Y);
    let (yaw, pitch, _) = look.rotation.to_euler(EulerRot::YXZ);
    commands.spawn((Camera3d::default(), look, FlyCamera { yaw, pitch }));

    commands.spawn((
        DirectionalLight { illuminance: 9_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(8.0, 14.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight { brightness: 600.0, ..default() });

    // Ground and props for scale: a ring of posts and a few crates.
    let mat = |materials: &mut Assets<StandardMaterial>, c: Color| {
        materials.add(StandardMaterial { base_color: c, perceptual_roughness: 0.9, ..default() })
    };
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(120.0, 120.0))),
        MeshMaterial3d(mat(&mut materials, Color::srgb(0.36, 0.52, 0.32))),
    ));
    let post = meshes.add(Cuboid::new(0.4, 3.0, 0.4));
    let post_mat = mat(&mut materials, Color::srgb(0.85, 0.82, 0.74));
    for i in 0..16 {
        let a = i as f32 / 16.0 * TAU;
        commands.spawn((
            Mesh3d(post.clone()),
            MeshMaterial3d(post_mat.clone()),
            Transform::from_xyz(a.cos() * 22.0, 1.5, a.sin() * 22.0),
        ));
    }
    let crate_mesh = meshes.add(Cuboid::from_length(1.2));
    let crate_mat = mat(&mut materials, Color::srgb(0.62, 0.42, 0.24));
    for (x, z, s) in [(3.5, -2.0, 1.0), (4.6, -2.3, 0.7), (-6.0, 4.0, 1.4), (-1.0, 9.0, 0.9)] {
        commands.spawn((
            Mesh3d(crate_mesh.clone()),
            MeshMaterial3d(crate_mat.clone()),
            Transform::from_xyz(x, 0.6 * s, z).with_scale(Vec3::splat(s)),
        ));
    }

    // The two characters: a capsule body, a name tag above the head.
    let body = meshes.add(Capsule3d::new(0.4, 1.0));
    let ink = Color::srgb(0.06, 0.07, 0.10);
    let tag_shadow = MsdfShadow::drop(Color::srgba(0.0, 0.0, 0.0, 0.55), Vec2::new(0.04, -0.06), 0.05);
    let characters = [
        (
            "Fixed Size",
            Color::srgb(0.95, 0.45, 0.20),
            Wanderer { center: Vec3::new(-2.0, 0.0, 0.0), radius: Vec2::new(5.0, 3.5), speed: 0.25, phase: 0.0 },
            MsdfText::new("Fixed Size", font.clone())
                .with_mode(MsdfTextMode::WorldFixedPx)
                .with_size(30.0)
                .with_color(Color::WHITE)
                .with_outline(0.12, Color::srgb(0.75, 0.25, 0.05)),
        ),
        (
            "Scales With Distance",
            Color::srgb(0.20, 0.65, 0.85),
            Wanderer { center: Vec3::new(1.0, 0.0, -1.0), radius: Vec2::new(9.0, 6.0), speed: -0.15, phase: 2.0 },
            MsdfText::new("Scales With Distance", font.clone())
                .with_mode(MsdfTextMode::World)
                .with_size(0.5)
                .with_color(Color::srgb(1.0, 0.93, 0.55))
                .with_outline(0.12, ink),
        ),
    ];
    for (name, color, wanderer, tag) in characters {
        commands.spawn((
            Name::new(name),
            Mesh3d(body.clone()),
            MeshMaterial3d(mat(&mut materials, color)),
            Transform::from_xyz(0.0, 0.9, 0.0),
            wanderer,
            children![(
                tag.with_anchor(Vec2::new(0.0, -0.5)).with_shadow(tag_shadow),
                Transform::from_xyz(0.0, 1.25, 0.0),
            )],
        ));
    }

    // A giant sign at the far end: fly right up to it, it stays sharp.
    commands.spawn((
        MsdfText::new("Sharp at\nany scale", font.clone())
            .with_mode(MsdfTextMode::Plane)
            .with_size(3.0)
            .with_color(Color::srgb(1.0, 0.97, 0.90))
            .with_outline(0.08, ink)
            .with_shadow(MsdfShadow::drop(Color::srgba(0.0, 0.0, 0.0, 0.45), Vec2::new(0.05, -0.05), 0.04)),
        Transform::from_xyz(-6.0, 4.5, -30.0).with_rotation(Quat::from_rotation_y(0.3)),
    ));
    // ...and a tiny one on a crate.
    commands.spawn((
        MsdfText::new("FRAGILE", font.clone())
            .with_mode(MsdfTextMode::Plane)
            .with_size(0.22)
            .with_color(Color::srgb(0.85, 0.12, 0.10)),
        Transform::from_xyz(3.5, 0.6, -1.39),
    ));

    // HUD: a glowing title, and the controls.
    commands.spawn(
        MsdfText::new("MSDF TEXT", font.clone())
            .with_mode(MsdfTextMode::Screen { at: Vec2::new(0.5, 0.0), offset: Vec2::new(0.0, 24.0) })
            .with_size(72.0)
            .with_anchor(Vec2::new(0.0, 0.5))
            .with_color(Color::WHITE)
            .with_outline(0.06, ink)
            .with_shadow(MsdfShadow::glow(Color::srgba(1.0, 0.55, 0.15, 0.9), 0.1)),
    );
    commands.spawn(
        MsdfText::new(
            "click to look · WASD + mouse · Q/E down/up · shift = fast · esc = let go",
            font,
        )
        .with_mode(MsdfTextMode::Screen { at: Vec2::new(0.0, 1.0), offset: Vec2::new(16.0, -14.0) })
        .with_size(20.0)
        .with_anchor(Vec2::new(-0.5, -0.5))
        .with_align(TextAlign::Left)
        .with_color(Color::WHITE)
        .with_outline(0.15, ink),
    );
}

fn wander(time: Res<Time>, mut walkers: Query<(&Wanderer, &mut Transform)>) {
    for (w, mut transform) in &mut walkers {
        let a = w.phase + time.elapsed_secs() * w.speed;
        let pos = w.center + Vec3::new(a.cos() * w.radius.x, 0.9, a.sin() * w.radius.y);
        let ahead = w.center
            + Vec3::new((a + w.speed.signum() * 0.1).cos() * w.radius.x, 0.9, (a + w.speed.signum() * 0.1).sin() * w.radius.y);
        *transform = Transform::from_translation(pos).looking_at(ahead, Vec3::Y);
    }
}

/// Click locks the pointer, Esc releases it. Mouse look only while locked.
fn grab_cursor(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
) {
    if mouse.just_pressed(MouseButton::Left) {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
    if keys.just_pressed(KeyCode::Escape) {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
}

fn fly_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    camera: Single<(&mut Transform, &mut FlyCamera)>,
) {
    let (mut transform, mut cam) = camera.into_inner();
    if cursor.grab_mode == CursorGrabMode::Locked && motion.delta != Vec2::ZERO {
        cam.yaw -= motion.delta.x * 0.003;
        cam.pitch = (cam.pitch - motion.delta.y * 0.003).clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);
        transform.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
    }

    let key = |k| keys.pressed(k) as i32 as f32;
    let local = Vec3::new(
        key(KeyCode::KeyD) - key(KeyCode::KeyA),
        key(KeyCode::KeyE) - key(KeyCode::KeyQ),
        key(KeyCode::KeyS) - key(KeyCode::KeyW),
    );
    let speed = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) { 24.0 } else { 6.0 };
    let (right, forward) = (transform.right(), transform.forward());
    let dir = right * local.x + Vec3::Y * local.y - forward * local.z;
    transform.translation += dir.normalize_or_zero() * speed * time.delta_secs();
}
