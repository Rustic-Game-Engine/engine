fn fog_density(fog:Fog,p:vec3<f32>)->f32 {
 let local=(fog.inverse*vec4<f32>(p,1.)).xyz/max(fog.dimensions_density.xyz*0.5,vec3<f32>(0.001));
 var distance=max(max(abs(local.x),abs(local.y)),abs(local.z));
 if fog.options.x>0.5 && fog.options.x<1.5 {distance=length(local);}
 if fog.options.x>1.5 && fog.options.x<2.5 {distance=max(length(local.xz),abs(local.y));}
 if distance>1. {return 0.;}
 let edge=clamp((1.-distance)/max(fog.options.y,0.001),0.,1.);
 let drift=vec3<f32>(scene.resolution.z*fog.noise.x*0.13,scene.resolution.z*fog.noise.x*0.07,scene.resolution.z*fog.noise.x*0.19);
 let q=p*fog.options.z+drift;
 var noise=0.65+0.35*sin(dot(q,vec3<f32>(1.7,2.3,1.1)));
 if fog.noise.y>0.5 {let layer=i32(fog.noise.y);noise=(textureSampleLevel(fog_noise,noise_sampler,q.xy,layer,0.).r+textureSampleLevel(fog_noise,noise_sampler,q.yz,layer,0.).r+textureSampleLevel(fog_noise,noise_sampler,q.zx,layer,0.).r)/3.;}
 return max(fog.dimensions_density.w*edge*noise*exp(-max(local.y+1.,0.)*fog.color_height.w),0.);
}
fn fog_lighting(fog:Fog,p:vec3<f32>,view:vec3<f32>)->vec3<f32>{
 var light=scene.ambient.rgb*scene.ambient.w;
 let sun=normalize(scene.sun_direction.xyz+vec3<f32>(0.000001));
 let self_shadow=exp(-fog_density(fog,p+sun)*2.);
 light+=scene.sun.rgb*scene.sun.w*phase(dot(sun,-view),fog.scatter.x)*scene_visibility(p,sun,30.)*self_shadow;
 for(var i=0u;i<u32(scene.counts.w);i+=1u){let l=scene.lights[i];var direction=-l.direction_range.xyz;var attenuation=1.;var distance=30.;if l.position_kind.w>0.5 {let offset=l.position_kind.xyz-p;distance=length(offset);direction=offset/max(distance,0.001);let relative=distance/max(l.direction_range.w,0.001);let cutoff=max(1.-pow(relative,4.),0.);attenuation=cutoff*cutoff/max(distance*distance,0.01);if l.position_kind.w>1.5 {attenuation*=smoothstep(l.cone.x,l.cone.y,dot(-direction,l.direction_range.xyz));}}
  light+=l.color_intensity.rgb*l.color_intensity.w*attenuation*phase(dot(direction,-view),fog.scatter.x)*scene_visibility(p,direction,distance);
 }
 return mix(vec3<f32>(1.),light,fog.scatter.y);
}
struct AtmosphereOutput {@location(0) color:vec4<f32>,@location(1) guide:vec4<f32>};
@fragment fn fs_main(i:Output)->AtmosphereOutput {
 let base=image(i.uv);var color=base.rgb;let d=depth_at(i.uv);let surface_world=world_at(i.uv,d);let offset=surface_world-scene.camera.xyz;let ray_length=min(length(offset),2000.);let direction=normalize(offset+vec3<f32>(0.000001));
 var representative=surface_world;var represented=0.;var fog_scatter_coverage=0.;
 // Jitter the bounded integration with a deterministic per-frame blue-noise-like hash.
 let jitter=fract(sin(dot(i.position.xy+vec2<f32>(scene.resolution.w*1.618),vec2<f32>(12.9898,78.233)))*43758.5453);
 for(var v=0u;v<u32(scene.counts.x);v+=1u){let fog=scene.fog[v];let steps=u32(clamp(fog.options.w,4.,128.));let origin=(fog.inverse*vec4<f32>(scene.camera.xyz,1.)).xyz;let dir=(fog.inverse*vec4<f32>(direction,0.)).xyz;let safe=select(vec3<f32>(0.000001),dir,abs(dir)>vec3<f32>(0.000001));let a=(-fog.dimensions_density.xyz*0.5-origin)/safe;let b=(fog.dimensions_density.xyz*0.5-origin)/safe;let entry=min(a,b);let exit=max(a,b);let begin=max(max(max(entry.x,entry.y),entry.z),0.);let end=min(min(min(exit.x,exit.y),exit.z),min(ray_length,fog.scatter.z));let step=max(end-begin,0.)/f32(steps);
  var transmittance=1.;var scatter=vec3<f32>(0.);var mean=vec3<f32>(0.);var mean_weight=0.;
  for(var j=0u;j<steps;j+=1u){let point=scene.camera.xyz+direction*(begin+(f32(j)+jitter)*step);let density=fog_density(fog,point);let optical=exp(-density*step);let contribution=transmittance*(1.-optical);scatter+=fog.color_height.rgb*fog_lighting(fog,point,direction)*contribution;mean+=point*contribution;mean_weight+=contribution;transmittance*=optical;}
  fog_scatter_coverage=max(fog_scatter_coverage,(1.-transmittance)*clamp(fog.scatter.y,0.,1.));color=color*transmittance+scatter;if mean_weight>0.001 {let weight=mean_weight*(1.-represented);representative=mix(representative,mean/max(mean_weight,0.001),weight);represented+=weight;}
 }
 for(var v=0u;v<u32(scene.counts.y);v+=1u){let shaft=scene.shafts[v];let light=shaft.position_intensity.xyz;let light_screen=project(light);if light_screen.z<=0.||light_screen.z>=1. {continue;}let distance=length(light-scene.camera.xyz);if distance>shaft.options.y {continue;}let steps=u32(clamp(shaft.options.x,4.,128.));var glow=vec3<f32>(0.);var illumination=1.;for(var j=0u;j<steps;j+=1u){let p=mix(i.uv,light_screen.xy,(f32(j)+jitter)/f32(steps));let open=select(1.,smoothstep(0.995,1.,depth_at(p)),shaft.options.w>0.5);glow+=image(p).rgb*open*illumination;illumination*=exp(-shaft.color_density.w/f32(steps));}var volume=1.;if shaft.temporal.y>0.5{let origin=(shaft.inverse*vec4<f32>(scene.camera.xyz,1.)).xyz;let local_direction=(shaft.inverse*vec4<f32>(direction,0.)).xyz;let safe=select(vec3<f32>(0.000001),local_direction,abs(local_direction)>vec3<f32>(0.000001));let half_size=shaft.bounds.xyz*0.5;let a=(-half_size-origin)/safe;let b=(half_size-origin)/safe;let lower=min(a,b);let upper=max(a,b);let begin=max(max(max(lower.x,lower.y),lower.z),0.);let end=min(min(min(upper.x,upper.y),upper.z),ray_length);volume=clamp((end-begin)/max(length(shaft.bounds.xyz)*0.1,0.001),0.,1.);if end>begin{let local=(origin+local_direction*(begin+end)*0.5)/max(half_size,vec3<f32>(0.001));var extent=max(max(abs(local.x),abs(local.y)),abs(local.z));if shaft.bounds.w>0.5&&shaft.bounds.w<1.5{extent=length(local);}if shaft.bounds.w>1.5{extent=max(length(local.xz),abs(local.y));}volume*=clamp((1.-extent)*5.,0.,1.);}}
let angle=dot(normalize(light-scene.camera.xyz),direction);color+=volume*(1.-fog_scatter_coverage)*glow/f32(steps)*shaft.color_density.rgb*shaft.position_intensity.w*shaft.color_density.w*phase(angle,shaft.options.z);}
 for(var v=0u;v<u32(scene.counts.z);v+=1u){let flare=scene.flares[v];let visibility=textureLoad(flare_history,vec2<i32>(i32(v),0),0).r;if visibility<=0.001{continue;}let aspect=scene.resolution.x/scene.resolution.y;for(var j=0u;j<=u32(flare.options.x);j+=1u){let center=mix(flare.screen.xy,vec2<f32>(0.5),f32(j)*flare.options.y);let delta=(i.uv-center)*vec2<f32>(aspect,1.);let radius=max(flare.color_scale.w*(1.+f32(j)*0.25),0.001);var ghost=vec3<f32>(pow(max(1.-dot(delta,delta)/(radius*radius),0.),3.));if flare.extras.x>0.5{let uv=delta/(radius*2.)+vec2<f32>(0.5);if all(uv>=vec2<f32>(0.))&&all(uv<=vec2<f32>(1.)){let tex=textureSampleLevel(fog_noise,noise_sampler,uv,i32(flare.extras.x),0.);ghost=srgb_to_linear(tex.rgb)*tex.a;}else{ghost=vec3<f32>(0.);}}let sine=sin(flare.options.w);let cosine=cos(flare.options.w);let rotated=vec2<f32>(delta.x*cosine+delta.y*sine,-delta.x*sine+delta.y*cosine);let streak=exp(-abs(rotated.y)*800.)*exp(-abs(rotated.x)/max(flare.options.z,0.0001));color+=(ghost+vec3<f32>(streak))*flare.color_scale.rgb*flare.screen.w*visibility/(1.+f32(j));}}
 let motion=textureLoad(motion_vectors,pixel(i.uv,textureDimensions(motion_vectors)),0);var previous_uv=i.uv-motion.xy;var previous_distance=-1.;
 if represented>0.01||motion.w<0.5 {let p=scene.previous*vec4<f32>(representative,1.);previous_uv=p.xy/max(abs(p.w),0.000001)*vec2<f32>(0.5,-0.5)+vec2<f32>(0.5);if p.w>0. {previous_distance=distance(representative,scene.previous_camera.xyz);}}
 else {let p=scene.previous_inverse*vec4<f32>(previous_uv*vec2<f32>(2.,-2.)+vec2<f32>(-1.,1.),motion.z,1.);previous_distance=distance(p.xyz/p.w,scene.previous_camera.xyz);}
 var o:AtmosphereOutput;o.color=vec4<f32>(max(color,vec3<f32>(0.)),base.a);o.guide=vec4<f32>(i.uv-previous_uv,previous_distance,distance(representative,scene.camera.xyz));return o;
}
