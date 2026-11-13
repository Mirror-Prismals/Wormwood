#version 300 es
precision highp float;layout(location=0) in vec4 position;layout(location=1) in vec4 tint;out vec4 data;out vec2 screen;flat out float material;out float viewDistance;
out vec2 soilUV;
void main(){screen=position.xy/position.w;viewDistance=position.w;gl_Position=position;gl_Position.z=2.0*position.z-position.w;data=tint;material=data.w;
    float code=max(data.w-4096.0,0.0);soilUV=vec2(mod(code,2048.0),floor(code/2048.0))/128.0;
    if(data.w<=-4096.){float rockCode=-data.w-4096.;float tag=floor(rockCode/1048576.);rockCode=mod(rockCode,1048576.);soilUV=vec2(mod(rockCode,1024.),floor(rockCode/1024.))/64.;material=-4096.-tag;}}