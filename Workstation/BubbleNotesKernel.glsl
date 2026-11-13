// Authored indexed membrane geometry; no textures, noise, SDF or implicit skin.
// Included after GemHit, gemSafeUnit, gemInverseRay and gemBox are available.
// Rows234..237 match BubbleNotes.rs. Node stride4, vertex stride6, triangle stride1.
// Native smooth normals differentiate the source membrane parameterization and
// transform through the exact squash/wave Jacobian at the current duration.
// The original Blender normals remain in the packet for source inspection.

bool gemBubbleSelected(int panel) {
    int index=panel-100000;
    if(index<0||index>=int(gemRow(234).x)){return false;}
    return gemRow(int(gemRow(237).x)+index).w>=4.0;
}

// x/y are shape-key weights; z is placement drop plus selected-note lift.
vec3 gemBubbleAnimation(float encodedAge,float height) {
    float age=encodedAge;float lift=0.0;float stop=1.35;
    if(encodedAge>=4.0){age-=4.0;lift=0.15*height;stop+=4.0;}
    if(encodedAge>=stop){return vec3(0.0,0.0,lift);}
    if(age<0.125){
        float progress=max(0.0,1.0-age/0.125);
        return vec3(0.0,0.0,lift+0.5*height*progress*progress);
    }
    float tau=age-0.125;
    return vec3(exp(-6.0*tau)*cos(20.0*tau),exp(-4.9*tau)*sin(25.0*tau),lift);
}

vec3 gemBubbleRest(int vertex,float width,float height) {
    int at=int(gemRow(236).z)+vertex*6;
    vec4 c=gemRow(at);float radius=min(width*0.46,height*0.28);
    return vec3(c.x*width+c.y*radius,c.z*height+c.w*radius,gemRow(at+1).x*height);
}

vec3 gemBubbleVertex(int vertex,float width,float height,vec3 animation) {
    vec3 p=gemBubbleRest(vertex,width,height);
    float u=p.x/width;float v=2.0*p.y/height;
    float sine=sin(3.141592653589793*u);float ends=sine*sine;
    float wave=sin(6.283185307179586*u);float across=1.0-v*v;
    float y=p.y+animation.x*0.1*p.y*across*ends+animation.y*(height/24.0)*wave*across*ends;
    float z=p.z*(1.0-0.32*animation.x+0.125*animation.y*wave*ends)+animation.z;
    return vec3(p.x,y,z);
}

vec3 gemBubbleNormal(int vertex,float width,float height,vec3 animation) {
    int at=int(gemRow(236).z)+vertex*6;
    vec4 c=gemRow(at);vec4 ring=gemRow(at+1);vec2 tangent=gemRow(at+2).xy;
    vec3 p=gemBubbleRest(vertex,width,height);vec3 n=vec3(0.0,0.0,ring.w);
    if(abs(ring.w)<0.5){
        // Divide by height before normalization to keep very small UI notes
        // numerically well conditioned. The boundary tangent's scale cancels.
        vec2 boundary=vec2(p.x-width*0.5,p.y)/(ring.y*height);
        n=vec3(0.3*ring.y*tangent.y,-0.3*ring.y*tangent.x,
               ring.z*(boundary.x*tangent.y-boundary.y*tangent.x));
    }
    float u=p.x/width;float v=2.0*p.y/height;
    float sine=sin(3.141592653589793*u);float ends=sine*sine;
    float wave=sin(6.283185307179586*u);
    float endsX=(3.141592653589793/width)*wave;
    float waveX=(6.283185307179586/width)*cos(6.283185307179586*u);
    float combinedX=waveX*ends+wave*endsX;
    float across=1.0-v*v;
    float yX=animation.x*0.1*p.y*across*endsX+animation.y*(height/24.0)*across*combinedX;
    float yY=1.0+animation.x*0.1*(1.0-3.0*v*v)*ends
              -animation.y*(height/24.0)*wave*ends*(8.0*p.y/(height*height));
    float zX=p.z*0.125*animation.y*combinedX;
    float zZ=1.0-0.32*animation.x+0.125*animation.y*wave*ends;
    // Inverse-transpose of [[1,0,0],[yX,yY,0],[zX,0,zZ]]. Both diagonal
    // factors remain positive throughout the authored damped placement curves.
    return gemSafeUnit(vec3(n.x-yX*n.y/yY-zX*n.z/zZ,n.y/yY,n.z/zZ));
}

bool gemBubbleNodeBox(int node,vec3 origin,vec3 inverseRay,float width,float height,vec3 animation,float farLimit) {
    vec4 low=gemRow(node);vec4 high=gemRow(node+1);vec4 links=gemRow(node+2);
    float radius=min(width*0.46,height*0.28);
    vec3 lo=vec3(low.x*width+low.y*radius,low.z*height+low.w*radius,links.x*height);
    vec3 hi=vec3(high.x*width+high.y*radius,high.z*height+high.w*radius,links.y*height);
    // Independent positive-parameter coefficient extrema bound every duration.
    // Exact local y stays within [-height/2,height/2] before placement; clamp
    // only its bounding interval before applying the complete displacement bound.
    lo.y=max(lo.y,-height*0.5);hi.y=min(hi.y,height*0.5);
    float yChange=0.1*abs(animation.x)*max(abs(lo.y),abs(hi.y))+abs(animation.y)*height/24.0;
    lo.y-=yChange;hi.y+=yChange;
    float factor=1.0-0.32*animation.x;
    lo.z=lo.z*(factor-0.125*abs(animation.y))+animation.z;
    hi.z=hi.z*(factor+0.125*abs(animation.y))+animation.z;
    float guard=max(0.00000001,height*0.00001);
    return gemBox(origin,inverseRay,(lo+hi)*0.5,(hi-lo)*0.5+vec3(guard),farLimit);
}

GemHit gemBubbleTriangle(int triangle,int instance,vec3 local,vec3 ray,float width,float height,vec3 animation,vec2 translation,float farLimit,GemHit previous) {
    vec3 ids=gemRow(triangle).xyz;
    vec3 a=gemBubbleVertex(int(ids.x),width,height,animation);
    vec3 e1=gemBubbleVertex(int(ids.y),width,height,animation)-a;
    vec3 e2=gemBubbleVertex(int(ids.z),width,height,animation)-a;
    vec3 h=cross(ray,e2);float determinant=dot(e1,h);
    vec3 geometricNormal=cross(e1,e2);
    // Use actual area, not |e1|*|e2|: valid very short notes have skinny
    // triangles. A squared norm can also underflow for subpixel geometry.
    vec3 areaComponents=abs(geometricNormal);
    float parallel=0.0000001*max(areaComponents.x,max(areaComponents.y,areaComponents.z));
    // Both sides are required: each enclosing membrane is air/film/air. The
    // optical integrator decides the orientation for its current incident ray.
    if(abs(determinant)<=parallel){return previous;}
    vec3 offset=local-a;float u=dot(offset,h)/determinant;
    if(u<0.0||u>1.0){return previous;}
    vec3 q=cross(offset,e1);float v=dot(ray,q)/determinant;
    if(v<0.0||u+v>1.0){return previous;}
    float distance=dot(e2,q)/determinant;
    float minimumDistance=max(0.000000000000001,min(width,height)*0.000001);
    if(distance<=minimumDistance||distance>=farLimit||distance>=previous.distance){return previous;}
    vec2 point=local.xy+ray.xy*distance+translation;vec4 clip=gemRow(235);
    if(point.x<clip.x||point.y<clip.y||point.x>clip.z||point.y>clip.w){return previous;}
    if(!gemMobileBackdropAllows(point)){return previous;}
    vec3 n=gemBubbleNormal(int(ids.x),width,height,animation)*(1.0-u-v)
           +gemBubbleNormal(int(ids.y),width,height,animation)*u
           +gemBubbleNormal(int(ids.z),width,height,animation)*v;
    n=gemSafeUnit(n);
    if(dot(n,geometricNormal)<0.0){n=-n;}
    return GemHit(distance,n,100000+instance);
}

GemHit gemBubbleHit(vec3 origin,vec3 ray,float farLimit) {
    GemHit hit=GemHit(farLimit,vec3(0.0),-1);
    vec4 scene=gemRow(234);if(scene.x<0.5){return hit;}
    vec4 clip=gemRow(235);vec3 inverseRay=gemInverseRay(ray);
    float height=scene.y;
    vec3 fieldLow=vec3(clip.xy,scene.w);
    vec3 fieldHigh=vec3(clip.zw,scene.w+1.55*height);
    if(!gemBox(origin,inverseRay,(fieldLow+fieldHigh)*0.5,(fieldHigh-fieldLow)*0.5,farLimit)){return hit;}
    vec4 addresses=gemRow(237);vec4 geometry=gemRow(236);
    for(int pitch=0;pitch<128;pitch++){
        int group=int(addresses.y)+pitch*2;vec4 low=gemRow(group);vec4 high=gemRow(group+1);
        if(high.w<0.5){continue;}
        if(addresses.w>1.5){height=(high.y-low.y)/1.2;}
        if(!gemBox(origin,inverseRay,(low.xyz+high.xyz)*0.5,(high.xyz-low.xyz)*0.5+vec3(0.0000001),hit.distance)){continue;}
        for(int candidate=0;candidate<2048;candidate++){
            if(candidate>=int(high.w)){break;}
            int instance=int(low.w)+candidate;vec4 note=gemRow(int(addresses.x)+instance);
            vec3 local=origin-vec3(note.xy,scene.w);vec3 animation=gemBubbleAnimation(note.w,height);
            float lift=0.0;if(note.w>=4.0){lift=0.15*height;}
            vec3 center=vec3(note.z*0.5,0.0,lift+height*0.7);
            vec3 extent=vec3(note.z*0.5,height*0.6,height*0.7);
            if(!gemBox(local,inverseRay,center,extent+vec3(0.0000001),hit.distance)){continue;}
            int node=int(geometry.x);
            for(int visit=0;visit<2048;visit++){
                if(node>=int(geometry.y)){break;}
                vec4 links=gemRow(node+2);
                if(!gemBubbleNodeBox(node,local,inverseRay,note.z,height,animation,hit.distance)){
                    node=int(links.z);continue;
                }
                int count=int(gemRow(node+3).x);
                if(count==0){node+=4;continue;}
                for(int triangle=0;triangle<4;triangle++){
                    if(triangle>=count){break;}
                    hit=gemBubbleTriangle(int(links.w)+triangle,instance,local,ray,note.z,height,animation,note.xy,farLimit,hit);
                }
                node=int(links.z);
            }
        }
    }
    return hit;
}
