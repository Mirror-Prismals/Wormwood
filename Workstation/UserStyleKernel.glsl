// User panel meshes use their original triangles, including zero-thickness faces.
// Inverse nonuniform transforms preserve world ray t; normals use inverse transpose.
// IDs 200000..599999 encode logical panel and immutable triangle row.
int userPanel(int id) { return (id-200000)/100000; }
int userTriangle(int id) { return int(gemRow(246).x)+(id-200000)%100000; }
vec2 userBarycentric(int triangle,vec3 p) {
    vec3 a=gemRow(triangle).xyz;
    vec3 e=gemRow(triangle+1).xyz-a;vec3 f=gemRow(triangle+2).xyz-a;vec3 q=p-a;
    vec3 area=cross(e,f);float den=max(dot(area,area),1e-30);
    float u=dot(cross(q,f),area)/den;
    float v=dot(cross(e,q),area)/den;
    return vec2(u,v);
}
// A long ray's rounded t can put a skinny triangle's hit several float32
// units beneath its surface. Rebuild from the original triangle before
// shading or launching shadow rays; increasing bias would erase real relief.
vec3 userSurfacePoint(int triangle,vec3 p) {
    vec2 bary=userBarycentric(triangle,p);vec3 a=gemRow(triangle).xyz;
    return a+(gemRow(triangle+1).xyz-a)*bary.x+(gemRow(triangle+2).xyz-a)*bary.y;
}
vec2 userUV(int triangle,vec3 p) {
    vec2 bary=userBarycentric(triangle,p);float u=bary.x;float v=bary.y;
    vec4 uv=gemRow(triangle+6);
    return uv.xy*(1.0-u-v)+uv.zw*u+gemRow(triangle+7).xy*v;
}
vec4 userColor(int panel,int triangle,vec3 local) {
    int pose=32+40*panel;int material=int(gemRow(pose).w)+int(gemRow(triangle).w)*2;
    vec4 color=vec4(gemRow(material).xyz,1.0);
    if(gemRow(pose+2).w>0.5){
        vec2 size=(gemRow(pose+4).xy-gemRow(pose+3).xy)*gemRow(pose+2).xy;
        color*=style_color(userUV(triangle,local),local,size,gemRow(245).w);
    }
    if(color.x!=color.x||color.y!=color.y||color.z!=color.z||color.w!=color.w){return vec4(1.0,0.0,1.0,1.0);}
    return clamp(color,vec4(0.0),vec4(65504.0,65504.0,65504.0,1.0));
}
GemHit userStyleHit(vec3 origin,vec3 ray,float farLimit,bool anyHit,bool shadow) {
    GemHit hit=GemHit(farLimit,vec3(0.0),-1);
    if(gemRow(245).x<0.5){return hit;}
    for(int panel=0;panel<4;panel++){
        int pose=32+40*panel;
        if(gemRow(pose+1).w<0.5||gemRow(pose).x>0.5){continue;}
        vec3 scale=gemRow(pose+2).xyz;
        vec3 local=(origin-gemRow(pose+1).xyz)/scale;vec3 direction=ray/scale;
        vec3 inverse=gemInverseRay(direction);
        int node=int(gemRow(pose).y);int end=int(gemRow(pose).z);
        for(int visit=0;visit<8192;visit++){
            if(node>=end){break;}
            vec4 lo=gemRow(node);vec4 hi=gemRow(node+1);
            if(!gemBox(local,inverse,(lo.xyz+hi.xyz)*0.5,(hi.xyz-lo.xyz)*0.5+vec3(0.0000001),hit.distance)){
                node=int(lo.w);continue;
            }
            int count=int(gemRow(node+2).x);
            if(count==0){node+=3;continue;}
            for(int index=0;index<4;index++){
                if(index>=count){break;}
                int at=int(hi.w)+8*index;
                vec3 a=gemRow(at).xyz;vec3 e=gemRow(at+1).xyz-a;vec3 f=gemRow(at+2).xyz-a;
                vec3 h=cross(direction,f);float det=dot(e,h);
                if(abs(det)<1e-7*length(e)*length(f)*length(direction)){continue;}
                if(!shadow&&det<0.0&&gemRow(at+2).w<0.5){continue;}
                vec3 offset=local-a;float u=dot(offset,h)/det;
                if(u<0.0||u>1.0){continue;}
                vec3 q=cross(offset,e);float v=dot(direction,q)/det;
                if(v<0.0||u+v>1.0){continue;}
                float t=dot(f,q)/det;
                if(t<=0.0000001||t>=hit.distance){continue;}
                if(userColor(panel,at,a+e*u+f*v).w<0.5){continue;}
                vec3 normal=gemRow(at+3).xyz*(1.0-u-v)+gemRow(at+4).xyz*u+gemRow(at+5).xyz*v;
                normal=gemSafeUnit(normal/scale);
                if(dot(normal,ray)>0.0){normal=-normal;}
                hit=GemHit(t,normal,200000+panel*100000+at-int(gemRow(246).x));
                if(anyHit){return hit;}
            }
            node=int(lo.w);
        }
    }
    return hit;
}
