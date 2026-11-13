"""Diagnostic queue-free primary shading, with complete canonical fallback.

split(source) appends workstationFastShade for the existing split GPU probe.
transform(source) uses the same early result inside one shader, allowing a fair
comparison of compiler register pressure versus a genuinely separate dispatch.
No runtime uses this module. It accepts only the approved studio terminal.
"""
import ruby_resolve


def block(source, marker):
    if source.count(marker) != 1:
        raise ValueError('Changed fast-primary marker: ' + marker)
    start = source.index(marker)
    opening = source.index('{', start)
    end, depth = opening + 1, 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return start, opening + 1, end - 1, end


def split(source):
    if 'fn rubyFastTrace(' in source:
        raise ValueError('Fast primary is already present')
    # Validate exterior policy, including variants that already resolved it.
    terminal = ruby_resolve._function(source, 'gemTerminal')
    if 'return vec4<f32>(radiance,1.0);' not in terminal or 'gemNearest(' in terminal or 'gemAny(' in terminal:
        raise ValueError('Fast primary requires the approved direct studio terminal')
    trace = ruby_resolve._function(source, 'gemTrace')
    _, a, b, _ = block(trace, 'if(responseAddress>=0){')
    playback = trace[a:b]
    continuation = '''else if(count<16){
                        pending[count]=GemRay(exitOrigin,-1,exitDirection,i32(position.w),weight,priority);count++;
                    }'''
    playback = playback.replace(continuation, '')
    playback = playback.replace('rubyUsed=true;finished=true;rubyUsedSamples+=1u;', '')
    if 'rubyUsed' in playback or 'pending[' in playback:
        raise ValueError('Unexpected cached playback side effects')
    for old, new in [('current.weight', 'vec3<f32>(1.0)'), ('current.origin', 'origin'), ('current.direction', 'direction')]:
        playback = playback.replace(old, new)

    # Copy the primary opaque branch's exact authored lighting and flags.
    _, a, b, _ = block(trace, 'if(material.w<0.0001){')
    opaque = trace[a:b]
    opaque = opaque.replace('result+=current.weight*(opaque+gemRow(base+5).xyz);',
                            'return vec4<f32>(opaque+gemRow(base+5).xyz,0.0);')
    opaque = opaque.replace('finished=true;', '').replace('-current.direction', '-direction')
    if 'current.' in opaque or 'result+=' in opaque:
        raise ValueError('Unexpected primary opaque dependencies')
    helper = '''
fn rubyFastTrace(origin:vec3<f32>,direction:vec3<f32>,random:vec2<f32>)->vec4<f32> {
    if(gemRow(8).w<1.0){return vec4<f32>(0.0,0.0,0.0,-1.0);}
    let environment=gemEnvironment(origin,direction);
    let hit=gemNearest(origin,direction,-1,environment.w);
    if(hit.panel<0||hit.distance<=0.0){
        var radiance=environment.xyz;
        if(radiance.x<0.0){radiance=gemFloor(origin+direction*environment.w,-direction);}
        return vec4<f32>(radiance,0.0);
    }
    let responseAddress=rubyResponseAddress(origin,direction,hit.panel,random);
    if(responseAddress>=0){
        var result=vec3<f32>(0.0);
''' + playback + '''
        return vec4<f32>(result,1.0);
    }
    if((hit.panel>=0&&hit.panel<16644)||(hit.panel>=700000&&hit.panel<704096)){
        let base=gemPanelRow(hit.panel);let material=gemRow(base);
        if(material.w<0.0001){
            let point=origin+direction*hit.distance;let n=hit.normal;
''' + opaque + '''
        }
    }
    if(hit.panel>=90000&&hit.panel<90064){
        return vec4<f32>(gemAudioSphereShade(origin+direction*hit.distance,-direction,hit.normal,hit.panel),0.0);
    }
    return vec4<f32>(0.0,0.0,0.0,-1.0);
}
'''
    shade = ruby_resolve._function(source, 'workstationShade')
    shade = shade.replace('fn workstationShade(', 'fn workstationFastShade(', 1)
    old = '        result+=gemTrace(origin,direction,random);'
    if shade.count(old) != 1:
        raise ValueError('Unrecognized workstation sampling loop')
    shade = shade.replace(old, '''        let fastSample=rubyFastTrace(origin,direction,random);
        if(fastSample.w<0.0){rubyUsedSamples=0u;return vec4<f32>(0.0,0.0,0.0,-1.0);}
        result+=fastSample.xyz;rubyUsedSamples+=u32(fastSample.w);''', 1)
    return source + helper + '\n' + shade + '\n'


def transform(source):
    result = split(source)
    result = result.replace('fn gemTrace(', 'fn rubyTraceFallback(', 1)
    return result + '''
fn gemTrace(origin:vec3<f32>,direction:vec3<f32>,random:vec2<f32>)->vec3<f32> {
    let fastSample=rubyFastTrace(origin,direction,random);
    if(fastSample.w>=0.0){rubyUsedSamples+=u32(fastSample.w);return fastSample.xyz;}
    return rubyTraceFallback(origin,direction,random);
}
'''
