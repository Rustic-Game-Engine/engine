//! WebGL 2 uses ESSL 300, which is more restricted than native GLES 3.1.
use naga::{
    back::glsl,
    valid::{Capabilities, ValidationFlags, Validator},
};

fn translate_webgl2(source: &str) -> Result<(), String> {
    let module =
        naga::front::wgsl::parse_str(source).map_err(|error| error.emit_to_string(source))?;
    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .map_err(|error| error.to_string())?;
    let mut options = glsl::Options {
        version: glsl::Version::Embedded {
            version: 300,
            is_webgl: true,
        },
        ..glsl::Options::default()
    };
    for (index, binding) in module
        .global_variables
        .iter()
        .filter_map(|(_, global)| global.binding.as_ref())
        .enumerate()
    {
        options.binding_map.insert(
            *binding,
            u8::try_from(index).map_err(|_| "too many WebGL bindings")?,
        );
    }
    for entry in &module.entry_points {
        if entry.stage == naga::ShaderStage::Compute {
            continue;
        }
        let pipeline = glsl::PipelineOptions {
            shader_stage: entry.stage,
            entry_point: entry.name.clone(),
            multiview: None,
        };
        let mut output = String::new();
        let mut writer = glsl::Writer::new(
            &mut output,
            &module,
            &info,
            &options,
            &pipeline,
            naga::proc::BoundsCheckPolicies::default(),
        )
        .map_err(|error| format!("{}: {error}", entry.name))?;
        writer
            .write()
            .map_err(|error| format!("{}: {error}", entry.name))?;
        if !output.contains("#version 300 es") {
            return Err("shader did not target WebGL 2".into());
        }
    }
    Ok(())
}

#[test]
fn main_material_and_mesh_shaders_translate_to_webgl2() {
    for (name, source) in [
        ("viewport", super::VIEWPORT_SHADER),
        ("mesh", super::MESH_SHADER),
    ] {
        translate_webgl2(source).unwrap_or_else(|error| panic!("WebGL 2 {name}: {error}"));
    }
}

#[test]
fn hdr_atmosphere_temporal_and_display_shaders_translate_to_webgl2() {
    let common = include_str!("post_common.wgsl");
    for (name, body) in [
        ("atmosphere", include_str!("atmosphere.wgsl")),
        ("temporal", include_str!("temporal.wgsl")),
        ("display", include_str!("final_post.wgsl")),
        ("flare visibility", include_str!("flare_visibility.wgsl")),
    ] {
        translate_webgl2(&format!("{common}\n{body}"))
            .unwrap_or_else(|error| panic!("WebGL 2 {name}: {error}"));
    }
}

#[test]
fn webgl_guard_detects_the_original_mip_query_failure() {
    let source = r"
@group(0) @binding(0) var image: texture_2d<f32>;
@fragment fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(f32(textureNumLevels(image)));
}";
    assert!(
        translate_webgl2(source).is_err(),
        "ESSL 300 cannot implement textureNumLevels"
    );
}
