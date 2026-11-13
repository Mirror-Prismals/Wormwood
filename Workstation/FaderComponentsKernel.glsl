// Original fader component and meter meshes, independent dynamic family.
// DecorativeControls immutable prefix/material addresses remain unchanged.
int gemMixerRecord(int instance){return 43152+8+(instance/3)*6;}
vec3 gemMixerScale(int instance){
 int at=gemMixerRecord(instance);int part=instance%3;vec4 r=gemRow(at+part);
 float width=0.04;float height=0.18;if(part==1){width=0.025;height=0.025;}if(part==2){width=0.01;height=0.156;}
 return vec3(r.z/width,r.w/height,gemRow(at+3).y);
}
vec3 gemMixerTranslation(int instance){
 int at=gemMixerRecord(instance);int part=instance%3;vec4 r=gemRow(at+part);vec3 scale=gemMixerScale(instance);
 float low=gemRow(43152).z;if(part==2){low=gemRow(43154).w;}
 vec3 p=vec3(r.xy,gemRow(at+3).x-low*scale.z);if(part==1){p.y-=gemRow(43154).z*scale.y;}return p;
}
vec2 gemMixerTree(int instance){
 int part=instance%3;vec4 fader=gemRow(43153);if(part==0){return fader.xy;}if(part==1){return fader.zw;}return gemRow(43154).xy;
}
bool gemMixerActive(int instance){
 int at=gemMixerRecord(instance);if(float(instance/3)>=gemRow(43152).x){return false;}if(gemRow(at+5).x>1.5&&instance%3!=2){return false;}
 vec3 s=gemMixerScale(instance);return min(s.x,min(s.y,s.z))>0.000000001;
}
GemHit gemMixerTraceInstance(vec3 origin,vec3 ray,int instance,int medium,float farLimit,bool anyHit){
 GemHit hit=GemHit(farLimit,vec3(0.0),-1);vec3 scale=gemMixerScale(instance);vec3 local=(origin-gemMixerTranslation(instance))/scale;vec3 direction=ray/scale;vec3 inverseRay=gemInverseRay(direction);
 vec2 tree=gemMixerTree(instance);bool inside=medium>=800000;int material=(medium-800000)%256;bool homogeneous=false;
 if(inside){vec4 interior=gemRow(12544+material*40+2);if(interior.z>0.0){
  float far=farLimit;vec3 normal=vec3(0.0);bool found=false;
  for(int p=0;p<26;p++){if(float(p)>=interior.z){break;}vec4 plane=gemRow(12544+material*40+6+p);float denominator=dot(plane.xyz,direction);if(denominator>0.0000001){float t=(plane.w-dot(plane.xyz,local))/denominator;if(t>0.0000001&&t<far){far=t;normal=gemSafeUnit(plane.xyz/scale);found=true;}}}
  if(found){return GemHit(far,normal,medium);}return hit;
 }if(interior.x>0.0){tree=interior.xy;homogeneous=true;}}
 int node=int(tree.x);
 for(int visit=0;visit<4096;visit++){
  if(node>=int(tree.y)){break;}vec4 lo=gemRow(node);vec4 hi=gemRow(node+1);vec4 metadata=gemRow(node+2);
  if(!gemBox(local,inverseRay,(lo.xyz+hi.xyz)*0.5,(hi.xyz-lo.xyz)*0.5+vec3(0.0000001),hit.distance)){node=int(lo.w);continue;}
  if(inside&&!homogeneous&&(float(material)<metadata.y||float(material)>metadata.z)){node=int(lo.w);continue;}
  int count=int(metadata.x);if(count==0){node+=3;continue;}
  for(int triangle=0;triangle<4;triangle++){
   if(triangle>=count){break;}int address=int(hi.w)+triangle*6;vec4 a=gemRow(address);if(inside&&!homogeneous&&int(a.w)!=material){continue;}
   vec4 edge=gemRow(address+1);vec3 e1=edge.xyz;vec3 e2=gemRow(address+2).xyz;if(edge.w<0.5){e1-=a.xyz;e2-=a.xyz;}
   vec3 h=cross(direction,e2);float det=dot(e1,h);if(abs(det)<0.000000000001){continue;}vec3 offset=local-a.xyz;float u=dot(offset,h)/det;if(u<0.0||u>1.0){continue;}
   vec3 q=cross(offset,e1);float v=dot(direction,q)/det;if(v<0.0||u+v>1.0){continue;}float t=dot(e2,q)/det;
   if(t<=0.0000001||t>=hit.distance){continue;}vec3 world=origin+ray*t;vec4 clip=gemRow(gemMixerRecord(instance)+4);if(world.x<clip.x||world.x>clip.z||world.y<clip.y||world.y>clip.w){continue;}
   vec3 normal=gemSafeUnit(gemControlNormal(address,u,v)/scale);float side=dot(ray,normal);
   if((inside&&side>0.0)||(!inside&&side<0.0)){hit=GemHit(t,normal,800000+instance*256+int(a.w));if(anyHit){return hit;}}
  }node=int(lo.w);
 }return hit;
}
GemHit gemMixerHit(vec3 origin,vec3 ray,int medium,float farLimit,bool anyHit){
 GemHit hit=GemHit(farLimit,vec3(0.0),-1);if(gemRow(43152).w<0.5){return hit;}
 if(medium>=800000&&medium<853760){int instance=(medium-800000)/256;if(!gemMixerActive(instance)){return hit;}return gemMixerTraceInstance(origin,ray,instance,medium,farLimit,anyHit);}
 vec3 lo=gemRow(43155).xyz;vec3 hi=gemRow(43156).xyz;if(!gemBox(origin,gemInverseRay(ray),(lo+hi)*0.5,(hi-lo)*0.5+vec3(0.000001),farLimit)){return hit;}
 for(int instance=0;instance<210;instance++){
  if(float(instance/3)>=gemRow(43152).x){break;}if(!gemMixerActive(instance)){continue;}
  GemHit next=gemMixerTraceInstance(origin,ray,instance,-1,hit.distance,anyHit);if(next.panel>=0){hit=next;if(anyHit){return hit;}}
 }return hit;
}
vec4 gemMixerMeterColor(vec3 point,int medium){
 if(medium<800000||medium>=853760){return vec4(0.0);}int instance=(medium-800000)/256;if(instance%3!=2){return vec4(0.0);}int material=(medium-800000)%256;vec4 roles=gemRow(43157);int channel=-1;
 if(float(material)>=roles.x&&float(material)<=roles.y){channel=0;}if(float(material)>=roles.z&&float(material)<=roles.w){channel=1;}if(channel<0){return vec4(0.0);}
 vec3 local=(point-gemMixerTranslation(instance))/gemMixerScale(instance);float y=clamp((local.y+0.078)/0.156,0.0,1.0);float t=(min(floor(y*60.0),59.0)+0.5)/60.0*5.0;int band=min(int(floor(t)),4);float blend=t-float(band);
 vec3 a=vec3(0.0,0.0,1.0);vec3 b=vec3(0.0,1.0,1.0);
 if(band==1){a=vec3(0.0,1.0,1.0);b=vec3(0.0,1.0,0.0);}if(band==2){a=vec3(0.0,1.0,0.0);b=vec3(1.0,1.0,0.0);}if(band==3){a=vec3(1.0,1.0,0.0);b=vec3(1.0,0.6509803921568628,0.0);}if(band==4){a=vec3(1.0,0.6509803921568628,0.0);b=vec3(1.0,0.0,0.0);}
 // Cardinal's master and monitor families keep their distinct six-stop ramps.
 int family=int(gemRow(gemMixerRecord(instance)+5).z);
 if(family==1){a=vec3(1.0)-a;b=vec3(1.0)-b;}
 if(family==2){
  a=vec3(0.0,128.0/255.0,128.0/255.0);b=vec3(0.0,0.0,128.0/255.0);
  if(band==1){a=vec3(0.0,0.0,128.0/255.0);b=vec3(0.0);}
  if(band==2){a=vec3(0.0);b=vec3(1.0,0.0,0.0);}
  if(band==3){a=vec3(1.0,0.0,0.0);b=vec3(1.0,44.0/255.0,0.0);}
  if(band==4){a=vec3(1.0,44.0/255.0,0.0);b=vec3(0.0,128.0/255.0,128.0/255.0);}
 }
 vec4 levels=gemRow(gemMixerRecord(instance)+3);float fill=levels.z;if(channel==1){fill=levels.w;}fill=clamp(fill,0.0,1.0);float strength=0.25;if(y<fill){strength=1.0;}return vec4(mix(a,b,blend)*strength,1.0);
}
