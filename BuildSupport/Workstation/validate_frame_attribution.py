#!/usr/bin/env python3
"""Build one separate app and collect short native cause/timing diagnostics.

Never installs/promotes. Three matched idle runs bracket profiling overhead;
one finite motion replay observes cache causes. These are not sustained60FPS
or visual-acceptance evidence. Root runs this only after other GPU work ends.
"""
from pathlib import Path
import datetime
import json
import math
import os
import shutil
import statistics
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT/'BuildSupport'))
from checked_process import run_checked
from release_approved_ruby import source_manifest, tree_hashes, digest
from profile_workstation_sustained import summarize, attempt_disposition, valid_counter_delta


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def stages(samples, field):
    records = [sample[field] for sample in samples]
    names = records[0]['stage_names']
    require(all(record['stage_names']==names for record in records), 'Wall stage names changed')
    for record in records:
        values = record['wall_ms']
        require(len(values)==len(names) and all(math.isfinite(x) and x>=0 for x in values),
                'Invalid wall stage interval')
        require(math.isclose(sum(values),record['total_wall_ms'],rel_tol=1e-9,abs_tol=1e-6),
                'Adjacent wall intervals do not match their scope')
    return {'scope':records[0]['scope'],'stage_mean_ms':{
        name:statistics.fmean(record['wall_ms'][i] for record in records) for i,name in enumerate(names)},
        'total_mean_ms':statistics.fmean(record['total_wall_ms'] for record in records)}


def validate_samples(native, profile, cause_enabled):
    benchmark = native['benchmark']
    samples = benchmark['samples']
    require(benchmark['complete'] is True and samples, 'Native replay incomplete')
    require([s['draw_index'] for s in samples]==list(range(benchmark['warmup_frames']+1,
        benchmark['warmup_frames']+1+len(samples))), 'Missing/reordered native draws')
    for sample in samples:
        web = sample['webgpu_draw_wall']
        require(web['complete'] is True and web['capture_performed'] is False,
                'Incomplete or captured measured draw')
        for name in ('performance_enabled','timing_object_enabled','query_slot_recorded'):
            require(web[name] is bool(profile), 'Profile selection mismatch: '+name)
        require(sample['application_wall']['performance_enabled'] is bool(profile),
                'Application profile selection mismatch')
        require(sample['application_wall']['audio_producer_called'] is True,
                'Audio producer absent from measured frame')
        require(('cache_causes' in sample['ruby_response']) is cause_enabled,
                'Cache observer flag mismatch')
    if profile:
        require(native['gpu_measurement']['complete'] is True, 'Missing GPU query coverage')
    else:
        require(native['gpu_measurement']['complete'] is False, 'Profile-off unexpectedly produced GPU queries')
    audio = native['benchmark_audio']
    device = audio['cleanup']['native']
    require(audio.get('started') is True and not audio.get('runtime_error')
        and device.get('callbacks',0)>0 and device.get('samples_produced',0)>0
        and device.get('stream_output_peak',0)>0
        and audio.get('final_status',{}).get('transport',{}).get('position_frame',0)>0,
        'Actual native audio playback was not established')
    window = audio['measured_counter_window']
    require(all(valid_counter_delta(window.get(k)) for k in ('transport_underruns','vm_underrun_periods')),
            'Invalid measured audio counter deltas')
    causes = {}
    if cause_enabled:
        for sample in benchmark['warmup_samples']+samples:
            cause = sample['ruby_response']['cache_causes']
            index = cause['update_index']
            require(index>0 and cause['status']!='sample_budget_exhausted', 'Cache diagnostic budget exhausted')
            if index in causes:
                require(causes[index]==cause, 'Same update index has inconsistent observations')
            causes[index]=cause
    return {'webgpu_wall':stages(samples,'webgpu_draw_wall'),
        'application_wall':stages(samples,'application_wall'),
        'audio_native_configuration':{k:device.get(k) for k in
            ('output_device_uid','sample_rate_hz','buffer_frames','mode','realtime')},
        'audio_playback_observed':True,'audio_measured_counter_window':window,
        'cache_update_indices':sorted(causes),'unique_cache_updates':len(causes),
        'cause_reports':list(causes.values()) if cause_enabled else [],
        'measured_loop_fps':benchmark['measured_loop_frames_per_second'],
        'framebuffer_dimensions':benchmark['dimensions'],
        'logical_dimensions':sorted({tuple(s['logical_dimensions']) for s in samples}),
        'graphics_device':{k:benchmark['device_last'].get(k) for k in
            ('api','implementation','native_backend','device','description','width','height')}}


def main():
    out = ROOT/'output/checks'/('frame-attribution-'+datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f'))
    out.mkdir(parents=True)
    installed = ROOT/'output/macOS/MirrorPrismals.app/Contents/MacOS/wormwood-engine'
    report = {'status':'running','installed':False,'sustained_fps_validation':False,
        'scope':'Short fullscreen1920x1200-class diagnostics with actual dimensions recorded. '
                'Idle profile0/1/0 uses identical rendering and wall observers. Motion separately '
                'enables cache-cause records; its FPS includes observer overhead. CPU wall stages '
                'are nested scopes, not additive GPU costs. No claim of sustained60FPS.',
        'steps':[],'attempts':[],'runs':{}}
    before = None
    staged_before = None
    app = out/'staged/MirrorPrismals.app'
    installed_before = digest(installed)

    def save():
        (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')

    def run(label, argv, timeout=120, env=None):
        print(label,flush=True)
        log = out/(label+'.log')
        try:
            run_checked(list(map(str,argv)),cwd=ROOT,log=log,env=env,timeout=timeout)
        except BaseException:
            if log.exists(): print(log.read_text(errors='replace')[-5000:],flush=True)
            raise
        report['steps'].append(label)
        save()

    try:
        require(shutil.disk_usage(ROOT).free>1024**3,'Need1GiB for separate app and native evidence')
        before = source_manifest()
        (out/'source-before.json').write_text(json.dumps(before,indent=2)+'\n')
        deps = ROOT/'target/release/deps'
        serde = max(deps.glob('libserde_json-*.rlib'),key=lambda p:p.stat().st_mtime)
        for name in ('RubyResponsePlan','Benchmark'):
            binary = out/('test-'+name)
            run('compile-test-'+name,['rustc','--edition=2024','--test','-O',
                ROOT/'Workstation'/f'{name}.rs','--extern','serde_json='+str(serde),
                '-L','dependency='+str(deps),'-o',binary])
            run('test-'+name,[binary,'--test-threads=1'])
        run('build-staged',[sys.executable,ROOT/'BuildSupport/macOS/build_app.py','--output-dir',out/'staged'],1200)
        run('verify-signing',['codesign','--verify','--deep','--strict',app])
        engine = app/'Contents/MacOS/wormwood-engine'
        env = {k:v for k,v in os.environ.items() if not k.startswith('WORMWOOD_')}
        env['WORMWOOD_ASSET_ROOT']=str(app/'Contents/Resources/EngineAssets')
        run('inspect-intent',[engine,'--inspect-intent','--json'],env=env)
        inspection = json.loads((out/'inspect-intent.log').read_text())
        require(inspection['summary']['active_structural_mismatches']==0,'Intent structure mismatch')
        report['intent_summary']=inspection['summary']
        require(source_manifest()==before,'Source changed while building')
        staged_before = tree_hashes(app)
        idle_signature = None
        for label,scenario,profile,causes in [('idle-profile0-a','idle',0,False),
            ('idle-profile1','idle',1,False),('idle-profile0-b','idle',0,False),
            ('motion-cache-causes','motion',1,True)]:
            warmup, frames = (120,32) if scenario=='idle' else (32,64)
            for attempt in range(1,4):
                destination = out/label/f'attempt-{attempt}'
                destination.mkdir(parents=True)
                effective = {'WORMWOOD_ASSET_ROOT':str(app/'Contents/Resources/EngineAssets'),
                    'WORMWOOD_OUTPUT_ROOT':str(destination),'WORMWOOD_FULLSCREEN':'1',
                    'WORMWOOD_WORKSTATION_RETINA':'0','WORMWOOD_PERFORMANCE':'0',
                    'WORMWOOD_WORKSTATION_GPU_TIMING':'0','WORMWOOD_WORKSTATION_CPU_STAGES':'0',
                    'WORMWOOD_WORKSTATION_FRAME_ATTRIBUTION':'1',
                    'WORMWOOD_WORKSTATION_BENCHMARK_PROFILE':str(profile),
                    'WORMWOOD_WORKSTATION_CACHE_CAUSES':str(int(causes)),
                    'WORMWOOD_WORKSTATION_PIXEL_TRANSPORT':'1',
                    'WORMWOOD_WORKSTATION_FLOOR_SAMPLE_CACHE':'1',
                    'WORMWOOD_WORKSTATION_RESOLVE_REUSE':'1',
                    'WORMWOOD_WORKSTATION_BENCHMARK_WARMUP':str(warmup),
                    'WORMWOOD_WORKSTATION_BENCHMARK_CYCLE':'32',
                    'WORMWOOD_WORKSTATION_BENCHMARK_AUDIO':'1',
                    'WORMWOOD_WORKSTATION_BENCHMARK_SCENARIO':scenario,
                    'WORMWOOD_WORKSTATION_BENCHMARK_MIN_SECONDS':'0'}
                native_env = {k:v for k,v in os.environ.items() if not k.startswith('WORMWOOD_')}
                native_env.update(effective)
                attempt_record = {'label':label,'attempt':attempt,'environment':effective,'exit_success':False}
                try:
                    run(label+f'-attempt-{attempt}',[engine,'--benchmark-workstation','WebGPU','--frames',frames],300,native_env)
                    attempt_record['exit_success']=True
                except Exception as error:
                    kind = 'exit_status' if isinstance(error,subprocess.CalledProcessError) else (
                        'timeout' if isinstance(error,subprocess.TimeoutExpired) else 'launch_or_internal')
                    attempt_record.update(error=str(error),process_failure_kind=kind,
                                          exit_code=getattr(error,'returncode',None))
                attempt_record['measurement']=summarize(destination,False,warmup,32,frames,True,scenario,0)
                disposition = attempt_disposition(attempt_record,attempt,3)
                attempt_record['disposition']=disposition
                report['attempts'].append(attempt_record)
                save()
                if disposition['action']=='retry_fresh_session': continue
                require(disposition['action']=='include','Native diagnostic failed: '+disposition['reason'])
                native = json.loads((destination/'workstation-benchmark-WebGPU.json').read_text())
                result = validate_samples(native,profile,causes)
                result['output']=str(destination)
                report['runs'][label]=result
                if scenario=='idle':
                    signature={k:result[k] for k in ('graphics_device','framebuffer_dimensions',
                        'logical_dimensions','audio_native_configuration')}
                    if idle_signature is None: idle_signature=signature
                    require(signature==idle_signature,'Idle profiling comparison conditions changed')
                save()
                break
        report['status']='completed_short_native_attribution'
    except BaseException as error:
        report.update(status='failed_short_native_attribution',error=str(error))
        raise
    finally:
        checks = {'installed_engine_unchanged':digest(installed)==installed_before}
        if before is not None: checks['source_unchanged']=source_manifest()==before
        if staged_before is not None: checks['staged_bundle_unchanged']=tree_hashes(app)==staged_before
        report.update(checks)
        if not all(checks.values()): report['status']='failed_identity_check'
        save()
        print(json.dumps({'status':report['status'],'installed':False,'report':str(out/'report.json')}),flush=True)
        if not all(checks.values()): raise RuntimeError('Source or app identity changed')


if __name__=='__main__':
    main()
