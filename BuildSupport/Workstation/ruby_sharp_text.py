"""Diagnostic-only full-resolution engraving variants for approved ruby WGSL.

transform(source) keeps the optical cache except at panel text bands, cached
samples containing engraved radiance, and entire engraved control components.
transform_components(source) rejects caching for every text-bearing component;
this is the conservative visual reference for the narrower variant.

Neither function changes the optical budget, primary scene visibility, studio
terminal policy, density atlas, font filtering, tone mapping, or cache layout.
Rejected samples use the existing full-resolution gemTrace. This module is not
loaded by the application and does not install or publish anything.

The narrow variant covers the actual and selected-cache primary refracted
segments with line bounds enlarged by a full cache cell and a drawable pixel.
It also rejects every selected cache sample with any engraved radiance. That
does not prove coverage of an unrecorded, arbitrarily displaced internal text
reflection. Use the entire-component variant to inspect that residual risk.
"""


def _once(source: str, old: str, new: str) -> str:
    if source.count(old) != 1:
        raise ValueError("Ruby sharp-text marker changed: " + old.splitlines()[0])
    return source.replace(old, new, 1)


_COMPONENTS = r"""
// Mirrors the actual supported engraving owners. In particular, extended
// palettes may use material+2.w for something other than an engraving address.
fn rubySharpTextComponent(componentId:i32)->bool {
    if(componentId==2||componentId==3){
        if(gemRow(212).w<0.5){return false;}
        for(var line:i32=0;line<64;line++){
            if(f32(line)>=gemRow(222).y){break;}
            let at=i32(gemRow(222).x)+3*line;
            if(i32(gemRow(at+2).x)==componentId&&gemRow(at+1).w>0.0){return true;}
        }
        return false;
    }
    if(componentId<4||componentId>=16644){return false;}
    let instance=(componentId-4)/256;
    // Plugin navigation owns procedural glyphs rather than material+2.w.
    if(instance>=61&&instance<65){return true;}
    if(instance>=10||gemRow(244).y<0.5){return false;}
    let base=gemPanelRow(componentId);
    return gemRow(base+2).w>0.0&&gemRow(base).w>=0.0001;
}
"""


_NARROW = r"""
// Test the exact initial refracted segment with the canonical sample's surface
// normal, roughness random sample, epsilon and component exit. Expanded bounds
// prevent a low-resolution cell's text contribution bleeding past a sharp band.
fn rubySharpPanelSegment(origin:vec3<f32>,direction:vec3<f32>,componentId:i32,
                         random:vec2<f32>,support:vec2<f32>)->bool {
    let hit=rubyComponentHit(origin,direction,-1,componentId,1e20);
    if(hit.panel!=componentId||hit.distance<=0.0){return true;}
    let base=gemPanelRow(componentId);let material=gemRow(base);
    let micro=gemMicroNormal(hit.normal,-direction,material.z,random);
    let ray=refract(direction,micro,1.0/material.y);
    if(dot(ray,hit.normal)>=0.0||dot(ray,ray)<0.5){return true;}
    let point=origin+direction*hit.distance;
    let size=gemRow(base+3).xyz;
    let epsilon=max(0.0000002,min(min(size.x,size.y),size.z)*0.00001);
    let inside=point+ray*epsilon;
    let exitHit=rubyComponentHit(inside,ray,componentId,componentId,1e20);
    if(exitHit.panel!=componentId||exitHit.distance<=0.0){return true;}
    let inverse=gemInverseRay(ray);
    for(var line:i32=0;line<64;line++){
        if(f32(line)>=gemRow(222).y){break;}
        let at=i32(gemRow(222).x)+3*line;
        if(i32(gemRow(at+2).x)!=componentId||gemRow(at+1).w<=0.0){continue;}
        if(gemBox(inside,inverse,gemRow(at).xyz,
                  gemRow(at+1).xyz+vec3<f32>(support,0.0000001),exitHit.distance)){
            return true;
        }
    }
    return false;
}

fn rubySharpTextReject(origin:vec3<f32>,direction:vec3<f32>,componentId:i32,
                       random:vec2<f32>,address:u32,pose:vec4<f32>,
                       bounds:vec4<f32>,edge:i32)->bool {
    if(!rubySharpTextComponent(componentId)){return false;}
    // A cached component's non-environment radiance includes reflected text.
    // Reject even tiny positive contributions; there is no visual threshold.
    if(any(abs(rubyResponse.rows[address+26u].xyz)>vec3<f32>(0.0))){return true;}
    // The original engraved control components and navigation arrows are
    // compact, and faders may be concave. Keep their complete tracing intact.
    if(componentId>=4){return true;}
    let pixel=2.0*gemRow(3).w/max(gemRow(8).y,1.0);
    let support=abs(bounds.zw-bounds.xy)*pose.w/f32(edge)+vec2<f32>(pixel);
    if(rubySharpPanelSegment(origin,direction,componentId,random,support)){return true;}
    // The cached entry position has the exact XY of its orthographic bake ray.
    let cachedOrigin=vec3<f32>(pose.xy+rubyResponse.rows[address].xy*pose.w,origin.z);
    return rubySharpPanelSegment(cachedOrigin,direction,componentId,random,support);
}
"""


def _transform(source: str, entire_components: bool) -> str:
    if "fn rubySharpTextComponent(" in source:
        raise ValueError("Ruby sharp-text variant has already been applied")
    required = (
        "fn rubyResponseAddress(", "fn rubyComponentHit(",
        "fn gemControlEngraving(", "fn gemPluginNavigationEngraving(",
        "// Exterior policy: resolve the studio directly without scene occlusion.",
    )
    for marker in required:
        if source.count(marker) != 1:
            raise ValueError("Requires approved generated ruby WGSL: " + marker)
    end = """    if(rubyResponse.rows[u32(address)].w<0.5){return -1;}
    return address;"""
    predicate = ("rubySharpTextComponent(componentId)" if entire_components else
                 "rubySharpTextReject(origin,direction,componentId,random,u32(address),pose,bounds,edge)")
    source = _once(source, end, """    if(rubyResponse.rows[u32(address)].w<0.5){return -1;}
    if(""" + predicate + """){
        rubyTextRejectedSamples+=1u;
        return -1;
    }
    return address;""")
    source = _once(source, "fn workstationShade(screen:vec2<f32>)->vec4<f32> {",
                   "fn workstationShade(screen:vec2<f32>)->vec4<f32> {\n    rubyTextRejectedSamples=0u;")
    return ("var<private> rubyTextRejectedSamples:u32=0u;\n" + source + "\n" +
            _COMPONENTS + ("" if entire_components else _NARROW))


def transform(source: str) -> str:
    """Conservative panel bands plus exact engraved control components."""
    return _transform(source, False)


def transform_components(source: str) -> str:
    """Reference variant: exact tracing of every text-bearing ruby component."""
    return _transform(source, True)


def instrument_rejections(source: str) -> str:
    """Optional diagnostic alpha: samples rejected for text instead of cache hits.

    Apply after either transform. RGB is unchanged. Accepts production alpha or
    the exporter/probe's existing cached-sample alpha, rejecting unknown ABIs.
    """
    if source.count("var<private> rubyTextRejectedSamples:u32=0u;") != 1:
        raise ValueError("Apply a sharp-text transform before instrumentation")
    for value in ("f32(rubyUsedSamples)", "1.0"):
        old = "return vec4<f32>(gemDisplay(result/samples)," + value + ");"
        if source.count(old) == 1:
            return _once(source, old,
                         "return vec4<f32>(gemDisplay(result/samples),f32(rubyTextRejectedSamples));")
    raise ValueError("Ruby sharp-text output marker changed")
