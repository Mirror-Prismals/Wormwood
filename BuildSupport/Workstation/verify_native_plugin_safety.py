#!/usr/bin/env python3
"""Build and exercise the native host without opening editors or audio devices."""
import argparse
from pathlib import Path
import subprocess

ROOT=Path(__file__).resolve().parents[2]
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--plugin',type=Path,required=True,help='Installed VST3 bundle to test')
parser.add_argument('--class-name',required=True,help='VST3 audio class name, e.g. Color Distortion')
parser.add_argument('--output',type=Path,required=True,help='Build and diagnostic output directory')
parser.add_argument('--asan',action='store_true',help='Instrument host code with AddressSanitizer')
args=parser.parse_args()
out=args.output.resolve();out.mkdir(parents=True,exist_ok=True)
def run(name,command):
    with (out/(name+'.log')).open('w') as log:
        subprocess.run([str(x) for x in command],cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,check=True)
    print((out/(name+'.log')).read_text()[-2000:])
sanitizer=['-fsanitize=address','-fno-omit-frame-pointer'] if args.asan else []
run('configure',['cmake','-S','Audio/Plugins','-B',out/'native','-DCMAKE_BUILD_TYPE=Release',
    '-DCMAKE_OSX_DEPLOYMENT_TARGET=13.0','-DCMAKE_CXX_FLAGS='+' '.join(sanitizer),
    '-DCMAKE_OBJCXX_FLAGS='+' '.join(sanitizer)])
run('build',['cmake','--build',out/'native','--parallel','4'])
common=['xcrun','clang++','-std=c++17','-O2',*sanitizer,'-IAudio/Plugins/native']
run('compile-mailbox',[*common,'BuildSupport/Workstation/test_vst3_parameters.cpp','-o',out/'mailbox'])
run('mailbox',[out/'mailbox'])
run('compile-probe',[*common,'-DRELEASE=1','-IAudio/Plugins/native/Host','-IAudio/Plugins/vendor/vst3sdk',
    '-IAudio/Plugins/vendor','-IAudio/Plugins/vendor/clap/include','-IAudio/Plugins/vendor/clap',
    'BuildSupport/Workstation/vst3_host_probe.cpp',out/'native/libwormwood_plugins.a',
    '-framework','Cocoa','-framework','AudioToolbox','-framework','AudioUnit','-framework','CoreFoundation',
    '-o',out/'vst3-host-probe'])
run('vst3-host-probe',[out/'vst3-host-probe',args.plugin.resolve(),args.class_name,ROOT])
