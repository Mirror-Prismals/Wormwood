"""Append a conservative, queue-free cached-pixel diagnostic WGSL kernel.

The original shader remains unchanged. A host must run its workstationShade
for every pixel whose workstationFastShade result has negative alpha. Accepted
pixels retain the reference's cached-sample count in alpha, including zero for
pixels that only see the environment. This is a diagnostic, not a release path.
"""


_SHADE = """fn workstationShade(screen:vec2<f32>)->vec4<f32> {
    rubyUsedSamples=0u;
    var result:vec3<f32>=vec3<f32>(0.0);var samples:f32=clamp(gemRow(0).z,1.0,4.0);
    var camera:vec4<f32>=gemRow(1);var right:vec4<f32>=gemRow(2);var up:vec4<f32>=gemRow(3);var forward:vec4<f32>=gemRow(4);
    var resolution:vec2<f32>=max(gemRow(8).xy,vec2<f32>(1.0));
    // Fixed stratification is stable while panels animate. No frame-random flicker.
    for(var sampleIndex:i32=0;sampleIndex<4;sampleIndex++){
        if(f32(sampleIndex)>=samples){break;}
        var xi:vec2<f32>=fract(vec2<f32>(0.375,0.125)+vec2<f32>(0.5,0.75)*f32(sampleIndex));
        var pixel:vec2<f32>=screen+(xi-vec2<f32>(0.5))*2.0/resolution;
        var offset:vec3<f32>=right.xyz*(pixel.x*right.w)+up.xyz*(pixel.y*up.w);
        var origin:vec3<f32>=camera.xyz+offset;var direction:vec3<f32>=gemSafeUnit(forward.xyz);
        if(camera.w>0.5){origin=camera.xyz;direction=gemSafeUnit(forward.xyz*forward.w+offset);}
        var random:vec2<f32>=fract(vec2<f32>(0.324717957,0.754877666)+vec2<f32>(0.618033989,0.414213562)*f32(sampleIndex));
        result+=gemTrace(origin,direction,random);
    }
    return vec4<f32>(gemDisplay(result/samples),f32(rubyUsedSamples));
}"""


_CACHE_PLAYBACK = """                let responseSlot=rubyResponseSlot(hit.panel);
                let pose=rubyPlan.rows[2u+u32(responseSlot)*8u];
                let address=u32(responseAddress);
                result+=current.weight*rubyResponse.rows[address+26u].xyz;
                for(var lobe:u32=0u;lobe<8u;lobe++){
                    let leaf=address+2u+3u*lobe;
                    let weight=current.weight*rubyResponse.rows[leaf+2u].xyz;
                    let priority=gemMax(weight);if(priority<=0.0){continue;}
                    let position=rubyResponse.rows[leaf];
                    let exitOrigin=pose.xyz+position.xyz*pose.w;
                    let exitDirection=gemSafeUnit(rubyResponse.rows[leaf+1u].xyz);
                    let terminal=gemTerminal(exitOrigin,exitDirection);
                    if(terminal.w>0.5){result+=weight*terminal.xyz;}
                    else if(count<16){
                        pending[count]=GemRay(exitOrigin,-1,exitDirection,i32(position.w),weight,priority);count++;
                    }
                }
                rubyUsed=true;finished=true;rubyUsedSamples+=1u;"""


_FAST_TRACE = """
// Alpha: -1 means the entire pixel must use the unchanged full shader;
// 0 is direct environment and 1 is a fully resolved cached sample.
fn rubyFastTrace(origin:vec3<f32>,direction:vec3<f32>,random:vec2<f32>)->vec4<f32> {
    let environment=gemEnvironment(origin,direction);
    let hit=gemNearest(origin,direction,-1,environment.w);
    if(hit.panel<0||hit.distance<=0.0){
        var radiance=environment.xyz;
        if(radiance.x<0.0){radiance=gemFloor(origin+direction*environment.w,-direction);}
        return vec4<f32>(radiance,0.0);
    }
    let responseAddress=rubyResponseAddress(origin,direction,hit.panel,random);
    if(responseAddress<0){return vec4<f32>(0.0,0.0,0.0,-1.0);}
    let responseSlot=rubyResponseSlot(hit.panel);
    let pose=rubyPlan.rows[2u+u32(responseSlot)*8u];
    let address=u32(responseAddress);
    var result=vec3<f32>(0.0);
    result+=vec3<f32>(1.0)*rubyResponse.rows[address+26u].xyz;
    for(var lobe:u32=0u;lobe<8u;lobe++){
        let leaf=address+2u+3u*lobe;
        let weight=vec3<f32>(1.0)*rubyResponse.rows[leaf+2u].xyz;
        let priority=gemMax(weight);if(priority<=0.0){continue;}
        let position=rubyResponse.rows[leaf];
        let exitOrigin=pose.xyz+position.xyz*pose.w;
        let exitDirection=gemSafeUnit(rubyResponse.rows[leaf+1u].xyz);
        let terminal=gemTerminal(exitOrigin,exitDirection);
        if(terminal.w>0.5){result+=weight*terminal.xyz;}
        else{return vec4<f32>(0.0,0.0,0.0,-1.0);}
    }
    return vec4<f32>(result,1.0);
}
"""


def source(original: str) -> str:
    """Return original WGSL plus workstationFastShade; reject unknown ABIs."""
    markers = (
        _SHADE,
        _CACHE_PLAYBACK,
        'var<private> rubyUsedSamples:u32=0u;',
        'fn gemTrace(origin:vec3<f32>,direction:vec3<f32>,random:vec2<f32>)->vec3<f32> {',
        'fn gemNearest(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32)->GemHit {',
        'fn gemEnvironment(origin:vec3<f32>,ray:vec3<f32>)->vec4<f32> {',
        'fn gemTerminal(origin:vec3<f32>,ray:vec3<f32>)->vec4<f32> {',
        'fn rubyResponseSlot(componentId:i32)->i32 {',
        'fn rubyResponseAddress(origin:vec3<f32>,direction:vec3<f32>,componentId:i32,random:vec2<f32>)->i32 {',
        '@group(0) @binding(2) var<storage,read_write> rubyResponse:RubyResponseData;',
        '@group(0) @binding(3) var<storage,read> rubyPlan:RubyResponseData;',
    )
    for marker in markers:
        if original.count(marker) != 1:
            raise ValueError('Unrecognized ruby split shader ABI: ' + marker.splitlines()[0])
    if 'fn workstationFastShade(' in original or 'fn rubyFastTrace(' in original:
        raise ValueError('Ruby split kernel is already present')
    fast_shade = _SHADE.replace('fn workstationShade(', 'fn workstationFastShade(', 1)
    fast_shade = fast_shade.replace(
        '    rubyUsedSamples=0u;',
        '    rubyUsedSamples=0u;\n'
        '    if(gemRow(0).z!=2.0||gemRow(8).w<1.0){return vec4<f32>(0.0,0.0,0.0,-1.0);}',
        1,
    )
    fast_shade = fast_shade.replace(
        '        result+=gemTrace(origin,direction,random);',
        '        let fastSample=rubyFastTrace(origin,direction,random);\n'
        '        if(fastSample.w<0.0){rubyUsedSamples=0u;return vec4<f32>(0.0,0.0,0.0,-1.0);}\n'
        '        result+=fastSample.xyz;rubyUsedSamples+=u32(fastSample.w);',
        1,
    )
    return original + '\n' + _FAST_TRACE + '\n' + fast_shade + '\n'
