"""Diagnostic spatial interpolation of the primary +Z-view studio floor.

Apply ``transform(production_resolver_wgsl)``. No production files are edited.
Each cs_ruby_resolve dispatch evaluates the original floor lighting on a 128²
viewport lattice before descriptor-specific returns. Values occupy sample0's
unused row29 in the first panel slot: (y*128+x)*60+29. Buffer ABI is unchanged.

The shading wrapper bilinearly interpolates those values only for exactly
(0,0,1) view, a finite point inside the camera-XY viewport, no custom style
background, and four valid cache samples. Other queries retain gemFloorExact.
Every resolver invocation sets a private guard before any work, so its own
lighting always uses the original function and cannot race the floor cache.

This deliberately approximates the spatial variation of floor lighting. It is
not algebraically equivalent, and soft shadows/specular highlights can change.
The parent must measure GPU time and compare output before accepting it.
"""


def _once(source: str, old: str, new: str) -> str:
    if source.count(old) != 1:
        raise ValueError("Ruby floor-resolve marker changed: " + old.splitlines()[0])
    return source.replace(old, new, 1)


_RESOLVE_ENTRY = """fn cs_ruby_resolve(@builtin(global_invocation_id) id:vec3<u32>) {
    // The private guard covers every invocation, including those which return
    // after filling the floor because their ruby descriptor is inactive.
    rubyResolving=true;
    if(id.z==0u&&id.x<128u&&id.y<128u){
        let floorAddress=(id.y*128u+id.x)*60u+29u;
        if(floorAddress<arrayLength(&rubyResponse.rows)){
            rubyResponse.rows[floorAddress]=vec4<f32>(0.0);
            let camera=gemRow(1).xy;
            let halfExtent=vec2<f32>(gemRow(2).w,gemRow(3).w);
            let supported=gemRow(245).z<=0.5&&all(halfExtent>vec2<f32>(0.0))
                &&all(abs(halfExtent)<vec2<f32>(1e30))&&all(abs(camera)<vec2<f32>(1e30));
            if(supported){
                // Endpoint samples cover the complete viewport, including its
                // boundary, without extrapolating the first/last half-cell.
                let uv=vec2<f32>(id.xy)/127.0;
                let xy=camera-halfExtent+uv*(2.0*halfExtent);
                let radiance=gemFloorExact(vec3<f32>(xy,gemRow(5).w),vec3<f32>(0.0,0.0,1.0));
                rubyResponse.rows[floorAddress]=vec4<f32>(radiance,1.0);
            }
        }
    }
"""


_SUPPORT = r"""
// Diagnostic: floor radiance interpolates over space, never over view angle.
// Reset by WGSL for every shader invocation; only the resolver sets it true.
var<private> rubyResolving:bool=false;

fn rubyFloorSample(pixel:vec2<u32>)->vec4<f32> {
    let address=(pixel.y*128u+pixel.x)*60u+29u;
    if(address>=arrayLength(&rubyResponse.rows)){return vec4<f32>(0.0);}
    return rubyResponse.rows[address];
}

fn gemFloor(point:vec3<f32>,view:vec3<f32>)->vec3<f32> {
    if(rubyResolving||any(view!=vec3<f32>(0.0,0.0,1.0))||gemRow(245).z>0.5){
        return gemFloorExact(point,view);
    }
    let camera=gemRow(1).xy;
    let halfExtent=vec2<f32>(gemRow(2).w,gemRow(3).w);
    // Ordered comparisons also reject NaN. Bounds prevent overflow during
    // normalization while remaining vastly larger than authored metre ranges.
    if(!all(abs(point)<vec3<f32>(1e30))||!all(abs(camera)<vec2<f32>(1e30))
        ||!all(halfExtent>vec2<f32>(0.0))||!all(halfExtent<vec2<f32>(1e30))){
        return gemFloorExact(point,view);
    }
    let uv=(point.xy-camera+halfExtent)/(2.0*halfExtent);
    if(any(uv<vec2<f32>(0.0))||any(uv>vec2<f32>(1.0))||any(uv!=uv)){
        return gemFloorExact(point,view);
    }
    let coordinate=uv*127.0;
    let low=vec2<u32>(floor(coordinate));
    let high=min(low+vec2<u32>(1u),vec2<u32>(127u));
    let a=rubyFloorSample(low);
    let b=rubyFloorSample(vec2<u32>(high.x,low.y));
    let c=rubyFloorSample(vec2<u32>(low.x,high.y));
    let d=rubyFloorSample(high);
    if(a.w!=1.0||b.w!=1.0||c.w!=1.0||d.w!=1.0){return gemFloorExact(point,view);}
    let blend=fract(coordinate);
    return mix(mix(a.xyz,b.xyz,blend.x),mix(c.xyz,d.xyz,blend.x),blend.y);
}
"""


def transform(source: str) -> str:
    """Return a private candidate; reject incompatible or duplicate transforms."""
    if "rubyResolving" in source or "fn gemFloorExact(" in source:
        raise ValueError("Floor-resolve candidate is already present")
    if "rubyResponse.rows[address+29u]=vec4<f32>(0.0);" not in source:
        raise ValueError("Ruby response spare-row ABI changed")
    source = _once(source, "fn gemFloor(point:vec3<f32>,view:vec3<f32>)->vec3<f32> {",
                   "fn gemFloorExact(point:vec3<f32>,view:vec3<f32>)->vec3<f32> {")
    source = _once(source, "fn cs_ruby_resolve(@builtin(global_invocation_id) id:vec3<u32>) {\n",
                   _RESOLVE_ENTRY)
    return source + _SUPPORT


def metadata() -> dict:
    return {
        "kind": "diagnostic-spatial-floor-radiance-interpolation",
        "edge": 128, "samples": 16384, "view": [0., 0., 1.],
        "storage": "first panel slot, sample0, row29; (y*128+x)*60+29",
        "interpolation": "bilinear, viewport endpoint lattice",
        "resolver_reads_exact_floor": True,
        "approximation": "Spatial floor illumination, shadows and highlights are interpolated; appearance is not mathematically identical.",
    }
