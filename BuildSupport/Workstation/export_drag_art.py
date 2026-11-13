#!/usr/bin/env python3
"""Offline display conversion of existing baked artwork for native drag sprites.
No lighting, tracing, or application-time asset generation.
"""
import array, json, math, struct, zlib
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]

def main():
    base = ROOT / 'Art/AudioWorkstation'
    # Same original OCIO tetrahedral lookup and display operations as gemDisplay.
    lut = array.array('f'); lut.frombytes((base / 'GemPanels/GemPanels_AgX.bin').read_bytes())
    pixels = array.array('f'); pixels.frombytes((base / 'Baked/images.rgba32f').read_bytes())
    manifest = json.loads((base / 'Baked/manifest.json').read_text())
    matrix = [[.5593711333100773,.07622070548350629,.06552671177877017], [.3047833466230091,.7879717701028462,.1645467546037241], [.13584556048047705,.13580746617472153,.7699265016503745]]
    def display(rgb):
        p = [min(1.,max(0.,math.log2(max(1.17549435e-38,sum(rgb[j]*matrix[j][i] for j in range(3))))*.0400000028+.498957247))*56 for i in range(3)]
        lo = [int(x) for x in p]; f = [p[i]-lo[i] for i in range(3)]
        order = sorted(range(3),key=lambda i:f[i],reverse=True)
        points = [lo.copy()]; q = lo.copy()
        for i in order:
            q = q.copy(); q[i] = min(56,q[i]+1); points.append(q)
        large,middle,small = [f[i] for i in order]
        weights = [1-large,large-middle,middle-small,small]
        values = [sum(lut[((q[2]*57+q[1])*57+q[0])*4+c]*w for q,w in zip(points,weights)) for c in range(3)]
        linear = [max(0.,v)**2.4 for v in values]
        return [round(255*min(1.,max(0.,v**.416666657*1.05499995-.0549999997 if v>.00303993467 else v*12.9232101))) for v in linear]
    def chunk(name,data):
        return struct.pack('>I',len(data))+name+data+struct.pack('>I',zlib.crc32(name+data))
    names = ['Panel0','CompLaneIvory','GemMeter_Ivory','TapeClip_Long','StockEQ0','EdisonHardware/connected.json:3172864']
    out = base/'Baked/Drag'; out.mkdir(exist_ok=True)
    for e in manifest['entries']:
        if e['name'] not in names: continue
        edge=e['edge']; raw=bytearray()
        for y in range(edge):
            raw.append(0)
            for x in range(edge):
                at=(e['offset']+(edge-1-y)*edge*2+x)*4; rgba=pixels[at:at+4]
                raw.extend(display(rgba[:3])+[255] if rgba[3]>0 else [0,0,0,0])
        name={'EdisonHardware/connected.json:3172864':'Edison'}.get(e['name'],e['name'])
        (out/(name+'.png')).write_bytes(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',edge,edge,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(raw))+chunk(b'IEND',b''))
        print(name)
if __name__=='__main__': main()
