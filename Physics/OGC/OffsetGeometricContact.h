#pragma once

#include <glm/glm.hpp>

#include <cstddef>
#include <cstdint>
#include <limits>
#include <memory>
#include <string>
#include <vector>

// Cardinal-native implementation of Offset Geometric Contact (OGC), adapted
// from Anka He Chen's Apache-2.0 Gaia implementation (commit c229692).
// This file has been substantially changed to provide a solver-independent,
// dependency-free API using Cardinal's GLM math types.
namespace OffsetGeometricContact {

using SurfaceHandle = std::uint64_t;
constexpr SurfaceHandle InvalidSurfaceHandle = 0;

enum class SurfaceMotion : std::uint8_t {
    Dynamic,
    Kinematic,
    Static
};

enum class ContactKind : std::uint8_t {
    VertexFace,
    EdgeEdge
};

struct Settings {
    bool parallelQueries = true;
    bool cacheCandidates = true;
    std::size_t maximumCandidatePairs = 1'000'000;
    float contactRadius = 0.02f;
    float queryRadius = 0.08f;
    float contactStiffness = 10000.0f;
    float frictionEpsilon = 0.001f;
    float conservativeRelaxation = 0.45f;
    float feasibleRegionEpsilon = 0.001f;
    // Small-list deduplication threshold, not a contact truncation limit. Dense
    // queries switch to exact markers bounded by registered mesh topology.
    std::size_t maximumContactsPerPrimitive = 64;
};

struct SurfaceDescription {
    std::string debugName;
    std::vector<glm::vec3> positions;
    std::vector<glm::vec3> previousPositions;
    std::vector<glm::uvec3> triangles;
    SurfaceMotion motion = SurfaceMotion::Dynamic;
    float thickness = 0.0f;
    float friction = 0.3f;
    std::uint32_t collisionGroup = 1u;
    std::uint32_t collisionMask = std::numeric_limits<std::uint32_t>::max();
    bool selfCollision = true;
};

struct ContactPoint {
    ContactKind kind = ContactKind::VertexFace;
    SurfaceHandle surfaceA = InvalidSurfaceHandle;
    SurfaceHandle surfaceB = InvalidSurfaceHandle;
    std::uint32_t primitiveA = 0;
    std::uint32_t primitiveB = 0;
    glm::vec3 pointA{0.0f};
    glm::vec3 pointB{0.0f};
    glm::vec3 normal{0.0f, 1.0f, 0.0f};
    glm::vec4 weights{0.0f};
    float distance = 0.0f;
};

struct VertexResponse {
    glm::vec3 force{0.0f};
    glm::mat3 hessian{0.0f};
    float conservativeBound = std::numeric_limits<float>::max();
    std::uint32_t contactCount = 0;
    bool active = false;
};

struct FilteredDisplacement {
    glm::vec3 position{0.0f};
    bool truncated = false;
    bool contactsNeedRefresh = false;
};

struct StepStatistics {
    std::size_t surfaceCount = 0;
    std::size_t vertexCount = 0;
    std::size_t triangleCount = 0;
    std::size_t edgeCount = 0;
    std::size_t vertexFaceContacts = 0;
    std::size_t edgeEdgeContacts = 0;
    std::size_t truncatedDisplacements = 0;
    std::size_t contactBufferOverflows = 0;
    double broadPhaseMilliseconds = 0.0;
    double narrowPhaseMilliseconds = 0.0;
};

struct State;

struct Context {
    Settings settings;
    std::shared_ptr<State> state;
    StepStatistics statistics;
    std::string lastError;
    bool initialized = false;
};

bool Initialize(Context& context);
void Reset(Context& context);

SurfaceHandle RegisterSurface(Context& context,
                              const SurfaceDescription& description);
bool RemoveSurface(Context& context, SurfaceHandle handle);
bool UpdateSurface(Context& context,
                   SurfaceHandle handle,
                   const std::vector<glm::vec3>& positions,
                   const std::vector<glm::vec3>* previousPositions = nullptr);

// Cohesive finite elements duplicate interface vertices for fracture. Intact
// neighbouring elements exclude their internal contact faces until rupture.
bool SetElementContactFilter(Context& context, SurfaceHandle handle,
                             const std::vector<std::uint32_t>& elements,
                             const std::vector<glm::uvec2>& excludedPairs);
// Optional query mask for vertices without an exposed collision face.
bool SetActiveVertices(Context& context, SurfaceHandle handle,
                       const std::vector<std::uint8_t>& active);
// Fixed material nodes still support moving matter; fixed/fixed pairs do no work.
bool SetMovableVertices(Context& context, SurfaceHandle handle,
                        const std::vector<std::uint8_t>& movable);
bool BuildContacts(Context& context);
bool GetVertexResponses(const Context& context, SurfaceHandle handle, std::vector<VertexResponse>& output);
VertexResponse GetVertexResponse(const Context& context,
                                 SurfaceHandle handle,
                                 std::uint32_t vertexIndex);
FilteredDisplacement FilterVertexPosition(Context& context,
                                           SurfaceHandle handle,
                                           std::uint32_t vertexIndex,
                                           const glm::vec3& proposedPosition);

std::vector<ContactPoint> CopyContacts(const Context& context);
StepStatistics GetStatistics(const Context& context);
bool ContainsSurface(const Context& context, SurfaceHandle handle);
std::string LastError(const Context& context);

} // namespace OffsetGeometricContact
