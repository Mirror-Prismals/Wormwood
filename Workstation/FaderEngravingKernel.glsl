// Preserve the existing fader well engraving inside its stationary component.
vec3 gemMixerEngravingLight(vec3 point,int medium,vec3 scale){
 int base=gemPanelRow(medium);vec3 sigma=gemRow(base+1).xyz;vec3 illumination=gemRow(5).xyz;
 for(int light=0;light<3;light++){int at=16+5*light;vec3 toLight=gemRow(at).xyz-point;float distance=length(toLight);vec3 direction=toLight/max(distance,0.000001);float cosine=max(dot(gemRow(at+3).xyz,-direction),0.0);
  GemHit exitHit=gemMixerHit(point,direction,medium,distance,false);if(cosine>0.0&&exitHit.panel==medium&&exitHit.distance>0.0){float area=4.0*gemRow(at+1).w*gemRow(at+2).w;float transmission=1.0-gemFresnel(dot(direction,exitHit.normal),1.0,gemRow(base).y);illumination+=gemRow(at+4).xyz*exp(-sigma*length(direction/scale)*exitHit.distance)*(transmission*cosine*area/(3.14159265359*distance*distance));}}
 return illumination*gemRow(214).xyz*gemRow(223).w;
}
vec4 gemMixerEngraving(vec3 origin,vec3 ray,int medium,float farLimit){
 if(medium<800000||medium>=853760||gemRow(244).y<0.5){return vec4(0.0,0.0,0.0,1.0);}int instance=(medium-800000)/256;if(instance%3!=0){return vec4(0.0,0.0,0.0,1.0);}
 int base=gemPanelRow(medium);int descriptor=int(gemRow(base+2).w);if(descriptor==0||gemRow(base).w<0.0001){return vec4(0.0,0.0,0.0,1.0);}
 vec3 scale=gemMixerScale(instance);vec3 translation=gemMixerTranslation(instance);vec3 local=(origin-translation)/scale;vec3 direction=ray/scale;if(abs(direction.z)<0.00001){return vec4(0.0,0.0,0.0,1.0);}vec4 region=gemRow(descriptor);vec4 depth=gemRow(descriptor+1);float pixel=2.0*gemRow(3).w/(gemRow(8).y*min(scale.x,scale.y));
 vec3 center=vec3(region.xy+region.zw*0.5,(depth.x+depth.z)*0.5);vec3 extent=vec3(region.zw*0.5+vec2(pixel),(depth.x-depth.z)*0.5+0.0000001);if(!gemBox(local,gemInverseRay(direction),center,extent,farLimit)){return vec4(0.0,0.0,0.0,1.0);}
 vec3 sigma=gemRow(base+1).xyz;vec3 result=vec3(0.0);float transmission=1.0;vec3 localUnit=gemSafeUnit(direction);
 for(int visit=0;visit<3;visit++){int layer=visit;if(direction.z>0.0){layer=2-visit;}float z=depth.x;float weight=1.0;vec2 shift=vec2(0.0);if(layer==1){z=depth.y;weight=gemRow(215).z;shift=gemRow(216).xy;}if(layer==2){z=depth.z;weight=gemRow(215).w;shift=gemRow(216).zw;}
  float t=(z-local.z)/direction.z;if(t<=0.0000001||t>=farLimit){continue;}vec3 point=local+direction*t;float density=gemControlDensity(point.xy,descriptor,shift,pixel*0.75,layer);
  if(density>0.0001){float alpha=1.0-exp(-gemRow(214).w*weight*min(density,1.0)/max(abs(localUnit.z),0.12));vec3 world=translation+point*scale;result+=transmission*alpha*exp(-sigma*t*length(direction))*gemMixerEngravingLight(world,medium,scale);transmission*=1.0-alpha;}}
 return vec4(result,transmission);
}
