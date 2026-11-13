"""Refresh carpet equations and dispatch; preserve explicit backend adapters."""
from pathlib import Path
import runpy
ROOT=Path(__file__).resolve().parents[2]
SH=ROOT/'Procedures/Shaders'
START='// BEGIN RED CARPET\n';END='// END RED CARPET\n'
for name in ['triangle.wgsl','triangle.frag','triangle.gl.frag','theatre_camera.wgsl','theatre_camera.glsl','theatre_camera.vulkan.glsl']:
    p=SH/name;s=p.read_text();wg=name.endswith('.wgsl');camera=name.startswith('theatre_camera')
    if START in s:s=s[:s.index(START)]+s[s.index(END)+len(END):]
    block=START+(SH/('carpet_material.wgsl' if wg else 'carpet_material.glsl')).read_text()+END
    if camera:s=s.rstrip()+'\n'+block
    else:
        pos=s.index('@fragment fn fs_main' if wg else 'vec4 surface(');s=s[:pos]+block+s[pos:]
        if 'return carpetSurface(i.data,' not in s and wg:
            needle=' if i.material>=106.0';assert needle in s
            s=s.replace(needle,' if i.material>=117. && i.material<=120.{return carpetSurface(i.data,max(length(dpdx(i.data.xyz)),length(dpdy(i.data.xyz))));}\n'+needle)
        if 'return carpetSurface(data,' not in s and not wg:
            needle=' if(material>=106.0';assert needle in s
            s=s.replace(needle,' if(material>=117. && material<=120.)return carpetSurface(data,max(length(dFdx(data.xyz)),length(dFdy(data.xyz))));\n'+needle)
    p.write_text(s)
for p in [SH/'generate_workstation.py',ROOT/'Render/WorkstationCameraColor.wgsl']:
    s=p.read_text()
    if 'return carpetSurface(input.cameraData,' not in s:
        needle='if(input.cameraData.w>=106.0';assert needle in s
        s=s.replace(needle,'if(input.cameraData.w>=117. && input.cameraData.w<=120.){return carpetSurface(input.cameraData,max(length(dpdx(input.cameraData.xyz)),length(dpdy(input.cameraData.xyz))));}'+needle)
    if p.suffix=='.py' and 'pixel=carpetSurface(cameraData,' not in s:
        needle='if(cameraData.w>=106.0';assert needle in s
        s=s.replace(needle,'if(cameraData.w>=117. && cameraData.w<=120.){pixel=carpetSurface(cameraData,max(length(dFdx(cameraData.xyz)),length(dFdy(cameraData.xyz))));return;}'+needle)
    p.write_text(s)
runpy.run_path(str(SH/'generate_workstation.py'),run_name='__main__')
