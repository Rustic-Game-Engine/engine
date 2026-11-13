// Temporal occlusion is a tiny retained4x1 image, independent of HDR TAA history.
@fragment fn fs_main(i:Output)->@location(0) vec4<f32>{
 let index=min(u32(i.position.x),3u);
 if index>=u32(scene.counts.z){return vec4<f32>(0.);}
 let flare=scene.flares[index];
 let in_view=all(flare.screen.xy>=vec2<f32>(0.))&&all(flare.screen.xy<=vec2<f32>(1.))&&flare.screen.z>0.&&flare.screen.z<1.;
 var wanted=select(0.,1.,in_view);
 if in_view&&flare.extras.z>0.5&&depth_at(flare.screen.xy)+0.001<flare.screen.z{wanted=0.;}
 var previous=textureLoad(flare_history,vec2<i32>(i32(index),0),0).r;
 if settings.temporal.y<0.5{previous=0.;}
 let blend=select(1.,1.-exp(-flare.extras.y*scene.effect_counts.z),flare.extras.y>0.);
 return vec4<f32>(mix(previous,wanted,clamp(blend,0.,1.)),0.,0.,1.);
}
