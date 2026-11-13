"""Render the actual export_southern_globe terrain samples; no invented geography.
Run cargo test export_southern_globe -- --ignored, then this script.
Requires Pillow and NumPy. Outputs orthographic hemispheres and a regional inset.
"""
from pathlib import Path
import math
import numpy as np
from PIL import Image, ImageDraw, ImageFont
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'output/southern-world'
source=np.asarray(Image.open(OUT/'sampler-globe.ppm').convert('RGB'))
fontpath='/System/Library/Fonts/Supplemental/Arial.ttf'
def font(n): return ImageFont.truetype(fontpath,n)
BG=(9,17,27); FG=(230,240,246); MUTED=(143,170,189); ACCENT=(113,230,191)
im=Image.new('RGB',(1800,1080),BG);draw=ImageDraw.Draw(im)
draw.text((70,45),'WORMWOOD / PLANETARY SCALE',font=font(37),fill=FG)
draw.text((70,100),'Actual terrain samples • radius 6,371 km • circumference 40,030 km',font=font(22),fill=MUTED)
def globe(cx,cy,r,back=False):
    yy,xx=np.mgrid[-r:r,-r:r]/r
    valid=xx*xx+yy*yy<=1
    depth=np.sqrt(np.maximum(0,1-xx*xx-yy*yy))
    x=xx*(-1 if back else 1); y=depth*(-1 if back else 1); z=yy
    lon=np.arctan2(x,y);lat=np.arcsin(np.clip(z,-1,1))
    u=((lon+math.pi)/(2*math.pi)*source.shape[1]).astype(int)%source.shape[1]
    v=np.clip(((lat+math.pi/2)/math.pi*source.shape[0]).astype(int),0,source.shape[0]-1)
    light=np.clip(depth*.55-xx*.26-yy*.24,0,1)*.6+.4
    rgb=(source[v,u]*light[:,:,None]).astype('uint8')
    # Fine reference graticule (15 degrees), only on the globe.
    grid=((np.abs((lon+math.pi/24)%(math.pi/12)-math.pi/24)<.0018)|
          (np.abs((lat+math.pi/24)%(math.pi/12)-math.pi/24)<.0018)) & valid
    rgb[grid]=(rgb[grid]*.68+np.array([140,180,200])*.32).astype('uint8')
    rgba=np.dstack([rgb,valid.astype('uint8')*255])
    im.paste(Image.fromarray(rgba),(cx-r,cy-r),Image.fromarray(rgba).getchannel('A'))
    draw.ellipse((cx-r-2,cy-r-2,cx+r+2,cy+r+2),outline=(57,104,129),width=2)
    return lambda east,south:(cx+r*math.cos((south-125)/6371000)*math.sin((east+125)/6371000),cy+r*math.sin((south-125)/6371000))
p=globe(460,540,345);globe(1320,540,345,True)
draw.text((225,925),'TOWN-FACING HEMISPHERE',font=font(24),fill=FG)
draw.text((1120,925),'OPPOSITE HEMISPHERE',font=font(24),fill=FG)
draw.text((1020,965),'Current engine chart: Nightworld terrain',font=font(20),fill=MUTED)
def label(east,south,label,x,y,color=ACCENT):
    a,b=p(east,south);draw.ellipse((a-4,b-4,a+4,b+4),outline=color,width=2)
    draw.line([(a,b),(x-12,y+12)],fill=color,width=1)
    draw.text((x,y),label,font=font(21),fill=color)
label(0,0,'Town',75,360)
label(0,106000,'100 km snowy island',72,690)
label(0,257000,'Jungle city · 1 km inland',70,745)
label(0,2200000,'Jungle continent',640,750)
draw.text((70,1008),'The 16 km coast, 40 km sea and 100 km gaps are intentionally tiny at globe scale. See the detail map.',font=font(21),fill=MUTED)
im.save(OUT/'wormwood-globe.png')
# Town-to-jungle section, with exact geographic placement and distance ruler.
im=Image.new('RGB',(1120,1290),BG);draw=ImageDraw.Draw(im)
draw.text((50,35),'TOWN → SOUTHERN JUNGLE',font=font(32),fill=FG)
draw.text((50,83),'Same sampled world • regional close-up • kilometres south of town',font=font(19),fill=MUTED)
mapim=Image.open(OUT/'sampler-detail.ppm').convert('RGB');im.paste(mapim,(80,170))
draw.rectangle((79,169,680,1170),outline=(70,112,137),width=1)
for km in [0,16,56,106,156,206,256,300]:
    y=170+km/300*1000
    draw.line([(65,y),(79,y)],fill=MUTED)
    draw.text((12,y-8),str(km),font=font(17),fill=MUTED)
for km,title,sub in [(0,'Town','Existing northern geography retained'),(16,'Southern coastline','Sea spans east–west'),(36,'Southern sea','40 km north–south'),(106,'Snowy island','100 × 100 km, ocean on both sides'),(206,'Open sea','100 km between the two islands'),(256,'Jungle beach','Continent continues 4,000 km south'),(272,'Jungle city','1 km inland; marker at 257 km')]:
    y=170+km/300*1000
    draw.text((710,y-12),title,font=font(22),fill=FG)
    draw.text((710,y+17),sub,font=font(15),fill=MUTED)
cy=170+257/300*1000;draw.ellipse((375,cy-5,385,cy+5),fill=ACCENT)
draw.text((80,1210),'West ← 180 km visible width → East',font=font(21),fill=MUTED)
draw.text((80,1245),'Colors and shorelines come from the game sampler; labels are cartographic overlays.',font=font(17),fill=MUTED)
im.save(OUT/'wormwood-southern-detail.png')
print(OUT/'wormwood-globe.png')
print(OUT/'wormwood-southern-detail.png')
