// The exact convex halfspaces exported from PianoKeyboard.blend.
bool gemPianoActive(){return gemRow(4041216).x>0.5;}
GemHit gemPianoHit(vec3 origin,vec3 ray,float farLimit){
 GemHit hit=GemHit(farLimit,vec3(0.0),-1);int count=int(gemRow(4041220).x);int planes=int(gemRow(4041220).y);
 for(int key=0;key<132;key++){
  if(key>=count){break;}int at=4041232+key*3;vec3 pose=gemRow(at).xyz;vec3 size=gemRow(at+1).xyz;
  vec3 local=(origin-pose)/size;vec3 direction=ray/size;
  if(!gemBox(local,gemInverseRay(direction),vec3(0.0),vec3(0.5),hit.distance)){continue;}
  float near=-1e20;float far=1e20;vec3 normal=vec3(0.0);bool miss=false;
  for(int p=0;p<64;p++){if(p>=planes){break;}vec4 plane=gemRow(4041632+p);float den=dot(plane.xyz,direction);float d=plane.w-dot(plane.xyz,local);
   if(abs(den)<0.0000001){if(d<0.0){miss=true;}}
   else{float t=d/den;if(den<0.0&&t>near){near=t;normal=plane.xyz;}if(den>0.0&&t<far){far=t;}}
   if(far<near){miss=true;}if(miss){break;}
  }
  if(!miss&&near>0.0000001&&near<hit.distance){hit=GemHit(near,gemSafeUnit(normal/size),950000+key);}
 }
 return hit;
}

bool gemPianoField(vec2 screen){
 vec4 room=gemRow(4041216);vec4 field=gemRow(4041217);
 vec2 p=vec2(screen.x*room.y*0.5+room.y*0.5,room.z*0.5-screen.y*room.z*0.5);
 return p.x>=field.x&&p.x<=field.x+field.z&&p.y>=field.y&&p.y<=field.y+field.w;
}
