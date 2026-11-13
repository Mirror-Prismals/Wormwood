#!/usr/bin/env python3
"""Build and install the user's approved direct-studio ruby appearance.

Promotion requires matching the accepted preview, passing focused/native checks,
and unchanged build inputs. The previous local app is preserved by atomic swap.
"""
from pathlib import Path
import ctypes
import datetime
import hashlib
import json
import os
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT/'BuildSupport'))
sys.path.insert(0, str(ROOT/'BuildSupport/macOS'))
from checked_process import run_checked
from build_combined import build_input, tree_hashes
from check_ruby_split import compare


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream,'sha256').hexdigest()


def source_manifest():
    names=subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z'],cwd=ROOT,timeout=30)
    names={os.fsdecode(n) for n in names.split(b'\0') if n}
    result={name:digest(ROOT/name) if (ROOT/name).is_file() else None
            for name in sorted(names) if build_input(name)}
    if 'Cargo.toml' not in result or 'lib.rs' not in result:
        raise RuntimeError('Build manifest is not rooted in Wormwood')
    return result


def swap(first,second):
    # Darwin's RENAME_SWAP exchanges both existing paths atomically.
    libc=ctypes.CDLL(None,use_errno=True)
    operation=libc.renamex_np
    operation.argtypes=[ctypes.c_char_p,ctypes.c_char_p,ctypes.c_uint]
    operation.restype=ctypes.c_int
    if operation(os.fsencode(first),os.fsencode(second),2):
        error=ctypes.get_errno()
        raise OSError(error,os.strerror(error))


def main():
    stamp=datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f')
    out=ROOT/'output/checks'/('ruby-approved-'+stamp)
    out.mkdir(parents=True)
    installed=ROOT/'output/macOS/MirrorPrismals.app'
    backup=installed.with_name('MirrorPrismals-before-ruby-'+stamp+'.app')
    reference=ROOT/'output/checks/ruby-cache-20261031-141437-244706'
    approved=ROOT/'output/checks/ruby-cost-20261031-142537-904513/omit-exterior-occlusion.rgba32f'
    fixture=ROOT/'output/checks/optics-rethink-20261031-061827-752714/inputs/controls.bin'
    report={'status':'running','installed':False,'steps':[],
            'approved_reference':str(approved),'exterior_policy':'studio_environment'}
    def save():
        (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    def run(name,command,timeout=180,env=None):
        print(name,flush=True)
        log=out/(name+'.log')
        try:
            run_checked(list(map(str,command)),cwd=ROOT,log=log,timeout=timeout,env=env)
        except BaseException:
            print(log.read_text(errors='replace')[-7000:],flush=True)
            raise
        report['steps'].append(name);save()
    def require(condition,message):
        if not condition: raise RuntimeError(message)
    try:
        require(shutil.disk_usage(ROOT).free>500*1024**2,'Need 500 MiB for release evidence')
        require(installed.is_dir() and not installed.is_symlink(),'Expected an existing local app')
        require(not backup.exists(),'Backup destination already exists')
        old_bundle=tree_hashes(installed)
        frame_hash=digest(fixture)
        approved_hash=digest(approved)
        run('inspect-intent-before',[installed/'Contents/MacOS/wormwood-engine','--inspect-intent'])
        run('generate-shaders',[sys.executable,'Procedures/Shaders/generate_workstation.py'])
        before=source_manifest()
        (out/'source-before.json').write_text(json.dumps(before,indent=2)+'\n')
        run('test-workstation',['cargo','test','--release','--offline','--locked','--lib','workstation','--','--test-threads=1'],1200)
        deps=ROOT/'target/release/deps'
        naga=max(deps.glob('libnaga-*.rlib'),key=lambda p:p.stat().st_mtime)
        serde_json=max(deps.glob('libserde_json-*.rlib'),key=lambda p:p.stat().st_mtime)
        exporter=out/'export-ruby'
        run('compile-exporter',['rustc','--edition=2024','-O','BuildSupport/Workstation/export_ruby_cache.rs',
            '--extern','naga='+str(naga),'--extern','serde_json='+str(serde_json),
            '-L','dependency='+str(deps),'-o',exporter])
        run('export-approved-shader',[exporter,ROOT/'Procedures/Shaders/workstation.wgsl',fixture,out/'response'])
        probe=reference/'ruby-cache-probe'
        for name,shader,plan,mode in [
            ('baseline',reference/'response.wgsl',reference/'response.plan.bin','baseline'),
            ('approved',out/'response.wgsl',out/'response.plan.bin','warm'),
            ('approved-cold',out/'response.wgsl',out/'response.plan.bin','cold')]:
            run('gpu-'+name,[probe,shader,fixture,plan,640,480,out/name,6,mode],300)
            report[name]=json.loads((out/(name+'.json')).read_text())
            require(not report[name].get('nonfinite_output_channels',0),'Nonfinite GPU output')
        report['preview_comparison']=compare(approved,out/'approved.rgba32f')
        require(report['preview_comparison']['max_rgb_error']<=0.00003 and
                report['preview_comparison']['alpha_count_mismatches']==0,
                'Production shader does not match the user-approved preview')
        report['warm_speedup']=report['baseline']['median_gpu_ms']/report['approved']['median_gpu_ms']
        report['cold_speedup']=report['baseline']['median_gpu_ms']/report['approved-cold']['median_gpu_ms']
        run('approved-image',['/usr/bin/sips','-s','format','png',out/'approved.ppm','--out',out/'approved.png'])
        run('build-app',[sys.executable,'BuildSupport/macOS/build_app.py','--output-dir',out/'candidate'],1200)
        app=out/'candidate/MirrorPrismals.app'
        engine=app/'Contents/MacOS/wormwood-engine'
        bundle=tree_hashes(app)
        report['staged_app']=str(app)
        for name,args in [('native-baseline',['--benchmark-workstation','WebGPU','--frames','32']),
                          ('native-approved',['--benchmark-workstation','WebGPU','--frames','32']),
                          ('native-controls',['--check-workstation-controls','WebGPU'])]:
            destination=out/name;destination.mkdir()
            env={k:v for k,v in os.environ.items() if not k.startswith('WORMWOOD_')}
            env.update(WORMWOOD_ASSET_ROOT=str(app/'Contents/Resources/EngineAssets'),
                WORMWOOD_OUTPUT_ROOT=str(destination),WORMWOOD_FULLSCREEN='0',
                WORMWOOD_WORKSTATION_RETINA='0',WORMWOOD_WORKSTATION_SIZE='640x480',WORMWOOD_PERFORMANCE='1')
            if name=='native-baseline': env['WORMWOOD_RUBY_RESPONSE']='0'
            run(name,[engine,*args],300,env)
            selection=json.loads((destination/'workstation-shader-WebGPU.json').read_text())
            if name!='native-baseline':
                require(selection['selected']=='ruby_response' and selection['snapshot']=='session_end' and
                        selection['ruby_response']['exterior_policy']=='studio_environment',
                        'Default native renderer did not select the approved shader')
            report[name+'-selection']=selection
            if name=='native-controls':
                controls=json.loads((destination/'workstation-controls-WebGPU.json').read_text())
                require(controls.get('complete') is True,'Native control diagnostic incomplete')
                run('native-approved-image',['/usr/bin/sips','-s','format','png',destination/'triangle-WebGPU.ppm',
                    '--out',out/'native-approved.png'])
            else:
                benchmark=json.loads((destination/'workstation-benchmark-WebGPU.json').read_text())
                require(benchmark['benchmark']['complete'] and benchmark['gpu_measurement']['complete'],
                        'Native performance measurement incomplete')
                report[name]={'benchmark':benchmark['benchmark'],'gpu_measurement':benchmark['gpu_measurement']}
            save()
        report['native_speedup']=report['native-baseline']['benchmark']['frame_ms_mean']/report['native-approved']['benchmark']['frame_ms_mean']
        require(report['native_speedup']>1.,'Native approved renderer did not improve measured frame time')
        run('codesign-verify',['/usr/bin/codesign','--verify','--deep','--strict',app],60)
        after=source_manifest()
        require(before==after,'Build inputs changed during validation; staged app retained')
        require(bundle==tree_hashes(app),'Staged app changed during validation')
        require(old_bundle==tree_hashes(installed),'Installed app changed concurrently; staged app retained')
        require(frame_hash==digest(fixture) and approved_hash==digest(approved),'Reference evidence changed')
        report.update(status='validated_promoting',backup=str(backup),installed_app=str(installed),
                      approved_preview_sha256=approved_hash)
        save()
        swap(app,installed)
        try:
            require(tree_hashes(installed)==bundle,'Promoted bundle hash mismatch')
            app.rename(backup)
        except BaseException:
            if app.exists(): swap(app,installed)
            raise
        report.update(status='completed_installed',installed=True)
        save()
        print(json.dumps({'status':report['status'],'app':str(installed),'backup':str(backup),
            'report':str(out/'report.json'),'warm_speedup':report['warm_speedup'],
            'cold_speedup':report['cold_speedup'],'native_speedup':report['native_speedup']}),flush=True)
    except BaseException as error:
        report['status']='failed';report['error']=str(error);save();raise


if __name__=='__main__':
    main()
