#version 330 core
layout(location=0) in vec4 position;
layout(location=1) in vec4 vertexData;
out vec4 data;out vec2 screen;
flat out float material;
out float viewDistance;
out vec2 soilUV;
void main() {
    screen=position.xy/position.w;viewDistance=position.w;gl_Position=position;
    gl_Position.z=2.0*gl_Position.z-gl_Position.w;
    data=vertexData;
    material=vertexData.w;
    float code=max(vertexData.w-4096.0,0.0);soilUV=vec2(mod(code,2048.0),floor(code/2048.0))/128.0;
    if(vertexData.w<=-4096.){float rockCode=-vertexData.w-4096.;float tag=floor(rockCode/1048576.);rockCode=mod(rockCode,1048576.);soilUV=vec2(mod(rockCode,1024.),floor(rockCode/1024.))/64.;material=-4096.-tag;}
}
