#version 450
// Ported Cardinal water optics; continuous geometry, no cell slope IDs.
layout(location=0) out vec2 v_uv;
void main(){vec2 positions[3]=vec2[3](vec2(-1.0,-1.0),vec2(3.0,-1.0),vec2(-1.0,3.0));
 vec2 p=positions[gl_VertexIndex];gl_Position=vec4(p,0.0,1.0);v_uv=p*0.5+0.5;}
