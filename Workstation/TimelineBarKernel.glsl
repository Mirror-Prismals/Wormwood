// Same 26 halfspaces as PanelLayout::optical_shape, evaluated in normalized
// source units. Aspect changes rebuild pavilion planes; authored gems are not
// stretched. Existing material, Fresnel, GGX, absorption and AgX stay unchanged.
vec4 gemBarPlane(int index,vec2 halfSize) {
    vec4 p=vec4(0.0,0.0,1.0,0.30);
    if(index==1){p=vec4(0.0,0.0,-1.0,0.85);}
    if(index>=2){
        int ring=(index-2)/8;int side=(index-2)%8;
        float angle=float(side)*0.7853981633974483;if(ring==2){angle+=0.39269908169872414;}
        float x=cos(angle);float y=sin(angle);
        float extent=abs(x)*halfSize.x+abs(y)*halfSize.y-0.18*min(abs(x),abs(y));
        float slope=extent*0.86/(0.85-0.07);float z=0.0;float distance=extent;
        if(ring==1){z=0.31/0.30;}
        if(ring==2){z=-slope;distance=extent+slope*0.07;}
        p=vec4(x,y,z,distance);
    }
    float magnitude=length(p.xyz);return vec4(p.xyz/magnitude,(p.w-0.30*p.z)/magnitude);
}
GemHit gemBarHit(vec3 origin,vec3 ray,int medium,float farLimit) {
    GemHit hit=GemHit(farLimit,vec3(0.0),-1);int base=45744;
    if(gemRow(base+8).z!=10532.0||gemRow(base+8).w<0.5){return hit;}
    for(int i=0;i<3;i++){
        int panel=720000+i;if(medium>=0&&medium!=panel){continue;}
        vec4 pose=gemRow(base+i);vec4 size=gemRow(base+3+i);
        if(size.w<0.5||pose.w<=0.0){continue;}
        vec3 local=(origin-pose.xyz)/pose.w;
        float near=-1e20;float far=1e20;vec3 entering=vec3(0.0);vec3 leaving=vec3(0.0);bool miss=false;
        for(int j=0;j<26;j++){
            vec4 plane=gemBarPlane(j,size.xy);float denominator=dot(plane.xyz,ray);
            float distance=plane.w-dot(plane.xyz,local);
            if(abs(denominator)<0.0000001){if(distance< -0.0000001){miss=true;}}
            else{float t=distance/denominator;
                if(denominator<0.0&&t>near){near=t;entering=plane.xyz;}
                if(denominator>0.0&&t<far){far=t;leaving=plane.xyz;}
            }
            if(far<near){miss=true;}if(miss){break;}
        }
        float t=near;vec3 normal=entering;if(medium==panel){t=far;normal=leaving;}
        float worldDistance=t*pose.w;
        if(!miss&&worldDistance>0.0000001&&worldDistance<hit.distance){
            if(i<2&&!gemMobileBackdropAllows((origin+ray*worldDistance).xy)){continue;}
            hit=GemHit(worldDistance,normal,panel);
        }
    }
    return hit;
}
