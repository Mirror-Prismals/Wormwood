// Face labels use the existing authored font atlas. No image of a keyboard.
vec3 gemPianoOverlay(vec3 initialColor,vec2 screen){
 vec3 color=initialColor;
 vec4 room=gemRow(4041216);vec2 point=vec2(screen.x*room.y*0.5+room.y*0.5,room.z*0.5-screen.y*room.z*0.5);
 // Unlit display data over the piano background, behind the baked note sprites.
 vec4 spectral=gemRow(4041223);vec4 field=gemRow(4041217);
 if(spectral.x>0.5&&point.x>=field.x&&point.x<field.x+field.z&&point.y>=field.y&&point.y<field.y+field.w){
  int sx=int(clamp((point.x-field.x)/field.z*128.0,0.0,127.0));
  int sy=int(clamp((point.y-field.y)/field.w*128.0,0.0,127.0));
  int index=sy*128+sx;vec4 packed=gemRow(int(spectral.w)+index/4);
  float energy=packed[index%4];
  vec3 ink=mix(vec3(0.04,0.13,0.21),vec3(0.12,0.65,0.72),energy);
  color=mix(color,ink,energy*0.72);
 }

 vec2 world=vec2((point.x-room.y*0.5)*1.3/room.z,(room.z*0.5-point.y)*1.3/room.z);
 for(int text=0;text<80;text++){
  if(text>=int(gemRow(4041219).w)){break;}if(text<int(gemRow(4041224).x)*2){continue;}int at=4041728+text*18;vec4 label=gemRow(at);vec2 em=(world-label.xy)/label.z;
  if(em.y< -0.3||em.y>1.1||em.x< -0.2||em.x>label.w*0.55+1.0){continue;}
  float density=0.0;float column=floor(em.x/0.55);
  for(int neighbor=0;neighbor<3;neighbor++){
   int i=int(column)-1+neighbor;if(i<0||i>=int(label.w)){continue;}
   vec4 codes=gemRow(at+2+i/4);int glyph=int(codes[i%4])-32;
   density+=gemEngravedDensity(em-vec2(float(i)*0.55,0.0),glyph,vec2(0.0),1.3/(room.z*label.z),0);
  }
  color=mix(color,gemRow(at+1).xyz,clamp(density*24.0,0.0,1.0));
 }
 return color;
}
