struct Settings {
 grade:vec4<f32>,lens:vec4<f32>,warp:vec4<f32>,bloom:vec4<f32>,tint:vec4<f32>,overlay:vec4<f32>,multiply:vec4<f32>,add_dof:vec4<f32>,motion:vec4<f32>,focus:vec4<f32>,temporal:vec4<f32>
};
struct Fog {inverse:mat4x4<f32>,dimensions_density:vec4<f32>,color_height:vec4<f32>,options:vec4<f32>,scatter:vec4<f32>,noise:vec4<f32>};
struct Shaft {inverse:mat4x4<f32>,position_intensity:vec4<f32>,color_density:vec4<f32>,options:vec4<f32>,temporal:vec4<f32>,bounds:vec4<f32>};
struct Flare {screen:vec4<f32>,color_scale:vec4<f32>,options:vec4<f32>,extras:vec4<f32>};
struct Light {position_kind:vec4<f32>,direction_range:vec4<f32>,color_intensity:vec4<f32>,cone:vec4<f32>};
struct Distortion {inverse:mat4x4<f32>,options:vec4<f32>,noise:vec4<f32>,offset:vec4<f32>};
struct Overlay {color:vec4<f32>,options:vec4<f32>};
struct Scene {
 inverse:mat4x4<f32>,previous:mat4x4<f32>,previous_inverse:mat4x4<f32>,view_projection:mat4x4<f32>,
 camera:vec4<f32>,previous_camera:vec4<f32>,sun_direction:vec4<f32>,sun:vec4<f32>,ambient:vec4<f32>,counts:vec4<f32>,resolution:vec4<f32>,effect_counts:vec4<f32>,
 fog:array<Fog,8>,shafts:array<Shaft,4>,flares:array<Flare,4>,lights:array<Light,32>,distortions:array<Distortion,4>,overlays:array<Overlay,8>
};
@group(0) @binding(0) var source:texture_2d<f32>;
@group(0) @binding(1) var image_sampler:sampler;
@group(0) @binding(2) var<uniform> settings:Settings;
@group(0) @binding(3) var depth:texture_2d<f32>;
@group(0) @binding(4) var<uniform> scene:Scene;
@group(0) @binding(5) var motion_vectors:texture_2d<f32>;
@group(0) @binding(6) var history:texture_2d<f32>;
@group(0) @binding(7) var history_guide:texture_2d<f32>;
@group(0) @binding(8) var fog_noise:texture_2d_array<f32>;
@group(0) @binding(9) var noise_sampler:sampler;
@group(0) @binding(10) var flare_history:texture_2d<f32>;
struct Output {@builtin(position) position:vec4<f32>,@location(0) uv:vec2<f32>};
@vertex fn vs_main(@builtin(vertex_index) index:u32)->Output {
 let p=array<vec2<f32>,3>(vec2<f32>(-1.,-1.),vec2<f32>(3.,-1.),vec2<f32>(-1.,3.));var o:Output;o.position=vec4<f32>(p[index],0.,1.);o.uv=p[index]*vec2<f32>(0.5,-0.5)+vec2<f32>(0.5);return o;
}
fn pixel(uv:vec2<f32>,size:vec2<u32>)->vec2<i32>{return clamp(vec2<i32>(uv*vec2<f32>(size)),vec2<i32>(0),vec2<i32>(size)-vec2<i32>(1));}
fn depth_at(uv:vec2<f32>)->f32{let c=textureLoad(depth,pixel(uv,textureDimensions(depth)),0).rgb;return dot(c,vec3<f32>(255.,65280.,16711680.))/16777215.;}
fn image(uv:vec2<f32>)->vec4<f32>{return textureSampleLevel(source,image_sampler,clamp(uv,vec2<f32>(0.),vec2<f32>(1.)),0.);}
fn srgb_to_linear(c:vec3<f32>)->vec3<f32>{return select(pow((c+vec3<f32>(0.055))/1.055,vec3<f32>(2.4)),c/12.92,c<=vec3<f32>(0.04045));}
fn linear_to_srgb(c:vec3<f32>)->vec3<f32>{return select(1.055*pow(max(c,vec3<f32>(0.)),vec3<f32>(1./2.4))-vec3<f32>(0.055),c*12.92,c<=vec3<f32>(0.0031308));}
fn world_at(uv:vec2<f32>,d:f32)->vec3<f32>{let p=scene.inverse*vec4<f32>(uv*vec2<f32>(2.,-2.)+vec2<f32>(-1.,1.),min(d,0.99999),1.);return p.xyz/p.w;}
fn project(p:vec3<f32>)->vec3<f32>{let q=scene.view_projection*vec4<f32>(p,1.);return vec3<f32>(q.xy/max(q.w,0.000001)*vec2<f32>(0.5,-0.5)+vec2<f32>(0.5),q.z/max(q.w,0.000001));}
fn phase(cosine:f32,g:f32)->f32{let gg=clamp(g,-0.95,0.95);return (1.-gg*gg)/max(pow(1.+gg*gg-2.*gg*clamp(cosine,-1.,1.),1.5),0.001);}
fn scene_visibility(p:vec3<f32>,direction:vec3<f32>,distance:f32)->f32 {
 var visibility=1.;
 for(var i=1u;i<=4u;i+=1u){let sample=p+direction*(f32(i)/4.*min(distance,30.));let screen=project(sample);if all(screen.xy>vec2<f32>(0.))&&all(screen.xy<vec2<f32>(1.))&&screen.z>0.&&screen.z<1. {let receiver=depth_at(screen.xy);if receiver+max(0.00001,(1.-screen.z)*0.01)<screen.z {visibility*=0.25;}}}
 return visibility;
}
