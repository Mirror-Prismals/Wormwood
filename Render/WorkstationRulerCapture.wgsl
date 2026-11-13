 if(group==5&&f32(id.y)>=retainedMeta.rows[61].y){
  retainedRulerFace=-1;
  for(var face:i32=0;face<3;face++){
   let tile=retainedMeta.rows[61+face];let xy=vec2<f32>(id.xy)-vec2<f32>(0.0,tile.y);
   if(any(xy<vec2<f32>(0.0))||any(xy>=tile.zw)){continue;}
   let bounds=retainedMeta.rows[58+face];
   let jitter=fract(vec2<f32>(0.375,0.125)+vec2<f32>(0.5,0.75)*f32(sample));
   let local=(xy+vec2<f32>(jitter.x,1.0-jitter.y))/tile.zw*2.0-1.0;
   let rayOrigin=vec3<f32>(bounds.xy+local*bounds.zw*vec2<f32>(1.0,-1.0),origin.z);
   retainedRulerFace=face;retainedOrigin=rayOrigin;
   let bar=gemNearest(rayOrigin,direction,-1,1e20);
   if(bar.panel>=0){value=vec4<f32>(gemTrace(rayOrigin,direction,random),bar.distance);}
   break;
  }
 }else
