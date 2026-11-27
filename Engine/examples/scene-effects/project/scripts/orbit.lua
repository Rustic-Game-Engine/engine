local origin, elapsed
return {
 Start = function() local x,y,z=rustic.get_translation();origin={x,y,z};elapsed=0 end,
 FixedUpdate = function(dt)
  elapsed=elapsed+dt
  rustic.set_translation(origin[1]+math.cos(elapsed*1.8)*1.1,origin[2]+1+math.sin(elapsed*2.0)*0.2,origin[3]+math.sin(elapsed*1.8)*1.1)
 end,
}
