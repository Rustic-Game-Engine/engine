//! Render the real serialized showcase without opening a native window.
use std::collections::BTreeMap;
use std::path::PathBuf;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "examples/scene-effects/project".into()),
    );
    let output = PathBuf::from(
        std::env::args()
            .nth(2)
            .unwrap_or_else(|| "/workspace/scratch/scene-effects.png".into()),
    );
    let loaded = engine_world::load_scene(&std::fs::read(root.join("scene/showcase.scene"))?)?;
    let mut world = loaded.document.create_world()?;
    let models: BTreeMap<_, _> = engine_assets::load_model_library(&root)?
        .into_iter()
        .map(|(id, (_, model))| (id, model))
        .collect();
    world.propagate_transforms();
    for _ in 0..90 {
        world.update_effects(1. / 60.);
    }
    let scene = renderer_wgpu::game_scene_with_models(&world, 16. / 9., &models);
    let mut renderer =
        renderer_wgpu::SceneViewportRenderer::new(renderer_wgpu::BackendRequest::Auto)?;
    renderer.set_environment_root(&root);
    renderer.set_effect_frame(world.effect_frame())?;
    let frame = renderer
        .render(1280, 720, &scene)?
        .ok_or("no render frame")?;
    image::save_buffer(
        &output,
        &frame.rgba8,
        frame.width,
        frame.height,
        image::ColorType::Rgba8,
    )?;
    println!(
        "Rendered {} meshes, {} particles with {:?}: {}",
        scene.meshes.len(),
        world.effect_frame().particles.len(),
        frame.adapter,
        output.display()
    );
    Ok(())
}
