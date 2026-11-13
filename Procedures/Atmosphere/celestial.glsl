// Distant, fictional blue-white sky. Direction-space noise has no UV seam or
// camera translation. No cloud/weather data or scene lighting is changed here.
float celestialHash(vec3 p) {
 return fract(sin(dot(p,vec3(127.1,311.7,74.7)))*43758.5453);
}
float celestialNoise(vec3 p) {
 vec3 cell=floor(p);
 vec3 f=fract(p);
 f=f*f*(vec3(3.0)-2.0*f);
 return mix(mix(mix(celestialHash(cell),celestialHash(cell+vec3(1.0,0.0,0.0)),f.x),mix(celestialHash(cell+vec3(0.0,1.0,0.0)),celestialHash(cell+vec3(1.0,1.0,0.0)),f.x),f.y),mix(mix(celestialHash(cell+vec3(0.0,0.0,1.0)),celestialHash(cell+vec3(1.0,0.0,1.0)),f.x),mix(celestialHash(cell+vec3(0.0,1.0,1.0)),celestialHash(cell+vec3(1.0,1.0,1.0)),f.x),f.y),f.z);
}
float celestialFilaments(vec3 p) {
 return celestialNoise(p)*0.55+celestialNoise(p*2.03+vec3(13.1))*0.28+celestialNoise(p*4.11+vec3(7.7))*0.12+celestialNoise(p*8.21)*0.05;
}
vec3 celestialStars(vec3 ray,float scale,float occupancy,float gain) {
 // Centers stay inside their cells, so neighboring cells cannot clip a star.
 vec3 p=ray*scale;
 vec3 cell=floor(p);
 float seed=celestialHash(cell);
 vec3 center=vec3(celestialHash(cell+vec3(17.0)),celestialHash(cell+vec3(43.0)),celestialHash(cell+vec3(91.0)))*0.5+vec3(0.25);
 float distance=length(fract(p)-center);
 float radius=mix(0.055,0.14,pow(seed,12.0));
 // Preserve point energy as a star becomes smaller than a render pixel.
 float footprint=length(fwidth(p))*0.5;
 float widthSquared=radius*radius+footprint*footprint;
 float core=exp(-distance*distance/widthSquared)*radius*radius/widthSquared;
 float halo=exp(-distance*distance/(radius*radius*5.0))*0.07;
 float present=step(1.0-occupancy,celestialHash(cell+vec3(113.0)));
 vec3 tint=mix(vec3(0.55,0.72,1.0),vec3(0.94,0.97,1.0),seed);
 return tint*(core+halo)*present*gain;
}
vec3 celestialSky(vec3 ray,float hour) {
 // One revolution per day; wrapping is continuous at midnight. This is an
 // authored celestial sphere, not an Earth catalog or a foreground galaxy.
 float angle=hour*0.2617993878;
 vec3 pole=normalize(vec3(0.18,0.83,0.53));
 vec3 sky=ray*cos(angle)+cross(pole,ray)*sin(angle)+pole*dot(pole,ray)*(1.0-cos(angle));
 float latitude=dot(sky,normalize(vec3(0.34,0.58,-0.74)));
 float broad=exp(-latitude*latitude*18.0);
 float ribbon=exp(-latitude*latitude*95.0);
 float filaments=celestialFilaments(sky*8.0+vec3(31.0));
 float dust=smoothstep(0.38,0.64,celestialFilaments(sky*15.0+vec3(71.0)));
 float structure=smoothstep(0.24,0.76,filaments);
 vec3 glow=vec3(0.055,0.095,0.18)*broad*structure;
 glow+=vec3(0.11,0.16,0.23)*ribbon*structure*(1.0-dust*0.88);
 float wisp=pow(max(0.0,celestialFilaments(sky*5.0+vec3(103.0))-0.38),2.0);
 glow+=vec3(0.10,0.23,0.38)*wisp*broad;
 vec3 stars=celestialStars(sky,150.0,0.30,0.68);
 stars+=celestialStars(sky+vec3(7.0),310.0,0.38,0.30);
 stars+=celestialStars(sky+vec3(19.0),95.0,0.07,1.65);
 stars+=celestialStars(sky+vec3(29.0),420.0,0.52,0.20)*broad;
 return glow+stars*(1.0-dust*ribbon*0.65);
}
