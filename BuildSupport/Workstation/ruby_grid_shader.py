"""Diagnostic exact primary visibility using ruby_visibility_grid's ABI.

Only axial exterior closest-hit queries use the grid. Original triangle tests,
instance ordering, activation, clipping, and all other BVH paths are retained.
"""
import ruby_resolve


def transform(source, header_row):
    old = ruby_resolve._function(source, 'gemControlHit')
    if 'rubyGridHeader' in old:
        raise ValueError('grid already present')
    start = old.index('                var at:i32=i32(hi.w)+triangle*6;')
    end = old.index('\n            }\n            node=i32(lo.w);', start)
    narrow = old[start:end]
    narrow = narrow.replace('var at:i32=i32(hi.w)+triangle*6;', '''let reference=i32(cell.x)+candidate;
                var at:i32=i32(gemRow(i32(gridLayout.y)+reference/4)[u32(reference%4)]);''', 1)
    injected = '''
        // Projected candidates preserve the source BVH's triangle order.
        // Invalid/changed geometry uses the original traversal below.
        if(medium<0&&!anyHit&&ray.x==0.0&&ray.y==0.0&&ray.z== -1.0){
            let rubyGridHeader=gemRow(HEADER);
            let mapping=gemRow(i32(rubyGridHeader.z)+instance);
            var materialBase=tree.w;if(materialBase<=0.5){materialBase=gemRow(12).y;}
            if(rubyGridHeader.x==1.0&&rubyGridHeader.y==32.0&&mapping.x>=0.0&&mapping.y==tree.x&&mapping.z==tree.y&&mapping.w==materialBase){
                let descriptor=i32(mapping.x);let domain=gemRow(descriptor+3);
                if(any(local.xy<domain.xy)||any(local.xy>domain.zw)){continue;}
                let coordinates=gemRow(descriptor);let gridLayout=gemRow(descriptor+1);
                let index=clamp(vec2<i32>(floor((local.xy-coordinates.xy)*coordinates.zw)),vec2<i32>(0),vec2<i32>(31));
                let cell=gemRow(i32(gridLayout.x)+index.y*32+index.x);
                for(var candidate:i32=0;candidate<4096;candidate++){
                    if(candidate>=i32(cell.y)){break;}
NARROW
                }
                continue;
            }
        }
'''.replace('HEADER', str(int(header_row))).replace('NARROW', narrow)
    marker = '        var local:vec3<f32>=(origin-pose.xyz)/pose.w;'
    if old.count(marker) != 1:
        raise ValueError('control local marker changed')
    new = old.replace(marker, marker + injected, 1)
    return source.replace(old, new, 1)
