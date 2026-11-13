@fragment fn fs_main(i:Output)->@location(0) vec4<f32>{
 let current=image(i.uv);let guide=textureLoad(motion_vectors,pixel(i.uv,textureDimensions(motion_vectors)),0);let previous_uv=i.uv-guide.xy;
 if settings.temporal.x<=0.||settings.temporal.y<0.5||guide.z<0.||any(previous_uv<vec2<f32>(0.))||any(previous_uv>vec2<f32>(1.))||length(guide.xy)>0.35 {return current;}
 let previous_distance=textureLoad(history_guide,pixel(previous_uv,textureDimensions(history_guide)),0).w;
 if abs(previous_distance-guide.z)>max(0.03,guide.z*0.025){return current;}
 let texel=vec2<f32>(1.)/vec2<f32>(textureDimensions(source));var low=current.rgb;var high=current.rgb;
 for(var y=-1;y<=1;y+=1){for(var x=-1;x<=1;x+=1){let sample=image(i.uv+vec2<f32>(f32(x),f32(y))*texel).rgb;low=min(low,sample);high=max(high,sample);}}
 let old=clamp(textureSampleLevel(history,image_sampler,previous_uv,0.).rgb,low,high);
 return vec4<f32>(mix(current.rgb,old,clamp(settings.temporal.x,0.,0.98)),current.a);
}
