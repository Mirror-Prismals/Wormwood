#!/usr/bin/env python3
"""Fetch SHA-256 pinned official iOS graphics SDKs into local build output."""
from pathlib import Path
import hashlib,json,subprocess,tarfile,zipfile,tempfile,shutil
ROOT=Path(__file__).resolve().parents[2]
ASSETS=[
 ('wgpu','https://github.com/gfx-rs/wgpu-native/releases/download/v27.0.4.0/wgpu-ios-aarch64-release.zip','d7adb36b2ca7aa22c40bcf9ac96f1222353533040027db4f95288cdb590c06be'),
 ('moltenvk','https://github.com/KhronosGroup/MoltenVK/releases/download/v1.4.2/MoltenVK-ios.tar','b5d947b1660e6e9fed40b9cd2387e160aaab9e80b775c0cef7e14059405178c1')]
def setup():
    cache=ROOT/'output/ios-sdk';cache.mkdir(parents=True,exist_ok=True)
    for name,url,digest in ASSETS:
        archive=cache/url.rsplit('/',1)[-1]
        if not archive.exists() or hashlib.sha256(archive.read_bytes()).hexdigest()!=digest:
            temporary=archive.with_suffix('.partial')
            subprocess.run(['curl','--fail','--location','--retry','2','--max-time','120',url,'--output',str(temporary)],check=True)
            if hashlib.sha256(temporary.read_bytes()).hexdigest()!=digest:raise RuntimeError(f'SDK digest mismatch: {name}')
            temporary.replace(archive)
        destination=cache/name
        if not destination.exists():
            temporary=Path(tempfile.mkdtemp(prefix=name+'-',dir=cache))
            if archive.suffix=='.zip':
                with zipfile.ZipFile(archive) as z:
                    for item in z.infolist():
                        path=Path(item.filename)
                        if path.is_absolute() or '..' in path.parts:raise RuntimeError('Unsafe SDK archive path')
                        if (item.external_attr>>16)&0o170000==0o120000:raise RuntimeError('SDK archive symlink rejected')
                    z.extractall(temporary)
            else:
                with tarfile.open(archive) as tar:tar.extractall(temporary,filter='data')
            temporary.rename(destination)
        print(f'Verified {name} SDK: {digest}',flush=True)
    wgpu=next((cache/'wgpu').rglob('libwgpu_native.a'))
    molten=next(p for p in (cache/'moltenvk').rglob('libMoltenVK.a') if 'ios-arm64' in p.parts)
    header=next((cache/'wgpu').rglob('wgpu.h'))
    info={'wgpu_library':str(wgpu),'moltenvk_library':str(molten),'wgpu_include':str(header.parent),'wgpu_version':'27.0.4.0','moltenvk_version':'1.4.2','archives':{name:digest for name,_,digest in ASSETS}}
    (cache/'sdk.json').write_text(json.dumps(info,indent=2)+'\n')
    return info
if __name__=='__main__':setup()
