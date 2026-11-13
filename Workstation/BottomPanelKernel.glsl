vec3 gemBottomHeat(float intensity){
    float scaled=pow(clamp(intensity,0.0,1.0),0.82)*6.0;
    vec3 a=vec3(0.008,0.006,0.028);vec3 b=vec3(0.055,0.018,0.20);
    if(scaled>=1.0){a=b;b=vec3(0.055,0.10,0.48);}
    if(scaled>=2.0){a=b;b=vec3(0.015,0.50,0.78);}
    if(scaled>=3.0){a=b;b=vec3(0.08,0.82,0.72);}
    if(scaled>=4.0){a=b;b=vec3(0.96,0.46,0.07);}
    if(scaled>=5.0){a=b;b=vec3(1.00,0.94,0.56);}
    return mix(a,b,scaled-min(floor(scaled),5.0));
}
float gemBottomIntensity(int base,int x,int y){
    vec4 full=gemRow(base+5);if(full.x>0.5&&full.z>0.5&&full.w>0.5){
        int i=clamp(y,0,int(full.w)-1)*int(full.z)+clamp(x,0,int(full.z)-1);int word=i/3;float packed=gemRow(int(full.x)+120+word/4)[word%4];
        float divisor=1.0;if(i%3==1){divisor=256.0;}if(i%3==2){divisor=65536.0;}float q=floor(packed/divisor);return (q-floor(q/256.0)*256.0)/255.0;
    }int i=clamp(y,0,15)*32+clamp(x,0,31);return gemRow(base+128+i/4)[i%4];
}
float gemBottomOutline(vec2 uv,vec4 r,vec2 width){
    if(uv.x<r.x-width.x||uv.x>r.y+width.x||uv.y<r.z-width.y||uv.y>r.w+width.y){return 0.0;}
    if(abs(uv.x-r.x)<=width.x||abs(uv.x-r.y)<=width.x||abs(uv.y-r.z)<=width.y||abs(uv.y-r.w)<=width.y){return 1.0;}return 0.0;
}

float gemBottomText(int base,vec2 uv,vec2 pixel){
    vec4 full=gemRow(base+5);if(full.x<0.5){return 0.0;}float result=0.0;
    for(int label=0;label<20;label++){if(float(label)>=gemRow(base+6).x){break;}int at=int(full.x)+2120+label*2;vec4 pose=gemRow(at);vec4 text=gemRow(at+1);vec2 font=pixel*pose.z;vec2 local=(uv-pose.xy)/font;
        if(local.y< -0.3||local.y>1.1||local.x< -0.2||local.x>pose.w*0.55+1.2){continue;}
        for(int i=0;i<12;i++){if(float(i)>=pose.w){break;}float divisor=1.0;if(i%3==1){divisor=256.0;}if(i%3==2){divisor=65536.0;}float q=floor(text[i/3]/divisor);int glyph=int(q-floor(q/256.0)*256.0)-32;
            vec2 em=local-vec2(float(i)*0.55,0.0);if(em.x< -0.17||em.x>1.13||em.y< -0.29||em.y>1.01){continue;}result+=gemEngravedDensity(em,glyph,vec2(0.0),0.75/pose.z,0);
        }
    }return clamp(result,0.0,1.0);
}

vec3 gemBottomSurfaceColor(vec3 point){
    int base=int(gemRow(255).x);vec4 view=gemRow(base+1);vec4 info=gemRow(base+2);vec4 extra=gemRow(base+4);
    vec2 uv=clamp((point.xy-view.xy)/(view.zw-view.xy),vec2(0.0),vec2(1.0));vec2 pixel=vec2(info.x)/(view.zw-view.xy);
    vec3 color=vec3(0.025,0.035,0.055);
    if(info.z>1.5){
        vec4 full=gemRow(base+5);vec2 size=vec2(31.0,15.0);if(full.x>0.5&&full.z>0.5&&full.w>0.5){size=full.zw-vec2(1.0);}
        vec2 p=uv*size;int x=int(floor(p.x));int y=int(floor(p.y));vec2 f=fract(p);
        // Cardinal pushColoredQuad emits TL/TR/BR then TL/BR/BL. Interpolate
        // the authored palette colors in those exact two triangles.
        vec3 tl=gemBottomHeat(gemBottomIntensity(base,x,y+1));vec3 tr=gemBottomHeat(gemBottomIntensity(base,x+1,y+1));
        vec3 br=gemBottomHeat(gemBottomIntensity(base,x+1,y));vec3 bl=gemBottomHeat(gemBottomIntensity(base,x,y));
        if(f.x+f.y>=1.0){color=tl*(1.0-f.x)+tr*(f.x+f.y-1.0)+br*(1.0-f.y);}
        else{color=tl*f.y+br*f.x+bl*(1.0-f.x-f.y);}
        float rate=gemRow(base+7).x;float guide=100.0;
        for(int i=0;i<5;i++){if(i==1){guide=1000.0;}if(i==2){guide=5000.0;}if(i==3){guide=10000.0;}if(i==4){guide=20000.0;}if(guide>rate*0.5*1.001){continue;}float y=log(max(30.0,guide)/30.0)/log(max(30.0001,rate*0.5)/30.0);if(abs(uv.y-y)<pixel.y*0.35){color=mix(color,vec3(0.72,0.78,0.88),0.22);}}
        for(int i=1;i<4;i++){if(abs(uv.x-float(i)*0.25)<pixel.x*0.3){color=mix(color,vec3(0.72,0.78,0.88),0.22);}}
        for(int i=0;i<16;i++){if(float(i)>=extra.z){break;}vec4 mask=gemRow(base+272+i);color=mix(color,vec3(1.0,0.3,0.06),gemBottomOutline(uv,mask,pixel*1.6));}
    }else{
        vec4 full=gemRow(base+5);int columns=64;int wave=base+96;if(full.x>0.5&&full.y>0.5){columns=int(full.y);wave=int(full.x);}
        int x=clamp(int(uv.x*float(columns)),0,columns-1);vec4 pair=gemRow(wave+x/2);float low=pair[(x%2)*2];float high=pair[(x%2)*2+1];
        float center=0.5;float height=0.44;float halfColumn=max(0.5*pixel.x*float(columns),0.4);
        bool stereo=info.y>1.5&&info.y<2.5&&full.z>1.5;
        if(info.y>1.5&&info.y<2.5){height=0.42;if(stereo){center=0.73;height=0.20;}halfColumn=max(0.55*pixel.x*float(columns),0.42);}
        float amplitude=(uv.y-center)/height;
        if(amplitude>=low-pixel.y/height&&amplitude<=high+pixel.y/height&&abs(uv.x*float(columns)-float(x)-0.5)<=halfColumn){color=vec3(0.35,0.82,0.95);}
        if(abs(uv.y-center)<pixel.y*0.5){color=mix(color,vec3(0.2),0.5);}
        if(stereo){vec4 right=gemRow(wave+120+x/2);float rightLow=right[(x%2)*2];float rightHigh=right[(x%2)*2+1];float a=(uv.y-0.27)/0.20;
            if(a>=rightLow&&a<=rightHigh&&abs(uv.x*float(columns)-float(x)-0.5)<=halfColumn){color=mix(vec3(0.35,0.82,0.95),vec3(0.35,0.72,0.95),0.45);}
            if(abs(uv.y-0.27)<pixel.y*0.5){color=mix(color,vec3(0.2),0.5);}
        }
        for(int i=0;i<64;i++){if(float(i)>=extra.y){break;}float marker=gemRow(base+256+i/4)[i%4];if(abs(uv.x-marker)<pixel.x*1.5){color=vec3(0.85,0.68,0.12);}}
    }
    if(extra.x>0.5){vec4 selection=gemRow(base+3);if(uv.x>=selection.x&&uv.x<=selection.y&&uv.y>=selection.z&&uv.y<=selection.w){color=mix(color,vec3(0.35,0.82,1.0),0.12);}color=mix(color,vec3(0.94,0.96,1.0),gemBottomOutline(uv,selection,pixel*2.0));if(info.z>1.5){float f=gemRow(base+7).z;vec2 d=vec2(selection.y-selection.x,selection.w-selection.z)*f;vec4 core=selection+vec4(d.x,-d.x,d.y,-d.y);color=mix(color,mix(vec3(0.94,0.96,1.0),vec3(0.35,0.82,1.0),0.55),gemBottomOutline(uv,core,pixel*0.8));}}
    color=mix(color,vec3(0.86,0.89,0.95),gemBottomText(base,uv,pixel));
    return color;
}
