#include "OffsetGeometricContact.h"
#include <cmath>
#include <exception>
#include <string>
#include <unordered_map>

using namespace OffsetGeometricContact;
struct Engine {
    Context context;
    std::string error;
    std::unordered_map<uint64_t, std::vector<glm::vec3>> current;
    std::vector<VertexResponse> responseScratch;
};
extern "C" {
struct WwStats {
    uint64_t vertex_face, edge_edge, truncated, overflow;
    double broad_ms, narrow_ms;
};
void* ww_ogc_create(float radius, float stiffness) {
    try {
        auto* e = new Engine;
        e->context.settings.contactRadius = radius;
        e->context.settings.queryRadius = radius * 4;
        e->context.settings.contactStiffness = stiffness;
        e->context.settings.maximumContactsPerPrimitive = 256;
        if (!Initialize(e->context)) { delete e; return nullptr; }
        return e;
    } catch (...) { return nullptr; }
}
void ww_ogc_destroy(void* ptr) { delete static_cast<Engine*>(ptr); }
void ww_ogc_parallel_queries(void* ptr, int enabled) { static_cast<Engine*>(ptr)->context.settings.parallelQueries = enabled != 0; }
const char* ww_ogc_error(void* ptr) {
    auto* e = static_cast<Engine*>(ptr);
    return e->error.empty() ? e->context.lastError.c_str() : e->error.c_str();
}
uint64_t ww_ogc_surface(void* ptr, const float* positions, size_t count,
                        const uint32_t* faces, size_t face_count, int motion,
                        int self_collision, float friction) {
    auto* e = static_cast<Engine*>(ptr);
    try {
        SurfaceDescription s;
        s.motion = motion == 2 ? SurfaceMotion::Static
                 : motion == 1 ? SurfaceMotion::Kinematic : SurfaceMotion::Dynamic;
        s.selfCollision = self_collision != 0;
        s.friction = friction;
        for (size_t i = 0; i < count; ++i)
            s.positions.emplace_back(positions[3*i], positions[3*i+1], positions[3*i+2]);
        for (size_t i = 0; i < face_count; ++i)
            s.triangles.emplace_back(faces[3*i], faces[3*i+1], faces[3*i+2]);
        auto handle = RegisterSurface(e->context, s);
        if (handle) e->current.emplace(handle, s.positions);
        return handle;
    } catch (const std::exception& x) { e->error = x.what(); return 0; }
      catch (...) { e->error = "OGC surface exception"; return 0; }
}
int ww_ogc_sync(void* ptr, uint64_t handle, const float* positions, const float* previous, size_t count) {
    auto* e=static_cast<Engine*>(ptr);
    try {
        if(e->current.at(handle).size()!=count)return 0;
        std::vector<glm::vec3> p,old;p.reserve(count);old.reserve(count);
        for(size_t i=0;i<count;++i){p.emplace_back(positions[3*i],positions[3*i+1],positions[3*i+2]);old.emplace_back(previous[3*i],previous[3*i+1],previous[3*i+2]);}
        if(!UpdateSurface(e->context,handle,p,&old))return 0;
        e->current[handle]=std::move(p);return 1;
    }catch(...){e->error="OGC accepted rigid synchronization failed";return 0;}
}
int ww_ogc_filter(void* ptr, uint64_t handle, const uint32_t* elements, size_t count,
                  const uint32_t* pairs, size_t pair_count) {
    auto* e = static_cast<Engine*>(ptr);
    try {
        std::vector<uint32_t> labels;
        if (count) labels.assign(elements,elements+count);
        std::vector<glm::uvec2> exclusions;
        for (size_t i=0;i<pair_count;++i) exclusions.emplace_back(pairs[2*i],pairs[2*i+1]);
        return SetElementContactFilter(e->context,handle,labels,exclusions) ? 1 : 0;
    } catch (...) { e->error="OGC cohesive contact filter failed"; return 0; }
}
int ww_ogc_active_vertices(void* ptr, uint64_t handle, const uint8_t* active, size_t count) {
    auto* e = static_cast<Engine*>(ptr);
    try {
        return SetActiveVertices(e->context, handle, std::vector<uint8_t>(active, active + count)) ? 1 : 0;
    } catch (...) { e->error="OGC active vertex mask failed"; return 0; }
}
int ww_ogc_build(void* ptr) {
    auto* e = static_cast<Engine*>(ptr);
    try { return BuildContacts(e->context) ? 1 : 0; }
    catch (const std::exception& x) { e->error = x.what(); return 0; }
    catch (...) { e->error = "OGC build exception"; return 0; }
}
int ww_ogc_movable_vertices(void* ptr, uint64_t handle, const uint8_t* movable, size_t count) {
    auto* e = static_cast<Engine*>(ptr);
    try { return SetMovableVertices(e->context, handle, std::vector<uint8_t>(movable, movable + count)) ? 1 : 0; }
    catch (...) { e->error="OGC movable vertex mask failed"; return 0; }
}
int ww_ogc_project(void* ptr, uint64_t handle, float* proposed, const float* previous,
                   const float* inverse_mass, size_t count, float dt) {
    auto* e = static_cast<Engine*>(ptr);
    try {
        std::vector<glm::vec3> positions, old;
        positions.reserve(count); old.reserve(count);
        for (size_t i = 0; i < count; ++i) {
            auto response = GetVertexResponse(e->context, handle, uint32_t(i));
            glm::vec3 p(proposed[3*i], proposed[3*i+1], proposed[3*i+2]);
            const double inertial = double(dt) * dt * inverse_mass[i];
            const glm::dvec3 anchor(e->current.at(handle).at(i));
            // Condition both the material target and contact force around the
            // accepted position. Conditioning only repulsion collapses clearance.
            if (inverse_mass[i] > 0.0f) {
                const glm::dmat3 matrix = glm::dmat3(1.0)
                                      + inertial * glm::dmat3(response.hessian);
                const double determinant = glm::determinant(matrix);
                if (!std::isfinite(determinant) || determinant <= 0.0) {
                    e->error = "contact Newton matrix is singular or invalid";
                    return 0;
                }
                const glm::dvec3 rhs = glm::dvec3(p) - anchor
                                    + inertial * glm::dvec3(response.force);
                p = glm::vec3(anchor + glm::inverse(matrix) * rhs);
            }
            auto filtered = FilterVertexPosition(e->context, handle, uint32_t(i), p);
            if (!std::isfinite(filtered.position.x) || !std::isfinite(filtered.position.y)
                || !std::isfinite(filtered.position.z)) {
                e->error = "non-finite contact projection";
                return 0;
            }
            positions.push_back(filtered.position);
            old.emplace_back(previous[3*i], previous[3*i+1], previous[3*i+2]);
            for (size_t k = 0; k < 3; ++k) proposed[3*i+k] = filtered.position[k];
        }
        if (!UpdateSurface(e->context, handle, positions, &old)) return 0;
        e->current[handle] = std::move(positions);
        return 1;
    } catch (const std::exception& x) { e->error = x.what(); return 0; }
      catch (...) { e->error = "OGC projection exception"; return 0; }
}
int ww_ogc_response(void* ptr, uint64_t handle, float* output, size_t count) {
    auto* e=static_cast<Engine*>(ptr);
    try {
        if(e->current.at(handle).size()!=count)return 0;
        if(!GetVertexResponses(e->context,handle,e->responseScratch) || e->responseScratch.size()!=count)return 0;
        for(size_t i=0;i<count;++i){const auto& r=e->responseScratch[i];
            for(size_t k=0;k<3;++k)output[13*i+k]=r.force[k];
            for(size_t col=0;col<3;++col)for(size_t row=0;row<3;++row)output[13*i+3+col*3+row]=r.hessian[col][row];
            output[13*i+12]=r.conservativeBound;
        }return 1;
    }catch(...){e->error="OGC rigid response extraction failed";return 0;}
}
int ww_ogc_forces(void* ptr, uint64_t handle, float* forces, size_t count) {
    auto* e = static_cast<Engine*>(ptr);
    try {
        if (e->current.at(handle).size() != count) return 0;
        for (size_t i = 0; i < count; ++i) {
            const auto response = GetVertexResponse(e->context, handle, uint32_t(i));
            for (size_t k = 0; k < 3; ++k) forces[3*i+k] = response.force[k];
        }
        return 1;
    } catch (...) { e->error = "OGC force extraction failed"; return 0; }
}
WwStats ww_ogc_stats(void* ptr) {
    auto s = GetStatistics(static_cast<Engine*>(ptr)->context);
    return {s.vertexFaceContacts, s.edgeEdgeContacts, s.truncatedDisplacements,
            s.contactBufferOverflows, s.broadPhaseMilliseconds, s.narrowPhaseMilliseconds};
}
}
