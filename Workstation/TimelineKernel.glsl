// Authored meshes with uniform transforms. The tape retains its thickness and
// perforations: arbitrary duration repeats/crops the existing Long geometry.
// It is a presentation tiling approximation; no fake source waveform is drawn.
// Header248..255; three rows per instance; 16 material IDs per instance.
GemHit gemTimelineMesh(vec3 origin,vec3 ray,int instance,int medium,float farLimit) {
    GemHit hit=GemHit(farLimit,vec3(0.0),-1);
    int at=gemTimelineInstanceRow(instance);
    vec4 pose=gemRow(at);vec4 tree=gemRow(at+1);vec4 size=gemRow(at+2);
    if(tree.w< -0.5){
        int panel=700000+instance*16;
        if(medium>=0&&medium!=panel){return hit;}
        vec3 local=(origin-pose.xyz)/pose.w;
        float near=-1e20;float far=1e20;vec3 entering=vec3(0.0);vec3 leaving=vec3(0.0);bool miss=false;
        for(int planeIndex=0;planeIndex<26;planeIndex++){
            vec4 plane=gemBarPlane(planeIndex,size.xy);float denominator=dot(plane.xyz,ray);
            float distance=plane.w-dot(plane.xyz,local);
            if(abs(denominator)<0.0000001){if(distance< -0.0000001){miss=true;}}
            else{float t=distance/denominator;
                if(denominator<0.0&&t>near){near=t;entering=plane.xyz;}
                if(denominator>0.0&&t<far){far=t;leaving=plane.xyz;}
            }
            if(far<near){miss=true;}if(miss){break;}
        }
        float t=near;vec3 normal=entering;if(medium==panel){t=far;normal=leaving;}
        float distance=t*pose.w;vec3 point=origin+ray*distance;vec4 clip=gemRow(249);
        if(!miss&&distance>0.0000001&&distance<farLimit&&point.x>=clip.x&&point.x<=clip.z&&point.y>=clip.y&&point.y<=clip.w&&gemMobileBackdropAllows(point.xy)){
            hit=GemHit(distance,normal,panel);
        }
        return hit;
    }
    bool tape=tree.w>0.5;
    vec3 local=(origin-pose.xyz)/pose.w;vec3 inverse=gemInverseRay(ray);
    vec3 lo=vec3(-size.x*0.5,-size.y*0.5,-size.z);
    vec3 hi=vec3(size.x*0.5,size.y*0.5,0.000000001);
    if(tape){lo=vec3(0.0,-size.y*0.5,0.0);hi=vec3(size.x,size.y*0.5,size.z);}
    vec3 intervalA=(lo-local)*inverse;vec3 intervalB=(hi-local)*inverse;
    vec3 lower=min(intervalA,intervalB);vec3 upper=max(intervalA,intervalB);
    float near=max(max(lower.x,lower.y),max(lower.z,0.0));
    float far=min(min(upper.x,upper.y),min(upper.z,farLimit/pose.w));
    if(far<near){return hit;}
    int firstTile=0;int lastTile=0;
    float phase=0.0;if(tape){phase=size.w;}
    if(tape){
        float x0=local.x+phase+ray.x*near;float x1=local.x+phase+ray.x*far;
        firstTile=max(0,int(floor(max(0.0,min(x0,x1)-0.0000001)/0.22)));
        lastTile=max(firstTile,int(floor(min(size.x+phase,max(x0,x1)+0.0000001)/0.22)));
    }
    for(int tileVisit=0;tileVisit<128;tileVisit++){
        int tile=firstTile+tileVisit;if(tile>lastTile){break;}
        vec3 shifted=local+vec3(phase-float(tile)*0.22,0.0,0.0);
        int node=int(tree.x);
        for(int visit=0;visit<16384;visit++){
            if(node>=int(tree.y)){break;}
            vec4 low=gemRow(node);vec4 high=gemRow(node+1);
            if(!gemBox(shifted,inverse,(low.xyz+high.xyz)*0.5,(high.xyz-low.xyz)*0.5+vec3(0.00000001),hit.distance/pose.w)){
                node=int(low.w);continue;
            }
            int count=int(gemRow(node+2).x);
            if(count==0){node+=3;continue;}
            for(int triangle=0;triangle<4;triangle++){
                if(triangle>=count){break;}
                int source=int(high.w)+triangle*6;
                vec4 a=gemRow(source);vec3 e1=gemRow(source+1).xyz;vec3 e2=gemRow(source+2).xyz;
                if(gemRow(source+1).w<0.5){e1-=a.xyz;e2-=a.xyz;}
                int panel=700000+instance*16+int(a.w);
                if(medium>=700000&&panel!=medium){continue;}
                vec3 h=cross(ray,e2);float determinant=dot(e1,h);
                if(abs(determinant)<0.000000000001){continue;}
                vec3 delta=shifted-a.xyz;float u=dot(delta,h)/determinant;
                if(u<0.0||u>1.0){continue;}
                vec3 q=cross(delta,e1);float v=dot(ray,q)/determinant;
                if(v<0.0||u+v>1.0){continue;}
                float distance=dot(e2,q)/determinant*pose.w;
                if(distance<=0.0000001||distance>=hit.distance){continue;}
                vec3 normal=gemControlNormal(source,u,v);
                if(medium<0&&dot(normal,ray)>=0.0){continue;}
                if(medium>=0&&dot(normal,ray)<=0.0){continue;}
                vec3 point=origin+ray*distance;vec4 clip=gemRow(249);if(instance>=192){clip=gemRow(int(gemRow(255).x));}
                if(point.x<clip.x||point.x>clip.z||point.y<clip.y||point.y>clip.w){continue;}
                if(instance<192&&!gemMobileBackdropAllows(point.xy)){continue;}
                float localX=local.x+ray.x*(distance/pose.w);
                if(tape&&(localX<0.0||localX>size.x)){continue;}
                hit=GemHit(distance,normal,panel);
            }
            node=int(low.w);
        }
    }
    return hit;
}

GemHit gemTimelineHit(vec3 origin,vec3 ray,int medium,float farLimit) {
    GemHit hit=gemBarHit(origin,ray,medium,farLimit);
    int count=int(gemRow(248).x);
    int bottomCount=int(gemRow(255).z);
    if(count==0&&bottomCount==0){return hit;}
    if(medium>=700000&&medium<704096){return gemTimelineMesh(origin,ray,(medium-700000)/16,medium,farLimit);}
    vec4 clip=gemRow(249);vec3 inverse=gemInverseRay(ray);
    if(bottomCount>0){vec4 bottom=gemRow(int(gemRow(255).x));if(count==0){clip=bottom;}else{clip=vec4(min(clip.xy,bottom.xy),max(clip.zw,bottom.zw));}}
    vec3 low=vec3(clip.xy,-0.000001);vec3 high=vec3(clip.zw,0.3);
    if(bottomCount>0){int base=int(gemRow(255).x);low.z=min(low.z,gemRow(base+8).z);high.z=max(high.z,gemRow(base+9).z);}
    if(!gemBox(origin,inverse,(low+high)*0.5,(high-low)*0.5,farLimit)){return hit;}
    // Later document clips are on top when their elapsed-time regions overlap.
    for(int candidate=0;candidate<256;candidate++){
        if(candidate>=count){break;}
        int instance=count-1-candidate;
        GemHit next=gemTimelineMesh(origin,ray,instance,-1,hit.distance);
        if(next.panel>=0){hit=next;}
    }
    for(int index=0;index<12;index++){if(index>=bottomCount){break;}GemHit next=gemTimelineMesh(origin,ray,192+index,-1,hit.distance);if(next.panel>=0){hit=next;}}
    return hit;
}

GemHit gemTimelineMarkerBox(vec3 origin,vec3 ray,vec3 center,vec3 extent,float farLimit) {
    GemHit hit=GemHit(farLimit,vec3(0.0),-1);
    vec3 a=(center-extent-origin)*gemInverseRay(ray);vec3 b=(center+extent-origin)*gemInverseRay(ray);
    vec3 lo=min(a,b);vec3 hi=max(a,b);float near=max(max(lo.x,lo.y),lo.z);float far=min(min(hi.x,hi.y),hi.z);
    if(near>0.0000001&&far>=near&&near<farLimit){
        vec3 normal=vec3(0.0,0.0,-sign(ray.z));
        if(lo.x>=lo.y&&lo.x>=lo.z){normal=vec3(-sign(ray.x),0.0,0.0);}
        else if(lo.y>=lo.z){normal=vec3(0.0,-sign(ray.y),0.0);}
        hit=GemHit(near,normal,719999);
    }
    return hit;
}

// Keep the bottom-panel plugin drop marker on the un-clipped box helper.
GemHit gemTimelineBackdropMarkerBox(vec3 origin,vec3 ray,vec3 center,vec3 extent,float farLimit) {
    GemHit hit=gemTimelineMarkerBox(origin,ray,center,extent,farLimit);
    if(hit.panel>=0&&!gemMobileBackdropAllows((origin+ray*hit.distance).xy)){return GemHit(farLimit,vec3(0.0),-1);}
    return hit;
}

// The rail/ticks and playhead are small extruded solids, using the same opaque
// studio material. The rail exposes the previously invisible ruler seek area.
GemHit gemTimelinePlayhead(vec3 origin,vec3 ray,float farLimit) {
    GemHit hit=GemHit(farLimit,vec3(0.0),-1);vec4 mark=gemRow(252);
    float pixel=gemRow(253).x;
    if(mark.w>0.5){
        vec3 center=vec3(mark.x,(mark.y+mark.z)*0.5,0.015);
        vec3 extent=vec3(pixel*0.75,(mark.z-mark.y)*0.5,0.0005);
        hit=gemTimelineBackdropMarkerBox(origin,ray,center,extent,hit.distance);
    }
    int barBase=45744;
    if(gemRow(barBase+8).z==10532.0&&gemRow(barBase+8).w>0.5){
        vec4 guide=gemRow(barBase+6);vec4 control=gemRow(barBase+7);vec4 bounds=gemRow(barBase+9);
        if(gemRow(barBase+8).y>0.5&&control.z>0.0){
            GemHit grab=gemTimelineBackdropMarkerBox(origin,ray,vec3(control.w,control.y,0.020),vec3(control.z,control.z,0.001),hit.distance);
            if(grab.panel>=0){hit=grab;}
        }
        if(control.x>0.5&&bounds.w>0.5){
            for(int side=0;side<2;side++){
                float x=guide.x;if(side==1){x=guide.y;}
                if(x>=bounds.x&&x<=bounds.y){
                    GemHit edge=gemTimelineBackdropMarkerBox(origin,ray,vec3(x,(guide.z+guide.w)*0.5,0.015),vec3(pixel*0.5,abs(guide.z-guide.w)*0.5,0.0005),hit.distance);
                    if(edge.panel>=0){hit=edge;}
                }
            }
        }
        // Original gem faces replace the thin opaque stand-in rail.
        return hit;
    }
    vec4 ruler=gemRow(250);
    if(ruler.w<0.5){return hit;}
    vec3 rulerCenter=vec3((ruler.x+ruler.y)*0.5,ruler.z+pixel*3.0,0.015);
    vec3 rulerExtent=vec3((ruler.y-ruler.x)*0.5+pixel*0.6,pixel*5.0,0.000501);
    if(!gemBox(origin,gemInverseRay(ray),rulerCenter,rulerExtent,hit.distance)){return hit;}
    vec3 center=vec3((ruler.x+ruler.y)*0.5,ruler.z,0.015);
    vec3 extent=vec3((ruler.y-ruler.x)*0.5,pixel*0.75,0.0005);
    GemHit rail=gemTimelineBackdropMarkerBox(origin,ray,center,extent,hit.distance);
    if(rail.panel>=0){hit=rail;}
    vec4 ticks=gemRow(254);
    for(int i=0;i<32;i++){
        if(i>=int(ticks.w)){break;}
        float x=ticks.x+float(i)*ticks.y;
        if(x<ruler.x-pixel*0.01||x>ruler.y+pixel*0.01){continue;}
        vec3 tickCenter=vec3(x,ruler.z+pixel*3.0,0.015);
        vec3 tickExtent=vec3(pixel*0.6,ticks.z,0.0005);
        GemHit tick=gemTimelineBackdropMarkerBox(origin,ray,tickCenter,tickExtent,hit.distance);
        if(tick.panel>=0){hit=tick;}
    }
    return hit;
}
