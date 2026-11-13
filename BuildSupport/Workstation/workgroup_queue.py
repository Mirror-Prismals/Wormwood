"""Generate private compute-only shared-memory ray-queue experiments.

python3 BuildSupport/Workstation/workgroup_queue.py --shader frozen.wgsl
  --output output/checks/queue-shared --workgroup-size 4
Add --scalar-priorities for a separate shared scalar array used by priority scans.

This script writes source and metadata only. It never compiles, renders, launches
the probe, edits production shaders or changes the optical frame. The caller must
run the generated shader with compute_probe's matching workgroup-size argument.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re


def digest(data):
    return hashlib.sha256(data).hexdigest()


def function_body(source, name):
    match = re.search(r"\bfn\s+" + re.escape(name) + r"\([^)]*\)[^{]*\{", source)
    if not match:
        raise ValueError(f"missing function {name}")
    start = match.end()
    depth = 1
    for end in range(start, len(source)):
        if source[end] == "{":
            depth += 1
        elif source[end] == "}":
            depth -= 1
            if depth == 0:
                return start, end
    raise ValueError(f"unclosed function {name}")


def remove_graphics_entries(source):
    # WGSL validates every entry, even when only a compute pipeline is created.
    # Workgroup variables cannot remain reachable from the fragment entry.
    removed = []
    while match := re.search(r"@(vertex|fragment)\s+fn\s+(\w+)\s*\(", source):
        _, end = function_body(source, match[2])
        removed.append(match[2])
        source = source[:match.start()] + source[end + 1:]
    return source, removed


def shared_queue(source, workgroup_size=4, scalar_priorities=False):
    if workgroup_size not in (1, 2, 4):
        raise ValueError("shared queues support square workgroups 1, 2 or 4; 8x8 exceeds the portable workgroup-memory limit")
    if "workstationProbeInvocation" in source or "workstationPendingRays" in source:
        raise ValueError("input already contains a probe-local queue")
    lanes = workgroup_size * workgroup_size
    slots = lanes * 16
    start, end = function_body(source, "gemTrace")
    original = source[start:end]
    declaration = "var pending:array<GemRay,16>;"
    if original.count(declaration) != 1:
        raise ValueError("gemTrace no longer has the expected private 16-ray queue")
    indices = re.findall(r"\bpending\[([^]]+)\]", original)
    if not indices or set(indices) != {"candidate", "strongest", "count"}:
        raise ValueError("unrecognized canonical queue indexing")
    body = original.replace(declaration, "")
    scalar_writes = 0
    if scalar_priorities:
        priority = "pending[candidate].priority"
        if body.count(priority) != 1:
            raise ValueError("canonical priority scan changed")
        body = body.replace(priority,
            f"workstationPendingPriorities[u32(candidate)*{lanes}u+workstationQueueLane]")

        def write_priority(match):
            return (match[0] + f"workstationPendingPriorities[u32({match[1]})*{lanes}u+"
                    f"workstationQueueLane]={match[2]}.priority;")

        expected_writes = len(re.findall(r"\bpending\[(\w+)\]=(\w+);", original))
        body, scalar_writes = re.subn(r"\bpending\[(\w+)\]=(\w+);", write_priority, body)
        if expected_writes < 3 or scalar_writes != expected_writes:
            raise ValueError("canonical queue write sites changed unexpectedly")
    body, replaced = re.subn(r"\bpending\[(\w+)\]",
        lambda m: f"workstationPendingRays[u32({m[1]})*{lanes}u+workstationQueueLane]", body)
    if replaced != len(indices) - int(scalar_priorities) or re.search(r"\bpending\b", body):
        raise ValueError("not all canonical queue accesses were redirected")
    # This is intentionally a storage substitution. The integrator's control
    # flow, queue length, count, full GemRay records, samples, event randoms and
    # floating-point arithmetic remain unchanged.
    result = source[:start] + body + source[end:]
    record = re.search(r"\bstruct\s+GemRay\s*\{[^}]+\};", result)
    if not record:
        raise ValueError("missing canonical GemRay record")
    declarations = f"""
// PRIVATE COMPUTE EXPERIMENT: one lane owns each slot at slot*{lanes}+lane.
// No lane accesses another lane's queue, so no workgroup barrier is needed.
// Serial primary samples reuse the same slots after count is reset to zero.
var<workgroup> workstationPendingRays:array<GemRay,{slots}>;
var<private> workstationQueueLane:u32=0u;
fn workstationProbeInvocation(lane:u32){{workstationQueueLane=lane;}}
"""
    if scalar_priorities:
        declarations += f"var<workgroup> workstationPendingPriorities:array<f32,{slots}>;\n"
    result = result[:record.end()] + declarations + result[record.end():]
    result, removed = remove_graphics_entries(result)
    if "fs_main" not in removed:
        raise ValueError("expected canonical fragment entry was not removed")
    return result, {
        "workgroup_size": workgroup_size,
        "workgroup_lanes": lanes,
        "queue_slots_per_invocation": 16,
        "queue_record_bytes": 48,
        "shared_bytes": slots * (48 + (4 if scalar_priorities else 0)),
        "scalar_priorities": scalar_priorities,
        "scalar_priority_write_sites": scalar_writes,
        "redirected_queue_accesses": replaced,
        "removed_graphics_entries": removed,
        "invocation_hook": "workstationProbeInvocation",
        "samples_unchanged": True,
        "optical_frame_unchanged": True,
        "integrator_arithmetic_unchanged": True,
        "validation": "source generation only; compilation, numerical/image equivalence and GPU speed are unverified",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--shader", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="private output prefix")
    parser.add_argument("--workgroup-size", type=int, default=4)
    parser.add_argument("--scalar-priorities", action="store_true")
    args = parser.parse_args()
    original = args.shader.read_bytes()
    generated, metadata = shared_queue(original.decode(), args.workgroup_size, args.scalar_priorities)
    shader = args.output.with_suffix(".wgsl")
    report = args.output.with_suffix(".json")
    production = Path(__file__).resolve().parents[2] / "Procedures" / "Shaders"
    for path in (shader, report):
        resolved = path.resolve()
        if resolved == args.shader.resolve() or production == resolved.parent:
            raise ValueError("private experiment must not overwrite its input or production shader directory")
    encoded = generated.encode()
    metadata.update({
        "schema": 1,
        "experiment": "workgroup_owned_ray_queue",
        "source_shader": str(args.shader.resolve()),
        "source_shader_sha256": digest(original),
        "generated_shader": str(shader.resolve()),
        "generated_shader_sha256": digest(encoded),
        "generator_sha256": digest(Path(__file__).read_bytes()),
        "probe_arguments": "shader.wgsl unchanged-frame.bin width height output-prefix iterations " + str(args.workgroup_size),
    })
    args.output.parent.mkdir(parents=True, exist_ok=True)
    shader.write_bytes(encoded)
    report.write_text(json.dumps(metadata, indent=2) + "\n")
    print(json.dumps({"shader": str(shader), "metadata": str(report), "shared_bytes": metadata["shared_bytes"]}))


if __name__ == "__main__":
    main()
