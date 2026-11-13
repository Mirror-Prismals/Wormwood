// Bounded display of source-derived envelope/FFT data. Heat palette is Cardinal
// DawFaderSystem::spectralHeatColor. Header255.w remains host deferred-floor tag.
GemHit gemBottomSurfaceHit(vec3 origin,vec3 ray,float farLimit){
    GemHit hit=GemHit(farLimit,vec3(0.0),-1);vec4 header=gemRow(255);
    if(header.y<0.5){return hit;}int base=int(header.x);vec4 view=gemRow(base+1);
    if(gemRow(base+2).z<0.5||view.z<=view.x||view.w<=view.y){return hit;}
    // Waveform editor uses transparent, inexpensive native display content.
    if(gemRow(base+4).w>0.5){return hit;}
    vec4 clip=gemRow(base);view=vec4(max(view.xy,clip.xy),min(view.zw,clip.zw));
    if(view.z<=view.x||view.w<=view.y){return hit;}
    hit=gemTimelineMarkerBox(origin,ray,vec3((view.xy+view.zw)*0.5,gemRow(base+2).w+0.001),vec3((view.zw-view.xy)*0.5,0.0001),farLimit);
    if(hit.panel>=0){hit.panel=719997;}return hit;
}
