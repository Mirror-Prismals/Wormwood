#!/usr/bin/env python3
"""Offline DAW artwork build. Never invoked by application startup or interaction.

AUTHOR POLICY (2026-11-08): lighting is artwork, not a live simulation. Bake
once, ship these files, and transform/sample each component at runtime. Scene,
text, transport, clip, meter, and panel changes MUST NOT invalidate lighting.
"""
from pathlib import Path
import argparse, json, array, hashlib, subprocess, sys, os
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'BuildSupport'))
from checked_process import run_checked

def replace_body(s,name,body):
    start=s.index('fn '+name+'('); a=s.index('{',start); depth=1;b=a+1
    while depth:
        depth+=(s[b]=='{')-(s[b]=='}');b+=1
    return s[:a+1]+'\n'+body+'\n'+s[b-1:]

def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--scene',type=Path,required=True);ap.add_argument('--stock',type=Path);ap.add_argument('--reuse',action='store_true');ap.add_argument('--only-tape',action='store_true',help='Preserve other artwork and rebuild the tape at close-zoom resolution');ap.add_argument('--only-mute',action='store_true',help='Preserve shipped artwork and bake the colorless mini/wide button variants');ap.add_argument('--only-comp',action='store_true',help='Preserve shipped artwork and bake only the ivory take handle');ap.add_argument('--probe',type=Path,required=True);ap.add_argument('--output',type=Path,default=ROOT/'Art/AudioWorkstation/Baked');args=ap.parse_args()
    out=args.output.resolve();out.mkdir(parents=True,exist_ok=True);work=ROOT/'output/checks/daw-baked-assets-20261108/bake';work.mkdir(parents=True,exist_ok=True)
    rows=array.array('f');rows.frombytes(args.scene.read_bytes());source=(ROOT/'Procedures/Shaders/workstation.wgsl').read_text();entries=[]
    def add(name,root, end,lo,hi,material,kind=1,panel=0,edge=192):
        entries.append(dict(name=name,root=root,end=end,min=lo,max=hi,material=material,kind=kind,panel=panel,edge=edge))
    def read(name):return json.loads((ROOT/'Art/AudioWorkstation'/name).read_text())
    for p in range(4):
        base=32+40*p;c=rows[(base+2)*4:(base+2)*4+3];h=rows[(base+3)*4:(base+3)*4+3]
        add('Panel'+str(p),-p-1,0,[c[i]-h[i] for i in range(3)],[c[i]+h[i] for i in range(3)],base,0,p,512)
    d=read('DecorativeControls/controls.json')
    for a in d['assets']:add(a['name'],a['root'],a['end'],a['min'],a['max'],d['base'])
    for path in ['InputMonitor/runtime.json','CrosswalkArming/runtime.json','EdisonHardware/runtime.json','EdisonHardware/connected.json']:
        d=read(path)
        for m in d.get('material_bases',[d['base']]):add(path+':'+str(m),d['root'],d['end'],d['min'],d['max'],m)
    for a in read('Timeline/runtime.json')['assets']:add(a['name'],a['root'],a['end'],a['min'],a['max'],a['material_base'],edge=1024 if a['name']=='TapeClip_Long' else 192)
    mini=next(a for a in read('Timeline/runtime.json')['assets'] if a['name']=='GemButton_Mini')
    add('MuteIvory',mini['root'],mini['end'],mini['min'],mini['max'],12704)
    wide=next(a for a in read('Timeline/runtime.json')['assets'] if a['name']=='GemButton_Wide')
    add('WideWhiteGem',wide['root'],wide['end'],wide['min'],wide['max'],12704)
    # Three original mixer mesh parts use the same decorative palette.
    d=read('DecorativeControls/controls.json')
    for a in d['assets']:
        for key,v in a.get('motion_components',{}).items():
            if isinstance(v,dict) and all(k in v for k in ['root','end','min','max']):add(a['name']+':'+key,v['root'],v['end'],v['min'],v['max'],d['base'])
    if args.stock:
        for a in json.loads(args.stock.read_text()):add(a['name'],a['root'],a['end'],a['min'],a['max'],a['material'])
    add('BubbleNote',-5,0,[0,-0.009,0],[0.0225,0.009,0.0108],0,2,0,256)
    add('AudioSphere',-6,0,[-1,-1,-1],[1,1,1],0,3,0,256)
    # Same analytic gem cut as the track handle; original ivory casing palette.
    add('CompLaneIvory',-7,0,[-.02,-.09,-.023],[.02,.09,.000000001],12704,0,0,512)
    # Fader housing/cap roots live in the shared dynamic-reserved metadata.
    dynamic=d.get('dynamic_reserved_ranges',{})
    (work/'definitions.json').write_text(json.dumps({'entries':entries,'dynamic':dynamic},indent=2))
    retained=[];prefix_bytes=b'';baked={}
    if args.only_comp or args.only_mute or args.only_tape:
        selected={'TapeClip_Long'} if args.only_tape else ({'MuteIvory','WideWhiteGem'} if args.only_mute else {'CompLaneIvory'})
        old=json.loads((out/'manifest.json').read_text())
        retained=[e for e in old['entries'] if e['name'] not in selected]
        old_pixels=(out/'images.rgba32f').read_bytes()
        prefix_bytes=b''
        for e in retained:
            start=e['offset']*16
            baked[e['name']]=old_pixels[start:start+e['bytes']]
            e['offset']=len(prefix_bytes)//16
            prefix_bytes+=old_pixels[start:start+e['bytes']]
        entries=[e for e in entries if e['name'] in selected]
    image_output=out/'images.rgba32f.pending'
    for i,e in enumerate(entries):
        edge=e['edge'];frame=array.array('f',rows);original=source
        def setrow(i,v):frame[i*4:i*4+4]=array.array('f',v)
        if e['name'] in ('MuteIvory','WideWhiteGem'):
            # HSV saturation zero in scene-linear artwork: retain every ruby
            # facet/highlight and its value, removing only chroma. Do not swap
            # in opaque ivory or re-light the button. Auxiliary coverage/depth
            # and alpha remain bit-identical to the matching ruby image.
            ruby='GemButton_Mini' if e['name']=='MuteIvory' else 'GemButton_Wide'
            pixels=array.array('f');pixels.frombytes(baked[ruby])
            for y in range(edge):
                for x in range(edge):
                    at=(y*edge*2+x)*4
                    value=max(pixels[at:at+3])
                    pixels[at:at+3]=array.array('f',[value]*3)
            raw=pixels.tobytes()
            e['appearance']='Desaturated ruby artwork; original facets, highlights, alpha and depth'
            e['source_art']=ruby
            e['offset']=(len(prefix_bytes)+sum(x.get('bytes',0) for x in entries[:i]))//16;e['bytes']=len(raw)
            with image_output.open('wb' if i==0 else 'ab') as f:
                if i==0:f.write(prefix_bytes)
                f.write(raw)
            continue
        # Disable unrelated first-hit families; light/environment geometry remains
        # authored. Only this offline command can run the optical integrator.
        setrow(0,[1,0,2,0]);setrow(8,[edge*2,edge,16,64]);setrow(9,[3,1,0,0]);setrow(12,[1,e['material'],0,0])
        for idx in [232,234,240,246,247,248,255,43152,44288,44592,45752,4041216]:setrow(idx,[0,0,0,0])
        setrow(212,[0,0,0,0])  # Keep fixed control-icon density enabled in offline artwork.
        lo=e['min'];hi=e['max'];bounds=[lo[0],lo[1],hi[0],hi[1]]
        # Density mip selection uses metres per output pixel. The original
        # fixture camera spans the whole DAW, not this isolated40mmcontrol.
        # Keeping its camera scale erases transport engraving in coarse mips.
        setrow(2,[1,0,0,(hi[0]-lo[0])*0.5])
        setrow(3,[0,1,0,(hi[1]-lo[1])*0.5])

        setrow(192,[0,0,0,1]);setrow(193,[e['root'],e['end'],0,e['material']]);setrow(13,[(lo[j]+hi[j])*.5 for j in range(3)]+[0]);setrow(14,[(hi[j]-lo[j])*.5+1e-5 for j in range(3)]+[0])
        if e['name']=='CompLaneIvory':
            # Panel zero's optical equations address row32. Copy all material
            # descriptors from GemFader_Ivory_Housing_CardinalIvory offline.
            for slot in [0,1,4,5]:
                frame[(32+slot)*4:(33+slot)*4]=rows[(12704+slot)*4:(12705+slot)*4]
            # Same26-plane cut as track handles, baked at casing dimensions
            # so ivory absorption has the same optical path as the fader.
            from analytic_controls import source_gem_planes
            import math
            setrow(34,[0.,0.,-.0115,0.]);setrow(35,[.020001,.090001,.011501,0.])
            for slot,(normal,distance) in enumerate(source_gem_planes(.04,.18)):
                length=math.sqrt(sum(x*x for x in normal))
                setrow(38+slot,[x/length for x in normal]+[distance/length])
        if e['kind']==2:
            d=read('BubbleNotes/BubbleNotes_runtime.json');note=40100;pitch=40101
            setrow(234,[1,0.018,0.0108,0]);setrow(235,[-1,-1,1,1]);setrow(236,[d['root'],d['end'],d['vertex_base'],d['triangle_base']]);setrow(237,[note,pitch,2048,1]);setrow(note,[0,0,0.0225,99])
            for k in range(256):setrow(pitch+k,[0,0,0,0])
            setrow(pitch,[-0.001,-0.0108,0,0]);setrow(pitch+1,[0.023,0.0108,0.0252,1])
            original=replace_body(original,'gemNearest','return gemBubbleHit(origin,ray,farLimit);')
        elif e['kind']==3:
            setrow(240,[1,40100,4,0]);setrow(241,[-2,-2,2,2]);setrow(242,[0,0,0,0]);setrow(243,[1,1,1,0]);setrow(40100,[0,0,0,0]);setrow(40101,[1,1,1,0]);setrow(40102,[1,1,1,0.24]);setrow(40103,[0,0,0,0])
            original=replace_body(original,'gemNearest','return gemAudioSphereHit(origin,ray,farLimit);')
        elif e['kind']==0:
            setrow(0,[1,e['panel']+1,2,0]);setrow(12,[0,e['material'],0,0])
            # Enter only the requested analytic panel; the exit equations and
            # canonical optical integrator remain unchanged.
            body=f'''if(medium>=0){{return gemExitPanel(origin,ray,medium);}}return gemEnterPanel(origin,ray,gemInverseRay(ray),{e['panel']},farLimit);'''
            original=replace_body(original,'gemNearest',body)
        else:
            original=replace_body(original,'gemNearest','return gemControlHit(origin,ray,medium,farLimit,false);')
        original=replace_body(original,'gemAny','return false;')
        # Keep authored geometry/materials but no mutable labels in the art.
        for fn in ['gemEngraving','gemPluginNavigationEngraving','gemTrackIoEngraving','gemTimelineEngraving','gemMixerEngraving','gemBarLoopEngraving']:
            original=replace_body(original,fn,'return vec4<f32>(0.0,0.0,0.0,1.0);')
        # gemEnterPanel uses a NEGATIVE DISTANCE with a valid panel ID on
        # misses. Never treat that ID alone as coverage: it bakes white opaque
        # rectangles into the cut-off facet corners. Misses must be RGBA zero
        # in both color and auxiliary coverage, not a backdrop color key.
        # Left half = original scene-linear light; right half = depth/material/
        # normal/coverage. Both are disk assets, sampled without live tracing.
        b=','.join(f'{x:.10f}' for x in bounds)
        body=f'''let q=(screen+1.0)*0.5;let auxiliary=q.x>=0.5;let uv=vec2<f32>(fract(q.x*2.0),1.0-q.y);let bounds=vec4<f32>({b});
let point=mix(bounds.xy,bounds.zw,uv);let origin=vec3<f32>(point,3.0);let ray=vec3<f32>(0.0,0.0,-1.0);let hit=gemNearest(origin,ray,-1,1e20);
if(hit.panel<0||hit.distance<=0.0||hit.distance>=1e20){{return vec4<f32>(0.0);}}if(auxiliary){{return vec4<f32>(3.0-hit.distance,f32(max(0,(hit.panel-4)%256)),hit.normal.z,1.0);}}
var rgb=vec3<f32>(0.0);for(var sample:i32=0;sample<2;sample++){{let random=fract(vec2<f32>(0.324717957,0.754877666)+vec2<f32>(0.618033989,0.414213562)*f32(sample));rgb+=gemTrace(origin,ray,random);}}return vec4<f32>(rgb*0.5,1.0);'''
        original=replace_body(original,'workstationShade',body)
        shader=work/'asset.wgsl';data=work/'asset.bin';shader.write_text(original);data.write_bytes(frame.tobytes());prefix=work/f'asset-{i:03d}'
        print(f'Baking {i+1}/{len(entries)} {e["name"]}',flush=True)
        signature=hashlib.sha256(json.dumps(e,sort_keys=True).encode()+original.encode()+frame.tobytes()).hexdigest()
        signature_path=Path(str(prefix)+'.signature')
        if not (args.reuse and signature_path.exists() and signature_path.read_text()==signature and Path(str(prefix)+'.rgba32f').exists() and Path(str(prefix)+'.rgba32f').stat().st_size==edge*edge*32):
            run_checked([str(args.probe.resolve()),str(shader),str(data),str(edge*2),str(edge),str(prefix),'1'],cwd=ROOT,log=work/f'asset-{i:03d}.log',timeout=180)
        signature_path.write_text(signature)
        raw=Path(str(prefix)+'.rgba32f').read_bytes();baked[e['name']]=raw;e['offset']=(len(prefix_bytes)+sum(x.get('bytes',0) for x in entries[:i]))//16;e['bytes']=len(raw)
        with image_output.open('wb' if i==0 else 'ab') as f:
            if i==0:f.write(prefix_bytes)
            f.write(raw)
    manifest={'schema':1,'policy':'Immutable offline lighting; no runtime optical recapture. See AGENTS.md.','source_sha256':hashlib.sha256(source.encode()).hexdigest(),'entries':retained+entries}
    image_output.replace(out/'images.rgba32f')
    (out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    from bake_channel_gems import bake
    if not args.only_tape:bake(out)
    print(out,flush=True)
if __name__=='__main__':main()
