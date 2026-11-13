"""Refresh shared table equations without modifying backend storage adapters.
Run after editing Procedures/Shaders/table_material.{wgsl,glsl}.
"""
from pathlib import Path
import runpy
ROOT=Path(__file__).resolve().parents[2]/'Procedures/Shaders'
for name,wgsl,ending in [('triangle.wgsl',True,'@fragment fn fs_main'),
                         ('triangle.frag',False,'vec4 surface('),
                         ('triangle.gl.frag',False,'vec4 surface('),
                         ('theatre_camera.wgsl',True,None),
                         ('theatre_camera.glsl',False,None),
                         ('theatre_camera.vulkan.glsl',False,None)]:
    path=ROOT/name;text=path.read_text();start=text.index('fn tableMap(' if wgsl else 'vec4 tableMap(')
    surface=text.index('fn tableSurface(' if wgsl else 'vec4 tableSurface(',start)
    brace=text.index('{',surface);depth=1;end=brace+1
    while depth:
        if text[end]=='{':depth+=1
        elif text[end]=='}':depth-=1
        end+=1
    source=(ROOT/('table_material.wgsl' if wgsl else 'table_material.glsl')).read_text()
    # Include the GLSL explanatory comment as part of the shared source only.
    prefix=text[:start]
    comment=source.splitlines()[0]+'\n'
    if comment.startswith('//'):
        while prefix.endswith(comment): prefix=prefix[:-len(comment)]
    path.write_text(prefix+source+'\n'+text[end:])
runpy.run_path(str(ROOT.parents[1]/'BuildSupport/Theatre/update_exterior_shaders.py'),run_name='__main__')
