"""Private unoccluded floor kernel for the isolated ruby response experiment.

The caller must validate zero panel instances (row 0.y), no custom geometry or
custom background (row 245.x/z), and a disabled old visibility field (row 10.y).
Under those conditions canonical gemShadow returns one: it does not intersect
decorative controls. This helper preserves the original rectangular emitters,
twelve quadrature samples, dielectric BRDF, live lights, and linear radiance.

Only floor-invariant Smith terms are hoisted. No bake, shader compilation,
process launch, or production modification occurs when importing this module.
"""


WGSL_FLOOR = r'''
// PRIVATE: isolated ruby fixture only. The caller validates that the original
// floor has no panel/custom occluders, background shader or cached visibility.
fn responseFloor(point:vec3<f32>,view:vec3<f32>)->vec3<f32> {
    let normal=vec3<f32>(0.0,0.0,1.0);
    let baseColor=gemRow(6).xyz;
    let roughness=gemRow(6).w;
    var result=baseColor*gemRow(5).xyz;
    let alpha=max(roughness*roughness,0.0001);
    let alpha2=alpha*alpha;
    let nv=max(dot(normal,view),0.00001);
    let gi=gemSmith(nv,alpha);
    for(var light:i32=0;light<3;light++) {
        if(f32(light)>=gemRow(9).x){break;}
        let row=16+light*5;
        let u=gemRow(row+1);
        let v=gemRow(row+2);
        let area=4.0*u.w*v.w;
        for(var sampleIndex:i32=0;sampleIndex<4;sampleIndex++) {
            let x=gemPick(-0.57735026919,0.57735026919,(sampleIndex%2)==1);
            let y=gemPick(-0.57735026919,0.57735026919,sampleIndex>=2);
            let toLight=gemRow(row).xyz+u.xyz*(u.w*x)+v.xyz*(v.w*y)-point;
            let distance=length(toLight);
            let l=toLight/max(distance,0.000001);
            let nl=max(dot(normal,l),0.0);
            let cosLight=max(dot(gemRow(row+3).xyz,-l),0.0);
            if(nl>0.0&&cosLight>0.0) {
                let halfVector=gemSafeUnit(l+view);
                let nh=max(dot(normal,halfVector),0.0);
                let d=alpha2/(3.14159265359*pow(nh*nh*(alpha2-1.0)+1.0,2.0));
                let fresnel=gemFresnel(dot(view,halfVector),1.0,1.5);
                let go=gemSmith(nl,alpha);
                let mask=go/max(gi+go-gi*go,0.0000001);
                let specular=d*fresnel*gi*mask/(4.0*nv*max(nl,0.00001));
                let brdf=baseColor*((1.0-fresnel)/3.14159265359)+vec3<f32>(specular);
                result+=brdf*gemRow(row+4).xyz*(nl*cosLight*area/(4.0*distance*distance));
            }
        }
    }
    return result;
}
'''
