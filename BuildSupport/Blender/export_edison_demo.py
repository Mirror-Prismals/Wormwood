"""Pack the rounded Edison socket as an immutable workstation model demo.

Reuses the opaque-GLB importer and BVH encoding, leaving the existing mixer,
monitor and engraving packets intact. No Blender process is needed here.
"""
import hashlib
import json
import math
import struct

import export_input_monitor as mesh

ART = mesh.ROOT / 'Art/AudioWorkstation/EdisonHardware'
BASE = mesh.BASE + mesh.CAPACITY
CAPACITY = 32768


def authored(v):
    # The old painter skewed its 2D drawing to suggest depth. These models
    # already have depth: preserve their mating axes, including their normals.
    return tuple(mesh.f32(c) for c in v)


def main():
    document, binary, digest = mesh.read_glb(ART / 'EdisonSocket.glb')
    triangles, components = mesh.unpack_triangles(document, binary, expected_counts=None)
    mesh.require(len(components) == 4, 'Edison socket needs housing and three slot lips')
    materials = document['materials']
    mesh.require(0 < len(materials) < 256, 'Edison material count exceeds instance encoding')
    rows = [row for material in materials for row in mesh.material_rows(material)]
    posed = []
    for vertices, normals, material, *_ in triangles:
        vertices, normals = tuple(map(authored, vertices)), tuple(map(authored, normals))
        lo = tuple(min(v[k] for v in vertices) for k in range(3))
        hi = tuple(max(v[k] for v in vertices) for k in range(3))
        center = tuple(sum(v[k] for v in vertices)/3 for k in range(3))
        posed.append((vertices, normals, material, lo, hi, center))
    geometry = mesh.pack_bvh(posed, rows, base=BASE)
    mesh.require(len(rows) <= CAPACITY and geometry['node_count'] <= 4096,
                 'Edison socket exceeds its bounded runtime packet')
    mesh.require(BASE + len(rows) < 2**24, 'row indices must remain exact float32 integers')
    mesh.require(all(math.isfinite(v) for row in rows for v in row), 'nonfinite Edison packet')
    blob = b''.join(struct.pack('<4f', *row) for row in rows)
    manifest = {'schema': 1, 'base': BASE, 'capacity': CAPACITY, 'rows': len(rows),
                'material_count': len(materials), 'rotation_degrees_xyz': [0, 0, 0],
                'source_sha256': {'EdisonSocket.glb': digest},
                'runtime_sha256': hashlib.sha256(blob).hexdigest(), **geometry}
    (ART / 'runtime.bin').write_bytes(blob)
    (ART / 'runtime.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'Edison: {len(triangles)} authored triangles, {len(rows)} immutable rows')


if __name__ == '__main__':
    main()

# A second immutable packet holds the connected silhouette. Keep the original
# socket and bubble packet addresses stable for every concurrent scene client.
def connected():
    base = ((BASE + CAPACITY + 255) & ~255) + 32768 + 2 * 262144
    capacity = 262144
    materials, posed, digests = [], [], {}
    for filename in ('EdisonSocket.glb', 'EdisonPlug.glb'):
        doc, binary, digest = mesh.read_glb(ART / filename)
        triangles, _ = mesh.unpack_triangles(doc, binary, expected_counts=None)
        material_offset = len(materials)
        materials.extend(doc['materials'])
        digests[filename] = digest
        for vertices, normals, material, *_ in triangles:
            # Both Blender roots are the same mating plane, with identical
            # metre scale. No flattening or per-component correction.
            vertices, normals = tuple(map(authored, vertices)), tuple(map(authored, normals))
            lo = tuple(min(v[k] for v in vertices) for k in range(3))
            hi = tuple(max(v[k] for v in vertices) for k in range(3))
            center = tuple(sum(v[k] for v in vertices)/3 for k in range(3))
            posed.append((vertices, normals, material + material_offset, lo, hi, center))
    rows = [row for material in materials for row in mesh.material_rows(material)]
    geometry = mesh.pack_bvh(posed, rows, base=base)
    mesh.require(len(rows) <= capacity and geometry['node_count'] <= 16384, 'connected Edison capacity')
    blob = b''.join(struct.pack('<4f', *row) for row in rows)
    manifest = {'schema': 1, 'base': base, 'capacity': capacity, 'rows': len(rows),
                'material_count': len(materials), 'rotation_degrees_xyz': [0, 0, 0], 'source_sha256': digests,
                'runtime_sha256': hashlib.sha256(blob).hexdigest(), **geometry}
    (ART / 'connected.bin').write_bytes(blob)
    (ART / 'connected.json').write_text(json.dumps(manifest, indent=2)+'\n')
    print(f'Connected Edison: {len(posed)} triangles, {len(rows)} rows')

if __name__ == '__main__':
    connected()
