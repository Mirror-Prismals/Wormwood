"""Bounded, independently addressable workstation state-family codec experiment.

Use the bundled Python with NumPy and Pillow. Input: DIRECTORY/captures/manifest.json.
Exact caches cover stored states only. Optional PCA predicts held-out parameters;
it is an image-error experiment, never a production quality change or FPS claim.
"""
from pathlib import Path
import argparse
import hashlib
import io
import itertools
import json
import math
import os
import random
import resource
import struct
import subprocess
import sys
import time
import tracemalloc
import zlib

# Bound BLAS concurrency unless the caller has explicitly selected a setting.
for _key in ('OPENBLAS_NUM_THREADS', 'OMP_NUM_THREADS', 'VECLIB_MAXIMUM_THREADS'):
    os.environ.setdefault(_key, '1')
import numpy as np
from PIL import Image, ImageDraw, __version__ as pillow_version

MAGIC = b'WWSTPK1\0'
TILE = 32
SEED = 20261030


def sha(data):
    return hashlib.sha256(data).hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + '\n')


def tiles(width, height):
    for y in range(0, height, TILE):
        for x in range(0, width, TILE):
            yield y, min(y + TILE, height), x, min(x + TILE, width)


def state_metadata(state):
    return {k: state[k] for k in ('id', 'split', 'extensions', 'optical_sha256')}


def encode_pack(path, states, images, base_id, provenance):
    """Index 0 means the base tile; other indexes address raw/XOR dictionary tiles."""
    started = time.perf_counter()
    base = images[base_id]
    height, width, _ = base.shape
    base_compressed = zlib.compress(base.tobytes(), 6)
    payload = [base_compressed]
    offset = len(base_compressed)
    entries, dictionary, state_rows = [], {}, []
    unchanged = 0
    for state in states:
        indices = []
        for y, yy, x, xx in tiles(width, height):
            tile = images[state['id']][y:yy, x:xx]
            reference = base[y:yy, x:xx]
            if np.array_equal(tile, reference):
                indices.append(0)
                unchanged += 1
                continue
            raw = tile.tobytes()
            delta = np.bitwise_xor(tile, reference).tobytes()
            raw_z, delta_z = zlib.compress(raw, 6), zlib.compress(delta, 6)
            mode, decoded, compressed = (1, delta, delta_z) if len(delta_z) < len(raw_z) else (0, raw, raw_z)
            # Byte equality, not hashes alone, determines exact sharing.
            key = (mode, decoded)
            index = dictionary.get(key)
            if index is None:
                index = len(entries) + 1
                dictionary[key] = index
                entries.append([mode, offset, len(compressed), len(decoded)])
                payload.append(compressed)
                offset += len(compressed)
            indices.append(index)
        packed = struct.pack('<' + 'I' * len(indices), *indices)
        state_rows.append(dict(state_metadata(state), index_offset=offset, index_bytes=len(packed)))
        payload.append(packed)
        offset += len(packed)
    header = {'schema': 1, 'width': width, 'height': height, 'channels': 3, 'tile': TILE,
              'base_id': base_id, 'base_bytes': len(base_compressed), 'dictionary': entries,
              'states': state_rows, 'provenance': provenance,
              'semantics': 'Exact stored-state RGB8 cache; each state refers directly to one shared base and dictionary. No previous-frame dependency.'}
    metadata = json.dumps(header, separators=(',', ':'), sort_keys=True, allow_nan=False).encode()
    with path.open('xb') as stream:
        stream.write(MAGIC)
        stream.write(struct.pack('<Q', len(metadata)))
        stream.write(metadata)
        for part in payload:
            stream.write(part)
    total = path.stat().st_size
    return {'file': path.name, 'bytes': total, 'sha256': sha(path.read_bytes()),
            'states': len(states), 'base_compressed_bytes': len(base_compressed),
            'metadata_and_header_bytes': len(MAGIC) + 8 + len(metadata),
            'index_bytes': sum(s['index_bytes'] for s in state_rows),
            'dictionary_compressed_bytes': sum(e[2] for e in entries),
            'dictionary_entries': len(entries), 'base_tile_references': unchanged,
            'raw_dictionary_entries': sum(e[0] == 0 for e in entries),
            'xor_dictionary_entries': sum(e[0] == 1 for e in entries),
            'encode_wall_seconds': time.perf_counter() - started,
            'raw_pixel_bytes': len(states) * width * height * 3}


class PackDecoder:
    """Only the persisted file is accepted; no references or encoder objects."""
    def __init__(self, path):
        self.stream = Path(path).open('rb')
        if self.stream.read(8) != MAGIC:
            raise ValueError('Not a workstation state pack')
        size_bytes = self.stream.read(8)
        if len(size_bytes) != 8:
            raise ValueError('Truncated header')
        size, = struct.unpack('<Q', size_bytes)
        if size > 8 * 1024 * 1024:
            raise ValueError('Oversized metadata')
        self.header = json.loads(self.stream.read(size))
        self.payload = 16 + size
        h = self.header
        if h['schema'] != 1 or h['tile'] != TILE or h['channels'] != 3:
            raise ValueError('Unsupported pack ABI')
        self.width, self.height = h['width'], h['height']
        if not (1 <= self.width <= 2048 and 1 <= self.height <= 2048):
            raise ValueError('Invalid image dimensions')
        raw = zlib.decompress(self.read(0, h['base_bytes']))
        if len(raw) != self.width * self.height * 3:
            raise ValueError('Invalid base image size')
        self.base = np.frombuffer(raw, dtype=np.uint8).reshape(self.height, self.width, 3)
        self.states = {s['id']: s for s in h['states']}
        if len(self.states) != len(h['states']):
            raise ValueError('Duplicate state ID')
        self.locations = list(tiles(self.width, self.height))

    def read(self, offset, size):
        if offset < 0 or size < 0:
            raise ValueError('Negative pack extent')
        self.stream.seek(self.payload + offset)
        data = self.stream.read(size)
        if len(data) != size:
            raise ValueError('Truncated pack payload')
        return data

    def decode(self, state_id):
        state = self.states[state_id]  # Unstored states deliberately fail.
        if state['index_bytes'] != 4 * len(self.locations):
            raise ValueError('Invalid tile index count')
        indexes = struct.unpack('<' + 'I' * len(self.locations),
                                self.read(state['index_offset'], state['index_bytes']))
        output = self.base.copy()
        for index, (y, yy, x, xx) in zip(indexes, self.locations):
            if index == 0:
                continue
            if index > len(self.header['dictionary']):
                raise ValueError('Invalid tile dictionary index')
            mode, offset, size, raw_size = self.header['dictionary'][index - 1]
            expected = (yy - y) * (xx - x) * 3
            raw = zlib.decompress(self.read(offset, size))
            if raw_size != expected or len(raw) != expected:
                raise ValueError('Invalid decoded tile size')
            tile = np.frombuffer(raw, dtype=np.uint8).reshape(yy - y, xx - x, 3)
            if mode == 1:
                output[y:yy, x:xx] = np.bitwise_xor(tile, self.base[y:yy, x:xx])
            elif mode == 0:
                output[y:yy, x:xx] = tile
            else:
                raise ValueError('Unknown tile mode')
        return output

    def close(self):
        self.stream.close()


def peak_rss_bytes():
    peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    return int(peak if sys.platform == 'darwin' else peak * 1024)


class IndependentDecoder:
    """Load the persisted PNG/zlib index and only the files it names."""
    def __init__(self, path):
        self.path = Path(path)
        header = json.loads(self.path.read_text())
        self.width, self.height = header['width'], header['height']
        self.codec = header['codec']
        self.states = {s['id']: s for s in header['states']}

    def decode(self, state_id):
        path = self.path.parent / self.states[state_id]['file']
        if self.codec == 'png':
            with Image.open(path) as image:
                return np.array(image, dtype=np.uint8)
        raw = zlib.decompress(path.read_bytes())
        if len(raw) != self.width * self.height * 3:
            raise ValueError('Invalid independent image size')
        return np.frombuffer(raw, dtype=np.uint8).reshape(self.height, self.width, 3)

    def close(self):
        pass


def decoder_measurements(path, trials, decoder_class=PackDecoder):
    # Separate process proves all random access comes from the pack. Reference
    # images and the encoder's dictionaries are absent from this process.
    baseline_rss = peak_rss_bytes()
    opening = time.perf_counter_ns()
    decoder = decoder_class(path)
    opened = time.perf_counter_ns()
    first_state = sorted(decoder.states)[0]
    decoded = decoder.decode(first_state)
    first_finished = time.perf_counter_ns()
    del decoded
    hashes = {}
    for state_id in decoder.states:
        decoded = decoder.decode(state_id)
        hashes[state_id] = sha(decoded.tobytes())
    del decoded
    rng = random.Random(SEED)
    ids = sorted(decoder.states)
    queries = [rng.choice(ids) for _ in range(trials)]
    timings = []
    for state_id in queries:
        start = time.perf_counter_ns()
        decoded = decoder.decode(state_id)
        timings.append((time.perf_counter_ns() - start) / 1e6)
        del decoded
    decoder.close()
    timed_peak = peak_rss_bytes()
    # Trace allocations separately so tracing cannot perturb the timed trials.
    tracemalloc.start()
    decoder = decoder_class(path)
    for state_id in queries[:min(8, len(queries))]:
        decoded = decoder.decode(state_id)
        del decoded
    _, traced_peak = tracemalloc.get_traced_memory()
    tracemalloc.stop()
    decoder.close()
    return {'hashes': hashes, 'trials': trials, 'seed': SEED,
            'open_wall_ms': (opened - opening) / 1e6,
            'first_state_id': first_state,
            'first_decode_wall_ms': (first_finished - opened) / 1e6,
            'open_plus_first_decode_wall_ms': (first_finished - opening) / 1e6,
            'decode_wall_ms_mean': float(np.mean(timings)),
            'decode_wall_ms_p50': float(np.percentile(timings, 50)),
            'decode_wall_ms_p95': float(np.percentile(timings, 95)),
            'decode_wall_ms_max': max(timings),
            'process_peak_rss_bytes': timed_peak, 'process_import_peak_rss_bytes': baseline_rss,
            'traced_decode_allocations_peak_bytes': traced_peak,
            'resident_base_pixel_bytes': decoder.width * decoder.height * 3 if decoder_class is PackDecoder else 0,
            'output_pixel_bytes': decoder.width * decoder.height * 3,
            'memory_scope': 'Fresh decoder-only process. RSS includes Python/NumPy/Pillow and OS allocations; traced allocations are a separate observation, not total RSS. No decoded-tile cache.',
            'timing_scope': 'Open and first decode begin in a fresh process, but OS file cache may be warm. Random stored-state lookup includes file reads, decompression and RGB assembly; cache warmed by exact verification. Excludes hashing, tracing, GPU upload and presentation.'}


def independent_files(out, states, images, provenance):
    directory = out / 'independent'
    directory.mkdir()
    sizes = {}
    for state in states:
        state_id = state['id']
        image = images[state_id]
        png, compressed = directory / (state_id + '.png'), directory / (state_id + '.zlib')
        Image.fromarray(image).save(png, optimize=True, compress_level=6)
        compressed.write_bytes(zlib.compress(image.tobytes(), 6))
        sizes[state_id] = {'png': png.stat().st_size, 'zlib': compressed.stat().st_size}
    result = {}
    for coverage, subset in (('train', [s for s in states if s['split'] == 'train']), ('all', states)):
        result[coverage] = {}
        for codec in ('png', 'zlib'):
            index = directory / f'{coverage}-{codec}.json'
            index.write_text(json.dumps({'schema': 1, 'width': images[states[0]['id']].shape[1],
                              'height': images[states[0]['id']].shape[0], 'channels': 3, 'codec': codec,
                              'provenance': provenance,
                              'states': [dict(state_metadata(s), file=s['id'] + '.' + codec) for s in subset]},
                              separators=(',', ':'), sort_keys=True, allow_nan=False) + '\n')
            result[coverage][codec + '_bytes'] = index.stat().st_size + sum(sizes[s['id']][codec] for s in subset)
            result[coverage][codec] = {'index': str(index.relative_to(out)), 'index_bytes': index.stat().st_size,
                                     'image_file_bytes': sum(sizes[s['id']][codec] for s in subset),
                                     'byte_scope': 'All selected independent image files plus their complete persisted state/provenance index.'}
    return result


def image_metrics(reference, reconstruction):
    error = np.abs(reference.astype(np.int16) - reconstruction.astype(np.int16))
    mae = float(np.mean(error, dtype=np.float64))
    mse = float(np.mean(error.astype(np.float32) ** 2, dtype=np.float64))
    return {'mae_byte': mae, 'rmse_byte': math.sqrt(mse),
            'psnr_db': 10 * math.log10(255 * 255 / mse) if mse else None,
            'exact': not np.any(error), 'max_channel_error': int(error.max()),
            'channels_over_1_percent': float(np.mean(error > 1) * 100),
            'channels_over_4_percent': float(np.mean(error > 4) * 100)}


class FactoredDecoder:
    def __init__(self, path):
        with np.load(path, allow_pickle=False) as packed:
            self.mean, self.basis = packed['mean'], packed['basis']
            self.coefficients = {tuple(p): c for p, c in zip(packed['states'], packed['coefficients'])}
            self.shape = tuple(packed['shape'])

    def decode(self, extensions):
        axes = []
        for value in extensions:
            if not math.isfinite(value) or not 0 <= value <= 1:
                raise ValueError('PCA family covers extensions in [0,1] only')
            if value in (0., .5, 1.):
                axes.append([(value, 1.)])
            else:
                low = 0. if value < .5 else .5
                fraction = (value - low) * 2
                axes.append([(low, 1 - fraction), (low + .5, fraction)])
        coefficients = np.zeros(len(self.basis), dtype=np.float32)
        for corners in itertools.product(*axes):
            point = tuple(v for v, _ in corners)
            weight = math.prod(w for _, w in corners)
            coefficients += self.coefficients[point] * np.float32(weight)
        values = self.mean + coefficients @ self.basis
        return np.rint(np.clip(values, 0, 255)).astype(np.uint8).reshape(self.shape)


def triptych(path, reference, reconstructed, label):
    error = np.max(np.abs(reference.astype(np.int16) - reconstructed.astype(np.int16)), axis=2)
    heat = np.zeros_like(reference)
    heat[:, :, 0] = np.minimum(error * 4, 255).astype(np.uint8)
    width, height = reference.shape[1], reference.shape[0]
    canvas = Image.new('RGB', (3 * width, height + 48), 'white')
    draw = ImageDraw.Draw(canvas)
    for i, (image, title) in enumerate(((reference, 'Unchanged reference'), (reconstructed, 'Reconstruction'),
                                      (heat, 'Max channel error x4; red saturates at 64'))):
        canvas.paste(Image.fromarray(image), (i * width, 48))
        draw.text((i * width + 8, 8), title, fill='black')
    draw.text((8, 28), label, fill='black')
    canvas.save(path)


def pca_experiment(out, states, images, provenance):
    started = time.perf_counter()
    points = list(itertools.product((0., .5, 1.), repeat=4))
    train = {tuple(s['extensions']): s for s in states if s['split'] == 'train'}
    if len(train) != 81 or set(train) != set(points):
        raise ValueError('PCA requires exactly the complete 3^4 training grid')
    # 81x921600 float32 is about285MiB. Bound the experiment before allocating.
    image_shape = images[states[0]['id']].shape
    if 81 * math.prod(image_shape) * 4 > 320 * 1024 * 1024:
        raise ValueError('PCA training matrix exceeds 320 MiB limit')
    matrix = np.empty((81, math.prod(image_shape)), dtype=np.float32)
    for i, point in enumerate(points):
        matrix[i] = images[train[point]['id']].reshape(-1)
    mean = matrix.mean(axis=0)
    matrix -= mean
    gram = matrix @ matrix.T  # Small sample-space eigensystem, never pixel-space SVD.
    values, vectors = np.linalg.eigh(gram)
    order = np.argsort(values)[::-1][:16]
    values, vectors = np.maximum(values[order], 0), vectors[:, order]
    singular = np.sqrt(values)
    useful = singular > max(float(singular[0]) * 1e-6, 1e-6)
    basis = vectors.T @ matrix
    basis[useful] /= singular[useful, None]
    basis[~useful] = 0
    coefficients = vectors * singular[None, :]
    coefficients[:, ~useful] = 0
    del matrix, gram
    fit_seconds = time.perf_counter() - started
    results = []
    for rank in (4, 8, 16):
        path = out / f'pca-rank-{rank}.npz'
        np.savez_compressed(path, mean=mean, basis=basis[:rank], coefficients=coefficients[:, :rank],
                            states=np.array(points, dtype=np.float32), shape=np.array(image_shape, dtype=np.int32),
                            schema=np.array([1], dtype=np.int32),
                            metadata=np.array(json.dumps({'provenance': provenance,
                                'training_states': [state_metadata(train[point]) for point in points]},
                                separators=(',', ':'), sort_keys=True, allow_nan=False)))
        decoder = FactoredDecoder(path)
        rows = []
        for state in states:
            start = time.perf_counter_ns()
            reconstructed = decoder.decode(state['extensions'])
            elapsed = (time.perf_counter_ns() - start) / 1e6
            rows.append(dict(state_metadata(state), decode_wall_ms=elapsed,
                             **image_metrics(images[state['id']], reconstructed)))
        worst = max((r for r in rows if r['split'] == 'holdout'), key=lambda r: r['rmse_byte'])
        triptych(out / f'pca-rank-{rank}-worst-holdout.png', images[worst['id']],
                 decoder.decode(worst['extensions']), f"Rank {rank}; {worst['id']}; RMSE {worst['rmse_byte']:.3f} byte")
        result = {'rank': rank, 'file': path.name, 'bytes': path.stat().st_size,
                  'sha256': sha(path.read_bytes()), 'states': rows, 'splits': {}}
        for split in ('train', 'holdout'):
            subset = [r for r in rows if r['split'] == split]
            result['splits'][split] = {'states': len(subset),
                'mean_mae_byte': float(np.mean([r['mae_byte'] for r in subset])),
                'worst_rmse_byte': max(r['rmse_byte'] for r in subset),
                'worst_channel_error': max(r['max_channel_error'] for r in subset),
                'decode_wall_ms_p50': float(np.percentile([r['decode_wall_ms'] for r in subset], 50)),
                'decode_wall_ms_p95': float(np.percentile([r['decode_wall_ms'] for r in subset], 95))}
        results.append(result)
        del decoder
    return {'enabled': True, 'fit_wall_seconds': fit_seconds,
            'total_experiment_wall_seconds': time.perf_counter() - started, 'models': results,
            'semantics': 'Approximate RGB8 training-only PCA; held-out coefficients use 4D multilinear interpolation of stored training coefficients. No held-out pixel projection, residuals, or fitting. Basis/coefficients float32; clipped rounded RGB8 errors included. Not production rendering.',
            'timing_scope': 'One reconstruction per state, after loading only the persisted model into the decoder. Excludes model load, quality metrics and presentation; not directly equivalent to random exact-cache trials.'}


def quality_plot(path, pca):
    # Standalone scientific SVG: no plotting dependency or modification of art.
    models = pca.get('models', [])
    values = [m['splits'][s]['mean_mae_byte'] for m in models for s in ('train', 'holdout')]
    ymax = max(values + [1.]) * 1.15
    parts = ['<svg xmlns="http://www.w3.org/2000/svg" width="860" height="460" viewBox="0 0 860 460">',
             '<rect width="860" height="460" fill="white"/>',
             '<g font-family="sans-serif" font-size="14" fill="#111">',
             '<text x="70" y="30" font-size="19">Rendered state-family reconstruction error</text>',
             '<text x="70" y="53">Lossless stored-state reconstruction: exact; unseen states require additional storage.</text>',
             '<path d="M70 80V360H810" fill="none" stroke="#333"/>']
    for i in range(6):
        value = ymax * i / 5
        y = 360 - i * 260 / 5
        parts.extend([f'<path d="M70 {y}H810" stroke="#ddd"/>',
                      f'<text x="10" y="{y + 5}">{value:.2f}</text>'])
    for i, model in enumerate(models):
        for j, split in enumerate(('train', 'holdout')):
            value = model['splits'][split]['mean_mae_byte']
            h = value / ymax * 260
            x = 180 + i * 210 + j * 55
            color = '#3478ba' if split == 'train' else '#bf5438'
            parts.append(f'<rect x="{x}" y="{360-h}" width="48" height="{h}" fill="{color}"/>')
        parts.append(f'<text x="{180+i*210}" y="385">PCA rank {model["rank"]}</text>')
    parts.extend(['<text x="70" y="415">Mean absolute channel error in RGB8 code values (lower is better)</text>',
                  '<text x="70" y="440" fill="#3478ba">Blue: training states</text>',
                  '<text x="300" y="440" fill="#bf5438">Red: independent held-out combinations</text>'])
    if not models:
        parts.append('<text x="170" y="205">PCA disabled. Both lossless packs reconstruct every stored pixel exactly.</text>')
    parts.append('</g></svg>')
    path.write_text('\n'.join(parts) + '\n')


def measure_persisted(path, trials, independent=False):
    mode = '--decode-independent' if independent else '--decode-pack'
    child = subprocess.run([sys.executable, str(Path(__file__).resolve()), mode, str(path),
                            '--decode-trials', str(trials)], capture_output=True, text=True, timeout=180)
    if child.returncode:
        raise RuntimeError(f'Decoder failed for {path.name}: {child.stderr[-8000:]}')
    return json.loads(child.stdout)


def require_hashes(measurements, expected):
    if measurements.pop('hashes') != expected:
        raise ValueError('Persisted representation pixel mismatch')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', nargs='?', type=Path)
    parser.add_argument('--pca', action='store_true', help='Also fit bounded experimental rank 4/8/16 models')
    parser.add_argument('--decode-trials', type=int, default=64)
    parser.add_argument('--decode-pack', type=Path, help=argparse.SUPPRESS)
    parser.add_argument('--decode-independent', type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if not 1 <= args.decode_trials <= 512:
        parser.error('--decode-trials must be 1..512')
    if args.decode_pack or args.decode_independent:
        if args.decode_pack and args.decode_independent:
            parser.error('Choose one persisted decoder input')
        print(json.dumps(decoder_measurements(args.decode_pack or args.decode_independent, args.decode_trials,
                         PackDecoder if args.decode_pack else IndependentDecoder), allow_nan=False))
        return
    if args.directory is None:
        parser.error('directory is required')
    root = args.directory.resolve()
    captures = root / 'captures'
    manifest_path = captures / 'manifest.json'
    manifest_bytes = manifest_path.read_bytes()
    document = json.loads(manifest_bytes)
    if document.get('complete') is not True:
        raise ValueError('Capture family must explicitly report complete=true')
    if document.get('determinism_passed') is not True:
        raise ValueError('Capture family must explicitly pass its determinism checks')
    states = sorted(document['states'], key=lambda s: s['id'])
    width, height = document['width'], document['height']
    if not (1 <= len(states) <= 128 and 1 <= width <= 2048 and 1 <= height <= 2048
            and len(states) * width * height * 3 <= 128 * 1024 * 1024):
        raise ValueError('Capture family exceeds bounded image/state budget')
    ids = [s['id'] for s in states]
    if len(ids) != len(set(ids)) or any(not i or not all(c.isascii() and (c.isalnum() or c in '-_') for c in i) for i in ids):
        raise ValueError('State IDs must be unique filename-safe ASCII identifiers')
    images, expected, source_hashes, optical_sources = {}, {}, {}, {}
    for state in states:
        if state['split'] not in ('train', 'holdout') or len(state['extensions']) != 4:
            raise ValueError('Invalid state split/parameter vector')
        if not all(math.isfinite(v) and 0 <= v <= 1 for v in state['extensions']):
            raise ValueError('State extensions must be finite in [0,1]')
        if len(state['optical_sha256']) != 64 or any(c not in '0123456789abcdef' for c in state['optical_sha256']):
            raise ValueError('State optical SHA256 required')
        if 'optical' in state:
            optical = (captures / state['optical']).resolve()
            if not optical.is_relative_to(captures.resolve()):
                raise ValueError('Optical path escapes capture directory')
            if sha(optical.read_bytes()) != state['optical_sha256']:
                raise ValueError('Capture optical data disagrees with its manifest SHA256')
            optical_sources[state['id']] = optical
        source = (captures / state['image']).resolve()
        if not source.is_relative_to(captures.resolve()):
            raise ValueError('Capture path escapes capture directory')
        source_bytes = source.read_bytes()
        source_hashes[state['id']] = sha(source_bytes)
        if state.get('image_sha256', source_hashes[state['id']]) != source_hashes[state['id']]:
            raise ValueError('Capture image disagrees with its manifest SHA256')
        with Image.open(io.BytesIO(source_bytes)) as image:
            if image.mode != 'RGB' or image.size != (width, height):
                raise ValueError('All captures must have identical RGB8 dimensions')
            images[state['id']] = np.array(image, dtype=np.uint8)
        expected[state['id']] = sha(images[state['id']].tobytes())
    train = [s for s in states if s['split'] == 'train']
    holdout = [s for s in states if s['split'] == 'holdout']
    if len(train) != 81 or {tuple(s['extensions']) for s in train} != set(itertools.product((0., .5, 1.), repeat=4)):
        raise ValueError('Expected 81 unique training states on the complete {0,.5,1}^4 grid')
    if len(holdout) != 16 or {tuple(s['extensions']) for s in holdout} != set(itertools.product((.25, .75), repeat=4)):
        raise ValueError('Expected 16 independent held-out combinations on the {.25,.75}^4 grid')
    base = min(train, key=lambda s: (sum((v - .5) ** 2 for v in s['extensions']), s['id']))['id']
    out = root / 'representation'
    out.mkdir()  # Never mix measurements with stale outputs.
    summary = {'schema': 1, 'complete': False, 'reference_manifest_sha256': sha(manifest_bytes),
               'width': width, 'height': height, 'train_states': len(train), 'holdout_states': len(holdout),
               'seed': SEED, 'base_state': base, 'lossless': {},
               'scope': 'Offline representation of independently selected rendered states. Decoder CPU wall time is not GPU time or application FPS. Training cache has no unseen-state synthesis.',
               'reference_pixel_sha256': expected, 'reference_image_file_sha256': source_hashes,
               'optical_files_hash_verified': len(optical_sources),
               'implementation_sha256': sha(Path(__file__).read_bytes()),
               'runtime': {'python': sys.version, 'numpy': np.__version__, 'pillow': pillow_version,
                           'zlib': zlib.ZLIB_RUNTIME_VERSION,
                           'blas_thread_environment': {k: os.environ.get(k) for k in
                               ('OPENBLAS_NUM_THREADS', 'OMP_NUM_THREADS', 'VECLIB_MAXIMUM_THREADS')}}}
    try:
        provenance = {k: v for k, v in document.items() if k != 'states'}
        provenance['manifest_sha256'] = summary['reference_manifest_sha256']
        summary['independent'] = independent_files(out, states, images, provenance)
        for coverage, subset in (('train', train), ('all', states)):
            expected_subset = {s['id']: expected[s['id']] for s in subset}
            for codec in ('png', 'zlib'):
                baseline = summary['independent'][coverage][codec]
                measured = measure_persisted(out / baseline['index'], args.decode_trials, independent=True)
                require_hashes(measured, expected_subset)
                baseline.update(bit_exact=True, decoder=measured)
            path = out / ('lossless-' + coverage + '.pack')
            encoded = encode_pack(path, subset, images, base, provenance)
            decoded = measure_persisted(path, args.decode_trials)
            require_hashes(decoded, expected_subset)
            encoded.update(bit_exact=True, decoder=decoded, coverage=coverage,
                           ratio_to_raw=encoded['bytes'] / encoded['raw_pixel_bytes'],
                           ratio_to_independent_png=encoded['bytes'] / summary['independent'][coverage]['png_bytes'],
                           ratio_to_independent_zlib=encoded['bytes'] / summary['independent'][coverage]['zlib_bytes'])
            summary['lossless'][coverage] = encoded
            write_json(out / 'summary.json', summary)
        summary['holdout_exact_coverage_increment_bytes'] = summary['lossless']['all']['bytes'] - summary['lossless']['train']['bytes']
        sample = holdout[0]
        decoder = PackDecoder(out / 'lossless-all.pack')
        rebuilt = decoder.decode(sample['id'])
        decoder.close()
        Image.fromarray(rebuilt).save(out / 'lossless-holdout-reconstruction.png')
        triptych(out / 'lossless-holdout-comparison.png', images[sample['id']], rebuilt, 'Exact stored holdout state: ' + sample['id'])
        summary['pca'] = pca_experiment(out, states, images, provenance) if args.pca else {'enabled': False}
        quality_plot(out / 'quality-comparison.svg', summary['pca'])
        if sha(manifest_path.read_bytes()) != summary['reference_manifest_sha256']:
            raise ValueError('Capture manifest changed during experiment')
        for state in states:
            if sha((captures / state['image']).read_bytes()) != source_hashes[state['id']]:
                raise ValueError('Capture image changed during experiment')
            if state['id'] in optical_sources and sha(optical_sources[state['id']].read_bytes()) != state['optical_sha256']:
                raise ValueError('Capture optical data changed during experiment')
        if sha(Path(__file__).read_bytes()) != summary['implementation_sha256']:
            raise ValueError('Codec implementation changed during experiment')
        summary['complete'] = True
        write_json(out / 'summary.json', summary)
        print(json.dumps({'complete': True, 'summary': str(out / 'summary.json'),
                          'lossless_bytes': {k: v['bytes'] for k, v in summary['lossless'].items()},
                          'pca': args.pca}), flush=True)
    except BaseException as error:
        summary['failure'] = str(error)
        write_json(out / 'summary.json', summary)
        raise


if __name__ == '__main__':
    main()
