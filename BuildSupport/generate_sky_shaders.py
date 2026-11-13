"""Embed authored RGB profiles in each native scene shader, from one JSON source.
Run after changing Procedures/Atmosphere/sky_palette.json. No compiler required.
The analytic directional sunset follows Salamander137's Expanse sky shader.
"""
import json
import re
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
profiles = json.loads((ROOT/'Procedures/Atmosphere/sky_palette.json').read_text())['profiles']
MARKER = '// BEGIN GENERATED EXPANSE SKY\n'

def f(v):
    return format(float(v), '.10g') + ('.0' if float(v).is_integer() else '')

def celestial(wgsl):
    source=(ROOT/'Procedures/Atmosphere/celestial.glsl').read_text()
    if not wgsl:
        return source
    types={'float':'f32','vec3':'vec3<f32>'}
    def function(m):
        ret,name,args=m.groups()
        params=[n+':'+types[t] for t,n in (a.strip().split() for a in args.split(','))]
        return 'fn '+name+'('+','.join(params)+')->'+types[ret]+' {'
    source=re.sub(r'\b(float|vec3) (\w+)\(([^()]*)\)\s*\{',function,source)
    source=re.sub(r'\b(float|vec3) (\w+)\s*=',lambda m:'var '+m[2]+':'+types[m[1]]+'=',source)
    source=source.replace('vec3(', 'vec3<f32>(')
    return source

def generate(wgsl):
    v3 = 'vec3<f32>' if wgsl else 'vec3'
    def rgb(stop): return v3+'('+','.join(f(c/255) for c in stop['rgb'])+')'
    code=MARKER+celestial(wgsl)
    for i,p in enumerate(profiles):
        code += (f'fn skyProfile{i}(z:f32)->vec3<f32>{{\n' if wgsl else f'vec3 skyProfile{i}(float z){{\n')
        for a,b in zip(p['stops'],p['stops'][1:]):
            code+=f' if(z<={f(b["z"])}){{return mix({rgb(a)},{rgb(b)},clamp((z-{f(a["z"])})/{f(b["z"]-a["z"])},0.0,1.0));}}\n'
        code+=' return '+rgb(p['stops'][-1])+';\n}\n'
    code+=('fn skyPalette(minute:f32,z:f32)->vec3<f32>{\n' if wgsl else 'vec3 skyPalette(float minute,float z){\n')
    for i,(a,b) in enumerate(zip(profiles,profiles[1:])):
        code+=f' if(minute<={f(b["minute"])}){{return mix(skyProfile{i}(z),skyProfile{i+1}(z),clamp((minute-{f(a["minute"])})/{f(b["minute"]-a["minute"])},0.0,1.0));}}\n'
    code+=' return skyProfile0(z);\n}\n'
    # GLSL-like body translated below; only local declarations and vector names differ.
    body='''
 vec3 ray=normalize(direction);
 float hour=fract(-kind-10.0)*24.0;
 float angle=(hour-6.0)/12.0*3.14159265359;
 vec3 sun=vec3(0.0,sin(angle),-cos(angle));
 float elevation=asin(clamp(ray.y,0.0,1.0))*57.295779513;
 vec3 color=skyPalette(hour*60.0,1.0-elevation/90.0);
 float sunAltitude=(18.0-hour)*15.0;
 float azimuthCos=clamp(dot(sun.xz,ray.xz)/max(length(sun.xz)*length(ray.xz),0.00001),-1.0,1.0);
 float toward=pow((1.0+azimuthCos)*0.5,0.75);
 float opposite=pow((1.0-azimuthCos)*0.5,1.25);
 float strength=smoothstep(15.0,17.5,hour)*(1.0-smoothstep(18.0,19.3,hour));
 float shadowHeight=max(0.0,-sunAltitude)*1.65;
 float shadow=smoothstep(0.0,5.0,-sunAltitude)*opposite*(1.0-smoothstep(shadowHeight,shadowHeight+12.0,elevation))*(1.0-smoothstep(18.75,19.5,hour));
 color=mix(color,vec3(0.025,0.062,0.16),shadow*0.72);
 float bandHeight=12.0+max(0.0,sunAltitude)*0.55;
 float warm=strength*toward*exp(-pow(elevation/bandHeight,1.45));
 color=mix(color,vec3(0.72,0.8,0.86),smoothstep(0.0,0.45,warm)*0.82);
 color=mix(color,vec3(0.95,0.95,0.8),smoothstep(0.18,0.66,warm));
 color=mix(color,vec3(1.0,0.9,0.32),smoothstep(0.48,1.0,warm));
 // Distant celestial light is behind the original sun/moon and cloud pass.
 float night=1.0-smoothstep(-0.38,-0.08,sun.y);
 color+=celestialSky(ray,hour)*night*smoothstep(-0.02,0.22,ray.y);
 // Angular disks are an adaptation of the original billboard celestials.
 float sunDisk=smoothstep(cos(0.0048),cos(0.0043),dot(ray,sun));
 float moonDisk=smoothstep(cos(0.0048),cos(0.0043),dot(ray,-sun));
 color=mix(color,vec3(1.0,1.0,0.8),sunDisk);
 color=mix(color,vec3(205.0,215.0,233.0)/255.0,moonDisk);
 return vec4(color,1.0);
'''
    if wgsl:
        import re
        body=body.replace('vec3(', 'vec3<f32>(').replace('vec4(', 'vec4<f32>(')
        body=re.sub(r' (?:vec3|float) (\w+)=', lambda m: (' var ' if m[1]=='color' else ' let ')+m[1]+'=', body)
        code+='fn expanseSky(direction:vec3<f32>,kind:f32)->vec4<f32>{'+body+'}\n'
    else:
        code+='vec4 expanseSky(vec3 direction,float kind){'+body+'}\n'
    return code+'// END GENERATED EXPANSE SKY\n'

for path in ['triangle.gl.frag','triangle.frag','scene.ios.frag','triangle.wgsl']:
    p=ROOT/'Procedures/Shaders'/path
    src=p.read_text()
    if MARKER in src:
        start=src.index(MARKER);end=src.index('// END GENERATED EXPANSE SKY\n',start)+len('// END GENERATED EXPANSE SKY\n')
        src=src[:start]+src[end:]
    wgsl=path.endswith('wgsl')
    at=src.index('@fragment fn fs_main') if wgsl else src.index('void main()')
    src=src[:at]+generate(wgsl)+src[at:]
    if wgsl:
        needle=' if i.material<=-3.5'
        if 'if i.material<=-9.5' not in src:
            src=src.replace(needle,' if i.material<=-9.5{return expanseSky(i.data.xyz,i.material);}\n'+needle)
    else:
        needle='if(material<=-3.5)'
        if 'if(material<=-9.5)' not in src:
            src=src.replace(needle,'if(material<=-9.5){pixel=expanseSky(data.xyz,material);}else '+needle)
    # Player luminance is supplied by scene composition in data.z.
    src=src.replace('return texel;','return vec4(texel.rgb*data.z,texel.a);') if not wgsl else src.replace('return t;}','return vec4<f32>(t.rgb*i.data.z,t.a);}')
    p.write_text(src)
