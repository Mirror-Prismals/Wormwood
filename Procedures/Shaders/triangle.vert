#version 450
struct SceneVertex { vec4 position; vec4 data; };
layout(set=0,binding=1,std430) readonly buffer SceneVertices { SceneVertex vertices[]; } scene;
layout(location=0) out vec4 data;
layout(location=2) out vec2 screen;
layout(location=1) flat out float material;
layout(location=3) out float viewDistance;
layout(location=4) out vec2 soilUV;
void main() {
    SceneVertex vertex=scene.vertices[gl_VertexIndex];
    screen=vertex.position.xy/vertex.position.w;
    viewDistance=vertex.position.w;gl_Position=vertex.position;
    gl_Position.y=-gl_Position.y;
    data=vertex.data;
    material=vertex.data.w;
    float code=max(vertex.data.w-4096.0,0.0);soilUV=vec2(mod(code,2048.0),floor(code/2048.0))/128.0;
    if(vertex.data.w<=-4096.){float rockCode=-vertex.data.w-4096.;float tag=floor(rockCode/1048576.);rockCode=mod(rockCode,1048576.);soilUV=vec2(mod(rockCode,1024.),floor(rockCode/1024.))/64.;material=-4096.-tag;}
}
