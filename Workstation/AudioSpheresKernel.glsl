// Analytic opaque ellipsoids for live source appearances. Scene RGB is linear.
// Header240..243 and four-row instances match AudioSpheres.rs. These primitives
// never enter gemAny/gemShadow, so the existing plain background stays unchanged.
// The three-light rig belongs only to these surfaces, not the panel studio.

GemHit gemAudioSphereHit(vec3 origin,vec3 ray,float farLimit) {
    vec4 descriptor=gemRow(240);
    GemHit hit=GemHit(farLimit,vec3(0.0),-1);
    if(descriptor.x<0.5){return hit;}
    if(!gemBox(origin,gemInverseRay(ray),gemRow(242).xyz,gemRow(243).xyz,farLimit)){return hit;}
    vec4 clip=gemRow(241);
    for(int index=0;index<64;index++){
        if(float(index)>=descriptor.x){break;}
        int at=int(descriptor.y)+index*int(descriptor.z);
        vec3 center=gemRow(at).xyz;vec3 radii=gemRow(at+1).xyz;
        if(!gemBox(origin,gemInverseRay(ray),center,radii,hit.distance)){continue;}
        vec3 local=(origin-center)/radii;vec3 direction=ray/radii;
        float a=dot(direction,direction);
        float closest=-dot(local,direction)/a;
        // Closest approach avoids subtracting two large, nearly equal squares
        // when a small source is far from the orthographic camera.
        vec3 perpendicular=local+direction*closest;
        float radial=1.0-dot(perpendicular,perpendicular);
        if(radial<0.0){continue;}
        float offset=sqrt(max(radial,0.0)/a);
        float distance=closest-offset;
        if(distance<=0.0000001){distance=closest+offset;}
        if(distance<=0.0000001||distance>=hit.distance){continue;}
        vec3 point=origin+ray*distance;
        if(point.x<clip.x||point.y<clip.y||point.x>clip.z||point.y>clip.w){continue;}
        if(!gemMobileBackdropAllows(point.xy)){continue;}
        vec3 normal=gemSafeUnit((point-center)/(radii*radii));
        hit=GemHit(distance,normal,90000+index);
    }
    return hit;
}

float gemAudioSphereSmith(float cosine,float alpha) {
    float c=max(cosine,0.000001);
    return (2.0*c)/(c+sqrt(alpha*alpha+(1.0-alpha*alpha)*c*c));
}

vec3 gemAudioSphereLight(vec3 normal,vec3 view,vec3 light,vec3 radiance,vec3 color,float metallic,float roughness) {
    float nl=max(dot(normal,light),0.0);float nv=max(dot(normal,view),0.000001);
    if(nl<=0.0){return vec3(0.0);}
    vec3 halfVector=gemSafeUnit(view+light);
    float nh=max(dot(normal,halfVector),0.0);float vh=max(dot(view,halfVector),0.0);
    float alpha=max(roughness*roughness,0.0001);float alpha2=alpha*alpha;
    float denominator=nh*nh*(alpha2-1.0)+1.0;
    float distribution=alpha2/(3.14159265359*denominator*denominator);
    vec3 f0=mix(vec3(0.04),color,metallic);
    vec3 fresnel=f0+(vec3(1.0)-f0)*pow(1.0-vh,5.0);
    float masking=gemAudioSphereSmith(nv,alpha)*gemAudioSphereSmith(nl,alpha);
    vec3 specular=fresnel*(distribution*masking/(4.0*nv*max(nl,0.000001)));
    vec3 diffuse=color*(1.0-metallic)*(vec3(1.0)-fresnel)/3.14159265359;
    return (diffuse+specular)*radiance*nl;
}

vec3 gemAudioSphereShade(vec3 point,vec3 view,vec3 normal,int panel) {
    vec4 descriptor=gemRow(240);int index=panel-90000;
    int at=int(descriptor.y)+index*int(descriptor.z);
    vec4 pose=gemRow(at);vec4 shape=gemRow(at+1);
    vec4 material=gemRow(at+2);vec4 theme=gemRow(at+3);
    vec3 color=material.xyz;
    // A small, physical base-color lift identifies the currently grabbed source.
    color=mix(color,mix(color,vec3(1.0),0.08),pose.w);
    float metallic=material.w;float roughness=theme.w;
    vec3 f0=mix(vec3(0.04),color,metallic);
    // SceneKit reference: gray environment at1.5. This constant-environment
    // approximation preserves the metallic tint without introducing a backdrop.
    vec3 result=(f0+color*(1.0-metallic))*0.32106171;
    result+=gemAudioSphereLight(normal,view,gemSafeUnit(vec3(0.35,0.6,1.0)),vec3(1.2),color,metallic,roughness);
    float time=descriptor.w;
    // Source geometry was radius1.5 with these two independent omni orbits.
    // Scale each local rig with its sphere so apparent distance does not alter
    // the material or secretly encode a second audio gain.
    float scale=max(shape.y,0.000001)/1.5;
    vec3 localPoint=(point-pose.xyz)/scale;
    vec3 themePosition=vec3(sin(time*0.6+cos(time*0.3)*0.5)*5.0,
        cos(time*0.5+sin(time*0.4)*0.5)*5.0+5.0,cos(time*0.35)*5.0);
    vec3 coolPosition=vec3(cos(time*0.45)*7.0,sin(time*0.65)*4.0+4.0,sin(time*0.25)*7.0);
    vec3 themeVector=themePosition-localPoint;vec3 coolVector=coolPosition-localPoint;
    // Finite emitter radius bounds the inverse-square energy when an orbit
    // approaches the surface; the reference's1200-intensity cool lamp is1.2x.
    vec3 themeRadiance=mix(theme.xyz,vec3(1.0),0.12)*(24.0/max(dot(themeVector,themeVector),4.0));
    vec3 coolRadiance=vec3(0.6,0.8,1.0)*(28.8/max(dot(coolVector,coolVector),4.0));
    result+=gemAudioSphereLight(normal,view,gemSafeUnit(themeVector),themeRadiance,color,metallic,roughness);
    result+=gemAudioSphereLight(normal,view,gemSafeUnit(coolVector),coolRadiance,color,metallic,roughness);
    return result;
}
