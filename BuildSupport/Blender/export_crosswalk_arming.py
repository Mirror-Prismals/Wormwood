"""Pack the existing CrosswalkArming GLB and its authored LED state palettes.

Never regenerate or simplify the model. Membership/brightness comes from GLB
material extras written by make_crosswalk_arming.py. Run in the private tree.
"""
import copy
import hashlib
import json
import struct
from pathlib import Path
import export_input_monitor as mesh

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'Art/AudioWorkstation/CrosswalkArming'
BASE=3_500_544  # after the unchanged user-style allocation
CAPACITY=524_288

def linear(x):
    return x/12.92 if x<=.04045 else ((x+.055)/1.055)**2.4

def main():
    document,binary,digest=mesh.read_glb(OUT/'CrosswalkArming.glb')
    triangles,components=mesh.unpack_triangles(document,binary,expected_counts=(1,48022))
    materials=document['materials'];rows=[];palettes=[]
    mesh.require(len(materials)<256,'crosswalk material overflow')
    for mode in range(3):
        palettes.append(BASE+len(rows))
        for source in materials:
            material=copy.deepcopy(source);extra=source.get('extras',{})
            if extra.get('role')=='LED':
                lit=(mode==1 and extra['walk_mask']) or (mode==2 and extra['hand_mask'])
                color=(.96,.98,1.) if mode==1 else (1.,.30,.025)
                off=(.012,.016,.018) if extra['edge'] else (.026,.029,.031)
                rgb=[linear(c*extra['brightness']) for c in color] if lit else [linear(c) for c in off]
                material['pbrMetallicRoughness']['baseColorFactor']=rgb+[1.]
                material['emissiveFactor']=rgb
                material.setdefault('extensions',{})['KHR_materials_emissive_strength']={'emissiveStrength':(1.8 if extra['edge'] else 2.6) if lit else 0.}
            rows.extend(mesh.material_rows(material))
    geometry=mesh.pack_bvh(triangles,rows,base=BASE)
    mesh.require(len(rows)<=CAPACITY,'crosswalk allocation exceeded')
    blob=b''.join(struct.pack('<4f',*row) for row in rows)
    (OUT/'runtime.bin').write_bytes(blob)
    (OUT/'runtime.json').write_text(json.dumps({'schema':1,'base':BASE,'capacity':CAPACITY,'rows':len(rows),
        'material_count':len(materials),'material_bases':palettes,'source_sha256':digest,
        'runtime_sha256':hashlib.sha256(blob).hexdigest(),'components':components,**geometry},indent=2)+'\n')
    print(f'Crosswalk: {len(rows)} rows, {len(triangles)} unchanged triangles, {len(materials)} materials, three palettes')

if __name__=='__main__':main()
