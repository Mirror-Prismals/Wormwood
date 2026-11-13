#!/usr/bin/env python3
"""Measure exact tile dependencies on the captured 81-state panel grid.

Run with the bundled NumPy/Pillow Python after capture_state_family.py:
    python state_dependencies.py DIRECTORY
Writes DIRECTORY/state-dependencies.json; never overwrites an existing report.
This diagnoses reuse on the sampled grid, not a continuous rendering model.
"""
import argparse
import hashlib
import io
import itertools
import json
from pathlib import Path

import numpy as np
from PIL import Image

TILE = 32
KNOTS = (0., .5, 1.)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def checked_file(captures, name, expected):
    path = (captures / name).resolve()
    if not path.is_relative_to(captures):
        raise ValueError('Capture path escapes its directory')
    data = path.read_bytes()
    if sha(data) != expected:
        raise ValueError(f'Capture hash mismatch: {name}')
    return path, data


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    root = args.directory.resolve()
    output = root / 'state-dependencies.json'
    if output.exists():
        raise ValueError('Dependency report already exists; preserve the prior evidence')
    captures = (root / 'captures').resolve()
    manifest_path = captures / 'manifest.json'
    manifest_bytes = manifest_path.read_bytes()
    manifest = json.loads(manifest_bytes)
    if manifest.get('complete') is not True or manifest.get('determinism_passed') is not True:
        raise ValueError('Complete, deterministic canonical captures are required')
    train = [s for s in manifest['states'] if s['split'] == 'train']
    points = list(itertools.product(KNOTS, repeat=4))
    by_point = {tuple(s['extensions']): s for s in train}
    if len(train) != 81 or set(by_point) != set(points) or len({s['id'] for s in train}) != 81:
        raise ValueError('Expected 81 unique training states on the complete {0,.5,1}^4 grid')
    width, height = manifest['width'], manifest['height']
    if not (1 <= width <= 2048 and 1 <= height <= 2048
            and 81 * width * height * 3 <= 128 * 1024 * 1024):
        raise ValueError('Training RGB8 array exceeds the bounded input budget')
    axes = manifest['family']['axes']
    if len(axes) != 4 or len(set(axes)) != 4:
        raise ValueError('Four distinct named extension axes are required')
    grid = np.empty((3, 3, 3, 3, height, width, 3), dtype=np.uint8)
    indexes = list(itertools.product(range(3), repeat=4))
    sources = []
    implementation_sha = sha(Path(__file__).read_bytes())
    for point, index in zip(points, indexes):
        state = by_point[point]
        path, data = checked_file(captures, state['image'], state['image_sha256'])
        sources.append((path, state['image_sha256']))
        optical, _ = checked_file(captures, state['optical'], state['optical_sha256'])
        sources.append((optical, state['optical_sha256']))
        with Image.open(io.BytesIO(data)) as image:
            if image.mode != 'RGB' or image.size != (width, height):
                raise ValueError('All training captures must share RGB8 dimensions')
            grid[index] = np.asarray(image, dtype=np.uint8)

    histogram = {str(k): 0 for k in range(5)}
    axis_counts = {axis: 0 for axis in axes}
    global_variants = set()
    rows = []
    for y in range(0, height, TILE):
        for x in range(0, width, TILE):
            yy, xx = min(y + TILE, height), min(x + TILE, width)
            tile = grid[:, :, :, :, y:yy, x:xx, :]
            sensitive = []
            for axis in range(4):
                # All other axes retain their complete Cartesian coordinates.
                # Comparing knot 0 to knots 1 and 2 proves finite-grid equality
                # along this axis; it does not sample between those knots.
                selection = [slice(None)] * 7
                selection[axis] = 0
                reference = tile[tuple(selection)]
                for level in (1, 2):
                    selection[axis] = level
                    if not np.array_equal(reference, tile[tuple(selection)]):
                        sensitive.append(axis)
                        break
            # Python byte equality resolves hash collisions; hashes alone never
            # declare variants equal. Include shape for edge tiles in global reuse.
            variants = {tile[index].tobytes() for index in indexes}
            global_variants.update(((yy - y, xx - x), data) for data in variants)
            assert len(variants) <= 3 ** len(sensitive)
            histogram[str(len(sensitive))] += 1
            for axis in sensitive:
                axis_counts[axes[axis]] += 1
            rows.append({'x': x, 'y': y, 'width': xx - x, 'height': yy - y,
                         'sensitive_axes': [axes[i] for i in sensitive],
                         'exact_variants': len(variants),
                         'axis_factored_grid_entries': 3 ** len(sensitive)})

    if manifest_path.read_bytes() != manifest_bytes or sha(Path(__file__).read_bytes()) != implementation_sha:
        raise ValueError('Manifest or analysis source changed during analysis')
    for path, expected in sources:
        if sha(path.read_bytes()) != expected:
            raise ValueError(f'Capture changed during analysis: {path.name}')
    references = 81 * len(rows)
    per_position = sum(row['exact_variants'] for row in rows)
    report = {
        'schema': 1, 'complete': True, 'manifest_sha256': sha(manifest_bytes),
        'implementation_sha256': implementation_sha,
        'backend': manifest.get('backend'), 'binary_sha256': manifest.get('binary_sha256'),
        'fixed_state': manifest['family'].get('fixed'),
        'width': width, 'height': height, 'channels': 3, 'tile_size': TILE,
        'training_states': 81, 'axes': axes, 'knots': KNOTS, 'tiles': len(rows),
        'tile_reference_count': references, 'sensitive_axis_histogram': histogram,
        'tiles_sensitive_to_axis': axis_counts,
        'per_position_exact_variants': per_position,
        'global_exact_variants_including_shape': len(global_variants),
        'per_position_duplicate_references': references - per_position,
        'global_duplicate_references': references - len(global_variants),
        'axis_factored_grid_entries': sum(row['axis_factored_grid_entries'] for row in rows),
        'tile_details': rows,
        'scope': 'Exact RGB8 equality on the 81 captured training states; held-out pixels are unused. '
                 'Sensitivity means at least one matching-other-axes comparison changed any channel.',
        'limits': [
            'Insensitivity on sampled knots does not establish insensitivity between knots or for other content, layouts, lighting, resolution or rendering settings.',
            'Axis counts identify finite-grid dependencies, not additive separability or absence of interactions among sensitive axes.',
            'Variant and reference counts are not compressed byte sizes. Dictionary indexes, metadata and codecs have costs; use the persisted pack measurements for actual storage.',
            'No decoder, runtime acceleration, GPU timing or FPS is measured here.'
        ],
    }
    with output.open('x') as stream:
        json.dump(report, stream, indent=2, allow_nan=False)
        stream.write('\n')
    print(json.dumps({'report': str(output), 'tile_references': references,
                      'sensitive_axis_histogram': histogram,
                      'per_position_exact_variants': per_position,
                      'global_exact_variants': len(global_variants)}), flush=True)


if __name__ == '__main__':
    main()
