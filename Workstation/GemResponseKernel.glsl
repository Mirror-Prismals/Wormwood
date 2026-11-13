// Four baked ruby transport caps. Intrinsic paths are replaced only for the
// fixed orthographic primary view. Exterior rays rejoin the existing work queue;
// the live scene, floor, panels and lighting still participate in rendering.
// Descriptor rows: layout, bounds, pose, tree, three materials, reserved,
// three light gains, three SSLE parameters, gain, reserved.
bool gemResponseDifferent(vec4 a,vec4 b) {
    return a.x!=b.x||a.y!=b.y||a.z!=b.z||a.w!=b.w;
}
int gemResponseDescriptor(vec3 origin,vec3 direction,GemHit hit) {
    int base=3500544;
    if(gemRow(base).x!=731.0||gemRow(0).z!=2.0||gemRow(1).w!=0.0||
       (direction.x!=0.0||direction.y!=0.0||direction.z!=-1.0)||gemRow(8).z!=16.0||gemRow(8).w!=64.0||
       gemRow(9).x!=3.0||gemRow(244).y<0.5){return -1;}
    if(hit.panel<4||hit.panel>=1028||hit.distance<=0.0){return -1;}
    int instance=(hit.panel-4)/256;
    int descriptor=base+instance*983056;
    vec4 pose=gemRow(192+2*instance);
    if(gemRow(descriptor).x!=731.0||gemResponseDifferent(gemRow(193+2*instance),gemRow(descriptor+3))||
       pose.w<=0.0){return -1;}
    float footprint=2.0*gemRow(3).w/(max(gemRow(8).y,1.0)*pose.w);
    float referenceFootprint=gemRow(descriptor+15).x;
    if(footprint<referenceFootprint*0.5||footprint>referenceFootprint*2.0){return -1;}
    if(hit.panel!=4+instance*256+instance){return -1;}
    int material=gemPanelRow(hit.panel);
    if(gemResponseDifferent(gemRow(material),gemRow(descriptor+4))||gemResponseDifferent(gemRow(material+1),gemRow(descriptor+5))||
       gemResponseDifferent(gemRow(material+2),gemRow(descriptor+6))||gemResponseDifferent(gemRow(214),gemRow(descriptor+11))||
       gemResponseDifferent(gemRow(215),gemRow(descriptor+12))||gemResponseDifferent(gemRow(216),gemRow(descriptor+13))||
       gemRow(223).w!=gemRow(descriptor+14).x){return -1;}
    return descriptor;
}
vec2 gemResponseLocation(int descriptor,vec3 origin,int instance) {
    vec4 pose=gemRow(192+2*instance);vec4 bounds=gemRow(descriptor+1);
    vec2 xy=(origin.xy-pose.xy)/pose.w;
    vec2 uv=vec2((xy.x-bounds.x)/(bounds.z-bounds.x),(bounds.w-xy.y)/(bounds.w-bounds.y));
    return uv*128.0-vec2(0.5);
}
int gemResponseAddress(int descriptor,vec2 location,int sampleIndex,int corner) {
    int x=int(floor(location.x));int y=int(floor(location.y));
    if(corner==1||corner==3){x++;}if(corner>=2){y++;}
    return descriptor+16+(clamp(y,0,127)*128+clamp(x,0,127))*60+sampleIndex*30;
}
vec4 gemResponseRead(int descriptor,vec2 location,int sampleIndex,int offset) {
    vec2 f=fract(location);
    vec4 a=gemRow(gemResponseAddress(descriptor,location,sampleIndex,0)+offset);
    vec4 b=gemRow(gemResponseAddress(descriptor,location,sampleIndex,1)+offset);
    vec4 c=gemRow(gemResponseAddress(descriptor,location,sampleIndex,2)+offset);
    vec4 d=gemRow(gemResponseAddress(descriptor,location,sampleIndex,3)+offset);
    return mix(mix(a,b,f.x),mix(c,d,f.x),f.y);
}
bool gemResponseCompatible(int descriptor,vec2 location,int sampleIndex) {
    if(location.x<0.0||location.y<0.0||location.x>127.0||location.y>127.0){return false;}
    int first=gemResponseAddress(descriptor,location,sampleIndex,0);
    // Interpolate corresponding paths only inside one smooth facet response.
    // At path/silhouette discontinuities use the original transport calculation.
    for(int corner=0;corner<4;corner++){
        int at=gemResponseAddress(descriptor,location,sampleIndex,corner);
        if(gemRow(at).w<0.5||gemMax(gemRow(at+1).xyz)>0.000001){return false;}
        for(int lobe=0;lobe<8;lobe++){
            int leaf=2+3*lobe;
            bool present=gemMax(gemRow(first+leaf+2).xyz)>0.0;
            if(present!=(gemMax(gemRow(at+leaf+2).xyz)>0.0)){return false;}
            if(present&&dot(gemRow(first+leaf+1).xyz,gemRow(at+leaf+1).xyz)<0.995){return false;}
        }
    }
    return true;
}
vec3 gemResponseEngraving(int descriptor,vec2 location,int sampleIndex,int instance) {
    vec4 pose=gemRow(192+2*instance);
    vec3 result=gemResponseRead(descriptor,location,sampleIndex,26).xyz*gemRow(5).xyz;
    for(int light=0;light<3;light++){
        int lr=16+5*light;vec3 delta=gemRow(lr).xyz-pose.xyz;
        float distance=max(length(delta),0.000001);
        float gain=max(dot(gemRow(lr+3).xyz,-delta/distance),0.0)/(distance*distance);
        result+=gemResponseRead(descriptor,location,sampleIndex,27+light).xyz*gemRow(lr+4).xyz*
            (gain/gemRow(descriptor+8+light).x);
    }
    return result;
}
