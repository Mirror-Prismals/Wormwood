"""Diagnostic halfspace certificate layered over ruby_convex_bake.

The recorded face is an actual authored clipping plane. If that plane alone
forces gemEnterPanel's far interval to be nonpositive, or its parallel-plane
test to reject, evaluating the other 25 planes cannot produce a positive hit.
Every uncertified case still uses the unchanged entry test. No production use.
"""
from ruby_convex_bake import _function, _once


def transform(source: str) -> str:
    entry = _function(source, 'gemEnterPanel')
    entry = _once(entry, 'fn gemEnterPanel(', 'fn rubyConvexEnterPanel(')
    entry = _once(entry, 'var base:i32=gemPanelRow(panel);',
                  'rubyConvexPlane=vec4<f32>(0.0);var base:i32=gemPanelRow(panel);')
    entry = _once(entry, 'near=t;entry=plane.xyz;',
                  'near=t;entry=plane.xyz;rubyConvexPlane=plane;')
    exit = _function(source, 'gemExitPanel')
    exit = _once(exit, 'fn gemExitPanel(', 'fn rubyConvexExitPanel(')
    exit = _once(exit, 'var base:i32=gemPanelRow(panel);',
                 'rubyConvexPlane=vec4<f32>(0.0);var base:i32=gemPanelRow(panel);')
    exit = _once(exit, 'far=t;exitNormal=plane.xyz;',
                 'far=t;exitNormal=plane.xyz;rubyConvexPlane=plane;')
    kernel = _function(source, 'rubyConvexBakeTrace')
    changed = kernel.replace('gemEnterPanel(', 'rubyConvexEnterPanel(').replace(
        'gemExitPanel(', 'rubyConvexExitPanel(')
    source = _once(source, kernel, changed)
    terminal = _function(source, 'rubyConvexTerminal')
    certificate = '''
    let denominator=dot(rubyConvexPlane.xyz,direction);
    let distance=rubyConvexPlane.w-dot(rubyConvexPlane.xyz,origin);
    let parallelReject=abs(denominator)<0.0000001&&distance< -0.0000001;
    let farReject=denominator>=0.0000001&&distance<=0.0;
    if(parallelReject||farReject){
        rubyRecord(origin,direction,weight,depth);
        return vec4<f32>(0.0,0.0,0.0,1.0);
    }
'''
    source = _once(source, terminal, terminal.replace('{', '{'+certificate, 1))
    return source+'\nvar<private> rubyConvexPlane:vec4<f32>;\n'+entry+'\n'+exit+'\n'


def statistics(source: str) -> str:
    """Expose four panel cache sample counters as pixels; not a timing shader."""
    source = _once(source, 'fn workstationShade(', 'fn workstationShadeOriginal(')
    return source+r'''
fn workstationShade(screen:vec2<f32>)->vec4<f32> {
    let x=u32(round((screen.x+1.0)*320.0-0.5));
    let y=u32(round((1.0-screen.y)*240.0-0.5));
    let index=y*640u+x;
    let slot=index/32768u;
    if(slot>=4u){return vec4<f32>(0.0);}
    let desc=rubyPlan.rows[1u+slot*8u];
    let item=index%32768u;
    let address=u32(desc.z)+(item/2u)*60u+(item%2u)*30u+29u;
    return rubyResponse.rows[address];
}
'''
