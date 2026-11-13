#!/usr/bin/env python3
"""Finite validation/promotion of guarded convex ruby panel baking.

Compare against the currently installed, user-approved Mac app, including its
real motion workload. Preserve that app unless tests, shader comparison, native
checks, a measured improvement, source stability and bundle signing all pass.
"""
from pathlib import Path
import datetime
import json
import os
import shutil
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT/'BuildSupport'))
sys.path.insert(0, str(ROOT/'BuildSupport/macOS'))
from checked_process import run_checked
from build_combined import tree_hashes
from release_approved_ruby import digest, source_manifest, swap
from check_ruby_split import compare


def main():
    stamp = datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f')
    out = ROOT/'output/checks'/('ruby-convex-release-'+stamp)
    out.mkdir(parents=True)
    prior = ROOT/'output/checks/ruby-convex-20261031-205611-021702'
    fixture = ROOT/'output/checks/optics-rethink-20261031-061827-752714/inputs/controls.bin'
    installed = ROOT/'output/macOS/MirrorPrismals.app'
    backup = installed.with_name('MirrorPrismals-before-convex-'+stamp+'.app')
    report = {'status':'running', 'installed':False, 'steps':[],
              'headless_evidence':str(prior/'report.json'),
              'appearance':'Same approved optical equations and full-resolution text; guarded convex panel specialization with canonical fallback.'}

    def save():
        (out/'report.json').write_text(json.dumps(report, indent=2)+'\n')

    def require(condition, message):
        if not condition:
            raise RuntimeError(message)

    def run(name, command, timeout=180, env=None):
        print(name, flush=True)
        log = out/(name+'.log')
        try:
            run_checked(list(map(str, command)), cwd=ROOT, log=log, timeout=timeout, env=env)
        except BaseException:
            print(log.read_text(errors='replace')[-7000:], flush=True)
            raise
        report['steps'].append(name)
        save()

    def native(name, app, controls=False):
        destination = out/name
        destination.mkdir()
        env = {k:v for k,v in os.environ.items() if not k.startswith('WORMWOOD_')}
        env.update(WORMWOOD_ASSET_ROOT=str(app/'Contents/Resources/EngineAssets'),
                   WORMWOOD_OUTPUT_ROOT=str(destination), WORMWOOD_FULLSCREEN='0',
                   WORMWOOD_WORKSTATION_RETINA='0', WORMWOOD_WORKSTATION_SIZE='640x480',
                   WORMWOOD_PERFORMANCE='1')
        args = ['--check-workstation-controls','WebGPU'] if controls else [
            '--benchmark-workstation','WebGPU','--frames','32']
        run(name, [app/'Contents/MacOS/wormwood-engine', *args], 180, env)
        selection = json.loads((destination/'workstation-shader-WebGPU.json').read_text())
        require(selection['selected']=='ruby_response' and selection['snapshot']=='session_end',
                name+': expected the native ruby response pipeline')
        report[name+'-selection'] = selection
        if controls:
            result = json.loads((destination/'workstation-controls-WebGPU.json').read_text())
            require(result.get('complete') is True, 'Native controls diagnostic incomplete')
            report[name] = result
            run(name+'-image', ['/usr/bin/sips','-s','format','png',destination/'triangle-WebGPU.ppm',
                                '--out',out/(name+'.png')], 30)
        else:
            result = json.loads((destination/'workstation-benchmark-WebGPU.json').read_text())
            require(result['benchmark']['complete'] and result['gpu_measurement']['complete'],
                    name+': native benchmark or GPU measurement incomplete')
            report[name] = {'benchmark':result['benchmark'], 'gpu_measurement':result['gpu_measurement']}
        save()

    try:
        require(shutil.disk_usage(ROOT).free>500*1024**2, 'Need500MiB for build and validation')
        require(installed.is_dir() and not installed.is_symlink(), 'Expected existing playable Mac app')
        require(not backup.exists(), 'Backup already exists')
        evidence = json.loads((prior/'report.json').read_text())
        require(evidence['status']=='headless_validated_native_pending', 'Headless evidence incomplete')
        require(all(v['max_rgb_error']<=.00003 and v['alpha_count_mismatches']==0
                    for v in evidence['comparisons'].values()), 'Convex panel image comparison failed')
        old_bundle = tree_hashes(installed)
        expected_image = digest(prior/'production-render.rgba32f')
        expected_shader = digest(prior/'production.wgsl')
        fixture_hash = digest(fixture)
        before = source_manifest()
        (out/'source-before.json').write_text(json.dumps(before, indent=2)+'\n')
        run('inspect-intent', [installed/'Contents/MacOS/wormwood-engine','--inspect-intent'])
        native('native-installed-before', installed)
        run('test-workstation', ['cargo','test','--release','--offline','--locked','--lib','workstation',
                                '--','--test-threads=1'], 1200)
        deps = ROOT/'target/release/deps'
        naga = max(deps.glob('libnaga-*.rlib'), key=lambda p:p.stat().st_mtime)
        serde_json = max(deps.glob('libserde_json-*.rlib'), key=lambda p:p.stat().st_mtime)
        exporter = out/'export-ruby'
        run('compile-exporter', ['rustc','--edition=2024','-O','BuildSupport/Workstation/export_ruby_cache.rs',
                                 '--extern','naga='+str(naga),'--extern','serde_json='+str(serde_json),
                                 '-L','dependency='+str(deps),'-o',exporter])
        run('export-shader', [exporter, ROOT/'Procedures/Shaders/workstation.wgsl', fixture, out/'response'])
        require(digest(out/'response.wgsl')==expected_shader, 'Built shader differs from reviewed candidate')
        run('gpu-candidate', [prior/'split-probe',out/'response.wgsl',fixture,out/'response.plan.bin',
                              640,480,out/'candidate',6,'warm','resolve'])
        report['gpu_candidate'] = json.loads((out/'candidate.json').read_text())
        require(report['gpu_candidate']['nonfinite_output_channels']==0, 'Nonfinite GPU output')
        report['preview_comparison'] = compare(prior/'production-render.rgba32f',out/'candidate.rgba32f')
        require(report['preview_comparison']['max_rgb_error']<=.00003 and
                report['preview_comparison']['alpha_count_mismatches']==0, 'Production preview changed')
        run('candidate-image', ['/usr/bin/sips','-s','format','png',out/'candidate.ppm',
                                '--out',out/'candidate.png'], 30)
        run('build-app', [sys.executable,'BuildSupport/macOS/build_app.py','--output-dir',out/'staged'], 1200)
        app = out/'staged/MirrorPrismals.app'
        bundle = tree_hashes(app)
        report['staged_app'] = str(app)
        native('native-candidate', app)
        native('native-controls', app, controls=True)
        native('native-installed-repeat', installed)
        selected = report['native-candidate-selection']['ruby_response']
        require(selected.get('timing_includes_resolve') is True and selected.get('resolve_submissions',0)>0,
                'Native run omitted live lighting or its timing')
        require(selected.get('bake_kernel')=='guarded_convex_panels' and
                selected.get('timing_includes_general_bake') is True and
                selected.get('general_bake_submissions',0)>0, 'Native run omitted guarded convex panel bake')
        require(selected.get('engraving')=='full_resolution_first_refracted_segment',
                'Native run selected an older text path')
        old_ms = min(report[name]['benchmark']['frame_ms_mean'] for name in
                     ['native-installed-before','native-installed-repeat'])
        new_ms = report['native-candidate']['benchmark']['frame_ms_mean']
        report['native_speedup'] = old_ms/new_ms
        report['native_fps'] = {'installed':1000/old_ms, 'candidate':1000/new_ms}
        require(report['native_speedup']>1.05, 'Candidate did not beat installed motion frame time by5%; retaining playable app')
        run('codesign-verify', ['/usr/bin/codesign','--verify','--deep','--strict',app], 60)
        require(before==source_manifest(), 'Build inputs changed during validation; staged app retained')
        require(bundle==tree_hashes(app), 'Staged app changed during validation')
        require(old_bundle==tree_hashes(installed), 'Installed app changed concurrently; staged app retained')
        require(fixture_hash==digest(fixture) and expected_image==digest(prior/'production-render.rgba32f'),
                'Reference evidence changed during validation')
        report.update(status='validated_promoting', installed_app=str(installed), backup=str(backup))
        save()
        swap(app, installed)
        try:
            require(tree_hashes(installed)==bundle, 'Promoted bundle hash mismatch')
            app.rename(backup)
        except BaseException:
            if app.exists(): swap(app, installed)
            raise
        report.update(status='completed_installed', installed=True)
        save()
        print(json.dumps({'status':report['status'],'app':str(installed),'report':str(out/'report.json'),
                          'native_speedup':report['native_speedup'],'native_fps':report['native_fps']}), flush=True)
    except BaseException as error:
        report.update(status='failed_existing_app_preserved', error=str(error))
        save()
        raise


if __name__=='__main__':
    main()
