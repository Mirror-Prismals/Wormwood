// Canonical workstation optical integrator. Generated frontends share these equations.
// Surface: the authored Blender Principled dielectric, white base, transmission 1,
// IOR 1.766, GGX roughness .018. Volume: authored RGB absorption per metre.
// Reshaped mesh halfspaces drive tracing; eligible primary ruby paths use baked transport.
// Deterministic Fresnel splitting retains reflection and refraction, including TIR.
// Bounded real-time quadrature is not the Cycles stochastic path integrator: the
// floor uses direct area quadrature and approximate transmission shadows (no caustics).
// Display uses a LUT evaluated from the source .blend's actual OCIO AgX transform.
struct GemHit { float distance; vec3 normal; int panel; };
// Pack each vec3's fourth word: this is 48 bytes in WGSL/std430 instead of64.
// The cached scalar priority avoids loading all RGB lanes while scanning work.
struct GemRay { vec3 origin; int medium; vec3 direction; int depth; vec3 weight; float priority; };

// USER_STYLE_FUNCTIONS
vec4 style_color(vec2 uv,vec3 position,vec2 size,float time) { return vec4(1.0); }
vec3 style_background(vec2 uv,float time) { return vec3(0.035,0.045,0.06); }
// END_USER_STYLE_FUNCTIONS

float gemMax(vec3 a) { return max(a.x,max(a.y,a.z)); }
float gemPick(float a,float b,bool choose) { if(choose){return b;}return a; }
// The mobile page covers one side of the fixed timeline projection. This
// clip applies only to background furniture, never panel controls or optics.
// Row246.xy remain the UserStyle base/count; zw are zero outside a slide.
bool gemMobileBackdropAllows(vec2 point) {
    vec4 clip=gemRow(246);
    if(clip.z==12001.0){return point.y<=clip.w;}
    if(clip.z==12002.0){return point.y>=clip.w;}
    if(clip.z==12003.0){return point.x>=clip.w;}
    if(clip.z==12004.0){return point.x<=clip.w;}
    return true;
}
int gemControlPoseRow(int instance) {
    if(instance>=125){return int(gemRow(44592).x)+5*(instance-125);}
    if(instance>=65){return int(gemRow(44288).x)+5*(instance-65);}
    if(instance>=61){return int(gemRow(232).x)+32+2*(instance-61);}
    if(instance>=29){return int(gemRow(247).x)+2*(instance-29);}
    if(instance>=13){return int(gemRow(232).x)+2*(instance-13);}
    if(instance>=10){return 226+2*(instance-10);}
    return 192+2*instance;
}
int gemTimelineInstanceRow(int instance){if(instance>=192){return int(gemRow(255).x)+12+(instance-192)*3;}return int(gemRow(248).y)+instance*3;}
int gemPanelRow(int panel) {
    if(panel>=720000&&panel<720003){return int(gemRow(45744+8).x);}
    if(panel>=800000&&panel<853760){return 12544+40*((panel-800000)%256);}
    if(panel>=700000&&panel<704096){
        int instance=(panel-700000)/16;
        return int(gemRow(gemTimelineInstanceRow(instance)+1).z)+40*((panel-700000)%16);
    }
    if(panel>=4){
        int instance=(panel-4)/256;float base=gemRow(gemControlPoseRow(instance)+1).w;
        if(base<0.5){base=gemRow(12).y;}
        return int(base)+40*((panel-4)%256);
    }
    return 32+40*panel;
}
float gemObjectScale(int panel) {
    if(panel>=720000&&panel<720003){return gemRow(45744+3+panel-720000).z;}
    if(panel>=800000&&panel<853760){return gemRow(43152+8+((panel-800000)/256/3)*6+3).y;}
    if(panel>=700000&&panel<704096){
        int at=gemTimelineInstanceRow((panel-700000)/16);
        if(gemRow(at+1).w< -0.5){return gemRow(at+2).z;}
        return gemRow(at).w;
    }
    if(panel>=4){return gemRow(gemControlPoseRow((panel-4)/256)).w;}
    return 1.0;
}
vec3 gemSafeUnit(vec3 v) { return v/max(length(v),0.0000001); }

vec3 gemInverseRay(vec3 ray) {
    return vec3(gemPick(1.0,-1.0,ray.x<0.0),gemPick(1.0,-1.0,ray.y<0.0),gemPick(1.0,-1.0,ray.z<0.0))/max(abs(ray),vec3(0.00000001));
}
bool gemBox(vec3 origin,vec3 inverseRay,vec3 center,vec3 extent,float farLimit) {
    vec3 a=(center-extent-origin)*inverseRay;vec3 b=(center+extent-origin)*inverseRay;
    vec3 lo=min(a,b);vec3 hi=max(a,b);
    return min(min(hi.x,hi.y),min(hi.z,farLimit))>=max(max(lo.x,lo.y),max(lo.z,0.0));
}
// USER_STYLE_KERNEL
// BUBBLE_NOTES_KERNEL
// AUDIO_SPHERES_KERNEL
// Optional packet flags retain the six-row triangle stride and every BVH offset.
// Unflagged packets keep the original position/interpolated-normal operations.
vec3 gemControlNormal(int at,float u,float v) {
    vec4 first=gemRow(at+3);
    if(first.w>0.5){return gemSafeUnit(first.xyz);}
    return gemSafeUnit(first.xyz*(1.0-u-v)+gemRow(at+4).xyz*u+gemRow(at+5).xyz*v);
}
// Accelerated meter queries still intersect the original triangle packets.
// During this helper only, panel carries the original triangle-row address:
// this preserves the reference BVH's first-triangle ownership at exact ties.
GemHit gemMeterTriangle(int at,int rank,vec3 local,vec3 ray,float scale,float farLimit,GemHit previous) {
    vec4 a=gemRow(at);vec4 edge=gemRow(at+1);
    vec3 e1=edge.xyz;vec3 e2=gemRow(at+2).xyz;
    if(edge.w<0.5){e1-=a.xyz;e2-=a.xyz;}
    vec3 h=cross(ray,e2);float det=dot(e1,h);
    if(abs(det)<0.000000000001){return previous;}
    vec3 offset=local-a.xyz;float u=dot(offset,h)/det;
    if(u<0.0||u>1.0){return previous;}
    vec3 q=cross(offset,e1);float v=dot(ray,q)/det;
    if(v<0.0||u+v>1.0){return previous;}
    float t=dot(e2,q)/det*scale;
    if(t<=0.0000001||t>=farLimit){return previous;}
    if(t>previous.distance||(t==previous.distance&&previous.panel>=0&&rank>=previous.panel)){return previous;}
    vec3 normal=gemControlNormal(at,u,v);
    if(dot(ray,normal)<0.0){return GemHit(t,normal,rank);}
    return previous;
}
// Two exact stacks of contiguous opaque boxes account for 1,464 meter
// triangles. Select bands spatially, including seam neighbours, then run the
// unchanged triangle narrowphase. Housing, liner and window keep their BVH.
GemHit gemMeterHit(vec3 local,vec3 ray,int instance,float scale,vec4 descriptor,float farLimit,bool anyHit) {
    GemHit hit=GemHit(farLimit,vec3(0.0),-1);vec3 inverseRay=gemInverseRay(ray);
    int node=int(descriptor.x);
    for(int visit=0;visit<4096;visit++){
        if(node>=int(descriptor.y)){break;}
        vec4 lo=gemRow(node);vec4 hi=gemRow(node+1);
        if(!gemBox(local,inverseRay,(lo.xyz+hi.xyz)*0.5,(hi.xyz-lo.xyz)*0.5+vec3(0.0000001),hit.distance/scale)){
            node=int(lo.w);continue;
        }
        int count=int(gemRow(node+2).x);
        if(count==0){node+=3;continue;}
        for(int triangle=0;triangle<4;triangle++){
            if(triangle>=count){break;}
            int at=int(hi.w)+triangle*6;int rank=int(gemRow(at+2).w);
            hit=gemMeterTriangle(at,rank,local,ray,scale,farLimit,hit);
            if(anyHit&&hit.panel>=0){return GemHit(hit.distance,hit.normal,4+instance*256+int(gemRow(hit.panel).w));}
        }
        node=int(lo.w);
    }
    for(int channel=0;channel<2;channel++){
        if(channel>=int(descriptor.w)){break;}
        vec4 lo=gemRow(int(descriptor.z)+2*channel);vec4 hi=gemRow(int(descriptor.z)+2*channel+1);
        vec3 a=(lo.xyz-vec3(0.0000001)-local)*inverseRay;
        vec3 b=(hi.xyz+vec3(0.0000001)-local)*inverseRay;
        vec3 lower=min(a,b);vec3 upper=max(a,b);
        float near=max(max(lower.x,lower.y),max(lower.z,0.0));
        float far=min(min(upper.x,upper.y),min(upper.z,hit.distance/scale));
        if(far<near){continue;}
        int count=int(lo.w);int records=int(hi.w);
        float entryY=local.y+near*ray.y;
        // Conservative candidate padding only; intersection coordinates,
        // distance, normals and materials always come from original triangles.
        // Scale the padding with cancellation error for distant ray origins.
        float guard=0.0000004+0.000004*(abs(local.y)+abs(near*ray.y));
        float firstY=entryY-guard;if(ray.y<0.0){firstY=entryY+guard;}
        int left=0;int right=count;
        for(int search=0;search<8;search++){
            if(left>=right){break;}
            int middle=(left+right)/2;
            if(gemRow(records+4*middle).y<firstY){left=middle+1;}else{right=middle;}
        }
        int band=min(left,count-1);int step=1;if(ray.y<0.0){step=-1;}
        for(int visit=0;visit<128;visit++){
            if(band<0||band>=count){break;}
            vec4 range=gemRow(records+4*band);
            float limit=min(far,hit.distance/scale);float lastY=local.y+limit*ray.y;
            float endGuard=0.0000004+0.000004*(abs(local.y)+abs(limit*ray.y));
            if((step>0&&range.x>lastY+endGuard)||(step<0&&range.y<lastY-endGuard)){break;}
            vec3 bandLo=vec3(lo.x,range.x,lo.z);vec3 bandHi=vec3(hi.x,range.y,hi.z);
            if(gemBox(local,inverseRay,(bandLo+bandHi)*0.5,(bandHi-bandLo)*0.5+vec3(0.0000001),hit.distance/scale)){
                for(int group=0;group<3;group++){
                    vec4 references=gemRow(records+4*band+1+group);
                    for(int triangle=0;triangle<4;triangle++){
                        int at=int(references[triangle]);
                        hit=gemMeterTriangle(at,at,local,ray,scale,farLimit,hit);
                        if(anyHit&&hit.panel>=0){return GemHit(hit.distance,hit.normal,4+instance*256+int(gemRow(hit.panel).w));}
                    }
                }
            }
            band+=step;
        }
    }
    if(hit.panel>=0){return GemHit(hit.distance,hit.normal,4+instance*256+int(gemRow(hit.panel).w));}
    return hit;
}
// An interior medium already names the one instance and material containing
// this ray. Dispatch directly to its planes or material-only BVH, without
// repeating scene/group/instance search for every internal reflection.
GemHit gemControlExit(vec3 origin,vec3 ray,int medium,float farLimit) {
    GemHit hit=GemHit(farLimit,vec3(0.0),-1);
    int instance=(medium-4)/256;int material=(medium-4)%256;
    bool monitor=(instance>=10&&instance<13)||(instance>=29&&instance<61);
    vec4 pose=gemRow(gemControlPoseRow(instance));vec3 local=(origin-pose.xyz)/pose.w;
    int base=gemPanelRow(medium);vec4 interior=gemRow(base+2);
    if(interior.z>0.0){
        float far=1e20;vec3 normal=vec3(0.0);
        for(int planeIndex=0;planeIndex<26;planeIndex++){
            if(float(planeIndex)>=interior.z){break;}
            vec4 plane=gemRow(base+6+planeIndex);float denom=dot(plane.xyz,ray);
            if(denom>=0.0000001){
                float t=(plane.w-dot(plane.xyz,local))/denom;
                if(t<far){far=t;normal=plane.xyz;}
            }
        }
        if(far*pose.w>0.0000001&&far*pose.w<farLimit){
            if(monitor&&!gemMobileBackdropAllows((origin+ray*(far*pose.w)).xy)){return hit;}
            return GemHit(far*pose.w,normal,medium);
        }
        return hit;
    }
    bool homogeneous=interior.x>0.0;
    vec4 tree=interior;if(!homogeneous){tree=gemRow(gemControlPoseRow(instance)+1);}
    vec3 inverseRay=gemInverseRay(ray);int node=int(tree.x);
    for(int visit=0;visit<4096;visit++){
        if(node>=int(tree.y)){break;}
        vec4 lo=gemRow(node);vec4 hi=gemRow(node+1);
        if(!gemBox(local,inverseRay,(lo.xyz+hi.xyz)*0.5,(hi.xyz-lo.xyz)*0.5+vec3(0.0000001),hit.distance/pose.w)){
            node=int(lo.w);continue;
        }
        vec4 metadata=gemRow(node+2);int count=int(metadata.x);
        if(!homogeneous&&(float(material)<metadata.y||float(material)>metadata.z)){node=int(lo.w);continue;}
        if(count==0){node+=3;continue;}
        for(int triangle=0;triangle<4;triangle++){
            if(triangle>=count){break;}
            int at=int(hi.w)+triangle*6;vec4 a=gemRow(at);
            if(!homogeneous&&int(a.w)!=material){continue;}
            vec4 edge=gemRow(at+1);vec3 e1=edge.xyz;vec3 e2=gemRow(at+2).xyz;
            if(edge.w<0.5){e1-=a.xyz;e2-=a.xyz;}
            vec3 h=cross(ray,e2);float det=dot(e1,h);
            if(abs(det)<0.000000000001){continue;}
            vec3 offset=local-a.xyz;float u=dot(offset,h)/det;
            if(u<0.0||u>1.0){continue;}
            vec3 q=cross(offset,e1);float v=dot(ray,q)/det;
            if(v<0.0||u+v>1.0){continue;}
            float t=dot(e2,q)/det*pose.w;
            if(t>0.0000001&&t<hit.distance){
                if(monitor&&!gemMobileBackdropAllows((origin+ray*t).xy)){continue;}
                vec3 normal=gemControlNormal(at,u,v);
                if(dot(ray,normal)>0.0){hit=GemHit(t,normal,medium);}
            }
        }
        node=int(lo.w);
    }
    return hit;
}
// Exact authored triangle meshes, traversed through stackless object-space BVHs.
// An instance owns its material medium so identical faders never share interiors.
GemHit gemControlHit(vec3 origin,vec3 ray,int medium,float farLimit,bool anyHit) {
    if(medium>=4){return gemControlExit(origin,ray,medium,farLimit);}
    GemHit hit=GemHit(farLimit,vec3(0.0),-1);
    vec3 groupInverse=gemInverseRay(ray);
    bool topVisible=gemBox(origin,groupInverse,gemRow(13).xyz,gemRow(14).xyz,farLimit);
    bool bottomVisible=gemBox(origin,groupInverse,gemRow(15).xyz,gemRow(31).xyz,farLimit);
    vec4 monitorCenter=gemRow(238);vec4 monitorExtent=gemRow(239);
    bool monitorsVisible=monitorCenter.w<monitorExtent.w&&gemBox(origin,groupInverse,monitorCenter.xyz,monitorExtent.xyz,farLimit);
    bool ioVisible=gemRow(44288).y>0.0&&gemBox(origin,groupInverse,gemRow(44289).xyz,gemRow(44290).xyz,farLimit);
    bool keyboardVisible=gemRow(44592).y>0.0&&gemBox(origin,groupInverse,gemRow(44593).xyz,gemRow(44594).xyz,farLimit);
    if(!topVisible&&!bottomVisible&&!monitorsVisible&&!ioVisible&&!keyboardVisible){return hit;}
    // Optional ordered instance BVH occupies previously unused control rows.
    // Old frames have zero header.zw and retain the original 65-instance loop.
    // Leaves visit the same ascending instance order, preserving exact-distance
    // triangle/material ownership. The authored mesh BVH below is unchanged.
    vec4 instanceHeader=gemRow(12);
    bool instancesAccelerated=instanceHeader.z>0.0&&instanceHeader.w>instanceHeader.z;
    int instanceNode=int(instanceHeader.z);
    for(int query=0;query<257;query++){
        int instance=query;
        if(instancesAccelerated){
            if(instanceNode>=int(instanceHeader.w)){break;}
            vec4 instanceLo=gemRow(instanceNode);vec4 instanceHi=gemRow(instanceNode+1);
            if(!gemBox(origin,groupInverse,(instanceLo.xyz+instanceHi.xyz)*0.5,(instanceHi.xyz-instanceLo.xyz)*0.5,hit.distance)){
                instanceNode=int(instanceLo.w);continue;
            }
            instanceNode+=2;
            if(instanceHi.w<0.0){continue;}
            instance=int(instanceHi.w);
        }else if(instance>=129){break;}
        bool monitor=(instance>=10&&instance<13)||(instance>=29&&instance<61);
        if(instance<4&&!topVisible){continue;}
        if(instance>=4&&instance<65&&!monitor&&!bottomVisible){continue;}
        if(monitor&&!monitorsVisible){continue;}
        if(instance>=125){
            if(!keyboardVisible||float(instance-125)>=gemRow(44592).y){continue;}
        }else if(instance>=65){
            if(!ioVisible||float(instance-65)>=gemRow(44288).y){continue;}
        }else if(instance>=61){
            if(float(instance-61)>=gemRow(232).z){continue;}
        }else if(instance>=29){
            if(float(instance-29)>=gemRow(247).y){continue;}
        }else if(float(instance)>=gemRow(12).x){continue;}
        vec4 pose=gemRow(gemControlPoseRow(instance));vec4 tree=gemRow(gemControlPoseRow(instance)+1);
        if(pose.w<=0.0){continue;}
        vec3 local=(origin-pose.xyz)/pose.w;
        // The exact band lookup preserved pixels but cost more GPU time than
        // this original BVH on the measured Apple GPU. Retain the faster path.
        int node=int(tree.x);
        // The full-detail traffic light has its own immutable BVH. Its validated
        // node count fits this bound; older control packets keep their limit.
        int visitLimit=4096;if(instance>=13){visitLimit=16384;}if(monitor||instance>=65){visitLimit=131072;}
        for(int visit=0;visit<131072;visit++){
            if(visit>=visitLimit){break;}
            if(node>=int(tree.y)){break;}
            vec4 lo=gemRow(node);vec4 hi=gemRow(node+1);
            if(!gemBox(local,groupInverse,(lo.xyz+hi.xyz)*0.5,(hi.xyz-lo.xyz)*0.5+vec3(0.0000001),hit.distance/pose.w)){
                node=int(lo.w);continue;
            }
            int count=int(gemRow(node+2).x);
            if(count==0){node+=3;continue;}
            for(int triangle=0;triangle<4;triangle++){
                if(triangle>=count){break;}
                int at=int(hi.w)+triangle*6;vec4 a=gemRow(at);
                int material=4+instance*256+int(a.w);
                vec4 edge=gemRow(at+1);vec3 e1=edge.xyz;vec3 e2=gemRow(at+2).xyz;
                if(edge.w<0.5){e1-=a.xyz;e2-=a.xyz;}
                vec3 h=cross(ray,e2);float det=dot(e1,h);
                if(abs(det)<0.000000000001){continue;}
                vec3 offset=local-a.xyz;float u=dot(offset,h)/det;
                if(u<0.0||u>1.0){continue;}
                vec3 q=cross(offset,e1);float v=dot(ray,q)/det;
                if(v<0.0||u+v>1.0){continue;}
                float t=dot(e2,q)/det*pose.w;
                if(u>=0.0&&v>=0.0&&u+v<=1.0&&t>0.0000001&&t<hit.distance){
                    float hitY=origin.y+ray.y*t;
                    if(monitor&&(hitY<monitorCenter.w||hitY>monitorExtent.w)){continue;}
                    if(monitor&&!gemMobileBackdropAllows((origin+ray*t).xy)){continue;}
                    if(instance>=65){
                        int clipRow=44291;if(instance>=125){clipRow=44595;}
                        vec4 clip=gemRow(clipRow);float hitX=origin.x+ray.x*t;
                        if(hitX<clip.x||hitX>clip.z||hitY<clip.y||hitY>clip.w){continue;}
                    }
                    if(instance>=13&&instance<29){
                        vec4 clip=gemRow(int(gemRow(232).x)+40);float hitX=origin.x+ray.x*t;
                        if(hitX<clip.x||hitX>clip.z||hitY<clip.y||hitY>clip.w){continue;}
                    }
                    vec3 normal=gemControlNormal(at,u,v);
                    if(dot(ray,normal)<0.0){
                        hit=GemHit(t,normal,material);
                        if(anyHit){return hit;}
                    }
                }
            }
            node=int(lo.w);
        }
    }
    return hit;
}
// A ray whose medium identifies this closed convex panel starts in its interior.
// Only outward-facing planes can end its interval: no entry interval/AABB or
// origin dot product for back-facing planes is needed for these dominant rays.
// TIMELINE_BAR_KERNEL
// FADER_COMPONENTS_KERNEL
// TIMELINE_KERNEL
// BOTTOM_PANEL_HIT_KERNEL

GemHit gemPluginDropMarker(vec3 origin,vec3 ray,float farLimit) {
    if(gemRow(232).z<0.5){return GemHit(farLimit,vec3(0.0),-1);}
    int at=int(gemRow(232).x)+41;vec4 mark=gemRow(at);
    if(mark.w<=0.0){return GemHit(farLimit,vec3(0.0),-1);}
    vec3 center=vec3(mark.x,(mark.y+mark.z)*0.5,gemRow(at+1).x);
    GemHit hit=gemTimelineMarkerBox(origin,ray,center,vec3(mark.w*0.5,(mark.z-mark.y)*0.5,0.0005),farLimit);
    if(hit.panel>=0){hit.panel=719998;}return hit;
}
GemHit gemExitPanel(vec3 origin,vec3 ray,int panel) {
    int base=gemPanelRow(panel);float far=1e20;vec3 exitNormal=vec3(0.0);
    for(int i=0;i<26;i++){
        vec4 plane=gemRow(base+6+i);float denom=dot(plane.xyz,ray);
        if(denom>=0.0000001){
            float t=(plane.w-dot(plane.xyz,origin))/denom;
            if(t<far){far=t;exitNormal=plane.xyz;}
        }
    }
    if(far<=0.0000001||far>=1e20){return GemHit(-1.0,vec3(0.0),panel);}
    return GemHit(far,exitNormal,panel);
}
GemHit gemEnterPanel(vec3 origin,vec3 ray,vec3 inverseRay,int panel,float farLimit) {
    int base=gemPanelRow(panel);
    if(gemRow(base).x<0.5){return GemHit(-1.0,vec3(0.0),panel);}
    if(!gemBox(origin,inverseRay,gemRow(base+2).xyz,gemRow(base+3).xyz,farLimit)){
        return GemHit(-1.0,vec3(0.0),panel);
    }
    float near=-1e20;float far=1e20;vec3 entry=vec3(0.0);
    for(int i=0;i<26;i++){
        vec4 plane=gemRow(base+6+i);float denom=dot(plane.xyz,ray);
        float distance=plane.w-dot(plane.xyz,origin);
        if(abs(denom)<0.0000001){
            if(distance < -0.0000001){return GemHit(-1.0,vec3(0.0),panel);}
        }else{
            float t=distance/denom;
            if(denom<0.0 && t>near){near=t;entry=plane.xyz;}
            if(denom>0.0 && t<far){far=t;}
            // Additional halfspaces can only narrow the current interval.
            if(far<near){return GemHit(-1.0,vec3(0.0),panel);}
        }
    }
    if(far<=0.0000001||near<0.0000001||near>farLimit){return GemHit(-1.0,vec3(0.0),panel);}
    return GemHit(near,entry,panel);
}
// PIANO_ROLL_HIT_KERNEL
GemHit gemNearest(vec3 origin,vec3 ray,int medium,float farLimit) {
    if(gemPianoActive()){GemHit hit=gemPianoHit(origin,ray,farLimit);GemHit bubble=gemBubbleHit(origin,ray,hit.distance);if(bubble.panel>=0){return bubble;}return hit;}
    if(medium>=720000&&medium<720003){return gemBarHit(origin,ray,medium,farLimit);}
    if(medium>=800000&&medium<853760){return gemMixerHit(origin,ray,medium,farLimit,false);}
    if(medium>=700000&&medium<704096){return gemTimelineHit(origin,ray,medium,farLimit);}
    if(medium>=4){return gemControlHit(origin,ray,medium,farLimit,false);}
    if(medium>=0){return gemExitPanel(origin,ray,medium);}
    GemHit hit=GemHit(farLimit,vec3(0.0),-1);vec3 inverseRay=gemInverseRay(ray);
    for(int panel=0;panel<4;panel++){
        if(float(panel)>=gemRow(0).y){break;}
        GemHit next=gemEnterPanel(origin,ray,inverseRay,panel,hit.distance);
        if(next.distance>0.0&&next.distance<hit.distance){hit=next;}
    }
    GemHit control=gemControlHit(origin,ray,-1,hit.distance,false);
    if(control.panel>=0){hit=control;}
    GemHit mixer=gemMixerHit(origin,ray,-1,hit.distance,false);
    if(mixer.panel>=0){hit=mixer;}
    GemHit bubble=gemBubbleHit(origin,ray,hit.distance);
    if(bubble.panel>=0){hit=bubble;}
    GemHit custom=userStyleHit(origin,ray,hit.distance,false,false);
    if(custom.panel>=0){hit=custom;}
    GemHit sphere=gemAudioSphereHit(origin,ray,hit.distance);
    if(sphere.panel>=0){hit=sphere;}
    GemHit timeline=gemTimelineHit(origin,ray,-1,hit.distance);
    if(timeline.panel>=0){hit=timeline;}
    GemHit bottomSurface=gemBottomSurfaceHit(origin,ray,hit.distance);
    if(bottomSurface.panel>=0){hit=bottomSurface;}
    GemHit playhead=gemTimelinePlayhead(origin,ray,hit.distance);
    if(playhead.panel>=0){hit=playhead;}
    GemHit dropMarker=gemPluginDropMarker(origin,ray,hit.distance);
    if(dropMarker.panel>=0){hit=dropMarker;}
    return hit;
}
bool gemAny(vec3 origin,vec3 ray,float farLimit) {
    if(gemPianoActive()){return gemPianoHit(origin,ray,farLimit).panel>=0;}
    vec3 inverseRay=gemInverseRay(ray);
    for(int panel=0;panel<4;panel++){
        if(float(panel)>=gemRow(0).y){break;}
        GemHit hit=gemEnterPanel(origin,ray,inverseRay,panel,farLimit);
        if(hit.distance>0.0&&hit.distance<farLimit){return true;}
    }
    GemHit control=gemControlHit(origin,ray,-1,farLimit,true);
    if(control.panel>=0){return true;}
    if(gemBarHit(origin,ray,-1,farLimit).panel>=0){return true;}
    if(gemMixerHit(origin,ray,-1,farLimit,true).panel>=0){return true;}
    return userStyleHit(origin,ray,farLimit,true,false).panel>=0;
}
float gemFresnel(float cosine,float etaI,float etaT) {
    float ci=clamp(abs(cosine),0.0,1.0);float ratio=etaI/etaT;
    float sinT2=ratio*ratio*(1.0-ci*ci);
    if(sinT2>=1.0){return 1.0;}
    float ct=sqrt(max(0.0,1.0-sinT2));
    float rs=(etaI*ci-etaT*ct)/max(etaI*ci+etaT*ct,0.0000001);
    float rp=(etaT*ci-etaI*ct)/max(etaT*ci+etaI*ct,0.0000001);
    return 0.5*(rs*rs+rp*rp);
}
float gemSmith(float cosine,float alpha) {
    float c=clamp(abs(cosine),0.0,1.0);
    return 2.0*c/max(c+sqrt(alpha*alpha+(1.0-alpha*alpha)*c*c),0.0000001);
}
// Correlated Smith masking, as used by Blender 5.0.1's GGX closure.
// With visible-normal sampling this is G2/G1(incident), not G1(outgoing).
float gemSampleMask(float incoming,float outgoing,float alpha) {
    float gi=gemSmith(incoming,alpha);float go=gemSmith(outgoing,alpha);
    return go/max(gi+go-gi*go,0.0000001);
}
// Exact angular slices of Cycles' glass E tables at the authored IOR and
// roughness. Runtime metadata carries their source licence and table hashes.
float gemEnergy(int panel,bool inside,float cosine) {
    float coordinate=clamp(abs(cosine),0.0,1.0)*15.0;
    int lo=int(floor(coordinate));int hi=min(lo+1,15);
    int base=gemPanelRow(panel)+32;if(inside){base+=4;}
    float a=gemRow(base+lo/4)[lo%4];float b=gemRow(base+hi/4)[hi%4];
    return mix(a,b,fract(coordinate));
}
// Heitz visible-normal sampling for isotropic GGX. All directions stay in the
// actual facet's frame; roughness never paints a highlight onto the mesh.
vec3 gemMicroNormal(vec3 n,vec3 view,float roughness,vec2 u) {
    float alpha=max(roughness*roughness,0.00001);
    vec3 tangent=gemSafeUnit(cross(n,vec3(0.0,0.0,1.0)));
    if(abs(n.z)>0.999){tangent=vec3(1.0,0.0,0.0);}
    vec3 bitangent=cross(n,tangent);
    vec3 v=gemSafeUnit(vec3(alpha*dot(view,tangent),alpha*dot(view,bitangent),max(dot(view,n),0.00001)));
    float lensq=v.x*v.x+v.y*v.y;vec3 t1=vec3(1.0,0.0,0.0);
    if(lensq>0.0000001){t1=vec3(-v.y,v.x,0.0)/sqrt(lensq);}
    vec3 t2=cross(v,t1);float r=sqrt(u.x);float phi=6.28318530718*u.y;
    float x=r*cos(phi);float y=r*sin(phi);float s=0.5*(1.0+v.z);
    y=(1.0-s)*sqrt(max(0.0,1.0-x*x))+s*y;
    vec3 nh=t1*x+t2*y+v*sqrt(max(0.0,1.0-x*x-y*y));
    vec3 local=gemSafeUnit(vec3(alpha*nh.x,alpha*nh.y,max(0.0,nh.z)));
    return gemSafeUnit(tangent*local.x+bitangent*local.y+n*local.z);
}
vec3 gemShadow(vec3 origin,vec3 ray,float distance) {
    if(userStyleHit(origin,ray,distance,true,true).panel>=0){return vec3(0.0);}
    vec3 visibility=vec3(1.0);vec3 inverseRay=gemInverseRay(ray);
    for(int panel=0;panel<4;panel++){
        if(float(panel)>=gemRow(0).y){break;}
        GemHit entry=gemEnterPanel(origin,ray,inverseRay,panel,distance);
        if(entry.distance>0.0&&entry.distance<distance){
            vec4 material=gemRow(gemPanelRow(panel));
            // The connection to the emitter is still a single-path visibility
            // approximation, but its dielectric crossing must obey Snell's law.
            // Applying glass-to-air Fresnel to the original AIR direction caused
            // false total internal reflection and over-dark floor shadows.
            vec3 interiorRay=refract(ray,entry.normal,1.0/material.y);
            vec3 start=origin+ray*entry.distance+interiorRay*0.000001;
            GemHit exitHit=gemExitPanel(start,interiorRay,panel);
            if(exitHit.distance>0.0){
                float entryF=gemFresnel(dot(ray,entry.normal),1.0,material.y);
                float exitF=gemFresnel(dot(interiorRay,exitHit.normal),material.y,1.0);
                visibility*=exp(-gemRow(gemPanelRow(panel)+1).xyz*exitHit.distance)*(1.0-entryF)*(1.0-exitF)*material.w;
            }
        }
    }
    return visibility;
}
float gemLightDistance(vec3 origin,vec3 ray,int light) {
    int base=16+5*light;vec3 n=gemRow(base+3).xyz;
    float denom=dot(ray,n);if(denom>=-0.0000001){return -1.0;}
    float t=dot(gemRow(base).xyz-origin,n)/denom;if(t<=0.000001){return -1.0;}
    vec3 p=origin+ray*t-gemRow(base).xyz;
    if(abs(dot(p,gemRow(base+1).xyz))>gemRow(base+1).w||abs(dot(p,gemRow(base+2).xyz))>gemRow(base+2).w){return -1.0;}
    return t;
}
// Per-light cosine-weighted visibility is shared by every escaped gem ray.
// The control thread integrates sixteen area connections at each grid point.
// Bilinear interpolation averages visibility, never the dielectric material or
// its view-dependent GGX BRDF. Outside this finite field use direct intersections.
vec4 gemCachedShadow(vec3 point,int light) {
    vec4 descriptor=gemRow(10);vec2 span=gemRow(11).xy;int edge=int(descriptor.y);
    if(edge<2||min(span.x,span.y)<=0.0){return vec4(0.0);}
    vec2 uv=(point.xy-descriptor.zw)/span;
    if(min(uv.x,uv.y)<0.0||max(uv.x,uv.y)>1.0){return vec4(0.0);}
    vec2 p=uv*float(edge-1);int x=int(floor(p.x));int y=int(floor(p.y));
    int nextX=min(x+1,edge-1);int nextY=min(y+1,edge-1);vec2 f=fract(p);
    int base=int(descriptor.x)+light*edge*edge;
    vec3 a=gemRow(base+y*edge+x).xyz;vec3 b=gemRow(base+y*edge+nextX).xyz;
    vec3 c=gemRow(base+nextY*edge+x).xyz;vec3 d=gemRow(base+nextY*edge+nextX).xyz;
    return vec4(mix(mix(a,b,f.x),mix(c,d,f.x),f.y),1.0);
}
vec3 gemOpaqueSurface(vec3 point,vec3 view,vec3 normal,vec3 baseColor,float roughness,bool floorSurface,float metallic,float coat,float coatRoughness) {
    vec3 result=baseColor*gemRow(5).xyz;
    float alpha=max(roughness*roughness,0.0001);float alpha2=alpha*alpha;
    float nv=max(dot(normal,view),0.00001);
    for(int light=0;light<3;light++){
        if(float(light)>=gemRow(9).x){break;}
        int row=16+light*5;vec4 u=gemRow(row+1);vec4 v=gemRow(row+2);
        float area=4.0*u.w*v.w;
        vec4 cachedVisibility=vec4(1.0);
        if(floorSurface){cachedVisibility=gemCachedShadow(point,light);}
        // Four-point Gauss-Legendre integration over the actual rectangular emitter.
        for(int sampleIndex=0;sampleIndex<4;sampleIndex++){
            float x=gemPick(-0.57735026919,0.57735026919,(sampleIndex%2)==1);
            float y=gemPick(-0.57735026919,0.57735026919,sampleIndex>=2);
            vec3 toLight=gemRow(row).xyz+u.xyz*(u.w*x)+v.xyz*(v.w*y)-point;
            float distance=length(toLight);vec3 l=toLight/max(distance,0.000001);
            float nl=max(dot(normal,l),0.0);float cosLight=max(dot(gemRow(row+3).xyz,-l),0.0);
            if(nl>0.0&&cosLight>0.0){
                vec3 halfVector=gemSafeUnit(l+view);float nh=max(dot(normal,halfVector),0.0);
                float d=alpha2/(3.14159265359*pow(nh*nh*(alpha2-1.0)+1.0,2.0));
                float fresnel=gemFresnel(dot(view,halfVector),1.0,1.5);
                float specular=d*fresnel*gemSmith(nv,alpha)*gemSampleMask(nv,nl,alpha)/(4.0*nv*max(nl,0.00001));
                vec3 brdf=baseColor*((1.0-fresnel)/3.14159265359)+vec3(specular);
                // The monitor GLBs also author metallic fasteners and a clear
                // coat over the lamps. Keep those lobes in the shared studio.
                // Existing controls pass zero weights and retain their shader.
                if(metallic>0.0){
                    float vh=max(dot(view,halfVector),0.0);
                    vec3 metalF=baseColor+(vec3(1.0)-baseColor)*pow(1.0-vh,5.0);
                    vec3 metalBrdf=metalF*(d*gemSmith(nv,alpha)*gemSampleMask(nv,nl,alpha)/(4.0*nv*max(nl,0.00001)));
                    brdf=mix(brdf,metalBrdf,metallic);
                }
                if(coat>0.0){
                    float coatAlpha=max(coatRoughness*coatRoughness,0.0001);float coatAlpha2=coatAlpha*coatAlpha;
                    float coatD=coatAlpha2/(3.14159265359*pow(nh*nh*(coatAlpha2-1.0)+1.0,2.0));
                    float coatBrdf=coatD*fresnel*gemSmith(nv,coatAlpha)*gemSampleMask(nv,nl,coatAlpha)/(4.0*nv*max(nl,0.00001));
                    float attenuation=(1.0-coat*gemFresnel(nv,1.0,1.5))*(1.0-coat*gemFresnel(nl,1.0,1.5));
                    brdf=brdf*attenuation+vec3(coat*coatBrdf);
                }
                vec3 visibility=cachedVisibility.xyz;
                if(cachedVisibility.w<0.5||gemRow(245).x>0.5){visibility=gemShadow(point+normal*0.000001,l,distance);}
                result+=brdf*gemRow(row+4).xyz*visibility*(nl*cosLight*area/(4.0*distance*distance));
            }
        }
    }
    return result;
}
vec3 gemOpaque(vec3 point,vec3 view,vec3 normal,vec3 baseColor,float roughness,bool floorSurface) {
    return gemOpaqueSurface(point,view,normal,baseColor,roughness,floorSurface,0.0,0.0,0.0);
}
vec3 gemFloor(vec3 point,vec3 view) {
    if(gemRow(245).z>0.5){
        vec2 uv=vec2(point.x/max(2.0*gemRow(2).w,0.000001)+0.5,point.y/max(2.0*gemRow(3).w,0.000001)+0.5);
        return clamp(style_background(clamp(uv,vec2(0.0),vec2(1.0)),gemRow(245).w),vec3(0.0),vec3(65504.0));
    }
    return gemOpaque(point,view,vec3(0.0,0.0,1.0),gemRow(6).xyz,gemRow(6).w,true);
}
vec4 gemEnvironment(vec3 origin,vec3 ray) {
    float distance=1e20;vec3 radiance=gemRow(5).xyz;
    if(gemPianoActive()){
        // Broad studio cards make a transparent membrane readable against a
        // plain backdrop. Primary/transmitted -Z rays follow the plain backdrop;
        // curved reflection rays see soft highlights and thin-film color.
        float sky=pow(max(ray.z,0.0),0.6);
        float key=pow(max(dot(ray,gemSafeUnit(vec3(-0.45,0.55,0.85))),0.0),10.0);
        float fill=pow(max(dot(ray,gemSafeUnit(vec3(0.70,-0.25,0.55))),0.0),6.0);
        radiance=gemRow(6).xyz*gemRow(5).xyz+sky*vec3(0.55,0.65,0.80)
            +key*vec3(6.5,6.0,5.3)+fill*vec3(2.0,2.8,4.5);
    }
    if(!gemPianoActive()&&gemRow(9).y>0.5&&ray.z < -0.0000001){
        float t=(gemRow(5).w-origin.z)/ray.z;
        if(t>0.000001){distance=t;radiance=vec3(-1.0);}
    }
    for(int light=0;light<3;light++){
        if(float(light)>=gemRow(9).x){break;}
        float t=gemLightDistance(origin,ray,light);
        if(t>0.0&&t<distance){distance=t;radiance=gemRow(16+light*5+4).xyz;}
    }
    return vec4(radiance,distance);
}
// Resolve terminal exterior leaves before allocating a pending ray. Most
// reflections/transmissions leave the four stones and see the studio directly.
// Other-panel intersections remain full ray work; only terminal leaves bypass
// queue storage/scanning. Alpha1 means the leaf terminated, alpha0 means it hit
// another stone and must remain in the normal transport work list.
vec4 gemTerminal(vec3 origin,vec3 ray) {
    vec4 environment=gemEnvironment(origin,ray);
    if(gemPianoActive()){if(gemPianoHit(origin,ray,environment.w).panel>=0||gemBubbleHit(origin,ray,environment.w).panel>=0){return vec4(0.0);}return vec4(environment.xyz,1.0);}
    if(gemAny(origin,ray,environment.w)){return vec4(0.0);}
    if(gemBubbleHit(origin,ray,environment.w).panel>=0){return vec4(0.0);}
    if(gemAudioSphereHit(origin,ray,environment.w).panel>=0){return vec4(0.0);}
    if(gemTimelineHit(origin,ray,-1,environment.w).panel>=0){return vec4(0.0);}
    if(gemBottomSurfaceHit(origin,ray,environment.w).panel>=0){return vec4(0.0);}
    if(gemTimelinePlayhead(origin,ray,environment.w).panel>=0){return vec4(0.0);}
    if(gemPluginDropMarker(origin,ray,environment.w).panel>=0){return vec4(0.0);}
    vec3 radiance=environment.xyz;
    if(radiance.x<0.0){radiance=gemFloor(origin+ray*environment.w,-ray);}
    return vec4(radiance,1.0);
}
// SSLE: font-mesh samples describe three embedded microfracture layers. This is
// evaluated along the REFRACTED interior segment, before its glass exit, so the
// text has real depth/occlusion and appears in internal reflections. A filtered
// point-density approximation avoids adding thousands of tiny triangle meshes.
// Analytically integrated fracture densities and their area-average mips are
// baked from the same Blender point centres. Sampling is still in refracted
// 3D panel space; the immutable atlas removes per-ray point hashes/Gaussians.
float gemDensityTexel(int glyph,int x,int y,int edge,int offset,int layer) {
    if(x<0||x>=edge||y<0||y>=edge){return 0.0;}
    int index=glyph*int(gemRow(224).w)+offset+y*edge+x;
    vec4 packed=gemRow(int(gemRow(224).x)+index/2);
    float rg=packed.x;float b=packed.y;
    if(index%2!=0){rg=packed.z;b=packed.w;}
    if(layer==2){return b/255.0;}
    if(layer==1){return floor(rg/256.0)/255.0;}
    return float(int(rg)%256)/255.0;
}
float gemDensityLevel(vec2 uv,int glyph,int level,int layer) {
    int edge=int(gemRow(224).y);int offset=0;
    for(int mip=0;mip<7;mip++){
        if(mip>=level){break;}
        offset+=edge*edge;edge/=2;
    }
    vec2 p=uv*float(edge)-vec2(0.5);vec2 f=fract(p);
    int x=int(floor(p.x));int y=int(floor(p.y));
    float a=gemDensityTexel(glyph,x,y,edge,offset,layer);
    float b=gemDensityTexel(glyph,x+1,y,edge,offset,layer);
    float c=gemDensityTexel(glyph,x,y+1,edge,offset,layer);
    float d=gemDensityTexel(glyph,x+1,y+1,edge,offset,layer);
    return mix(mix(a,b,f.x),mix(c,d,f.x),f.y);
}
float gemEngravedDensity(vec2 em,int glyph,vec2 shift,float footprint,int layer) {
    vec4 grid=gemRow(213);
    vec2 uv=(em-grid.yz-shift*grid.x)/gemRow(224).z;
    float lod=clamp(log2(max(footprint*gemRow(224).y/gemRow(224).z,1.0)),0.0,7.0);
    int lo=int(floor(lod));int hi=min(lo+1,7);
    return mix(gemDensityLevel(uv,glyph,lo,layer),gemDensityLevel(uv,glyph,hi,layer),fract(lod));
}
// Generic panels mount the same host-owned dynamic labels on their content face.
// The baked alphabet is reused as coverage; no gem interior or volume is assumed.
float userSurfaceLabel(vec3 point,int panel) {
    if(panel<2||panel>3||gemRow(212).w<0.5){return 0.0;}
    float pixel=2.0*gemRow(3).w/max(gemRow(8).y,1.0);float density=0.0;
    for(int line=0;line<64;line++){
        if(float(line)>=gemRow(222).y){break;}
        int at=int(gemRow(222).x)+3*line;
        if(int(gemRow(at+2).x)!=panel){continue;}
        vec4 bounds=gemRow(at);vec4 extent=gemRow(at+1);
        if(abs(point.x-bounds.x)>extent.x+pixel||abs(point.y-bounds.y)>extent.y+pixel){continue;}
        int first=int(bounds.w);int size=int(extent.w);
        for(int character=0;character<48;character++){
            if(character>=size){break;}
            int cp=int(gemRow(212).y)+2*(first+character);vec4 pose=gemRow(cp);
            vec2 em=(point.xy-pose.xy)/max(pose.w,0.0000001);
            if(em.x< -0.17){break;}
            if(em.x>1.13||em.y< -0.29||em.y>1.01){continue;}
            density+=gemEngravedDensity(em,int(gemRow(cp+1).x),vec2(0.0),pixel*0.75/max(pose.w,0.0000001),0);
        }
    }
    return clamp(density*4.0,0.0,1.0);
}

vec3 gemEngravingLight(vec3 point,int panel) {
    vec3 illumination=gemRow(5).xyz;
    vec3 sigma=gemRow(gemPanelRow(panel)+1).xyz;
    for(int light=0;light<3;light++){
        int at=16+5*light;vec3 toLight=gemRow(at).xyz-point;
        float distance=length(toLight);vec3 l=toLight/max(distance,0.000001);
        float cosine=max(dot(gemRow(at+3).xyz,-l),0.0);
        GemHit exitHit=gemExitPanel(point,l,panel);
        if(cosine>0.0&&exitHit.distance>0.0){
            float area=4.0*gemRow(at+1).w*gemRow(at+2).w;
            float transmission=1.0-gemFresnel(dot(l,exitHit.normal),1.0,gemRow(gemPanelRow(panel)).y);
            illumination+=gemRow(at+4).xyz*exp(-sigma*exitHit.distance)*(transmission*cosine*area/(3.14159265359*distance*distance));
        }
    }
    return illumination*gemRow(214).xyz*gemRow(223).w;
}
vec4 gemEngraving(vec3 origin,vec3 ray,int panel,float farLimit) {
    if(panel<2||panel>3||gemRow(212).w<0.5||abs(ray.z)<0.00001){return vec4(0.0,0.0,0.0,1.0);}
    float pixel=2.0*gemRow(3).w/gemRow(8).y;
    vec3 margin=vec3(pixel,pixel,0.0000001);vec3 inverseRay=gemInverseRay(ray);
    int group=218+2*(panel-2);
    if(!gemBox(origin,inverseRay,gemRow(group).xyz,gemRow(group+1).xyz+margin,farLimit)){return vec4(0.0,0.0,0.0,1.0);}
    vec3 result=vec3(0.0);float transmission=1.0;
    vec3 sigma=gemRow(gemPanelRow(panel)+1).xyz;
    for(int line=0;line<64;line++){
        if(float(line)>=gemRow(222).y){break;}
        int at=int(gemRow(222).x)+3*line;
        if(int(gemRow(at+2).x)!=panel){continue;}
        vec4 bounds=gemRow(at);vec4 extent=gemRow(at+1);
        if(!gemBox(origin,inverseRay,bounds.xyz,extent.xyz+margin,farLimit)){continue;}
        int first=int(bounds.w);int size=int(extent.w);
        vec4 firstPose=gemRow(int(gemRow(212).y)+first*2);
        for(int layerVisit=0;layerVisit<3;layerVisit++){
            int layer=layerVisit;if(ray.z>0.0){layer=2-layerVisit;}
            float offset=0.0;float layerWeight=1.0;vec2 shift=vec2(0.0);
            if(layer==1){offset=gemRow(215).x;shift=gemRow(216).xy;layerWeight=gemRow(215).z;}
            if(layer==2){offset=gemRow(215).y;shift=gemRow(216).zw;layerWeight=gemRow(215).w;}
            float t=(firstPose.z-offset*firstPose.w-origin.z)/ray.z;
            if(t<=0.0000001||t>=farLimit){continue;}
            vec3 point=origin+ray*t;
            if(point.x<bounds.x-extent.x-margin.x||point.x>bounds.x+extent.x+margin.x||point.y<bounds.y-extent.y-margin.y||point.y>bounds.y+extent.y+margin.y){continue;}
            float density=0.0;
            // Area-filter the baked density at the actual drawable pixel size.
            // Mips preserve thin-stroke coverage when the panels are reduced.
            float footprint=pixel*0.75/firstPose.w;
            for(int character=0;character<48;character++){
                if(character>=size){break;}
                int cp=int(gemRow(212).y)+2*(first+character);vec4 pose=gemRow(cp);
                vec2 em=(point.xy-pose.xy)/pose.w;
                vec2 unshifted=em-shift*gemRow(213).x;
                // Baselines increase along each line; later glyphs cannot hit.
                if(unshifted.x< -0.17){break;}
                if(unshifted.x>1.13||unshifted.y< -0.29||unshifted.y>1.01){continue;}
                density+=gemEngravedDensity(em,int(gemRow(cp+1).x),shift,footprint,layer);
            }
            if(density>0.0001){
                float alpha=1.0-exp(-gemRow(214).w*layerWeight*min(density,1.0)/max(abs(ray.z),0.12));
                result+=transmission*alpha*exp(-sigma*t)*gemEngravingLight(point,panel);
                transmission*=1.0-alpha;
            }
        }
    }
    return vec4(result,transmission);
}
// CONTROL_ENGRAVING_KERNEL
// FADER_ENGRAVING_KERNEL
// TIMELINE_ENGRAVING_KERNEL
// BOTTOM_PANEL_KERNEL

// Page and track arrows use the existing font mesh inside the mini gem.
vec4 gemPluginNavigationEngraving(vec3 origin,vec3 ray,int medium,float farLimit) {
    int instance=(medium-4)/256;
    if(instance<61||instance>=65||abs(ray.z)<0.00001){return vec4(0.0,0.0,0.0,1.0);}
    int at=gemControlPoseRow(instance);vec4 pose=gemRow(at);
    int glyph=int(gemRow(at+1).z)-32;float font=0.014;
    vec3 local=(origin-pose.xyz)/pose.w;float limit=farLimit/pose.w;
    float pixel=2.0*gemRow(3).w/(gemRow(8).y*pose.w);
    vec3 result=vec3(0.0);float transmission=1.0;
    for(int visit=0;visit<3;visit++){
        int layer=visit;if(ray.z>0.0){layer=2-visit;}
        float t=(-font*(0.10+0.035*float(layer))-local.z)/ray.z;
        if(t<=0.0000001||t>=limit){continue;}
        vec3 point=local+ray*t;vec2 em=point.xy/font+vec2(0.275,0.28);
        float density=gemEngravedDensity(em,glyph,vec2(0.0),pixel*0.75/font,layer);
        float weight=1.0;if(layer==1){weight=gemRow(215).z;}if(layer==2){weight=gemRow(215).w;}
        float alpha=1.0-exp(-gemRow(214).w*weight*min(density,1.0)/max(abs(ray.z),0.12));
        result+=transmission*alpha*gemControlEngravingLight(pose.xyz+point*pose.w,medium,pose.w);
        transmission*=1.0-alpha;
    }
    return vec4(result,transmission);
}
vec4 gemTrackIoEngraving(vec3 origin,vec3 ray,int medium,float farLimit) {
    int instance=(medium-4)/256;
    if(instance<65||instance>=129||abs(ray.z)<0.00001){return vec4(0.0,0.0,0.0,1.0);}
    int at=gemControlPoseRow(instance);vec4 pose=gemRow(at);
    float font=gemRow(at+1).z;
    if(font<=0.0){return vec4(0.0,0.0,0.0,1.0);}
    int length=0;
    for(int i=0;i<12;i++){vec4 codes=gemRow(at+2+i/4);if(codes[i%4]<1.0){break;}length++;}
    vec3 local=(origin-pose.xyz)/pose.w;float limit=farLimit/pose.w;
    float pixel=2.0*gemRow(3).w/(gemRow(8).y*pose.w);
    vec3 result=vec3(0.0);float transmission=1.0;
    for(int visit=0;visit<3;visit++){
        int layer=visit;if(ray.z>0.0){layer=2-visit;}
        float t=(-font*(0.10+0.035*float(layer))-local.z)/ray.z;
        if(t<=0.0000001||t>=limit){continue;}
        vec3 point=local+ray*t;float density=0.0;
        for(int i=0;i<12;i++){
            if(i>=length){break;}
            vec4 codes=gemRow(at+2+i/4);int glyph=int(codes[i%4])-32;
            vec2 em=point.xy/font-vec2((float(i)-float(length)*0.5)*0.55,-0.28);
            density+=gemEngravedDensity(em,glyph,vec2(0.0),pixel*0.75/font,layer);
        }
        float weight=1.0;if(layer==1){weight=gemRow(215).z;}if(layer==2){weight=gemRow(215).w;}
        float alpha=1.0-exp(-gemRow(214).w*weight*min(density,1.0)/max(abs(ray.z),0.12));
        result+=transmission*alpha*gemControlEngravingLight(pose.xyz+point*pose.w,medium,pose.w);
        transmission*=1.0-alpha;
    }
    return vec4(result,transmission);
}
// A zero-volume air / water-film / air membrane. Interference is evaluated at
// three representative visible wavelengths, not a rainbow texture or pigment.
// Uniform450nm thickness replaces the Blender asset's procedural4D field.
// This RGB spectral approximation is intentionally distinct from Cycles.
vec3 gemFilmPolarization(float amplitude,float cosFilm) {
    float power=amplitude*amplitude;
    vec3 phase=vec3(4.0*3.14159265359*1.333*450.0*cosFilm)/vec3(650.0,510.0,475.0);
    vec3 interference=cos(phase);
    return clamp(vec3(2.0*power)*(vec3(1.0)-interference)/
        max(vec3(1.0+power*power)-vec3(2.0*power)*interference,vec3(0.0000001)),vec3(0.0),vec3(1.0));
}
vec3 gemFilmReflectance(float cosine) {
    float ci=clamp(abs(cosine),0.000001,1.0);
    float ct=sqrt(max(0.0,1.0-(1.0-ci*ci)/(1.333*1.333)));
    float rs=(ci-1.333*ct)/(ci+1.333*ct);
    float rp=(1.333*ci-ct)/(1.333*ci+ct);
    return (gemFilmPolarization(rs,ct)+gemFilmPolarization(rp,ct))*0.5;
}
// Straight-through film transmittance for camera composition. Reflections
// remain in gemTrace; source-over display composition is an SDR approximation.
float gemPianoFilmOpacity(vec3 start,vec3 ray){
    vec3 origin=start;vec3 transmission=vec3(1.0);
    for(int layer=0;layer<16;layer++){
        GemHit film=gemBubbleHit(origin,ray,1e20);
        GemHit key=gemPianoHit(origin,ray,film.distance);
        if(key.panel>=0){return 1.0;}
        if(film.panel<0){break;}
        transmission*=vec3(1.0)-gemFilmReflectance(dot(-ray,film.normal));
        origin+=ray*(film.distance+0.0000002);
    }
    return clamp(1.0-dot(transmission,vec3(0.3333333333)),0.0,1.0);
}
// Scene-linear ray transport. Surface branches conserve Fresnel energy; volume
// absorption integrates only the actual travelled interior distance. The eta²
// radiance factor cancels across entrance/exit, including multiple glass objects.
vec3 gemTrace(vec3 origin,vec3 direction,vec2 random) {
    GemRay pending[16];int count=0;
    GemRay current=GemRay(origin,-1,direction,0,vec3(1.0),1.0);
    vec3 result=vec3(0.0);
    for(int event=0;event<64;event++){
        if(float(event)>=gemRow(8).w){break;}
        // Spend the finite ray budget on the strongest remaining throughput.
        // Depth-first transmission could exhaust the budget in a dark branch
        // while the primary reflected card was still waiting on the stack.
        int strongest=-1;float strongestWeight=current.priority;
        for(int candidate=0;candidate<16;candidate++){
            if(candidate>=count){break;}
            float weight=pending[candidate].priority;
            if(weight>strongestWeight){strongest=candidate;strongestWeight=weight;}
        }
        if(strongest>=0){GemRay swapRay=current;current=pending[strongest];pending[strongest]=swapRay;}
        vec4 environment=vec4(0.0,0.0,0.0,1e20);
        if(current.medium<0){environment=gemEnvironment(current.origin,current.direction);}
        GemHit hit=gemNearest(current.origin,current.direction,current.medium,environment.w);
        bool finished=hit.panel<0||hit.distance<=0.0;
        if(finished&&current.medium<0){
            vec3 environmentRadiance=environment.xyz;
            if(environmentRadiance.x<0.0){environmentRadiance=gemFloor(current.origin+current.direction*environment.w,-current.direction);}
            result+=current.weight*environmentRadiance;
        }
        if(!finished&&hit.panel>=950000&&hit.panel<950132){
            vec3 point=current.origin+current.direction*hit.distance;
            vec3 color=gemRow(4041232+(hit.panel-950000)*3+2).xyz;
            vec3 radiance=gemOpaque(point,-current.direction,hit.normal,color,0.24,false);
            if(gemMax(color)<0.02){radiance*=0.045;}
            result+=current.weight*radiance;finished=true;
        }
        bool custom=hit.panel>=200000&&hit.panel<600000;
        if(!finished&&custom){
            int panel=userPanel(hit.panel);int at=userTriangle(hit.panel);int pose=32+40*panel;
            int material=int(gemRow(pose).w)+int(gemRow(at).w)*2;
            vec3 point=current.origin+current.direction*hit.distance;
            vec3 local=(point-gemRow(pose+1).xyz)/gemRow(pose+2).xyz;
            local=userSurfacePoint(at,local);
            point=gemRow(pose+1).xyz+local*gemRow(pose+2).xyz;
            vec3 color=userColor(panel,at,local).xyz;vec4 extra=gemRow(material+1);
            vec3 radiance=color;
            if(gemRow(at+1).w<0.5){
                radiance=gemOpaqueSurface(point,-current.direction,hit.normal,color,gemRow(material).w,false,extra.w,0.0,0.0);
            }
            radiance+=extra.xyz;
            if(abs(point.z)<0.0001&&hit.normal.z>0.5){
                // Opaque authors can use bright or dark materials under the
                // same studio. Select contrasting ink without altering their
                // surface color or the built-in gem's volume engraving.
                vec3 ink=gemRow(214).xyz*2.0;
                if(dot(radiance,vec3(0.2126,0.7152,0.0722))>0.18){ink=vec3(0.002);}
                radiance=mix(radiance,ink,userSurfaceLabel(point,panel));
            }
            result+=current.weight*radiance;finished=true;
        }
        bool sphere=hit.panel>=90000&&hit.panel<90064;
        if(!finished&&sphere){
            vec3 point=current.origin+current.direction*hit.distance;
            result+=current.weight*gemAudioSphereShade(point,-current.direction,hit.normal,hit.panel);
            finished=true;
        }
        if(!finished&&hit.panel==719997){
            vec3 point=current.origin+current.direction*hit.distance;
            result+=current.weight*gemOpaque(point,-current.direction,hit.normal,gemBottomSurfaceColor(point),0.35,false);finished=true;
        }
        if(!finished&&hit.panel==719999){
            vec3 point=current.origin+current.direction*hit.distance;
            result+=current.weight*gemOpaque(point,-current.direction,hit.normal,vec3(0.75,0.5,0.08),0.24,false);
            finished=true;
        }
        if(!finished&&hit.panel==719998){
            vec3 point=current.origin+current.direction*hit.distance;
            result+=current.weight*gemOpaque(point,-current.direction,hit.normal,vec3(0.08,0.35,0.85),0.24,false);
            finished=true;
        }
        bool membrane=hit.panel>=100000&&hit.panel<102048;
        if(!finished&&membrane){
            vec3 point=current.origin+current.direction*hit.distance;
            vec3 n=hit.normal;if(dot(n,current.direction)>0.0){n=-n;}
            vec2 xi=fract(random+vec2(float(current.depth)*0.754877666,float(event)*0.569840296));
            vec3 micro=gemMicroNormal(n,-current.direction,0.012,xi);
            vec3 reflected=reflect(current.direction,micro);
            // The enclosing volume is air: transmission keeps its direction.
            vec3 transmitted=current.direction;
            vec3 reflectance=gemFilmReflectance(dot(-current.direction,micro));
            vec3 reflectWeight=current.weight*reflectance;
            if(dot(reflected,n)<=0.0){reflectWeight=vec3(0.0);}
            vec3 transmitWeight=current.weight*(vec3(1.0)-reflectance);
            int noteIndex=hit.panel-100000;
            float noteWidth=gemRow(int(gemRow(237).x)+noteIndex).z;
            int depth=current.depth+1;
            // Move representably in world float32 coordinates. Subpixel /
            // sub-ULP features remain stored but cannot promise resolved optics.
            float epsilon=max(max(0.000000001,gemMax(abs(point))*0.0000002),
                min(noteWidth,gemRow(234).y*0.6)*0.00001);
            float reflectPriority=gemMax(reflectWeight);float transmitPriority=gemMax(transmitWeight);
            bool retainReflection=float(depth)<gemRow(8).z&&reflectPriority>0.00005;
            bool retainTransmission=float(depth)<gemRow(8).z&&transmitPriority>0.00005;
            if(retainReflection){
                vec4 terminal=gemTerminal(point+reflected*epsilon,reflected);
                if(terminal.w>0.5){result+=reflectWeight*terminal.xyz;retainReflection=false;}
            }
            if(retainTransmission){
                vec4 terminal=gemTerminal(point+transmitted*epsilon,transmitted);
                if(terminal.w>0.5){result+=transmitWeight*terminal.xyz;retainTransmission=false;}
            }
            GemRay reflection=GemRay(point+reflected*epsilon,-1,reflected,depth,reflectWeight,reflectPriority);
            GemRay transmission=GemRay(point+transmitted*epsilon,-1,transmitted,depth,transmitWeight,transmitPriority);
            if(retainReflection&&retainTransmission){
                if(reflectPriority>transmitPriority){current=reflection;if(count<16){pending[count]=transmission;count++;}}
                else{current=transmission;if(count<16){pending[count]=reflection;count++;}}
            }else if(retainReflection){current=reflection;}
            else if(retainTransmission){current=transmission;}
            else{finished=true;}
        }
        if(!finished&&!membrane){
            int base=gemPanelRow(hit.panel);vec4 material=gemRow(base);
            bool inside=current.medium>=0;
            if(inside&&current.medium<4){
                vec4 engraving=gemEngraving(current.origin,current.direction,current.medium,hit.distance);
                result+=current.weight*engraving.xyz;current.weight*=engraving.w;
            }
            if(inside&&current.medium>=800000&&current.medium<853760){
                vec4 engraving=gemMixerEngraving(current.origin,current.direction,current.medium,hit.distance);
                result+=current.weight*engraving.xyz;current.weight*=engraving.w;
            }
            if(inside&&current.medium>=4&&current.medium<800000){
                vec4 engraving=gemControlEngraving(current.origin,current.direction,current.medium,hit.distance);
                result+=current.weight*engraving.xyz;current.weight*=engraving.w;
                vec4 navigation=gemPluginNavigationEngraving(current.origin,current.direction,current.medium,hit.distance);
                result+=current.weight*navigation.xyz;current.weight*=navigation.w;
            }
            if(inside&&current.medium>=16644&&current.medium<32772){
                vec4 trackIo=gemTrackIoEngraving(current.origin,current.direction,current.medium,hit.distance);
                result+=current.weight*trackIo.xyz;current.weight*=trackIo.w;
            }
            if(inside&&current.medium>=700000&&current.medium<704096){
                vec4 engraving=gemTimelineEngraving(current.origin,current.direction,current.medium,hit.distance);
                result+=current.weight*engraving.xyz;current.weight*=engraving.w;
            }
            if(inside){float opticalDistance=hit.distance/gemObjectScale(hit.panel);
                if(current.medium>=800000&&current.medium<853760){opticalDistance=hit.distance*length(current.direction/gemMixerScale((current.medium-800000)/256));}
                current.weight*=exp(-gemRow(base+1).xyz*opticalDistance);}
            vec3 point=current.origin+current.direction*hit.distance;
            vec3 n=hit.normal;if(inside){n=-n;}
            if(material.w<0.0001){
                vec3 opaque=vec3(0.0);
                vec3 surfaceColor=gemRow(base+4).xyz;vec3 emission=gemRow(base+5).xyz;
                vec4 waveform=gemTimelineWaveform(point,n,hit.panel);
                if(waveform.w>0.5){surfaceColor=waveform.xyz;}
                vec4 meterColor=gemMixerMeterColor(point,hit.panel);
                if(meterColor.w>0.5){surfaceColor=meterColor.xyz;emission=surfaceColor*0.85;}
                int instance=(hit.panel-4)/256;
                if(instance>=13&&instance<29){
                    float flags=gemRow(gemControlPoseRow(instance)+1).z;
                    if(flags>=4.0){surfaceColor*=0.45;flags-=4.0;}
                    if((hit.panel-4)%256==0){surfaceColor=mix(surfaceColor,vec3(0.18,0.52,0.95),flags*0.14);}
                }
                if((hit.panel>=4+10*256&&hit.panel<4+13*256)||(hit.panel>=4+29*256&&hit.panel<4+61*256)){
                    opaque=gemOpaqueSurface(point,-current.direction,n,surfaceColor,material.z,false,gemRow(base+5).w,gemRow(base+2).w,gemRow(base+3).w);
                }else{
                    opaque=gemOpaque(point,-current.direction,n,surfaceColor,material.z,false);
                }
                result+=current.weight*(opaque+emission);
                finished=true;
            }else{
            float etaI=gemPick(1.0,material.y,inside);float etaT=gemPick(material.y,1.0,inside);
            vec2 xi=fract(random+vec2(float(current.depth)*0.754877666,float(event)*0.569840296));
            vec3 micro=gemMicroNormal(n,-current.direction,material.z,xi);
            float f=gemFresnel(dot(-current.direction,micro),etaI,etaT);
            vec3 reflected=reflect(current.direction,micro);
            vec3 transmitted=refract(current.direction,micro,etaI/etaT);
            float alpha=max(material.z*material.z,0.00001);
            current.weight/=gemEnergy(hit.panel,inside,dot(-current.direction,n));
            vec3 reflectWeight=current.weight*f*gemSampleMask(dot(-current.direction,n),dot(reflected,n),alpha);
            if(dot(reflected,n)<=0.0){reflectWeight=vec3(0.0);}
            vec3 transmitWeight=current.weight*((1.0-f)*material.w*(etaI*etaI)/(etaT*etaT))*gemRow(base+4).xyz;
            if(dot(transmitted,n)>=0.0){transmitWeight=vec3(0.0);}
            transmitWeight*=gemSampleMask(dot(-current.direction,n),dot(transmitted,n),alpha);
            int transmittedMedium=hit.panel;if(inside){transmittedMedium=-1;}
            int depth=current.depth+1;
            float epsilon=max(0.0000002,min(min(gemRow(base+3).x,gemRow(base+3).y),gemRow(base+3).z)*0.00001);
            bool canContinue=float(depth)<gemRow(8).z;
            float reflectPriority=gemMax(reflectWeight);float transmitPriority=gemMax(transmitWeight);
            bool retainReflection=canContinue&&reflectPriority>0.00005;
            bool retainTransmission=canContinue&&transmitPriority>0.00005;
            if(retainReflection&&current.medium<0){
                vec4 terminal=gemTerminal(point+reflected*epsilon,reflected);
                if(terminal.w>0.5){result+=reflectWeight*terminal.xyz;retainReflection=false;}
            }
            if(retainTransmission&&transmittedMedium<0){
                vec4 terminal=gemTerminal(point+transmitted*epsilon,transmitted);
                if(terminal.w>0.5){result+=transmitWeight*terminal.xyz;retainTransmission=false;}
            }
            GemRay reflection=GemRay(point+reflected*epsilon,current.medium,reflected,depth,reflectWeight,reflectPriority);
            GemRay transmission=GemRay(point+transmitted*epsilon,transmittedMedium,transmitted,depth,transmitWeight,transmitPriority);
            if(retainReflection&&retainTransmission){
                if(reflectPriority>transmitPriority){
                    current=reflection;if(count<16){pending[count]=transmission;count++;}
                }else{
                    current=transmission;if(count<16){pending[count]=reflection;count++;}
                }
            }else if(retainReflection){current=reflection;}
            else if(retainTransmission){current=transmission;}
            else{finished=true;}
            }
        }
        if(finished){
            if(count==0){break;}count--;current=pending[count];
        }
    }
    // Work remaining at the bounded path budget is deliberately not fabricated.
    return result;
}
// Original OCIO 57-cube, red-fastest. Tetrahedral interpolation matches the
// extracted AgX shader; trilinear interpolation is visibly different in saturated HDR.
vec3 gemLut(vec3 coordinate) {
    int size=int(gemRow(7).y);int base=int(gemRow(7).x);
    vec3 p=clamp(coordinate,vec3(0.0),vec3(1.0))*float(size-1);
    ivec3 lo=ivec3(floor(p));ivec3 hi=min(lo+ivec3(1),ivec3(size-1));vec3 f=fract(p);
    ivec3 first=ivec3(hi.x,lo.y,lo.z);ivec3 second=ivec3(hi.x,hi.y,lo.z);
    float large=f.x;float middle=f.y;float small=f.z;
    if(f.x>=f.y){
        if(f.y>=f.z){
            first=ivec3(hi.x,lo.y,lo.z);second=ivec3(hi.x,hi.y,lo.z);
        }else if(f.x>=f.z){
            first=ivec3(hi.x,lo.y,lo.z);second=ivec3(hi.x,lo.y,hi.z);
            middle=f.z;small=f.y;
        }else{
            first=ivec3(lo.x,lo.y,hi.z);second=ivec3(hi.x,lo.y,hi.z);
            large=f.z;middle=f.x;small=f.y;
        }
    }else{
        if(f.y<=f.z){
            first=ivec3(lo.x,lo.y,hi.z);second=ivec3(lo.x,hi.y,hi.z);
            large=f.z;middle=f.y;small=f.x;
        }else if(f.x>=f.z){
            first=ivec3(lo.x,hi.y,lo.z);second=ivec3(hi.x,hi.y,lo.z);
            large=f.y;middle=f.x;small=f.z;
        }else{
            first=ivec3(lo.x,hi.y,lo.z);second=ivec3(lo.x,hi.y,hi.z);
            large=f.y;middle=f.z;small=f.x;
        }
    }
    vec3 v0=gemRow(base+(lo.z*size+lo.y)*size+lo.x).xyz;
    vec3 v1=gemRow(base+(first.z*size+first.y)*size+first.x).xyz;
    vec3 v2=gemRow(base+(second.z*size+second.y)*size+second.x).xyz;
    vec3 v3=gemRow(base+(hi.z*size+hi.y)*size+hi.x).xyz;
    return v0*(1.0-large)+v1*(large-middle)+v2*(middle-small)+v3*small;
}
float gemDisplayChannel(float value) {
    float linear=pow(max(value,0.0),2.4);
    if(linear>0.00303993467){return pow(linear,0.416666657)*1.05499995-0.0549999997;}
    return linear*12.9232101;
}
vec3 gemDisplay(vec3 radiance) {
    vec3 linear=radiance*exp2(gemRow(0).w);
    // These constants are the original Blender OCIO GPU operations, exported in
    // GemPanels_AgX_reference.glsl, with the LUT operation above kept unchanged.
    vec3 working=linear.x*vec3(0.55937113331007726,0.07622070548350629,0.065526711778770175)
                +linear.y*vec3(0.3047833466230091,0.78797177010284625,0.16454675460372409)
                +linear.z*vec3(0.13584556048047705,0.13580746617472153,0.76992650165037446);
    vec3 coordinate=log2(max(working,vec3(1.17549435e-38)))*0.0400000028+vec3(0.498957247);
    vec3 encoded=gemLut(coordinate);
    return vec3(gemDisplayChannel(encoded.x),gemDisplayChannel(encoded.y),gemDisplayChannel(encoded.z));
}
// PIANO_ROLL_OVERLAY_KERNEL
vec4 workstationShade(vec2 screen) {
    vec3 result=vec3(0.0);float samples=clamp(gemRow(0).z,1.0,4.0);
    vec4 camera=gemRow(1);vec4 right=gemRow(2);vec4 up=gemRow(3);vec4 forward=gemRow(4);
    vec2 resolution=max(gemRow(8).xy,vec2(1.0));
    // Fixed stratification is stable while panels animate. No frame-random flicker.
    for(int sampleIndex=0;sampleIndex<4;sampleIndex++){
        if(float(sampleIndex)>=samples){break;}
        vec2 xi=fract(vec2(0.375,0.125)+vec2(0.5,0.75)*float(sampleIndex));
        vec2 pixel=screen+(xi-vec2(0.5))*2.0/resolution;
        vec3 offset=right.xyz*(pixel.x*right.w)+up.xyz*(pixel.y*up.w);
        vec3 origin=camera.xyz+offset;vec3 direction=gemSafeUnit(forward.xyz);
        if(camera.w>0.5){origin=camera.xyz;direction=gemSafeUnit(forward.xyz*forward.w+offset);}
        vec2 random=fract(vec2(0.324717957,0.754877666)+vec2(0.618033989,0.414213562)*float(sampleIndex));
        result+=gemTrace(origin,direction,random);
    }
    vec3 display=gemDisplay(result/samples);if(gemPianoActive()){display=gemPianoOverlay(display,screen);}
    if(gemPianoActive()&&gemRow(9).z>0.5&&gemPianoField(screen)){
        vec3 origin=camera.xyz+right.xyz*(screen.x*right.w)+up.xyz*(screen.y*up.w);
        vec3 direction=gemSafeUnit(forward.xyz);
        float opacity=gemPianoFilmOpacity(origin,direction);
        vec3 backdrop=gemDisplay(gemRow(6).xyz*gemRow(5).xyz);
        return vec4(max(display-backdrop*(1.0-opacity),vec3(0.0))/max(opacity,0.0001),opacity);
    }
    return vec4(display,1.0);
}

// Camera passthrough changes only directly visible empty background pixels.
bool gemBackdropVisible(vec2 screen) {
    if(gemPianoActive()&&!gemPianoField(screen)){return false;}
    vec4 camera=gemRow(1);vec4 right=gemRow(2);vec4 up=gemRow(3);vec4 forward=gemRow(4);
    vec3 offset=right.xyz*(screen.x*right.w)+up.xyz*(screen.y*up.w);
    vec3 origin=camera.xyz+offset;vec3 direction=gemSafeUnit(forward.xyz);
    if(camera.w>0.5){origin=camera.xyz;direction=gemSafeUnit(forward.xyz*forward.w+offset);}
    GemHit hit=gemNearest(origin,direction,-1,1e20);return hit.panel<0;
}
