"""Pack existing blank gem buttons and magnetic tape for document track rows.

This is a read-only import of the authored GLBs. Tape's flat base shape is
selected explicitly; its optional placement/peel morph is not active here.
No example audio waveform is imported. Run directly; no Blender is required.
"""
import copy
import hashlib
import json
import math
import struct

import export_input_monitor as mesh

ART = mesh.ROOT / 'Art/AudioWorkstation'
OUT = ART / 'Timeline'
BASE = 2_910_720  # after ControlEngraving's immutable capacity
CAPACITY = 262_144
FILES = ['GemButtons/GemButton_Wide.glb', 'GemButtons/GemButton_Mini.glb',
         'TapeClips/TapeClip_Long.glb']


def material_rows(material, energy):
    p = material.get('pbrMetallicRoughness', {})
    e = material.get('extensions', {})
    volume = e.get('KHR_materials_volume', {})
    rows = [[0.] * 4 for _ in range(40)]
    ior = e.get('KHR_materials_ior', {}).get('ior', 1.5)
    rows[0] = [1., ior, p.get('roughnessFactor', 1.),
               e.get('KHR_materials_transmission', {}).get('transmissionFactor', 0.)]
    rows[1] = [-math.log(max(x, 1e-20)) / volume.get('attenuationDistance', 1.)
               for x in volume.get('attenuationColor', [1., 1., 1.])] + [0.]
    rows[3] = [.001, .001, .001, 0.]
    rows[4] = p.get('baseColorFactor', [1., 1., 1., 1.])
    rows[5] = material.get('emissiveFactor', [0., 0., 0.]) + [0.]
    for at, key in [(32, 'ggx_glass_E'), (36, 'ggx_glass_inv_E')]:
        values = energy[key] if abs(ior - 1.766) < 1e-4 else [1.] * 16
        for n in range(4):
            rows[at+n] = values[n*4:n*4+4]
    return rows


def main():
    energy = json.loads((ART / 'GemPanels/GemPanels_runtime.json').read_text())['materials'][0]['ggx_multiscatter']
    rows, assets = [], []
    for file in FILES:
        document, binary, digest = mesh.read_glb(ART / file)
        original = document
        document = copy.deepcopy(document)
        morphs = 0
        for model in document['meshes']:
            mesh.require(all(weight == 0 for weight in model.get('weights', [])),
                         'timeline tape must start in the authored flat pose')
            for primitive in model['primitives']:
                morphs += len(primitive.pop('targets', []))
        for node in document['nodes']:
            mesh.require(all(weight == 0 for weight in node.get('weights', [])),
                         'timeline node must have zero morph weight')
        triangles, components = mesh.unpack_triangles(document, binary, expected_counts=None)
        material_base = BASE + len(rows)
        mesh.require(len(document['materials']) < 16, 'timeline material index overflow')
        for material in document['materials']:
            rows.extend(material_rows(material, energy))
        geometry = mesh.pack_bvh(triangles, rows, base=BASE)
        assets.append({'name': file.split('/')[-1][:-4], 'source': file,
                       'source_sha256': digest, 'material_base': material_base,
                       'materials': len(document['materials']), 'flat_morph_targets': morphs,
                       **geometry})
        mesh.require(original['meshes'], 'empty authored source')
    mesh.require(len(rows) <= CAPACITY, 'timeline kit exceeds immutable capacity')
    blob = b''.join(struct.pack('<4f', *row) for row in rows)
    OUT.mkdir(exist_ok=True)
    (OUT / 'runtime.bin').write_bytes(blob)
    (OUT / 'runtime.json').write_text(json.dumps({
        'schema': 1, 'base': BASE, 'capacity': CAPACITY, 'rows': len(rows),
        'runtime_sha256': hashlib.sha256(blob).hexdigest(), 'assets': assets,
        'duration_method': 'uniform authored tape tiles clipped at exact elapsed-time boundaries; no X stretching',
        'waveform': 'none; only source-derived content may be added'
    }, indent=2) + '\n')
    print(f'Timeline: {len(rows)} immutable rows; ' + ', '.join(a['name'] + ': ' + str(a['triangles']) + ' triangles' for a in assets))


if __name__ == '__main__':
    main()
