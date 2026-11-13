"""Diagnostic-only projected control-triangle grid for axial (0,0,-1) rays.

No production shader or frame header is modified. ``build(frame_bytes)`` returns
``(packed_frame_bytes, metadata)``; the caller embeds metadata["header_row"] in
its private shader. Original frame bytes remain an identical prefix.

Appended float4 ABI (all addresses are absolute row addresses):
  H   = [version=1, edge=32, instance_map_base=H+1, instance_count=65]
  map = [descriptor or -1, expected_root, expected_end, expected_material_base]
  D+0 = [lo_x, lo_y, inverse_cell_width, inverse_cell_height]
  D+1 = [cells_base, packed_references_base, edge, maximum_cell_count]
  D+2 = [root, end, material_base, admitted_triangle_count]
  D+3 = [lo_x, lo_y, hi_x, hi_y]
  cell= [reference_offset, reference_count, 0, 0], indexed y*edge+x
  refs= four original triangle row addresses per row; cell offset is a scalar
        index, so address=gemRow(refs_base+index/4)[index%4].

Cell lists keep the original node-ascending, then leaf-triangle order. The
shader must keep instance order, original triangle narrowphase/front-face test,
strict t<nearest comparison, group visibility and all monitor/plugin clipping.
Use only for exactly axial negative-Z exterior rays. A missing descriptor or
changed tree/material identity requires the original traversal. Geometry must
remain immutable while a generated grid is in use; no shape hash is computed
per frame. Pose, uniform scale, material values and clip bounds stay live.

Each projected triangle's padded XY AABB is inserted conservatively. This is
only a candidate selector: it does not approximate ray/triangle intersections.
Triangles whose authored shading normals cannot face -Z may be omitted. A
geometry exceeding a bound falls back in its entirety, without partial lists.
Maximum output is 128 MiB; bounds also limit per-cell and total duplicate work.
"""
from __future__ import annotations

import argparse
import array
import hashlib
import json
import math
from pathlib import Path
import struct
import sys

EDGE = 32
MAX_BYTES = 128 * 1024 * 1024
MAX_INSTANCES = 65
MAX_NODES = 131072
MAX_TRIANGLES = 524288
MAX_CELL_REFERENCES = 4096
MAX_GEOMETRY_REFERENCES = 2_000_000
MAX_TOTAL_REFERENCES = 4_000_000
FLOAT32_EPSILON = 2.0 ** -23


class GridFallback(ValueError):
    """A bounded geometry cannot use the grid and must use its source BVH."""


def f32(value: float) -> float:
    return struct.unpack("<f", struct.pack("<f", value))[0]


def integer(value: float, label: str, maximum: int = 16_777_215) -> int:
    if not math.isfinite(value) or value < 0 or value > maximum or value != int(value):
        raise GridFallback(f"{label} is not a bounded exact nonnegative integer")
    return int(value)


class Frame:
    def __init__(self, payload: bytes):
        if len(payload) < 256 * 16 or len(payload) % 16 or len(payload) > MAX_BYTES:
            raise ValueError("frame must contain 256..8388608 complete float4 rows")
        self.values = array.array("f")
        self.values.frombytes(payload)
        if sys.byteorder != "little":
            self.values.byteswap()
        if not all(map(math.isfinite, self.values)):
            raise ValueError("frame contains nonfinite values")
        self.count = len(self.values) // 4

    def row(self, address: int) -> tuple[float, ...]:
        if address < 0 or address >= self.count:
            raise GridFallback(f"frame row {address} is outside the original packet")
        return tuple(self.values[4 * address:4 * address + 4])


def pose_row(frame: Frame, instance: int) -> int:
    if instance >= 61:
        return integer(frame.row(232)[0], "navigation pose base") + 32 + 2 * (instance - 61)
    if instance >= 29:
        return integer(frame.row(247)[0], "lane monitor pose base") + 2 * (instance - 29)
    if instance >= 13:
        return integer(frame.row(232)[0], "plugin pose base") + 2 * (instance - 13)
    if instance >= 10:
        return 226 + 2 * (instance - 10)
    return 192 + 2 * instance


def active(frame: Frame, instance: int) -> bool:
    if instance >= 61:
        return instance - 61 < frame.row(232)[2]
    if instance >= 29:
        return instance - 29 < frame.row(247)[1]
    return instance < frame.row(12)[0]


def projected_triangles(frame: Frame, root: int, end: int, prune_backfaces: bool):
    if root <= 0 or end <= root or end > frame.count or (end - root) % 3:
        raise GridFallback("invalid three-row control BVH range")
    if (end - root) // 3 > MAX_NODES:
        raise GridFallback("control BVH exceeds node bound")
    triangles = []
    source_count = 0
    pruned_count = 0
    for node in range(root, end, 3):
        low, high, metadata = frame.row(node), frame.row(node + 1), frame.row(node + 2)
        escape = integer(low[3], "BVH escape", end)
        if escape <= node or (escape - root) % 3:
            raise GridFallback("control BVH escape is not forward/aligned")
        if any(low[axis] > high[axis] for axis in range(3)):
            raise GridFallback("control BVH has inverted bounds")
        count = integer(metadata[0], "leaf triangle count", 4)
        if count == 0:
            continue
        first = integer(high[3], "triangle row", frame.count - 1)
        if first < end or first + count * 6 > frame.count:
            raise GridFallback("control leaf references invalid triangle rows")
        for triangle in range(count):
            address = first + triangle * 6
            a, b, c = frame.row(address), frame.row(address + 1), frame.row(address + 2)
            integer(a[3], "triangle material", 255)
            source_count += 1
            if source_count > MAX_TRIANGLES:
                raise GridFallback("control geometry exceeds triangle bound")
            normals = [frame.row(address + offset) for offset in (3, 4, 5)]
            if prune_backfaces:
                # This uses authored shading normals, not geometric winding.
                # For accepted barycentrics, interpolation is a convex sum.
                if (normals[0][3] > .5 and normals[0][2] <= 0.) or (
                    normals[0][3] <= .5 and all(normal[2] <= 0. for normal in normals)
                ):
                    pruned_count += 1
                    continue
            if b[3] >= .5:
                # Edge-packed triangles represent a + u*e1 + v*e2. Compute
                # endpoints in double, then pad below for f32 arithmetic.
                points = [a[:2], [a[i] + b[i] for i in range(2)], [a[i] + c[i] for i in range(2)]]
            else:
                points = [a[:2], b[:2], c[:2]]
            low_xy = [min(point[i] for point in points) for i in range(2)]
            high_xy = [max(point[i] for point in points) for i in range(2)]
            triangles.append((address, low_xy, high_xy))
    return triangles, source_count, pruned_count


def make_grid(frame: Frame, root: int, end: int, prune_backfaces: bool):
    triangles, source_count, pruned_count = projected_triangles(frame, root, end, prune_backfaces)
    if not triangles:
        # An all-backfacing geometry is a valid empty grid, but use the original
        # BVH bounds so the descriptor retains a finite, nonzero domain.
        low = list(frame.row(root)[:2])
        high = list(frame.row(root + 1)[:2])
    else:
        low = [min(t[1][axis] for t in triangles) for axis in range(2)]
        high = [max(t[2][axis] for t in triangles) for axis in range(2)]
    guard = [1e-7 + 8. * FLOAT32_EPSILON * max(1., abs(low[i]), abs(high[i])) for i in range(2)]
    origin = [f32(low[i] - 2. * guard[i]) for i in range(2)]
    maximum = [f32(high[i] + 2. * guard[i]) for i in range(2)]
    inverse = [f32(EDGE / (maximum[i] - origin[i])) for i in range(2)]
    if not all(math.isfinite(x) and x > 0. for x in inverse):
        raise GridFallback("grid domain is not finite and positive")
    cells: list[list[int]] = [[] for _ in range(EDGE * EDGE)]
    references = 0
    for address, low_xy, high_xy in triangles:
        # The small cell-coordinate margin also covers the shader's rounded
        # subtraction/multiply at exact grid boundaries. No triangle is shrunk.
        first = [max(0, min(EDGE - 1, math.floor((low_xy[i] - guard[i] - origin[i]) * inverse[i] - 1e-4))) for i in range(2)]
        last = [max(0, min(EDGE - 1, math.floor((high_xy[i] + guard[i] - origin[i]) * inverse[i] + 1e-4))) for i in range(2)]
        additions = (last[0] - first[0] + 1) * (last[1] - first[1] + 1)
        references += additions
        if references > MAX_GEOMETRY_REFERENCES:
            raise GridFallback("projected triangle duplicates exceed geometry bound")
        for y in range(first[1], last[1] + 1):
            for x in range(first[0], last[0] + 1):
                cell = cells[y * EDGE + x]
                if len(cell) >= MAX_CELL_REFERENCES:
                    raise GridFallback("one grid cell exceeds reference bound")
                cell.append(address)
    return {
        "origin": origin, "maximum": maximum, "inverse": inverse, "cells": cells,
        "references": references, "source_triangles": source_count,
        "triangles": len(triangles), "pruned_backfaces": pruned_count,
        "maximum_cell_count": max(map(len, cells)), "coordinate_guard": guard,
    }


def build(payload: bytes, *, prune_backfaces: bool = True) -> tuple[bytes, dict]:
    """Build one bounded immutable-fixture grid; never mutate input bytes."""
    frame = Frame(payload)
    header = frame.count
    # Prefix identity matters: no current shader ABI field is repurposed.
    appended = [[1., float(EDGE), float(header + 1), float(MAX_INSTANCES)]]
    appended.extend([[-1., 0., 0., 0.] for _ in range(MAX_INSTANCES)])
    geometries: dict[tuple[int, int, int], dict] = {}
    instances = []
    total_references = 0
    for instance in range(MAX_INSTANCES):
        if not active(frame, instance):
            continue
        detail = {"instance": instance}
        try:
            at = pose_row(frame, instance)
            pose, tree = frame.row(at), frame.row(at + 1)
            if pose[3] <= 0.:
                detail["fallback"] = "nonpositive scale"
                instances.append(detail)
                continue
            root = integer(tree[0], "control tree root", frame.count - 1)
            end = integer(tree[1], "control tree end", frame.count)
            material = integer(tree[3] if tree[3] > .5 else frame.row(12)[1], "material base", frame.count - 1)
            key = (root, end, material)
            detail.update({"pose_row": at, "root": root, "end": end, "material_base": material})
            appended[1 + instance] = [-1., float(root), float(end), float(material)]
            if key not in geometries:
                geometry = {"root": root, "end": end, "material_base": material, "descriptor_row": -1}
                geometries[key] = geometry
                try:
                    grid = make_grid(frame, root, end, prune_backfaces)
                    if total_references + grid["references"] > MAX_TOTAL_REFERENCES:
                        raise GridFallback("total projected references exceed bound")
                    grid_rows = 4 + EDGE * EDGE + (grid["references"] + 3) // 4
                    if (header + len(appended) + grid_rows) * 16 > MAX_BYTES:
                        raise GridFallback("appended grid would exceed 128 MiB frame limit")
                    descriptor = header + len(appended)
                    cells_base = descriptor + 4
                    references_base = cells_base + EDGE * EDGE
                    appended.extend([
                        [*grid["origin"], *grid["inverse"]],
                        [float(cells_base), float(references_base), float(EDGE), float(grid["maximum_cell_count"])],
                        [float(root), float(end), float(material), float(grid["triangles"])],
                        [*grid["origin"], *grid["maximum"]],
                    ])
                    flat = []
                    for cell in grid["cells"]:
                        appended.append([float(len(flat)), float(len(cell)), 0., 0.])
                        flat.extend(cell)
                    for offset in range(0, len(flat), 4):
                        values = flat[offset:offset + 4]
                        appended.append([float(value) for value in values] + [-1.] * (4 - len(values)))
                    total_references += grid["references"]
                    geometry.update({k: v for k, v in grid.items() if k != "cells"})
                    geometry.update({"descriptor_row": descriptor, "cells_base": cells_base, "references_base": references_base})
                except GridFallback as error:
                    geometry["fallback"] = str(error)
            geometry = geometries[key]
            appended[1 + instance][0] = float(geometry["descriptor_row"])
            detail["descriptor_row"] = geometry["descriptor_row"]
            if "fallback" in geometry:
                detail["fallback"] = geometry["fallback"]
        except GridFallback as error:
            detail["fallback"] = str(error)
        instances.append(detail)
    packed = array.array("f", (value for row in appended for value in row))
    if sys.byteorder != "little":
        packed.byteswap()
    result = payload + packed.tobytes()
    if len(result) > MAX_BYTES:
        raise ValueError("frame has insufficient space for even the grid header")
    metadata = {
        "schema_version": 1, "kind": "diagnostic-axial-control-visibility-grid",
        "header_row": header, "grid_base": header, "map_base": header + 1,
        "edge": EDGE, "instance_count": MAX_INSTANCES, "ray_direction": [0., 0., -1.],
        "source_bytes": len(payload), "output_bytes": len(result), "appended_rows": len(appended),
        "source_sha256": hashlib.sha256(payload).hexdigest(),
        "output_sha256": hashlib.sha256(result).hexdigest(),
        "backface_pruning": prune_backfaces, "total_references": total_references,
        "accelerated_instances": sum(item.get("descriptor_row", -1) >= 0 for item in instances),
        "geometries": list(geometries.values()), "instances": instances,
        "limits": {"bytes": MAX_BYTES, "nodes": MAX_NODES, "triangles": MAX_TRIANGLES,
                   "cell_references": MAX_CELL_REFERENCES, "geometry_references": MAX_GEOMETRY_REFERENCES,
                   "total_references": MAX_TOTAL_REFERENCES},
        "warning": "Candidate selection only; original triangle math, front faces, clipping, instance order and strict nearest tie ownership are required. Frozen geometry only; other ray directions must fall back.",
    }
    return result, metadata


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--frame", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path, help="output prefix (.bin and .json)")
    parser.add_argument("--keep-backfaces", action="store_true")
    args = parser.parse_args()
    if args.frame.stat().st_size > MAX_BYTES:
        raise SystemExit("source frame exceeds 128 MiB")
    binary = args.output.with_suffix(".bin")
    report = args.output.with_suffix(".json")
    if binary.exists() or report.exists():
        raise SystemExit("refusing to overwrite existing diagnostic output")
    packed, metadata = build(args.frame.read_bytes(), prune_backfaces=not args.keep_backfaces)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with binary.open("xb") as file:
        file.write(packed)
    with report.open("x") as file:
        json.dump(metadata, file, indent=2)
        file.write("\n")
    print(json.dumps({key: metadata[key] for key in ("header_row", "output_bytes", "accelerated_instances", "total_references")}))


if __name__ == "__main__":
    main()
