"""Fine loam sampling, using the same bounded relief model as meadow grass.
No new resource bindings; existing grass/rock/walnut bytes keep their offsets.
"""
import ast
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
R=ROOT/'Procedures/Shaders'
START='// BEGIN GENERATED FINE LOAM\n';END='// END GENERATED FINE LOAM\n'
tree=ast.parse((ROOT/'BuildSupport/generate_grass_shaders.py').read_text())
templates={n.targets[0].id:ast.literal_eval(n.value) for n in tree.body
           if isinstance(n,ast.Assign) and isinstance(n.targets[0],ast.Name)
           and n.targets[0].id in ['GLSL','WGSL']}
raw=(ROOT/'Procedures/Assets/Terrain/dirt_material.rgba').read_bytes()
mean=','.join(f'{v/255:.8f}' for v in raw[349524*4:349524*4+3])
for name in ['triangle.gl.frag','triangle.frag','scene.ios.frag','triangle.wgsl']:
    p=R/name;s=p.read_text();wg=name.endswith('wgsl')
    if START in s:
        a=s.index(START);b=s.index(END,a)+len(END);s=s[:a]+s[b:]
    code=templates['WGSL' if wg else 'GLSL'].replace('grass','dirt')
    code=code.replace('256>>','512>>').replace('65536-size','262144-size')
    code=code.replace('1572868','1747632').replace('87381','349525')
    code=code.replace('footprint*512.','footprint*1024.').replace('0.,8.','0.,9.').replace('4.,8.,lod','5.,9.,lod')
    code=code.replace('.167,.224,.067',mean)
    if name=='scene.ios.frag':code=code.replace('tableRead(1747632+', 'rockRead(699052+')
    a=s.index('@fragment fn fs_main' if wg else 'vec4 surface(')
    s=s[:a]+START+code+END+s[a:]
    if wg:
        needle=' if grass>0.5{return grassDiffuse(rgb,uv,footprint);}'
        branch='\n if rgb.r>rgb.g*1.28 && rgb.b<rgb.g*.85{return dirtDiffuse(rgb,uv,footprint);}'
    else:
        decl='vec3 dirtDiffuse(vec3 rgb,vec2 metres,float footprint);\n'
        if decl not in s:s=s.replace('vec3 soilDiffuse(',decl+'vec3 soilDiffuse(',1)
        needle=' if(grass>0.5)return grassDiffuse(rgb,uv,footprint);'
        branch='\n if(rgb.r>rgb.g*1.28 && rgb.b<rgb.g*.85)return dirtDiffuse(rgb,uv,footprint);'
    if branch not in s:
        assert needle in s
        s=s.replace(needle,needle+branch,1)
    # Tail length is also used by monitor/table reads relative to buffer end.
    if name in ['triangle.frag','triangle.wgsl']:
        s=s.replace('-1747632','-2446684')
    p.write_text(s)
