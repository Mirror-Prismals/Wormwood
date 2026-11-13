// SSLE in authored control-local metres. Three embedded density layers share
// the panel lettering's fracture style, lighting and transmission model.
// Header244 and material+2.w descriptors match ControlEngraving.rs.
float gemControlDensityTexel(int descriptor,int level,int x,int y,int layer) {
    vec4 packet=gemRow(descriptor+2);
    vec4 mip=gemRow(int(packet.z)+level);
    if(x<0||y<0||x>=int(mip.x)||y>=int(mip.y)){return 0.0;}
    int texel=int(mip.z)+y*int(mip.x)+x;
    vec4 packed=gemRow(int(gemRow(244).x)+int(packet.x)+texel/2);
    float rg=packed.x;float b=packed.y;
    if(texel%2==1){rg=packed.z;b=packed.w;}
    if(layer==2){return b/255.0;}
    if(layer==1){return floor(rg/256.0)/255.0;}
    return float(int(rg)%256)/255.0;
}
float gemControlDensityLevel(vec2 uv,int descriptor,int level,int layer) {
    vec4 packet=gemRow(descriptor+2);vec2 size=gemRow(int(packet.z)+level).xy;
    vec2 at=uv*size-vec2(0.5);vec2 cell=floor(at);vec2 f=fract(at);
    int x=int(cell.x);int y=int(cell.y);
    float a=gemControlDensityTexel(descriptor,level,x,y,layer);
    float b=gemControlDensityTexel(descriptor,level,x+1,y,layer);
    float c=gemControlDensityTexel(descriptor,level,x,y+1,layer);
    float d=gemControlDensityTexel(descriptor,level,x+1,y+1,layer);
    return mix(mix(a,b,f.x),mix(c,d,f.x),f.y);
}
float gemControlDensity(vec2 point,int descriptor,vec2 shift,float footprint,int layer) {
    vec4 region=gemRow(descriptor);vec4 depth=gemRow(descriptor+1);
    vec4 packet=gemRow(descriptor+2);vec2 size=gemRow(int(packet.z)).xy;
    vec2 uv=(point-region.xy-shift*(depth.w*gemRow(213).x))/region.zw;
    float resolution=max(size.x/region.z,size.y/region.w);
    float lod=clamp(log2(max(footprint*resolution,1.0)),0.0,packet.y-1.0);
    int lo=int(floor(lod));int hi=min(lo+1,int(packet.y)-1);
    return mix(gemControlDensityLevel(uv,descriptor,lo,layer),
               gemControlDensityLevel(uv,descriptor,hi,layer),fract(lod));
}
vec3 gemControlEngravingLight(vec3 point,int medium,float scale) {
    int base=gemPanelRow(medium);vec3 sigma=gemRow(base+1).xyz;
    vec3 illumination=gemRow(5).xyz;
    for(int light=0;light<3;light++){
        int at=16+5*light;vec3 toLight=gemRow(at).xyz-point;
        float distance=length(toLight);vec3 l=toLight/max(distance,0.000001);
        float cosine=max(dot(gemRow(at+3).xyz,-l),0.0);
        // The fader is concave: its material-only BVH preserves the well walls
        // and floor. A panel's convex exit routine cannot be used here.
        GemHit exitHit=gemControlExit(point,l,medium,distance);
        if(cosine>0.0&&exitHit.panel==medium&&exitHit.distance>0.0){
            float area=4.0*gemRow(at+1).w*gemRow(at+2).w;
            float transmission=1.0-gemFresnel(dot(l,exitHit.normal),1.0,gemRow(base).y);
            illumination+=gemRow(at+4).xyz*exp(-sigma*(exitHit.distance/scale))
                *(transmission*cosine*area/(3.14159265359*distance*distance));
        }
    }
    return illumination*gemRow(214).xyz*gemRow(223).w;
}
// Existing Cardinal LOOP caption, embedded in the same three SSLE layers.
vec3 gemBarEngravingLight(vec3 point,int medium,float scale) {
    int base=gemPanelRow(medium);vec3 illumination=gemRow(5).xyz;
    for(int light=0;light<3;light++){
        int at=16+5*light;vec3 toLight=gemRow(at).xyz-point;
        float distance=length(toLight);vec3 l=toLight/max(distance,0.000001);
        float cosine=max(dot(gemRow(at+3).xyz,-l),0.0);
        GemHit exitHit=gemBarHit(point,l,medium,distance);
        if(cosine>0.0&&exitHit.panel==medium){
            float area=4.0*gemRow(at+1).w*gemRow(at+2).w;
            float transmit=1.0-gemFresnel(dot(l,exitHit.normal),1.0,gemRow(base).y);
            illumination+=gemRow(at+4).xyz*exp(-gemRow(base+1).xyz*(exitHit.distance/scale))
                *(transmit*cosine*area/(3.14159265359*distance*distance));
        }
    }
    return illumination*gemRow(214).xyz*gemRow(223).w;
}
vec4 gemBarLoopEngraving(vec3 origin,vec3 ray,int medium,float farLimit) {
    int base=45744;
    if(gemRow(base+8).z!=10532.0||gemRow(base+5).w<0.5||abs(ray.z)<0.00001){return vec4(0.0,0.0,0.0,1.0);}
    vec4 pose=gemRow(base+2);vec3 local=(origin-pose.xyz)/pose.w;
    float limit=farLimit/pose.w;float font=1.0;
    float pixel=2.0*gemRow(3).w/(gemRow(8).y*pose.w);
    vec3 result=vec3(0.0);float transmission=1.0;
    for(int visit=0;visit<3;visit++){
        int layer=visit;if(ray.z>0.0){layer=2-visit;}
        float z=-font*(0.10+0.035*float(layer));float weight=1.0;
        if(layer==1){weight=gemRow(215).z;}if(layer==2){weight=gemRow(215).w;}
        float t=(z-local.z)/ray.z;if(t<=0.0000001||t>=limit){continue;}
        vec3 point=local+ray*t;float density=0.0;
        for(int i=0;i<4;i++){
            int glyph=47;if(i==0){glyph=44;}if(i==3){glyph=48;}
            vec2 baseline=vec2((float(i)-2.0)*0.55*font,-font*0.28);
            vec2 em=(point.xy-baseline)/font;
            if(em.x< -0.17||em.x>1.13||em.y< -0.29||em.y>1.01){continue;}
            density+=gemEngravedDensity(em,glyph,vec2(0.0),pixel*0.75/font,layer);
        }
        if(density>0.0001){
            float alpha=1.0-exp(-gemRow(214).w*weight*min(density,1.0)/max(abs(ray.z),0.12));
            vec3 world=pose.xyz+point*pose.w;
            result+=transmission*alpha*exp(-gemRow(gemPanelRow(medium)+1).xyz*t*pose.w/gemObjectScale(medium))
                *gemBarEngravingLight(world,medium,gemObjectScale(medium));
            transmission*=1.0-alpha;
        }
    }
    return vec4(result,transmission);
}

vec4 gemControlEngraving(vec3 origin,vec3 ray,int medium,float farLimit) {
    if(medium==720002){return gemBarLoopEngraving(origin,ray,medium,farLimit);}
    int instance=(medium-4)/256;
    // Extended palettes use material+2.w for other properties, e.g. opaque
    // monitor coat weight. Only the ten original glass controls own SSLE.
    if(medium<4||instance>=10||gemRow(244).y<0.5||abs(ray.z)<0.00001){return vec4(0.0,0.0,0.0,1.0);}
    int base=gemPanelRow(medium);int descriptor=int(gemRow(base+2).w);
    if(descriptor==0||gemRow(base).w<0.0001){return vec4(0.0,0.0,0.0,1.0);}
    vec4 pose=gemRow(gemControlPoseRow(instance));
    vec3 local=(origin-pose.xyz)/pose.w;float limit=farLimit/pose.w;
    vec4 region=gemRow(descriptor);vec4 depth=gemRow(descriptor+1);
    float pixel=2.0*gemRow(3).w/(gemRow(8).y*pose.w);
    vec3 center=vec3(region.xy+region.zw*0.5,(depth.x+depth.z)*0.5);
    vec3 extent=vec3(region.zw*0.5+vec2(pixel),(depth.x-depth.z)*0.5+0.0000001);
    if(!gemBox(local,gemInverseRay(ray),center,extent,limit)){return vec4(0.0,0.0,0.0,1.0);}
    vec3 sigma=gemRow(base+1).xyz;vec3 result=vec3(0.0);float transmission=1.0;
    for(int visit=0;visit<3;visit++){
        int layer=visit;if(ray.z>0.0){layer=2-visit;}
        float z=depth.x;float weight=1.0;vec2 shift=vec2(0.0);
        if(layer==1){z=depth.y;weight=gemRow(215).z;shift=gemRow(216).xy;}
        if(layer==2){z=depth.z;weight=gemRow(215).w;shift=gemRow(216).zw;}
        float t=(z-local.z)/ray.z;
        if(t<=0.0000001||t>=limit){continue;}
        vec3 point=local+ray*t;
        float density=gemControlDensity(point.xy,descriptor,shift,pixel*0.75,layer);
        if(density>0.0001){
            float alpha=1.0-exp(-gemRow(214).w*weight*min(density,1.0)/max(abs(ray.z),0.12));
            vec3 worldPoint=pose.xyz+point*pose.w;
            result+=transmission*alpha*exp(-sigma*t)*gemControlEngravingLight(worldPoint,medium,pose.w);
            transmission*=1.0-alpha;
        }
    }
    return vec4(result,transmission);
}
