#!/usr/bin/env python3
"""Create reproducible, opt-in Mirror user-style packages without dependencies.

Geometry is authored in panel metres (X right, Y up, Z toward the viewer),
then written to GLB with Blender's glTF axis conversion (x, z, -y). These are
ordinary user packages, not additional built-in rendering branches.
"""
from pathlib import Path
import argparse
import hashlib
import json
import math
import struct

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUT = ROOT / 'Art/AudioWorkstation/StyleExamples'


class Mesh:
    def __init__(self):
        self.positions = []
        self.normals = []
        self.uvs = []

    def triangle(self, a, b, c):
        ab = [b[i] - a[i] for i in range(3)]
        ac = [c[i] - a[i] for i in range(3)]
        n = [ab[1] * ac[2] - ab[2] * ac[1],
             ab[2] * ac[0] - ab[0] * ac[2],
             ab[0] * ac[1] - ab[1] * ac[0]]
        length = math.sqrt(sum(v * v for v in n))
        if length < 1e-14:
            raise ValueError('Degenerate generated triangle')
        n = [v / length for v in n]
        for p in (a, b, c):
            self.positions.append((p[0], p[2], -p[1]))
            self.normals.append((n[0], n[2], -n[1]))
            self.uvs.append((p[0] + 0.5, 0.5 - p[1] / 0.25))

    def quad(self, a, b, c, d):
        self.triangle(a, b, c)
        self.triangle(a, c, d)

    def ring(self, half_width, half_height, clip, z):
        return [(x, y, z) for x, y in [
            (-half_width + clip, -half_height), (half_width - clip, -half_height),
            (half_width, -half_height + clip), (half_width, half_height - clip),
            (half_width - clip, half_height), (-half_width + clip, half_height),
            (-half_width, half_height - clip), (-half_width, -half_height + clip)]]

    def join_rings(self, lower, upper):
        for i in range(len(lower)):
            j = (i + 1) % len(lower)
            self.quad(lower[i], lower[j], upper[j], upper[i])

    def plate(self):
        rings = [self.ring(0.492, 0.117, 0.015, -0.018),
                 self.ring(0.500, 0.125, 0.018, -0.009),
                 self.ring(0.500, 0.125, 0.018, -0.002),
                 self.ring(0.482, 0.107, 0.012, 0.004)]
        for lower, upper in zip(rings, rings[1:]):
            self.join_rings(lower, upper)
        for i in range(8):
            j = (i + 1) % 8
            self.triangle((0., 0., 0.004), rings[-1][i], rings[-1][j])
            self.triangle((0., 0., -0.018), rings[0][j], rings[0][i])

    def tube(self, path, radius, sides=4):
        rings = []
        for index, p in enumerate(path):
            before, after = path[max(0, index - 1)], path[min(len(path) - 1, index + 1)]
            tangent = [after[i] - before[i] for i in range(3)]
            length = math.sqrt(sum(v * v for v in tangent))
            tangent = [v / length for v in tangent]
            # The examples' ornament paths are horizontal enough that this
            # stable perpendicular cannot collapse onto the path tangent.
            across = [-tangent[1], tangent[0], 0.]
            length = math.sqrt(sum(v * v for v in across))
            across = [v / length for v in across]
            up = [tangent[1] * across[2] - tangent[2] * across[1],
                  tangent[2] * across[0] - tangent[0] * across[2],
                  tangent[0] * across[1] - tangent[1] * across[0]]
            rings.append([tuple(p[k] + radius * (math.cos(angle) * across[k]
                            + math.sin(angle) * up[k]) for k in range(3))
                          for angle in (2 * math.pi * j / sides for j in range(sides))])
        for lower, upper in zip(rings, rings[1:]):
            self.join_rings(lower, upper)
        for j in range(sides):
            following = (j + 1) % sides
            self.triangle(path[0], rings[0][following], rings[0][j])
            self.triangle(path[-1], rings[-1][j], rings[-1][following])


def carved_metal():
    mesh = Mesh()
    mesh.plate()
    # Raised scrolls and vine stems are actual triangles, not shader relief.
    for vertical_sign in (-1, 1):
        for horizontal_sign in (-1, 1):
            path = []
            for i in range(22):
                t = i / 21
                angle = t * math.pi * 3
                radius = 1 - t * 0.82
                path.append((horizontal_sign * (0.379 + 0.050 * radius * math.cos(angle)),
                             vertical_sign * (0.105 + 0.013 * radius * math.sin(angle)),
                             0.009 + 0.001 * math.sin(angle)))
            mesh.tube(path, 0.0024, sides=5)
        for phase in (0., math.pi):
            path = [(-0.286 + 0.572 * i / 32,
                     vertical_sign * (0.103 + 0.006 * math.sin(i / 32 * math.pi * 8 + phase)),
                     0.007) for i in range(33)]
            mesh.tube(path, 0.0017, sides=3)
    return mesh


def wood():
    mesh = Mesh()
    mesh.plate()
    # A narrow raised moulding stays outside the declared working rectangle.
    rings = [mesh.ring(0.475, 0.102, 0.012, 0.004),
             mesh.ring(0.468, 0.095, 0.012, 0.008),
             mesh.ring(0.461, 0.088, 0.012, 0.004)]
    for lower, upper in zip(rings, rings[1:]):
        mesh.join_rings(lower, upper)
    return mesh


def flat_ui():
    mesh = Mesh()
    mesh.quad((-0.5, -0.125, 0.), (0.5, -0.125, 0.),
              (0.5, 0.125, 0.), (-0.5, 0.125, 0.))
    return mesh


BACKGROUND = '''
fn style_background(uv: vec2<f32>, time: f32) -> vec3<f32> {
    let center = uv - vec2<f32>(0.5, 0.44);
    let halo = max(0.0, 1.0 - length(center * vec2<f32>(0.9, 1.2)));
    return vec3<f32>(0.008, 0.013, 0.022) + halo * vec3<f32>(0.014, 0.021, 0.032);
}
'''

SHADERS = {
    'carved-metal': '''// Procedural aged bronze over authored metal scrollwork.
fn style_color(uv: vec2<f32>, position: vec3<f32>, size: vec2<f32>, time: f32) -> vec4<f32> {
    let grain = 0.5 + 0.5 * sin(uv.x * 731.0 + sin(uv.y * 103.0) * 2.0);
    let tarnish = 0.5 + 0.5 * sin(uv.x * 39.0 + sin(uv.y * 21.0) * 3.0);
    let patina = smoothstep(0.66, 0.97, tarnish) * 0.55;
    let bronze = vec3<f32>(0.31, 0.17, 0.052) * (0.85 + 0.15 * grain);
    let oxidized = vec3<f32>(0.032, 0.092, 0.068);
    return vec4<f32>(mix(bronze, oxidized, patina), 1.0);
}
''' + BACKGROUND,
    'wood': '''// Continuous walnut growth grain; no bitmap textures or VM.
fn style_color(uv: vec2<f32>, position: vec3<f32>, size: vec2<f32>, time: f32) -> vec4<f32> {
    let centered = uv - vec2<f32>(0.30, 0.58);
    let knot = exp(-dot(centered * vec2<f32>(7.0, 3.0), centered * vec2<f32>(7.0, 3.0)));
    let warp = sin(uv.x * 8.0) * 0.018 + knot * sin(uv.x * 22.0) * 0.13;
    let growth = 0.5 + 0.5 * sin((uv.y + warp) * 155.0);
    let fiber = 0.5 + 0.5 * sin(uv.y * 950.0 + sin(uv.x * 31.0) * 1.2);
    let light = vec3<f32>(0.29, 0.105, 0.027);
    let dark = vec3<f32>(0.075, 0.020, 0.008);
    let color = mix(dark, light, 0.23 + 0.65 * growth) * (0.88 + 0.12 * fiber);
    return vec4<f32>(color, 1.0);
}
''' + BACKGROUND,
    'flat-ui': '''// A two-triangle, zero-thickness UI surface viewed head-on.
fn style_color(uv: vec2<f32>, position: vec3<f32>, size: vec2<f32>, time: f32) -> vec4<f32> {
    let edge = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
    let inset = smoothstep(0.012, 0.020, edge);
    let band = 1.0 - smoothstep(0.16, 0.18, uv.y);
    let paper = mix(vec3<f32>(0.065, 0.11, 0.15), vec3<f32>(0.025, 0.046, 0.070), uv.y);
    let accent = vec3<f32>(0.035, 0.42, 0.36);
    return vec4<f32>(mix(accent, paper + band * vec3<f32>(0.015, 0.035, 0.035), inset), 1.0);
}
''' + BACKGROUND,
}


def write_glb(path, mesh, *, metallic, roughness, unlit=False):
    blob = bytearray()
    views = []
    accessors = []

    def append(values, components, kind, target, bounds=False):
        while len(blob) % 4:
            blob.append(0)
        offset = len(blob)
        flat = [v for row in values for v in row] if components > 1 else values
        blob.extend(struct.pack('<' + ('f' if kind == 5126 else 'I') * len(flat), *flat))
        views.append({'buffer': 0, 'byteOffset': offset,
                      'byteLength': len(blob) - offset, 'target': target})
        accessor = {'bufferView': len(views) - 1, 'componentType': kind,
                    'count': len(values), 'type': {1: 'SCALAR', 2: 'VEC2', 3: 'VEC3'}[components]}
        if bounds:
            accessor['min'] = [min(v[i] for v in values) for i in range(components)]
            accessor['max'] = [max(v[i] for v in values) for i in range(components)]
        accessors.append(accessor)
        return len(accessors) - 1

    positions = append(mesh.positions, 3, 5126, 34962, True)
    normals = append(mesh.normals, 3, 5126, 34962)
    uvs = append(mesh.uvs, 2, 5126, 34962)
    indices = append(list(range(len(mesh.positions))), 1, 5125, 34963)
    material = {'name': 'User surface', 'doubleSided': True, 'alphaMode': 'OPAQUE',
                'pbrMetallicRoughness': {'baseColorFactor': [1., 1., 1., 1.],
                'metallicFactor': metallic, 'roughnessFactor': roughness}}
    if unlit:
        material['extensions'] = {'KHR_materials_unlit': {}}
    document = {'asset': {'version': '2.0', 'generator': 'Mirror make_style_samples.py'},
                'scene': 0, 'scenes': [{'nodes': [0]}], 'nodes': [{'mesh': 0}],
                'meshes': [{'name': path.parent.name, 'primitives': [{
                    'attributes': {'POSITION': positions, 'NORMAL': normals, 'TEXCOORD_0': uvs},
                    'indices': indices, 'material': 0, 'mode': 4}]}],
                'materials': [material], 'accessors': accessors, 'bufferViews': views,
                'buffers': [{'byteLength': len(blob)}]}
    if unlit:
        document['extensionsUsed'] = ['KHR_materials_unlit']
    encoded = json.dumps(document, separators=(',', ':')).encode('utf-8')
    encoded += b' ' * (-len(encoded) % 4)
    blob.extend(b'\x00' * (-len(blob) % 4))
    data = (struct.pack('<III', 0x46546C67, 2, 12 + 8 + len(encoded) + 8 + len(blob))
            + struct.pack('<II', len(encoded), 0x4E4F534A) + encoded
            + struct.pack('<II', len(blob), 0x004E4942) + blob)
    path.write_bytes(data)
    return {'triangles': len(mesh.positions) // 3, 'bytes': len(data),
            'sha256': hashlib.sha256(data).hexdigest()}


def generate(out):
    definitions = [('carved-metal', 'Carved bronze scrollwork', carved_metal, 0.85, 0.32, False, 'stretch'),
                   ('wood', 'Carved walnut', wood, 0., 0.48, False, 'stretch'),
                   ('flat-ui', 'Flat mint interface', flat_ui, 0., 1., True, 'snap')]
    report = {'format': 'mirror.style-example-generation', 'version': 1, 'examples': {}}
    for slug, name, geometry, metallic, roughness, unlit, resize in definitions:
        folder = out / slug
        folder.mkdir(parents=True, exist_ok=True)
        mesh = geometry()
        result = write_glb(folder / 'panel.glb', mesh, metallic=metallic,
                           roughness=roughness, unlit=unlit)
        assert result['triangles'] <= 2000, result
        manifest = {'format': 'mirror.workstation.style', 'version': 1,
                    'id': 'example.' + slug, 'name': name,
                    'models': {'frame': {'file': 'panel.glb', 'axes': 'blender-gltf',
                        'resize': resize, 'content_rect_m': [-0.43, -0.08, 0.43, 0.08],
                        'content_z_m': 0. if unlit else 0.004, 'shader': True}},
                    'panels': {edge: {'model': 'frame'} for edge in ('top', 'bottom', 'left', 'right')},
                    'shader': {'file': 'style.wgsl', 'api': 1, 'animated': False,
                               'panel': True, 'background': True}}
        (folder / 'style.json').write_text(json.dumps(manifest, indent=2) + '\n')
        (folder / 'style.wgsl').write_text(SHADERS[slug])
        result.update({'resize': resize, 'unlit': unlit,
                       'manifest': str(folder / 'style.json')})
        report['examples'][slug] = result
    (out / 'generation.json').write_text(json.dumps(report, indent=2) + '\n')
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, default=DEFAULT_OUT)
    args = parser.parse_args()
    print(json.dumps(generate(args.out.resolve()), indent=2))


if __name__ == '__main__':
    main()
