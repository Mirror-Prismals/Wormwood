#!/usr/bin/env python3
"""Check compiled arm64 session frames against the explicit iOS thread budget."""
from pathlib import Path
import json,re,subprocess
ROOT=Path(__file__).resolve().parents[2]
def verify(output):
    library=ROOT/'target/aarch64-apple-ios/release/libwormwood.a'
    names=subprocess.run(['xcrun','nm','-gU',str(library)],capture_output=True,text=True).stdout
    host=[s.split()[-1] for s in names.splitlines() if re.search(r'Host10load_paths17h[0-9a-f]+E$',s)]
    if len(host)!=1:raise RuntimeError('Expected one compiled Host::load_paths symbol')
    targets=host+['_wormwood_ios_run_game','_wormwood_ios_run_material_workshop']
    command=['xcrun','llvm-objdump','--disassemble-symbols='+','.join(targets),str(library)]
    result=subprocess.run(command,capture_output=True,text=True,check=True)
    prologues={};current=None
    for line in result.stdout.splitlines():
        label=re.match(r'^[0-9a-f]+ <([^>]+)>:$',line.strip())
        if label:current=label[1];prologues[current]=[]
        elif current and len(prologues[current])<24:prologues[current].append(line)
    frames={}
    for name in targets:
        lines=prologues.get(name,[])
        if not lines:raise RuntimeError('Missing session entry disassembly: '+name)
        frame=0;probed=False
        for line in lines:
            match=re.search(r'sub\s+x\d+, sp, #(0x[0-9a-f]+|\d+), lsl #12',line)
            if match:frame+=int(match[1],0)*4096;probed=True;continue
            match=re.search(r'sub\s+sp, sp, #(0x[0-9a-f]+|\d+)(.*)',line)
            if match:
                if 'lsl #12' in match[2]:
                    if not probed:frame+=int(match[1],0)*4096
                else:frame+=int(match[1],0)
            match=re.search(r'\[sp, #-(0x[0-9a-f]+|\d+)\]!',line)
            if match:frame+=int(match[1],0)
        frames[name]=frame
    header=(ROOT/'BuildSupport/iOS/SessionLimits.h').read_text()
    budget=int(re.search(r'#define WW_IOS_SESSION_STACK_MIB (\d+)',header)[1])*1024*1024
    source=(ROOT/'Platform/iOS/App.m').read_text()
    if 'sessionThread.stackSize=WW_IOS_SESSION_STACK_MIB*1024u*1024u;' not in source:
        raise RuntimeError('The iOS session thread does not apply its declared stack budget')
    nested=max(frames[name] for name in targets[1:])+frames[host[0]]
    if nested*2>budget:raise RuntimeError(f'Session startup needs {nested} bytes plus call headroom; stack budget is {budget}')
    report={'passed':True,'thread_budget_bytes':budget,'compiled_frame_bytes':frames,'nested_startup_frame_bytes':nested,'headroom_factor':round(budget/nested,2),'device_runtime_verified':False}
    Path(output).write_text(json.dumps(report,indent=2)+'\n')
    return report
if __name__=='__main__':
    import argparse
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True)
    print(json.dumps(verify(parser.parse_args().output),indent=2))
