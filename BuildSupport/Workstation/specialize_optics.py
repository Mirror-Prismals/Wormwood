#!/usr/bin/env python3
"""Private scene-specialization experiment; never edits production shaders.

Specialization only removes absent geometry families. Their frame predicates
must remain true for every frame using this shader. Ray arithmetic, geometry,
lighting, sampling, path budgets and display conversion are untouched.
"""
from pathlib import Path
import argparse
import hashlib
import json


def replace_body(source, name, body):
    start = source.index('fn ' + name + '(')
    opening = source.index('{', start)
    depth, end = 1, opening + 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[:opening + 1] + '\n' + body + '\n' + source[end - 1:]


def replace_once(source, old, new):
    if source.count(old) != 1:
        raise ValueError(f'Expected exactly one shader marker: {old}')
    return source.replace(old, new)


def specialize(source, rows):
    absent = {'bubbles': rows[234][0] < .5, 'spheres': rows[240][0] < .5,
              'custom_panels': rows[245][0] < .5, 'timeline_meshes': rows[248][0] < .5}
    no_hit = 'return GemHit(farLimit,vec3<f32>(0.0),-1);'
    for feature, function, marker in (
        ('bubbles', 'gemBubbleHit', 'var membrane:bool=hit.panel>=100000&&hit.panel<102048;'),
        ('spheres', 'gemAudioSphereHit', 'var sphere:bool=hit.panel>=90000&&hit.panel<90064;'),
        ('custom_panels', 'userStyleHit', 'var custom:bool=hit.panel>=200000&&hit.panel<600000;'),
    ):
        if absent[feature]:
            source = replace_body(source, function, no_hit)
            source = replace_once(source, marker, marker.split('=')[0] + '=false;')
    if absent['timeline_meshes']:
        source = replace_body(source, 'gemTimelineHit', no_hit)
        # The physical ruler/playhead remain separate geometry and still render.
        source = replace_once(source,
            'if(inside&&current.medium>=700000&&current.medium<704096){', 'if(false){')
    return source, absent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--shader', type=Path, required=True)
    parser.add_argument('--frame-json', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    original = args.shader.read_text()
    frame = json.loads(args.frame_json.read_text())
    result, absent = specialize(original, frame['rows'])
    production = Path(__file__).resolve().parents[2] / 'Procedures/Shaders'
    for target in (args.output.with_suffix('.wgsl'), args.output.with_suffix('.json')):
        if target.resolve() == args.shader.resolve() or target.resolve().parent == production:
            raise ValueError('Private experiment must not overwrite its input or production shaders')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    path = args.output.with_suffix('.wgsl')
    path.write_text(result)
    args.output.with_suffix('.json').write_text(json.dumps({
        'kind': 'private_scene_specialization', 'workgroup_size': 8,
        'required_absent_features': [name for name, value in absent.items() if value],
        'frame_predicates': {'bubbles': [234, 0, '<0.5'], 'spheres': [240, 0, '<0.5'],
                             'custom_panels': [245, 0, '<0.5'], 'timeline_meshes': [248, 0, '<0.5']},
        'source_shader_sha256': hashlib.sha256(original.encode()).hexdigest(),
        'generated_shader_sha256': hashlib.sha256(result.encode()).hexdigest(),
        'source_frame_sha256': hashlib.sha256(args.frame_json.read_bytes()).hexdigest(),
        'source_frame': str(args.frame_json.resolve()),
        'samples_unchanged': True, 'production_installed': False,
    }, indent=2) + '\n')
    print(path)


if __name__ == '__main__':
    main()
