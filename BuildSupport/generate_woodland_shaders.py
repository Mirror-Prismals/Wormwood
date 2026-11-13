"""Generate the retained tree-bark sampler. Ground never samples this atlas."""
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
raw = (ROOT / 'Procedures/Assets/Woodland/materials.rgba').read_bytes()[8:]
START = '// BEGIN GENERATED WOODLAND MATERIALS\n'
END = '// END GENERATED WOODLAND MATERIALS\n'
averages = []
for tile in range(48):
    pixels = raw[tile*24*24*4:(tile+1)*24*24*4]
    averages.append([sum(pixels[c::4])/(576*255) for c in range(3)])

def sampler(wgsl, vulkan):
    if wgsl:
        code = 'struct WoodlandStorage {width:u32,height:u32,pixels:array<u32>};\n@group(0) @binding(2) var<storage,read> woodlandStorage:WoodlandStorage;\n'
        code += 'fn woodlandFetch(tile:i32,point:vec2<i32>)->vec4<f32>{let p=(point%vec2<i32>(24)+vec2<i32>(24))%vec2<i32>(24);return unpack4x8unorm(woodlandStorage.pixels[4194306u+u32((p.y+tile*24)*24+p.x)]);}\n'
        code += 'fn woodlandAverage(tile:i32)->vec3<f32>{\n'
        for i, c in enumerate(averages):
            code += f'if tile=={i}{{return vec3<f32>('+','.join(f'{v:.8f}' for v in c)+');}\n'
        code += 'return vec3<f32>(0.3);}\n'
        code += '''fn woodlandSample(tile:i32,uv:vec2<f32>,footprint:f32)->vec3<f32>{
 let p=fract(uv)*24.0-vec2<f32>(0.5);let i=vec2<i32>(floor(p));let f=fract(p);
 let color=mix(mix(woodlandFetch(tile,i).rgb,woodlandFetch(tile,i+vec2<i32>(1,0)).rgb,f.x),mix(woodlandFetch(tile,i+vec2<i32>(0,1)).rgb,woodlandFetch(tile,i+vec2<i32>(1,1)).rgb,f.x),f.y);
 return mix(color,woodlandAverage(tile),smoothstep(1.0,8.0,footprint*24.0));
}
'''
    else:
        code = 'layout(set=0,binding=2,std430) readonly buffer WoodlandStorage {uint width;uint height;uint pixels[];} woodlandStorage;\n' if vulkan else 'uniform sampler2D woodlandAtlas;\n'
        fetch = 'return unpackUnorm4x8(woodlandStorage.pixels[4194306u+uint((p.y+tile*24)*24+p.x)]);' if vulkan else 'return texelFetch(woodlandAtlas,ivec2(p.x,p.y+tile*24),0);'
        code += 'vec4 woodlandFetch(int tile,ivec2 p){p=ivec2((p.x%24+24)%24,(p.y%24+24)%24);'+fetch+'}\nvec3 woodlandAverage(int tile){\n'
        for i,c in enumerate(averages):
            code += f'if(tile=={i})return vec3('+','.join(f'{v:.8f}' for v in c)+');\n'
        code += 'return vec3(0.3);}\n'
        code += '''vec3 woodlandSample(int tile,vec2 uv,float footprint){
 vec2 p=fract(uv)*24.0-0.5;ivec2 i=ivec2(floor(p));vec2 f=fract(p);
 vec3 color=mix(mix(woodlandFetch(tile,i).rgb,woodlandFetch(tile,i+ivec2(1,0)).rgb,f.x),mix(woodlandFetch(tile,i+ivec2(0,1)).rgb,woodlandFetch(tile,i+ivec2(1,1)).rgb,f.x),f.y);
 return mix(color,woodlandAverage(tile),smoothstep(1.0,8.0,footprint*24.0));
}
'''
    return START+code+END

for name in ['vegetation.gl.frag','vegetation.ios.frag','vegetation.frag','vegetation.wgsl']:
    path=ROOT/'Procedures/Shaders'/name
    source=path.read_text()
    a=source.index(START);b=source.index(END,a)+len(END)
    path.write_text(source[:a]+sampler(name.endswith('wgsl'),name=='vegetation.frag')+source[b:])
