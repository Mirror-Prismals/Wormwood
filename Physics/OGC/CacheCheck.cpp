// Explicit regression executable, not linked into the game.
// Compare cached and streaming contact after small motion, large motion,
// changing exclusions, topology changes, and cache-memory overflow.
#include "OffsetGeometricContact.h"
#include <algorithm>
#include <array>
#include <cassert>
#include <cmath>
#include <iostream>
#include <iomanip>
using namespace OffsetGeometricContact;
#if defined(WORMWOOD_OGC_CACHE_CHECK)
namespace OffsetGeometricContact {
std::array<std::size_t, 3> CandidateStateForCheck(const Context& context);
std::array<double, 7> TriangleForCheck(glm::vec3 p, glm::vec3 a, glm::vec3 b, glm::vec3 c);
std::array<double, 8> SegmentsForCheck(glm::vec3 a, glm::vec3 b, glm::vec3 c, glm::vec3 d);
glm::vec3 NormalizeForCheck(glm::vec3 v);
}
#endif

static void tetra(SurfaceDescription& s, glm::vec3 center) {
    auto n = static_cast<unsigned>(s.positions.size());
    for (auto p : {glm::vec3(-.04f,0,-.04f), glm::vec3(.04f,0,-.04f),
                  glm::vec3(0,0,.04f), glm::vec3(0,.07f,0)}) s.positions.push_back(center+p);
    for (auto f : {glm::uvec3(0,1,2), glm::uvec3(0,3,1), glm::uvec3(1,3,2), glm::uvec3(2,3,0)})
        s.triangles.push_back(f+glm::uvec3(n));
}
static void prescribedGroundContact(bool cacheCandidates, SurfaceMotion motion) {
    Context context;
    context.settings.cacheCandidates = cacheCandidates;
    context.settings.parallelQueries = false;
    SurfaceDescription tool;
    tool.motion = motion;
    tool.selfCollision = false;
    tetra(tool, {0,.005f,0});
    SurfaceDescription ground;
    ground.motion = SurfaceMotion::Static;
    ground.selfCollision = false;
    ground.positions = {{-1,0,-1},{1,0,-1},{1,0,1},{-1,0,1}};
    ground.triangles = {{0,2,1},{0,3,2}};
    auto blade = RegisterSurface(context, tool);
    auto floor = RegisterSurface(context, ground);
    assert(BuildContacts(context));
    std::vector<VertexResponse> responses, fixed;
    assert(GetVertexResponses(context, blade, responses));
    assert(GetVertexResponses(context, floor, fixed));
    glm::vec3 force(0);
    for (const auto& response : responses) force += response.force;
    for (const auto& response : fixed) assert(glm::length(response.force) == 0.f);
    const auto stats = GetStatistics(context);
    if (motion == SurfaceMotion::Static) {
        assert(stats.vertexFaceContacts == 0 && stats.edgeEdgeContacts == 0);
        assert(glm::length(force) == 0.f);
        return;
    }
    assert(stats.vertexFaceContacts + stats.edgeEdgeContacts > 0);
    assert(force.y > 0.f);
    // A kinematic command must retain the same native non-crossing bound as a
    // dynamic body, even though its coordinates are integrated by its owner.
    assert(responses[0].conservativeBound < tool.positions[0].y);
    const auto filtered = FilterVertexPosition(context, blade, 0,
        tool.positions[0] - glm::vec3(0, .02f, 0));
    assert(filtered.truncated && filtered.position.y > 0.f);
    // A deliberately immobile vertex mask still suppresses fixed/fixed work.
    assert(SetMovableVertices(context, blade, std::vector<std::uint8_t>(tool.positions.size(), 0)));
    assert(BuildContacts(context));
    assert(GetStatistics(context).vertexFaceContacts == 0);
    assert(GetStatistics(context).edgeEdgeContacts == 0);
}
static std::vector<VertexResponse> denseContact(bool cacheCandidates, bool parallel,
                                                std::size_t threshold, bool near) {
    Context context;
    context.settings.cacheCandidates = cacheCandidates;
    context.settings.parallelQueries = parallel;
    context.settings.maximumContactsPerPrimitive = threshold;
    SurfaceDescription tool;
    tool.motion = SurfaceMotion::Kinematic;
    tool.selfCollision = false;
    tetra(tool, {0,.005f,0});
    SurfaceDescription support;
    support.motion = SurfaceMotion::Static;
    support.selfCollision = false;
    // Distinct nearby features deliberately exceed the old 64-contact cutoff.
    // The far variant has only conservative bounds, with no repulsion at all.
    constexpr unsigned planes = 140;
    for (unsigned i=0; i<planes; ++i) {
        const float y = -(near ? 0.f : .04f) - i * .00005f;
        const unsigned n = static_cast<unsigned>(support.positions.size());
        support.positions.insert(support.positions.end(), {{-1,y,-1},{1,y,-1},{0,y,1}});
        support.triangles.push_back({n,n+2,n+1});
    }
    const auto blade = RegisterSurface(context, tool);
    RegisterSurface(context, support);
    std::vector<VertexResponse> response;
    // Repeat to check that per-query stamps cannot suppress the next build.
    for (int pass=0; pass<2; ++pass) {
        assert(BuildContacts(context));
        assert(GetStatistics(context).contactBufferOverflows == 0);
        assert(GetVertexResponses(context, blade, response));
        std::size_t count = 0;
        for (const auto& contact : CopyContacts(context)) {
            if (contact.kind == ContactKind::VertexFace && contact.surfaceA == blade
                && contact.primitiveA == 0) ++count;
        }
        assert(count == (near ? planes : 0));
        assert(response[0].conservativeBound < context.settings.queryRadius * context.settings.conservativeRelaxation);
        assert(near ? response[0].force.y > 0.f : glm::length(response[0].force) == 0.f);
    }
    return response;
}
static void denseContactFallback() {
    auto close=[](float a,float b){return std::abs(a-b)<=1e-4f*std::max({1.f,std::abs(a),std::abs(b)});};
    for (bool near : {false, true}) {
        const auto expected = denseContact(false, false, 4096, near);
        for (bool cached : {false, true}) for (bool parallel : {false, true}) {
            for (std::size_t threshold : {std::size_t(0), std::size_t(1), std::size_t(64)}) {
                const auto actual = denseContact(cached, parallel, threshold, near);
                assert(actual.size() == expected.size());
                for (std::size_t i=0; i<actual.size(); ++i) {
                    assert(close(actual[i].conservativeBound, expected[i].conservativeBound));
                    for (int k=0; k<3; ++k) {
                        assert(close(actual[i].force[k], expected[i].force[k]));
                        for (int j=0; j<3; ++j) assert(close(actual[i].hessian[k][j], expected[i].hessian[k][j]));
                    }
                }
            }
        }
    }
}
#if defined(WORMWOOD_OGC_CACHE_CHECK)
static void geometryScaleChecks() {
    auto close=[](double a,double b){assert(std::abs(a-b)<2e-6);};
    for (float scale : {1.f,.01f,.001f}) {
        auto v=[&](float x,float y,float z){return scale*glm::vec3(x,y,z);};
        auto t=TriangleForCheck(v(.25f,.25f,.3f),v(0,0,0),v(1,0,0),v(0,1,0));
        assert(t[6]==6); close(t[0]/scale,.25); close(t[1]/scale,.25); close(t[2]/scale,0);
        close(t[3],.5); close(t[4],.25); close(t[5],.25);
        // Actual 23 cm wide, 1.2 mm thick cutting-edge bottom triangle.
        t=TriangleForCheck(v(.115f/3,.0002f,-.0002f),v(-.115f,0,-.0006f),v(.115f,0,-.0006f),v(.115f,0,.0006f));
        assert(t[6]==6); close(t[0]/scale,.115/3); close(t[1]/scale,0); close(t[2]/scale,-.0002);
        for(int i=3;i<6;++i) close(t[i],1./3);
        t=TriangleForCheck(v(.5f,.3f,0),v(0,0,0),v(1,0,0),v(2,0,0));
        assert(t[6]!=7); close(t[0]/scale,.5); close(t[1]/scale,0);
        t=TriangleForCheck(v(.5f,.3f,0),v(0,0,0),v(0,0,0),v(0,0,0));
        close(t[0],0); close(t[3],1);
        auto s=SegmentsForCheck(v(-.5f,0,0),v(.5f,0,0),v(0,-.5f,.25f),v(0,.5f,.25f));
        close(s[0]/scale,0); close(s[1]/scale,0); close(s[3]/scale,0); close(s[4]/scale,0); close(s[5]/scale,.25);
        close(s[6],.5); close(s[7],.5);
        s=SegmentsForCheck(v(0,0,0),v(1,0,0),v(.5f,.25f,0),v(.5f,.25f,0));
        close(s[0]/scale,.5); close(s[4]/scale,.25); close(s[6],.5);
        const auto n=NormalizeForCheck(v(0,0,scale*1e-6f));
        close(n.x,0); close(n.y,0); close(n.z,1);
        Context context;
        context.settings.contactRadius=.001f*scale;
        context.settings.queryRadius=.004f*scale;
        SurfaceDescription blade;
        blade.motion=SurfaceMotion::Static; blade.selfCollision=false;
        blade.positions={v(-.115f,0,-.0006f),v(.115f,0,-.0006f),v(.115f,0,.0006f)};
        blade.triangles={{0,1,2}};
        SurfaceDescription probe;
        probe.motion=SurfaceMotion::Kinematic; probe.selfCollision=false;
        probe.positions={v(.115f/3,.0002f,-.0002f),v(.115f/3,.003f,-.0002f),v(.115f/3,.0002f,.003f)};
        probe.triangles={{0,1,2}};
        const auto ground=RegisterSurface(context,blade), point=RegisterSurface(context,probe);
        assert(BuildContacts(context));
        bool interior=false;
        for(const auto& c:CopyContacts(context)) if(c.kind==ContactKind::VertexFace && c.surfaceA==point && c.primitiveA==0 && c.surfaceB==ground) {
            interior=true; close(c.distance/scale,.0002); close(c.normal.y,1);
        }
        assert(interior);
        assert(GetVertexResponse(context,point,0).force.y>0);
    }
    // Crossing 0.2 mm edges must not be classified as points by a squared-
    // length threshold. Exercise the full edge/edge query, not only distance.
    Context context; context.settings.contactRadius=.0002f; context.settings.queryRadius=.0008f;
    SurfaceDescription a,b; a.selfCollision=b.selfCollision=false;
    a.motion=SurfaceMotion::Kinematic; b.motion=SurfaceMotion::Static;
    a.positions={{-.0001f,0,0},{.0001f,0,0},{0,-.0001f,0}};
    b.positions={{0,-.0001f,.00005f},{0,.0001f,.00005f},{.0001f,0,.00005f}};
    a.triangles=b.triangles={{0,1,2}};
    const auto x=RegisterSurface(context,a), y=RegisterSurface(context,b);
    assert(BuildContacts(context)); bool crossing=false;
    for(const auto& c:CopyContacts(context)) if(c.kind==ContactKind::EdgeEdge && c.primitiveA==0 && c.primitiveB==0
        && ((c.surfaceA==x&&c.surfaceB==y)||(c.surfaceA==y&&c.surfaceB==x))) {
        crossing=true; close(c.distance,.00005); close(std::abs(c.normal.z),1);
    }
    assert(crossing);
}
static void repeatedFilterPreservesCandidates() {
    Context cached, streamed;
    cached.settings.parallelQueries = streamed.settings.parallelQueries = false;
    streamed.settings.cacheCandidates = false;
    SurfaceDescription soil;
    tetra(soil, {0,0,0});
    tetra(soil, {.083f,0,0});
    const auto a = RegisterSurface(cached, soil), b = RegisterSurface(streamed, soil);
    const std::vector<std::uint32_t> labels = {0,0,0,0,1,1,1,1};
    auto filter = [&](const std::vector<std::uint32_t>& ids, const std::vector<glm::uvec2>& pairs) {
        assert(SetElementContactFilter(cached, a, ids, pairs));
        assert(SetElementContactFilter(streamed, b, ids, pairs));
    };
    auto compare = [&] {
        assert(BuildContacts(cached)); assert(BuildContacts(streamed));
        const auto x = GetStatistics(cached), y = GetStatistics(streamed);
        assert(x.vertexFaceContacts == y.vertexFaceContacts && x.edgeEdgeContacts == y.edgeEdgeContacts);
        std::vector<VertexResponse> actual, expected;
        assert(GetVertexResponses(cached, a, actual)); assert(GetVertexResponses(streamed, b, expected));
        auto close=[](float a,float b){return std::abs(a-b)<=1e-4f*std::max({1.f,std::abs(a),std::abs(b)});};
        for (std::size_t i=0; i<actual.size(); ++i) {
            assert(close(actual[i].conservativeBound, expected[i].conservativeBound));
            for (int k=0; k<3; ++k) {
                assert(close(actual[i].force[k], expected[i].force[k]));
                for (int j=0; j<3; ++j) assert(close(actual[i].hessian[k][j], expected[i].hessian[k][j]));
            }
        }
        return x.vertexFaceContacts + x.edgeEdgeContacts;
    };
    filter(labels, {{0,1}});
    assert(compare() == 0);
    const auto prepared = CandidateStateForCheck(cached);
    assert(prepared[0] == 1 && prepared[1] == 0 && prepared[2] > 0);
    filter(labels, {{1,0},{0,1},{1,0}});
    assert(CandidateStateForCheck(cached) == prepared);
    filter({}, {{0,1}}); // Omitted labels retain the existing element map.
    assert(CandidateStateForCheck(cached) == prepared);
    assert(compare() == 0);
    filter({}, {});
    assert(CandidateStateForCheck(cached)[0] == 0);
    assert(compare() > 0);
    filter(std::vector<std::uint32_t>(labels.size(), 0), {});
    assert(CandidateStateForCheck(cached)[0] == 0);
    assert(compare() == 0); // A label change is independently significant.
    // The same filter must also retain streaming fallback after cache overflow.
    cached.settings.maximumCandidatePairs = 1;
    filter(labels, {});
    assert(compare() > 0);
    const auto suppressed = CandidateStateForCheck(cached);
    assert(suppressed[0] == 0 && suppressed[1] == 1 && suppressed[2] == 0);
    filter({}, {});
    assert(CandidateStateForCheck(cached) == suppressed);
    assert(compare() > 0);
    assert(CandidateStateForCheck(cached) == suppressed);
    filter({}, {{0,1}});
    assert(CandidateStateForCheck(cached)[1] == 0);
    assert(compare() == 0);
    assert(CandidateStateForCheck(cached)[0] == 1);
}
#endif
int main() {
#if defined(WORMWOOD_OGC_CACHE_CHECK)
    geometryScaleChecks();
    std::cout << "millimetre geometry, uniform scaling, degenerate boundaries and native blade/edge contacts passed\n";
    repeatedFilterPreservesCandidates();
    std::cout << "identical filters preserve cached or suppressed candidates; changed filters invalidate exactly\n";
#endif
    denseContactFallback();
    for (bool cached : {false, true}) {
        prescribedGroundContact(cached, SurfaceMotion::Kinematic);
        prescribedGroundContact(cached, SurfaceMotion::Dynamic);
        prescribedGroundContact(cached, SurfaceMotion::Static);
    }
    for (bool bounded : {false, true}) {
        Context cached, streamed;
        streamed.settings.cacheCandidates = false;
        cached.settings.parallelQueries = streamed.settings.parallelQueries = false;
        if (bounded) cached.settings.maximumCandidatePairs = 1;
        SurfaceDescription soil; tetra(soil, {0,0,0}); tetra(soil, {.083f,0,0});
        SurfaceDescription tool; tool.selfCollision = false; tetra(tool,{0,.073f,0});
        SurfaceDescription ground; ground.motion=SurfaceMotion::Static; ground.selfCollision=false;
        ground.positions={{-1,-.003f,-1},{1,-.003f,-1},{1,-.003f,1},{-1,-.003f,1}};
        ground.triangles={{0,2,1},{0,3,2}};
        auto a=RegisterSurface(cached,soil), b=RegisterSurface(streamed,soil);
        auto ta=RegisterSurface(cached,tool), tb=RegisterSurface(streamed,tool);
        auto ga=RegisterSurface(cached,ground), gb=RegisterSurface(streamed,ground);
        const std::vector<std::uint32_t> ids={0,0,0,0,1,1,1,1};
        assert(SetElementContactFilter(cached,a,ids,{{0,1}}));
        assert(SetElementContactFilter(streamed,b,ids,{{0,1}}));
        std::size_t contacts=0;
        auto previous=tool.positions;
        for (int step=0;step<120;++step) {
            auto now=tool.positions;
            for (auto& p:now) p.x += step<70 ? .0003f*step : .20f-.0004f*(step-70);
            assert(UpdateSurface(cached,ta,now,&previous));
            assert(UpdateSurface(streamed,tb,now,&previous)); previous=now;
            if (step==30) {
                assert(SetElementContactFilter(cached,a,ids,{}));
                assert(SetElementContactFilter(streamed,b,ids,{}));
            }
            if (step==45) {
                const std::vector<std::uint8_t> movable={0,0,0,0,1,1,1,1};
                assert(SetMovableVertices(cached,a,movable)); assert(SetMovableVertices(streamed,b,movable));
            }
            if (step==90) {
                assert(RemoveSurface(cached,ga)); assert(RemoveSurface(streamed,gb));
                ga=RegisterSurface(cached,ground); gb=RegisterSurface(streamed,ground);
            }
            assert(BuildContacts(cached)); assert(BuildContacts(streamed));
            auto ca=GetStatistics(cached), cb=GetStatistics(streamed);
            assert(ca.vertexFaceContacts==cb.vertexFaceContacts && ca.edgeEdgeContacts==cb.edgeEdgeContacts);
            contacts+=ca.vertexFaceContacts+ca.edgeEdgeContacts;
            for (auto pair : {std::pair{a,b},std::pair{ta,tb},std::pair{ga,gb}}) {
                std::vector<VertexResponse> x,y;
                assert(GetVertexResponses(cached,pair.first,x)); assert(GetVertexResponses(streamed,pair.second,y));
                assert(x.size()==y.size());
                auto close=[](float a,float b){return std::abs(a-b)<=1e-4f*std::max({1.f,std::abs(a),std::abs(b)});};
                for(std::size_t i=0;i<x.size();++i) {
                    assert(close(x[i].conservativeBound,y[i].conservativeBound));
                    for(int k=0;k<3;++k) {
                        assert(close(x[i].force[k],y[i].force[k]));
                        for(int j=0;j<3;++j) {
                            if(!close(x[i].hessian[k][j],y[i].hessian[k][j])) {
                                std::cerr<<std::setprecision(10)<<"cache Hessian mismatch bounded="<<bounded<<" step="<<step
                                    <<" cached_surface="<<pair.first<<" streamed_surface="<<pair.second<<" vertex="<<i
                                    <<" col="<<k<<" row="<<j<<" cached="<<x[i].hessian[k][j]
                                    <<" streamed="<<y[i].hessian[k][j]<<" cached_bound="<<x[i].conservativeBound
                                    <<" streamed_bound="<<y[i].conservativeBound<<" vf="<<ca.vertexFaceContacts
                                    <<" ee="<<ca.edgeEdgeContacts<<"\n";
                                for(int axis=0;axis<3;++axis) {
                                    std::cerr<<"axis "<<axis<<" force "<<x[i].force[axis]<<" / "<<y[i].force[axis]<<" Hessian ";
                                    for(int row=0;row<3;++row) std::cerr<<x[i].hessian[axis][row]<<"/"<<y[i].hessian[axis][row]<<" ";
                                    std::cerr<<"\n";
                                }
                            }
                            assert(close(x[i].hessian[k][j],y[i].hessian[k][j]));
                        }
                    }
                }
            }
        }
        assert(contacts>0);
    }
    std::cout << "cached/streamed OGC responses agree; dense exact contact fallback, kinematic ground contact, invalidation and bounded fallback passed\n";
}
