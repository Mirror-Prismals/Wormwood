"""Diagnostic: resolve approved ruby cache lighting once per cache sample.

The host must dispatch cs_ruby_resolve after any intrinsic bake and before every
render. This preserves the complete approved trace for uncached samples. It is
not a runtime integration, and deliberately does not invent a lighting key.
"""


_PLAYBACK = """                let responseSlot=rubyResponseSlot(hit.panel);
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
"""

_RESOLVE = """
// Diagnostic separate pass: row27 is unused by the intrinsic transport cache.
// Each invocation owns one texel, and reads only intrinsic rows0..26 before
// writing row27. A separate pass boundary makes the results visible to shading.
@compute @workgroup_size(8,8,1)
fn cs_ruby_resolve(@builtin(global_invocation_id) id:vec3<u32>) {
    if(arrayLength(&rubyPlan.rows)<161u||id.z>=20u||f32(id.z)>=rubyPlan.rows[0].x||rubyPlan.rows[0].y<0.5){return;}
    let at=1u+id.z*8u;let descriptor=rubyPlan.rows[at];
    let edge=u32(descriptor.y);
    if(descriptor.w<0.5||(edge!=64u&&edge!=128u)||id.x>=edge||id.y>=edge){return;}
    let componentId=i32(descriptor.x);
    if(componentId<0||componentId>=16644){return;}
    let pose=rubyPlan.rows[at+1u];
    if(pose.w<=0.0){return;}
    let first=u32(descriptor.z)+(id.y*edge+id.x)*60u;
    if(first+59u>=arrayLength(&rubyResponse.rows)){return;}
    for(var sample:u32=0u;sample<2u;sample++){
        let address=first+sample*30u;
        rubyResponse.rows[address+27u]=vec4<f32>(0.0);
        if(rubyResponse.rows[address].w<0.5){continue;}
        var result=vec3<f32>(0.0);
        result+=rubyResponse.rows[address+26u].xyz;
        for(var lobe:u32=0u;lobe<8u;lobe++){
            let leaf=address+2u+3u*lobe;
            let weight=rubyResponse.rows[leaf+2u].xyz;
            let priority=gemMax(weight);if(priority<=0.0){continue;}
            let position=rubyResponse.rows[leaf];
            let exitOrigin=pose.xyz+position.xyz*pose.w;
            let exitDirection=gemSafeUnit(rubyResponse.rows[leaf+1u].xyz);
            let terminal=gemTerminal(exitOrigin,exitDirection);
            result+=weight*terminal.xyz;
        }
        rubyResponse.rows[address+27u]=vec4<f32>(result,1.0);
    }
}
"""


def _function(source: str, name: str) -> str:
    marker = 'fn ' + name + '('
    if source.count(marker) != 1:
        raise ValueError('Expected exactly one function: ' + name)
    start = source.index(marker)
    opening = source.index('{', start)
    depth = 1
    end = opening + 1
    while depth and end < len(source):
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    if depth:
        raise ValueError('Unclosed function: ' + name)
    return source[start:end]


def transform(source: str) -> str:
    """Return the approved shader with exact shared texel lighting playback."""
    if 'fn cs_ruby_resolve(' in source:
        raise ValueError('Ruby resolve shader is already present')
    terminal = _function(source, 'gemTerminal')
    normalized = ''.join(line.split('//', 1)[0] for line in terminal.splitlines())
    normalized = ''.join(normalized.split())
    expected = ('fngemTerminal(origin:vec3<f32>,ray:vec3<f32>)->vec4<f32>{'
        'letenvironment=gemEnvironment(origin,ray);varradiance=environment.xyz;'
        'if(radiance.x<0.0){radiance=gemFloor(origin+ray*environment.w,-ray);}'
        'returnvec4<f32>(radiance,1.0);}')
    if normalized != expected:
        raise ValueError('Resolve requires the approved direct-studio terminal')
    if source.count(_PLAYBACK) != 1:
        raise ValueError('Unrecognized ruby cache playback ABI')
    if 'rubyResponse.rows[address+27u]=vec4<f32>(0.0);' not in _function(source, 'cs_ruby_response'):
        raise ValueError('Cache row27 is not available for resolved lighting')
    trace = _function(source, 'gemTrace')
    if 'if(event==0&&!finished)' not in trace or _PLAYBACK not in trace:
        raise ValueError('Resolve requires primary-only cache playback')
    if 'var current:GemRay=GemRay(origin,-1,direction,0,vec3<f32>(1.0),1.0);' not in trace:
        raise ValueError('Resolve requires unit primary throughput')
    return source.replace(_PLAYBACK,
        '                let address=u32(responseAddress);\n'
        '                result+=current.weight*rubyResponse.rows[address+27u].xyz;\n', 1) + _RESOLVE


source = transform
