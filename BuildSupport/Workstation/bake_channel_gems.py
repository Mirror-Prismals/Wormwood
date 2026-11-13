#!/usr/bin/env python3
"""Offline channel variants of the shipped mini ruby; never run by the app."""
from pathlib import Path
import array
import colorsys
import json

VARIANTS = [('ChannelBlue', 12744, 2 / 3),
            ('ChannelCyan', 12784, 1 / 2),
            ('ChannelOrange', 12824, 1 / 12)]

def bake(directory):
    manifest_path = directory / 'manifest.json'
    manifest = json.loads(manifest_path.read_text())
    data = (directory / 'images.rgba32f').read_bytes()
    retained = [e for e in manifest['entries'] if e['name'] not in {v[0] for v in VARIANTS}]
    chunks = []
    offset = 0
    for entry in retained:
        start = entry['offset'] * 16
        chunks.append(data[start:start + entry['bytes']])
        entry['offset'] = offset // 16
        offset += entry['bytes']
    source = next(e for e in retained if e['name'] == 'GemButton_Mini')
    original = chunks[retained.index(source)]
    for name, material, hue in VARIANTS:
        pixels = array.array('f')
        pixels.frombytes(original)
        edge = source['edge']
        for y in range(edge):
            for x in range(edge):
                at = (y * edge * 2 + x) * 4
                _, saturation, value = colorsys.rgb_to_hsv(*pixels[at:at + 3])
                pixels[at:at + 3] = array.array('f', colorsys.hsv_to_rgb(hue, saturation, value))
        raw = pixels.tobytes()
        entry = dict(source, name=name, material=material, offset=offset // 16,
                     source_art='GemButton_Mini',
                     appearance='Offline hue variant; original saturation, value, alpha and auxiliary depth')
        retained.append(entry)
        chunks.append(raw)
        offset += len(raw)
    manifest['entries'] = retained
    pending = directory / 'images.rgba32f.pending'
    pending.write_bytes(b''.join(chunks))
    pending.replace(directory / 'images.rgba32f')
    manifest_path.write_text(json.dumps(manifest, indent=2) + '\n')

if __name__ == '__main__':
    bake(Path(__file__).resolve().parents[2] / 'Art/AudioWorkstation/Baked')
