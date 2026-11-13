// Source-derived ink on the opaque tape face. Interior engraving is never
// visited for these materials because their transmission is zero.
vec4 gemTimelineWaveform(vec3 point,vec3 normal,int panel) {
    if(panel<700000||panel>=703072||normal.z<0.5){return vec4(0.0);}
    int instance=(panel-700000)/16;
    if(instance<int(gemRow(248).w)||instance>=int(gemRow(248).x)){return vec4(0.0);}
    int at=gemTimelineInstanceRow(instance);
    if(gemRow(at+1).w<0.5){return vec4(0.0);}
    vec4 pose=gemRow(at);vec4 tapeSize=gemRow(at+2);
    vec3 p=(point-pose.xyz)/pose.w;
    if(p.x<0.0||p.x>tapeSize.x||abs(p.y)>0.013||p.z<tapeSize.z*0.75){return vec4(0.0);}
    int waveBase=int(gemRow(251).w)+instance*1025;
    vec4 mapping=gemRow(waveBase);
    if(mapping.x<0.5){return vec4(0.0);}
    // Unlit, source-derived log-frequency spectrogram on the baked tape face.
    if(mapping.x>2.5){
        float time=clamp(mapping.y+p.x/tapeSize.x*mapping.z,0.0,0.999999);
        float frequency=clamp((p.y+0.013)/0.026,0.0,0.999999);
        float sx=time*127.0;float sy=frequency*31.0;
        int x0=int(floor(sx));int y0=int(floor(sy));
        int x1=min(x0+1,127);int y1=min(y0+1,31);
        int c00=y0*128+x0;int c10=y0*128+x1;
        int c01=y1*128+x0;int c11=y1*128+x1;
        float low=mix(gemRow(waveBase+1+c00/4)[c00%4],gemRow(waveBase+1+c10/4)[c10%4],fract(sx));
        float high=mix(gemRow(waveBase+1+c01/4)[c01%4],gemRow(waveBase+1+c11/4)[c11%4],fract(sx));
        float energy=mix(low,high,fract(sy));
        vec3 color=mix(vec3(0.015,0.02,0.065),vec3(0.18,0.08,0.5),clamp(energy*3.0,0.0,1.0));
        color=mix(color,vec3(0.95,0.22,0.12),clamp((energy-0.33)*3.0,0.0,1.0));
        color=mix(color,vec3(1.0,0.92,0.42),clamp((energy-0.67)*3.0,0.0,1.0));
        return vec4(color,1.0);
    }
    float position=clamp((mapping.y+p.x/tapeSize.x*mapping.z)*128.0-0.5,0.0,127.0);
    int column=int(floor(position));
    vec4 audio=mix(gemRow(waveBase+1+column),gemRow(waveBase+1+min(column+1,127)),fract(position));
    int channel=1;if(p.y>0.0){channel=0;}
    // Signed source ranges at close zoom are inexpensive display ink over the
    // same baked tape. No lighting, captures or optical refreshes occur here.
    if(mapping.x>1.5){
        float center=-0.0065;if(channel==0){center=0.0065;}
        float sampleY=(p.y-center)/0.0055;
        float low=audio[channel*2];float high=audio[channel*2+1];
        float thickness=max(0.025,2.0*gemRow(3).w/(gemRow(8).y*pose.w*0.0055));
        if(sampleY<low-thickness||sampleY>high+thickness){return vec4(0.0);}
        return vec4(0.35,0.75,0.85,1.0);
    }
    float across=abs(abs(p.y)-0.0065)/0.0055;
    float amplitude=audio[channel];
    if(across>min(amplitude,1.0)||amplitude<0.00001){return vec4(0.0);}
    float level=clamp((20.0*log2(max(amplitude,0.00001))/log2(10.0)+60.0)/60.0,0.0,1.0);
    float hue=audio.z*1.35+level*0.65+across*0.55;
    vec3 rainbow=0.5+0.5*cos(6.2831853*(hue+vec3(0.0,-0.333333,0.333333)));
    vec3 color=mix(vec3(0.8),rainbow,0.45+0.5*audio.w);
    return vec4(color*(0.18+0.62*level),1.0);
}

// Short track names and M/S/gain labels use the same authored SSLE glyph atlas
// as the side panels, sampled on three real refracted interior depth planes.
vec3 gemTimelineEngravingLight(vec3 point,int medium,float scale) {
    int base=gemPanelRow(medium);vec3 illumination=gemRow(5).xyz;
    for(int light=0;light<3;light++){
        int at=16+5*light;vec3 toLight=gemRow(at).xyz-point;
        float distance=length(toLight);vec3 l=toLight/max(distance,0.000001);
        float cosine=max(dot(gemRow(at+3).xyz,-l),0.0);
        GemHit exitHit=gemTimelineHit(point,l,medium,distance);
        if(cosine>0.0&&exitHit.panel==medium){
            float area=4.0*gemRow(at+1).w*gemRow(at+2).w;
            float transmit=1.0-gemFresnel(dot(l,exitHit.normal),1.0,gemRow(base).y);
            illumination+=gemRow(at+4).xyz*exp(-gemRow(base+1).xyz*(exitHit.distance/scale))
                *(transmit*cosine*area/(3.14159265359*distance*distance));
        }
    }
    return illumination*gemRow(214).xyz*gemRow(223).w;
}

vec4 gemTimelineEngraving(vec3 origin,vec3 ray,int medium,float farLimit) {
    if(medium<700000||medium>=704096||abs(ray.z)<0.00001){return vec4(0.0,0.0,0.0,1.0);}
    int instance=(medium-700000)/16;
    if(instance<192&&instance>=int(gemRow(248).w)){return vec4(0.0,0.0,0.0,1.0);}
    int at=gemTimelineInstanceRow(instance);
    vec4 pose=gemRow(at);float font=gemRow(at+2).w/pose.w;
    vec3 local=(origin-pose.xyz)/pose.w;float limit=farLimit/pose.w;
    int letters=int(gemRow(248).z)+instance*2;int maximum=8;if(instance>=192){letters=int(gemRow(255).x)+48+(instance-192)*4;maximum=16;}int length=0;
    for(int i=0;i<16;i++){
        if(i>=maximum){break;}
        vec4 codes=gemRow(letters+i/4);float code=codes[i%4];
        if(code<1.0){break;}length++;
    }
    vec3 result=vec3(0.0);float transmission=1.0;
    float pixel=2.0*gemRow(3).w/(gemRow(8).y*pose.w);
    for(int visit=0;visit<3;visit++){
        int layer=visit;if(ray.z>0.0){layer=2-visit;}
        float z=-font*(0.10+0.035*float(layer));float weight=1.0;
        if(layer==1){weight=gemRow(215).z;}if(layer==2){weight=gemRow(215).w;}
        float t=(z-local.z)/ray.z;if(t<=0.0000001||t>=limit){continue;}
        vec3 point=local+ray*t;float density=0.0;
        for(int i=0;i<16;i++){
        if(i>=maximum){break;}
            if(i>=length){break;}
            vec4 codes=gemRow(letters+i/4);int glyph=int(codes[i%4])-32;
            vec2 baseline=vec2((float(i)-float(length)*0.5)*0.55*font,-font*0.28);
            vec2 em=(point.xy-baseline)/font;
            if(em.x< -0.17||em.x>1.13||em.y< -0.29||em.y>1.01){continue;}
            density+=gemEngravedDensity(em,glyph,vec2(0.0),pixel*0.75/font,layer);
        }
        if(density>0.0001){
            float alpha=1.0-exp(-gemRow(214).w*weight*min(density,1.0)/max(abs(ray.z),0.12));
            if(gemRow(at+1).w< -1.5){alpha*=0.35;}
            vec3 world=pose.xyz+point*pose.w;
            result+=transmission*alpha*exp(-gemRow(gemPanelRow(medium)+1).xyz*t)*gemTimelineEngravingLight(world,medium,pose.w);
            transmission*=1.0-alpha;
        }
    }
    return vec4(result,transmission);
}
