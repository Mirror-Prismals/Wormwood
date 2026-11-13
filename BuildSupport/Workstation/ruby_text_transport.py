"""Diagnostic: sharp first-segment engraving over an unengraved ruby cache.

This is an explicit optical approximation, not an equivalent integrator. The
cache retains the approved ruby transport but excludes all engraving. At each
drawable sample, the actual primary hit and GGX refraction locate the first
interior segment. Its original three-layer engraving, density filtering,
lighting, Fresnel/Smith throughput, absorption and opacity are evaluated there.
Later internal reflections do not acquire additional engraving in this variant.

The primary reflected lobe is kept outside the opacity composite. Remaining
cached radiance is multiplied by the first segment's transmission before its
sharp engraved radiance is added. This is not an additive text decal. Pixels
that encounter no engraving return their cached color without alteration.

Apply transform(source) to approved generated response.wgsl. For the separate
lighting pass, apply ruby_resolve.transform next, then finish_resolved. The last
step resolves the primary reflection into unused row28 so lettering does not
repeat its studio/floor evaluation per drawable pixel. All changes are private
diagnostics; this module does not touch runtime assets or install an app.
"""


def _once(source: str, old: str, new: str) -> str:
    if source.count(old) != 1:
        raise ValueError("Ruby text-transport marker changed: " + old.splitlines()[0])
    return source.replace(old, new, 1)


def _function(source: str, name: str) -> str:
    marker = "fn " + name + "("
    if source.count(marker) != 1:
        raise ValueError("Expected exactly one function: " + name)
    start = source.index(marker)
    opening = source.index("{", start)
    depth = 1
    end = opening + 1
    while depth and end < len(source):
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    if depth:
        raise ValueError("Unclosed function: " + name)
    return source[start:end]


_ENGRAVING_BLOCKS = (
    """            if(inside&&current.medium<4){
                var engraving:vec4<f32>=gemEngraving(current.origin,current.direction,current.medium,hit.distance);
                result+=current.weight*engraving.xyz;current.weight*=engraving.w;
            }
""",
    """            if(inside&&current.medium>=4){
                var engraving:vec4<f32>=gemControlEngraving(current.origin,current.direction,current.medium,hit.distance);
                result+=current.weight*engraving.xyz;current.weight*=engraving.w;
                var navigation:vec4<f32>=gemPluginNavigationEngraving(current.origin,current.direction,current.medium,hit.distance);
                result+=current.weight*navigation.xyz;current.weight*=navigation.w;
            }
""",
    """            if(inside&&current.medium>=700000&&current.medium<704096){
                var engraving:vec4<f32>=gemTimelineEngraving(current.origin,current.direction,current.medium,hit.distance);
                result+=current.weight*engraving.xyz;current.weight*=engraving.w;
            }
""",
)


_SUPPORT = r"""
// Only owners of the actual authored engraving can enter this path. Extended
// palette material+2.w values have unrelated meanings and must not be addresses.
fn rubyLiveTextOwner(componentId:i32)->bool {
    if(componentId==2||componentId==3){return gemRow(212).w>=0.5;}
    if(componentId<4||componentId>=16644){return false;}
    let instance=(componentId-4)/256;
    if(instance>=61&&instance<65){return true;}
    if(instance>=10||gemRow(244).y<0.5){return false;}
    let base=gemPanelRow(componentId);
    return gemRow(base+2).w>0.0&&gemRow(base).w>=0.0001;
}

// Use the same conservative text boxes as the canonical density routines,
// before paying for a component exit query. An infinite limit can only retain
// extra work; the eventual exact exit still clips engraving to its glass body.
fn rubyLiveTextSegmentMayHit(origin:vec3<f32>,ray:vec3<f32>,componentId:i32)->bool {
    if(componentId==2||componentId==3){
        let pixel=2.0*gemRow(3).w/gemRow(8).y;
        let group=218+2*(componentId-2);
        return gemBox(origin,gemInverseRay(ray),gemRow(group).xyz,
            gemRow(group+1).xyz+vec3<f32>(pixel,pixel,0.0000001),1e20);
    }
    let instance=(componentId-4)/256;
    if(instance>=61&&instance<65){return true;}
    let descriptor=i32(gemRow(gemPanelRow(componentId)+2).w);
    let pose=gemRow(gemControlPoseRow(instance));
    let region=gemRow(descriptor);let depth=gemRow(descriptor+1);
    let pixel=2.0*gemRow(3).w/(gemRow(8).y*pose.w);
    let center=vec3<f32>(region.xy+region.zw*0.5,(depth.x+depth.z)*0.5);
    let extent=vec3<f32>(region.zw*0.5+vec2<f32>(pixel),(depth.x-depth.z)*0.5+0.0000001);
    return gemBox((origin-pose.xyz)/pose.w,gemInverseRay(ray),center,extent,1e20);
}

// Exact contribution of the cache's primary reflection, not a reflection
// recomputed at a different screen point. Depth1 escaped leaves originate at
// the first exterior hit; all entered-glass branches escape at depth2 or later.
fn rubyLiveFrontReflection(origin:vec3<f32>,direction:vec3<f32>,componentId:i32,
                           random:vec2<f32>)->vec3<f32> {
    let responseAddress=rubyResponseAddress(origin,direction,componentId,random);
    if(responseAddress<0){return vec3<f32>(0.0);}
    let address=u32(responseAddress);
    let slot=rubyResponseSlot(componentId);
    if(slot<0){return vec3<f32>(0.0);}
    let pose=rubyPlan.rows[2u+u32(slot)*8u];
    var front=vec3<f32>(0.0);
    for(var lobe:u32=0u;lobe<8u;lobe++){
        let leaf=address+2u+3u*lobe;
        let position=rubyResponse.rows[leaf];
        if(i32(position.w)!=1){continue;}
        let weight=rubyResponse.rows[leaf+2u].xyz;
        if(gemMax(weight)<=0.0){continue;}
        let exitOrigin=pose.xyz+position.xyz*pose.w;
        let exitDirection=gemSafeUnit(rubyResponse.rows[leaf+1u].xyz);
        front+=weight*gemTerminal(exitOrigin,exitDirection).xyz;
    }
    return front;
}

// Approximation boundary: only the first refracted segment has engraving.
// The cache contains no coarse engraved radiance or opacity, including its
// internally reflected branches. Its glass transport remains otherwise intact.
fn rubyLiveEngraving(origin:vec3<f32>,direction:vec3<f32>,hit:GemHit,
                     random:vec2<f32>,cachedRGB:vec3<f32>)->vec3<f32> {
    if(!rubyLiveTextOwner(hit.panel)||hit.distance<=0.0){return cachedRGB;}
    let base=gemPanelRow(hit.panel);
    let material=gemRow(base);
    if(material.w<0.0001||material.y<=0.0||gemRow(8).z<=1.0){return cachedRGB;}
    let n=hit.normal;
    // At the primary event, depth and event are zero, so canonical xi=random.
    let micro=gemMicroNormal(n,-direction,material.z,random);
    let ray=refract(direction,micro,1.0/material.y);
    if(dot(ray,n)>=0.0||dot(ray,ray)<0.5){return cachedRGB;}
    let f=gemFresnel(dot(-direction,micro),1.0,material.y);
    let alpha=max(material.z*material.z,0.00001);
    let weight=((1.0-f)*material.w/(material.y*material.y))
        *gemRow(base+4).xyz
        *gemSampleMask(dot(-direction,n),dot(ray,n),alpha)
        /gemEnergy(hit.panel,false,dot(-direction,n));
    if(gemMax(weight)<=0.00005){return cachedRGB;}
    let point=origin+direction*hit.distance;
    let dimensions=gemRow(base+3).xyz;
    let epsilon=max(0.0000002,min(min(dimensions.x,dimensions.y),dimensions.z)*0.00001);
    let inside=point+ray*epsilon;
    if(!rubyLiveTextSegmentMayHit(inside,ray,hit.panel)){return cachedRGB;}
    let exitHit=rubyComponentHit(inside,ray,hit.panel,hit.panel,1e20);
    if(exitHit.panel!=hit.panel||exitHit.distance<=0.0){return cachedRGB;}
    var engraving=vec4<f32>(0.0,0.0,0.0,1.0);
    if(hit.panel<4){
        engraving=gemEngraving(inside,ray,hit.panel,exitHit.distance);
    }else{
        engraving=gemControlEngraving(inside,ray,hit.panel,exitHit.distance);
        let navigation=gemPluginNavigationEngraving(inside,ray,hit.panel,exitHit.distance);
        engraving=vec4<f32>(engraving.xyz+engraving.w*navigation.xyz,
                            engraving.w*navigation.w);
    }
    // Do not perturb the glass outside the actual refracted glyph coverage.
    if(engraving.w==1.0&&all(engraving.xyz==vec3<f32>(0.0))){return cachedRGB;}
    let front=rubyLiveFrontReflection(origin,direction,hit.panel,random);
    // The front lobe did not enter the glass and must not become opaque text.
    // Clamp only subtraction roundoff; both operands use the same cached lobe.
    let behind=max(cachedRGB-front,vec3<f32>(0.0));
    return front+engraving.w*behind+weight*engraving.xyz;
}
"""


def transform(source: str) -> str:
    """Unengraved optical cache plus full-resolution first-segment lettering."""
    if "fn rubyLiveEngraving(" in source:
        raise ValueError("Ruby live engraving is already present")
    required = (
        "// Exterior policy: resolve the studio directly without scene occlusion.",
        "fn rubyComponentHit(", "fn rubyResponseAddress(", "fn rubyBakeTrace(",
    )
    for marker in required:
        if source.count(marker) != 1:
            raise ValueError("Requires approved generated ruby WGSL: " + marker)
    bake = _function(source, "rubyBakeTrace")
    unengraved = bake
    for block in _ENGRAVING_BLOCKS:
        unengraved = _once(unengraved, block, "")
    source = _once(source, bake, unengraved)

    trace = _function(source, "gemTrace")
    if "var current:GemRay=GemRay(origin,-1,direction,0,vec3<f32>(1.0),1.0);" not in trace:
        raise ValueError("Live engraving requires unit primary throughput")
    if "if(event==0&&!finished)" not in trace:
        raise ValueError("Live engraving requires primary-only cache playback")
    # Keep ruby_resolve._PLAYBACK byte-for-byte recognizable. Its entire cached
    # result is bracketed, then transformed before marking this ray finished.
    adjusted = _once(trace,
        "            if(responseAddress>=0){\n",
        "            if(responseAddress>=0){\n"
        "                let rubyTextBefore=result;\n")
    adjusted = _once(adjusted,
        "                rubyUsed=true;finished=true;",
        "                // RUBY_LIVE_ENGRAVING_PLAYBACK\n"
        "                result=rubyTextBefore+rubyLiveEngraving(current.origin,current.direction,hit,random,result-rubyTextBefore);\n"
        "                rubyUsed=true;finished=true;")
    return _once(source, trace, adjusted) + _SUPPORT


def finish_resolved(source: str) -> str:
    """After ruby_resolve.transform, resolve front reflection into spare row28.

    The host's existing cs_ruby_resolve dispatch must run after every dirty bake
    and before drawing. No extra buffer, dispatch or changed workgroup is needed.
    """
    if "fn rubyLiveEngraving(" not in source:
        raise ValueError("Apply transform before finishing the resolved variant")
    resolve = _function(source, "cs_ruby_resolve")
    updated = _once(resolve,
        "        rubyResponse.rows[address+27u]=vec4<f32>(0.0);",
        "        rubyResponse.rows[address+27u]=vec4<f32>(0.0);\n"
        "        rubyResponse.rows[address+28u]=vec4<f32>(0.0);")
    updated = _once(updated,
        "        var result=vec3<f32>(0.0);",
        "        var result=vec3<f32>(0.0);\n"
        "        var front=vec3<f32>(0.0);")
    updated = _once(updated,
        "            result+=weight*terminal.xyz;",
        "            result+=weight*terminal.xyz;\n"
        "            if(i32(position.w)==1){front+=weight*terminal.xyz;}")
    updated = _once(updated,
        "        rubyResponse.rows[address+27u]=vec4<f32>(result,1.0);",
        "        rubyResponse.rows[address+27u]=vec4<f32>(result,1.0);\n"
        "        rubyResponse.rows[address+28u]=vec4<f32>(front,1.0);")
    source = _once(source, resolve, updated)
    front = _function(source, "rubyLiveFrontReflection")
    return _once(source, front, r"""fn rubyLiveFrontReflection(origin:vec3<f32>,direction:vec3<f32>,componentId:i32,
                           random:vec2<f32>)->vec3<f32> {
    let responseAddress=rubyResponseAddress(origin,direction,componentId,random);
    if(responseAddress<0){return vec3<f32>(0.0);}
    return rubyResponse.rows[u32(responseAddress)+28u].xyz;
}""")
