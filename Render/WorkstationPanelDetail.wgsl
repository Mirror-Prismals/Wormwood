// Previous lighting is sampled in panel-local coordinates. Depth and coverage
// always come from the newly published capture.
struct PanelLightingHistory { weight:vec4<f32>, groups:array<vec4<f32>,12> };
@group(0) @binding(16) var previousPanelAtlas:texture_2d_array<f32>;
@group(0) @binding(17) var<uniform> previousPanel:PanelLightingHistory;

fn panelLighting(group:i32,p:vec2<f32>,sample:i32,current:vec4<f32>)->vec4<f32>{
 if(group<1||group>4||previousPanel.weight.x<=0.0||current.w>=1e19){return current;}
 let i=(group-1)*3;
 let rect=previousPanel.groups[i];let captured=previousPanel.groups[i+1];
 let now=retainedMeta.rows[4u+u32(group)*4u];
 let bands=vec2<f32>(previousPanel.groups[i+2].z,retainedMeta.rows[5u+u32(group)*4u].w);
 let xy=vec2<i32>(floor(retainedPanelSourceWithBands(p,captured,now,bands)-rect.xy+vec2<f32>(0.5)));
 if(any(xy<vec2<i32>(0))||any(xy>=vec2<i32>(rect.zw))){return current;}
 let old=textureLoad(previousPanelAtlas,xy+vec2<i32>(previousPanel.groups[i+2].xy),sample,0);
 if(old.w>=1e19){return current;}
 return vec4<f32>(mix(current.xyz,old.xyz,previousPanel.weight.x),current.w);
}
