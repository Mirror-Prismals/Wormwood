"""Validate production scene assets/shaders without unrelated workstation code.
The sole harness stub is audio::vector (JSON parsing for the table camera test).
This is not native graphics, gameplay, audio or frame-time validation.
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'output/checks/grass-material'
OUT.mkdir(parents=True,exist_ok=True)
def run(name,args,env=None):
    with (OUT/(name+'.log')).open('w') as log:
        subprocess.run(args,cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)

for shader in ['triangle.gl.frag','scene.ios.frag','triangle.frag']:
    args=['glslangValidator','-S','frag',str(ROOT/'Procedures/Shaders'/shader)]
    if shader=='triangle.frag':args+=['-V','-o',str(OUT/'grass.spv')]
    run(shader,args)

with tempfile.TemporaryDirectory(prefix='wormwood-grass-') as folder:
    h=Path(folder);(h/'src').mkdir()
    (h/'Cargo.toml').write_text('[package]\nname="grass-material-validation"\nversion="0.1.0"\nedition="2024"\n[dependencies]\nserde_json="=1.0.145"\nnaga={version="=27.0.3",features=["wgsl-in"]}\n')
    (h/'src/lib.rs').write_text('mod audio {pub fn vector(v:&serde_json::Value)->Result<[f64;3],String>{serde_json::from_value(v.clone()).map_err(|e|e.to_string())}}\n#[path='+json.dumps(str(ROOT/'Render/SceneAssets.rs'))+'] mod scene_assets;\n')
    (h/'scene_materials.rgba').write_bytes(b''.join((ROOT/p).read_bytes() for p in ['Procedures/Assets/Theatre/table_material.rgba','Procedures/Assets/Terrain/rock_materials.rgba','Procedures/Assets/Terrain/grass_material.rgba','Procedures/Assets/Terrain/dirt_material.rgba']))
    env=dict(os.environ,OUT_DIR=str(h),CARGO_TARGET_DIR=str(OUT/'isolated-target'))
    run('scene-assets',['cargo','test','--offline','--manifest-path',str(h/'Cargo.toml')],env)
print('GL, GLES, Vulkan and WGSL validation passed; production scene-asset tests passed.')
