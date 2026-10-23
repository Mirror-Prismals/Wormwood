#!/usr/bin/env python3
"""Snapshot Wormwood and optionally deploy it to an isolated Windows directory."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import io
import base64
import time

ROOT = Path(__file__).resolve().parents[2]
EXCLUDED = {'.git', 'target', 'output', '__pycache__', '.DS_Store', '.idea', '.vscode'}

def snapshot(root, destination):
    files = {}
    mtimes = {}
    for path in sorted(root.rglob('*')):
        relative = path.relative_to(root)
        if any(part in EXCLUDED for part in relative.parts):
            continue
        if path.is_symlink():
            raise ValueError(f'Symlinks must be resolved explicitly before transfer: {relative}')
        if path.is_file():
            files[relative.as_posix()] = path.read_bytes()
            mtimes[relative.as_posix()] = path.stat().st_mtime
    hashes = {name: hashlib.sha256(data).hexdigest() for name, data in files.items()}
    fingerprint = hashlib.sha256(json.dumps(hashes, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
    manifest = json.dumps({'fingerprint': fingerprint, 'files': hashes}, indent=2).encode()
    destination.mkdir(parents=True, exist_ok=True)
    archive = destination / f'{fingerprint}.tar.gz'
    with tarfile.open(archive, 'w:gz') as tar:
        for name, data in [*files.items(), ('source-manifest.json', manifest)]:
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mode = 0o644
            info.mtime = mtimes.get(name, time.time())
            tar.addfile(info, io.BytesIO(data))
    return fingerprint, archive

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--host', default='salamander-windows')
    parser.add_argument('--package-only', action='store_true')
    parser.add_argument('--run', choices=['build', 'test', 'diagnostics', 'verify'], help='Optional Windows verification; native prerequisites must already be installed.')
    parser.add_argument('--soak-seconds',type=int,default=0,help='Optional paced same-process stress per API with --run verify (1..10)')
    parser.add_argument('--soak-repeats',type=int,default=2)
    args = parser.parse_args()
    if not 0<=args.soak_seconds<=10 or not 1<=args.soak_repeats<=20:parser.error('soak seconds 0..10 and repeats 1..20 required')
    if args.soak_seconds and args.run!='verify':parser.error('--soak-seconds requires --run verify')
    with tempfile.TemporaryDirectory(prefix='wormwood-sync-') as temporary:
        fingerprint, archive = snapshot(ROOT, ROOT / 'output' / 'source-packages')
        print(f'Source: {fingerprint}\nPackage: {archive}', flush=True)
        if args.package_only:
            return
        # Upload to the login home. Never touch an existing checkout or branch.
        remote_archive = f'wormwood-{fingerprint}.tar.gz'
        subprocess.run(['scp', str(archive), f'{args.host}:{remote_archive}'], check=True)
        script = r'''
$ErrorActionPreference = 'Stop'
$id = '__ID__'
$archive = Join-Path $HOME ('wormwood-' + $id + '.tar.gz')
$base = Join-Path $HOME 'WormwoodSync'
New-Item -ItemType Directory -Force $base | Out-Null
$run = Join-Path $base ($id + '-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $run | Out-Null
& tar -xzf $archive -C $run
if ($LASTEXITCODE -ne 0) { throw 'Archive extraction failed' }
Remove-Item $archive
Set-Location $run
$manifest = Get-Content source-manifest.json -Raw | ConvertFrom-Json
foreach ($entry in $manifest.files.PSObject.Properties) {
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $entry.Name).Hash.ToLowerInvariant()
    if ($actual -ne $entry.Value) { throw ('Source mismatch: ' + $entry.Name) }
}
$result = @{ fingerprint=$id; directory=$run; source_verified=$true; action='__ACTION__'; exit_code=0 }
try {
    if ('__ACTION__' -ne 'none') {
        if ('__ACTION__' -eq 'verify') {
            & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File BuildSupport/Windows/build_and_verify.ps1 -SoakSeconds __SOAK__ -SoakRepeats __REPEATS__
        } else {
            $cargoArgs = @('__CARGO__', '--locked')
            & cargo @cargoArgs 2>&1 | Tee-Object -FilePath verification.log
        }
        $result.exit_code = $LASTEXITCODE
    }
} catch {
    $_ | Out-String | Set-Content verification.log
    $result.exit_code = 1
}
$result | ConvertTo-Json | Set-Content -Encoding UTF8 result.json
# Machine-readable final line for the Mac collector.
Write-Output ('WORMWOOD_RESULT=' + ($result | ConvertTo-Json -Compress))
'''
        action = args.run or 'none'
        cargo = {'build': 'build', 'test': 'test', 'diagnostics': 'run', 'none': '', 'verify': ''}[action]
        script = script.replace('__ID__', fingerprint).replace('__ACTION__', action).replace('__CARGO__', cargo).replace('__SOAK__',str(args.soak_seconds)).replace('__REPEATS__',str(args.soak_repeats))
        encoded = base64.b64encode(script.encode('utf-16le')).decode()
        completed = subprocess.run(['ssh', args.host, 'powershell.exe', '-NoProfile', '-NonInteractive', '-EncodedCommand', encoded], text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        print(completed.stdout, end='')
        completed.check_returncode()
        lines = [line for line in completed.stdout.splitlines() if line.startswith('WORMWOOD_RESULT=')]
        if not lines:
            raise RuntimeError('Windows did not return a verification result')
        result = json.loads(lines[-1].split('=', 1)[1])
        destination = ROOT / 'output' / 'windows-sync' / Path(result['directory'].replace('\\', '/')).name
        destination.mkdir(parents=True, exist_ok=True)
        (destination / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
        if args.run:
            remote = result['directory'].replace('\\', '/')
            for filename in (['windows-results.zip'] if args.run == 'verify' else ['verification.log']):
                subprocess.run(['scp', f'{args.host}:{remote}/{filename}', str(destination / filename)], check=True)
        if result['exit_code']:
            raise SystemExit(result['exit_code'])

if __name__ == '__main__':
    main()
