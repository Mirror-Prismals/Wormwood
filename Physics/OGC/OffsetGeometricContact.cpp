#include "OffsetGeometricContact.h"
#if defined(__APPLE__)
#include <dispatch/dispatch.h>
#endif
#include <exception>

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <mutex>
#include <numeric>
#include <unordered_map>
#include <unordered_set>

namespace OffsetGeometricContact {
namespace {

constexpr float kEpsilon = 1.0e-7f;
constexpr float kAbsoluteRelaxation = 1.0e-6f;
constexpr double kParallelMeasureEpsilon = 1.0e-7;

struct Edge {
    std::uint32_t a = 0;
    std::uint32_t b = 0;
    int oppositeA = -1;
    int oppositeB = -1;
};

struct Aabb {
    glm::vec3 minimum{std::numeric_limits<float>::max()};
    glm::vec3 maximum{-std::numeric_limits<float>::max()};

    void include(const glm::vec3& p) {
        minimum = glm::min(minimum, p);
        maximum = glm::max(maximum, p);
    }

    void include(const Aabb& other) {
        minimum = glm::min(minimum, other.minimum);
        maximum = glm::max(maximum, other.maximum);
    }

    void expand(float radius) {
        minimum -= glm::vec3(radius);
        maximum += glm::vec3(radius);
    }

    bool overlapsSphere(const glm::vec3& center, float radius) const {
        const glm::vec3 closest = glm::clamp(center, minimum, maximum);
        const glm::vec3 delta = center - closest;
        return glm::dot(delta, delta) <= radius * radius;
    }

    bool overlaps(const Aabb& other) const {
        return minimum.x <= other.maximum.x && maximum.x >= other.minimum.x
            && minimum.y <= other.maximum.y && maximum.y >= other.minimum.y
            && minimum.z <= other.maximum.z && maximum.z >= other.minimum.z;
    }
};

struct PrimitiveRef {
    SurfaceHandle surface = InvalidSurfaceHandle;
    std::uint32_t primitive = 0;
    Aabb bounds;
    glm::vec3 center{0.0f};
};

struct BvhNode {
    Aabb bounds;
    // A uniform subtree can be rejected before visiting thousands of material
    // primitives when that surface explicitly disables self collision.
    SurfaceHandle onlySurface = InvalidSurfaceHandle;
    int left = -1;
    int right = -1;
    std::uint32_t first = 0;
    std::uint32_t count = 0;

    bool leaf() const { return count != 0; }
};

struct Bvh {
    std::vector<PrimitiveRef> primitives;
    std::vector<std::uint32_t> order;
    std::vector<BvhNode> nodes;

    int buildNode(std::uint32_t first, std::uint32_t count) {
        const int nodeIndex = static_cast<int>(nodes.size());
        nodes.emplace_back();
        Aabb bounds;
        Aabb centers;
        for (std::uint32_t i = first; i < first + count; ++i) {
            const PrimitiveRef& primitive = primitives[order[i]];
            bounds.include(primitive.bounds);
            centers.include(primitive.center);
        }
        nodes[nodeIndex].bounds = bounds;
        const auto surface = primitives[order[first]].surface;
        nodes[nodeIndex].onlySurface = surface;
        for (std::uint32_t i = first + 1; i < first + count; ++i) {
            if (primitives[order[i]].surface != surface) {
                nodes[nodeIndex].onlySurface = InvalidSurfaceHandle;
                break;
            }
        }
        if (count <= 4) {
            nodes[nodeIndex].first = first;
            nodes[nodeIndex].count = count;
            return nodeIndex;
        }

        const glm::vec3 extent = centers.maximum - centers.minimum;
        int axis = 0;
        if (extent.y > extent.x) axis = 1;
        if (extent.z > extent[axis]) axis = 2;
        std::uint32_t middle = first + count / 2;
        if (nodes[nodeIndex].onlySurface == InvalidSurfaceHandle) {
            // Split whole surfaces before their spatial subtrees. Otherwise
            // one large tray triangle/edge stretches mixed leaf bounds across
            // the entire soil bed and defeats self-surface rejection.
            std::sort(order.begin() + first, order.begin() + first + count,
                [&](std::uint32_t a, std::uint32_t b) { return primitives[a].surface < primitives[b].surface; });
            while (middle < first + count && primitives[order[middle]].surface == primitives[order[middle-1]].surface) ++middle;
            if (middle == first + count) {
                middle = first + count / 2;
                while (middle > first && primitives[order[middle]].surface == primitives[order[middle-1]].surface) --middle;
            }
        } else std::nth_element(order.begin() + first,
                         order.begin() + middle,
                         order.begin() + first + count,
                         [&](std::uint32_t a, std::uint32_t b) {
                             return primitives[a].center[axis]
                                 < primitives[b].center[axis];
                         });
        const int left = buildNode(first, middle - first);
        const int right = buildNode(middle, first + count - middle);
        nodes[nodeIndex].left = left;
        nodes[nodeIndex].right = right;
        return nodeIndex;
    }

    void build() {
        nodes.clear();
        order.resize(primitives.size());
        std::iota(order.begin(), order.end(), 0u);
        if (!primitives.empty()) buildNode(0, static_cast<std::uint32_t>(primitives.size()));
    }

    template <typename Fn>
    void querySphere(const glm::vec3& center, float radius, SurfaceHandle excluded, Fn&& fn,
                     SurfaceHandle included = InvalidSurfaceHandle) const {
        if (nodes.empty()) return;
        std::array<int, 128> stack; // every live entry is assigned before use
        std::size_t stackSize = 1;
        stack[0] = 0;
        while (stackSize != 0) {
            const int index = stack[--stackSize];
            const BvhNode& node = nodes[index];
            if (excluded != InvalidSurfaceHandle && node.onlySurface == excluded) continue;
            if (included != InvalidSurfaceHandle && node.onlySurface != InvalidSurfaceHandle && node.onlySurface != included) continue;
            if (!node.bounds.overlapsSphere(center, radius)) continue;
            if (node.leaf()) {
                for (std::uint32_t i = node.first; i < node.first + node.count; ++i) {
                    const auto& primitive = primitives[order[i]];
                    if (primitive.surface != excluded && (included == InvalidSurfaceHandle || primitive.surface == included) && primitive.bounds.overlapsSphere(center, radius)) fn(primitive);
                }
            } else {
                stack[stackSize++] = node.left;
                stack[stackSize++] = node.right;
            }
        }
    }

    template <typename Fn>
    void queryAabb(const Aabb& bounds, SurfaceHandle excluded, Fn&& fn,
                   SurfaceHandle included = InvalidSurfaceHandle) const {
        if (nodes.empty()) return;
        std::array<int, 128> stack; // every live entry is assigned before use
        std::size_t stackSize = 1;
        stack[0] = 0;
        while (stackSize != 0) {
            const int index = stack[--stackSize];
            const BvhNode& node = nodes[index];
            if (excluded != InvalidSurfaceHandle && node.onlySurface == excluded) continue;
            if (included != InvalidSurfaceHandle && node.onlySurface != InvalidSurfaceHandle && node.onlySurface != included) continue;
            if (!node.bounds.overlaps(bounds)) continue;
            if (node.leaf()) {
                for (std::uint32_t i = node.first; i < node.first + node.count; ++i) {
                    const auto& primitive = primitives[order[i]];
                    if (primitive.surface != excluded && (included == InvalidSurfaceHandle || primitive.surface == included) && primitive.bounds.overlaps(bounds)) fn(primitive);
                }
            } else {
                stack[stackSize++] = node.left;
                stack[stackSize++] = node.right;
            }
        }
    }

    Aabb refitNode(int nodeIndex) {
        BvhNode& node = nodes[nodeIndex];
        Aabb bounds;
        if (node.leaf()) {
            for (std::uint32_t i = node.first; i < node.first + node.count; ++i) {
                bounds.include(primitives[order[i]].bounds);
            }
        } else {
            bounds.include(refitNode(node.left));
            bounds.include(refitNode(node.right));
        }
        node.bounds = bounds;
        return bounds;
    }

    void refit() {
        if (!nodes.empty()) refitNode(0);
    }
};

struct AccumulatedResponse {
    glm::dvec3 force{0.0};
    glm::dmat3 hessian{0.0};
    float conservativeBound = std::numeric_limits<float>::max();
    std::uint32_t contactCount = 0;
    bool active = false;
};

struct Surface {
    SurfaceDescription description;
    std::vector<Edge> edges;
    std::vector<std::vector<std::uint32_t>> vertexFaces;
    std::vector<std::vector<std::uint32_t>> vertexEdges;
    std::vector<std::vector<std::uint32_t>> vertexNeighbors;
    std::vector<VertexResponse> responses;
    std::size_t responseOffset = 0;
    std::size_t featureOffset = 0;
    std::vector<glm::vec3> boundAnchors;
    std::vector<std::vector<std::uint32_t>> nearbyFaces;
    std::vector<std::uint32_t> edgePrimitives;
    std::vector<std::uint32_t> facePrimitives;
    bool geometryDirty = true;
    std::vector<std::uint32_t> elements;
    std::vector<std::uint8_t> activeVertices;
    std::vector<std::uint8_t> movableVertices;
    std::unordered_set<std::uint64_t> excludedElements;
};

enum class ClosestRegion : std::uint8_t {
    A, B, C, AB, BC, AC, Interior, Invalid
};

struct ClosestTriangleResult {
    glm::vec3 point{0.0f};
    glm::vec3 barycentrics{0.0f};
    ClosestRegion region = ClosestRegion::Invalid;
};

struct PrimitiveKey {
    SurfaceHandle surface = InvalidSurfaceHandle;
    std::uint32_t type = 0;
    std::uint32_t primitive = 0;

    bool operator==(const PrimitiveKey& other) const {
        return surface == other.surface && type == other.type
            && primitive == other.primitive;
    }
};

struct PrimitiveKeyHash {
    std::size_t operator()(const PrimitiveKey& key) const noexcept {
        std::size_t h = std::hash<std::uint64_t>{}(key.surface);
        h ^= std::hash<std::uint32_t>{}(key.type + 0x9e3779b9u + static_cast<std::uint32_t>(h));
        h ^= std::hash<std::uint32_t>{}(key.primitive + 0x85ebca6bu + static_cast<std::uint32_t>(h));
        return h;
    }
};

std::uint64_t edgeKey(std::uint32_t a, std::uint32_t b) {
    if (a > b) std::swap(a, b);
    return (static_cast<std::uint64_t>(a) << 32u) | b;
}

bool finiteVec(const glm::vec3& value) {
    return std::isfinite(value.x) && std::isfinite(value.y)
        && std::isfinite(value.z);
}

bool elementContactEnabled(const Surface& surface, std::uint32_t a, std::uint32_t b) {
    if (surface.elements.empty()) return true;
    const auto x = surface.elements.at(a), y = surface.elements.at(b);
    if (x == y) return false;
    const auto key = (std::uint64_t(std::min(x,y)) << 32u) | std::max(x,y);
    return surface.excludedElements.count(key) == 0;
}
bool collisionEnabled(const Surface& a, const Surface& b) {
    return (a.description.collisionMask & b.description.collisionGroup) != 0u
        && (b.description.collisionMask & a.description.collisionGroup) != 0u;
}

bool canMove(const Surface& surface) {
    // Prescribed kinematic motion still needs conservative contact bounds and
    // reaction forces against static geometry. Only static/static pairs can be
    // omitted; whether a solver integrates a body does not determine contact.
    return surface.description.motion != SurfaceMotion::Static;
}
bool movable(const Surface& surface, std::uint32_t vertex) {
    return canMove(surface) && (surface.movableVertices.empty() || surface.movableVertices[vertex]);
}
bool movingFacePair(const Surface& source, std::uint32_t vertex, const Surface& target, glm::uvec3 face) {
    return movable(source, vertex) || movable(target, face.x) || movable(target, face.y) || movable(target, face.z);
}
bool movingEdgePair(const Surface& first, const Edge& a, const Surface& second, const Edge& b) {
    return movable(first, a.a) || movable(first, a.b) || movable(second, b.a) || movable(second, b.b);
}

float safeLength(const glm::vec3& value) {
    return std::sqrt(std::max(0.0f, glm::dot(value, value)));
}

glm::vec3 safeNormalize(const glm::dvec3& value, const glm::vec3& fallback) {
    const double lengthSquared = glm::dot(value, value);
    return lengthSquared > 0.0 ? glm::vec3(value / std::sqrt(lengthSquared)) : fallback;
}
glm::vec3 safeNormalize(const glm::vec3& value, const glm::vec3& fallback) {
    return safeNormalize(glm::dvec3(value), fallback);
}

ClosestTriangleResult closestPointTriangle(const glm::vec3& p,
                                           const glm::vec3& a,
                                           const glm::vec3& b,
                                           const glm::vec3& c) {
    ClosestTriangleResult result;
    // Float positions are promoted before subtraction/products. Area products
    // have units length^4: a fixed metre-scale epsilon cannot classify them.
    const glm::dvec3 da(a), db(b), dc(c), dp(p);
    const glm::dvec3 ab = db - da;
    const glm::dvec3 ac = dc - da;
    const glm::dvec3 ap = dp - da;
    const glm::dvec3 normal = glm::cross(ab, ac);
    const double areaSquared = glm::dot(normal, normal);
    if (areaSquared == 0.0) {
        // A collapsed triangle still has its segment/point boundary. Returning
        // it keeps conservative distance bounds meaningful without inventing
        // an interior or discarding a small but nondegenerate triangle.
        const std::array<glm::dvec3, 3> points = {da, db, dc};
        const std::array<ClosestRegion, 3> vertices = {ClosestRegion::A, ClosestRegion::B, ClosestRegion::C};
        const std::array<ClosestRegion, 3> edges = {ClosestRegion::AB, ClosestRegion::BC, ClosestRegion::AC};
        double best = std::numeric_limits<double>::max();
        for (int i=0; i<3; ++i) {
            const int j = (i+1)%3;
            const auto direction = points[j] - points[i];
            const double lengthSquared = glm::dot(direction, direction);
            const double t = lengthSquared > 0.0
                ? glm::clamp(glm::dot(dp-points[i], direction)/lengthSquared, 0.0, 1.0) : 0.0;
            const auto closest = points[i] + t*direction;
            const double distanceSquared = glm::dot(dp-closest, dp-closest);
            if (distanceSquared < best) {
                best = distanceSquared;
                result.point = glm::vec3(closest);
                result.barycentrics = glm::vec3(0);
                result.barycentrics[i] = float(1.0-t);
                result.barycentrics[j] = float(t);
                result.region = t == 0.0 ? vertices[i] : t == 1.0 ? vertices[j] : edges[i];
            }
        }
        return result;
    }
    const double d1 = glm::dot(ab, ap);
    const double d2 = glm::dot(ac, ap);
    if (d1 <= 0.0f && d2 <= 0.0f) {
        result.point = a; result.barycentrics = {1, 0, 0}; result.region = ClosestRegion::A;
        return result;
    }
    const glm::dvec3 bp = dp - db;
    const double d3 = glm::dot(ab, bp);
    const double d4 = glm::dot(ac, bp);
    if (d3 >= 0.0f && d4 <= d3) {
        result.point = b; result.barycentrics = {0, 1, 0}; result.region = ClosestRegion::B;
        return result;
    }
    const glm::dvec3 cp = dp - dc;
    const double d5 = glm::dot(ab, cp);
    const double d6 = glm::dot(ac, cp);
    if (d6 >= 0.0f && d5 <= d6) {
        result.point = c; result.barycentrics = {0, 0, 1}; result.region = ClosestRegion::C;
        return result;
    }
    const double vc = d1 * d4 - d3 * d2;
    if (vc <= 0.0f && d1 >= 0.0f && d3 <= 0.0f) {
        const double v = d1 / (d1 - d3);
        result.point = glm::vec3(da + v * ab); result.barycentrics = {float(1.0 - v), float(v), 0}; result.region = ClosestRegion::AB;
        return result;
    }
    const double vb = d5 * d2 - d1 * d6;
    if (vb <= 0.0f && d2 >= 0.0f && d6 <= 0.0f) {
        const double v = d2 / (d2 - d6);
        result.point = glm::vec3(da + v * ac); result.barycentrics = {float(1.0 - v), 0, float(v)}; result.region = ClosestRegion::AC;
        return result;
    }
    const double va = d3 * d6 - d5 * d4;
    if (va <= 0.0f && d4 - d3 >= 0.0f && d5 - d6 >= 0.0f) {
        const double v = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        result.point = glm::vec3(db + v * (dc - db)); result.barycentrics = {0, float(1.0 - v), float(v)}; result.region = ClosestRegion::BC;
        return result;
    }
    const double v = glm::dot(glm::cross(ap, ac), normal) / areaSquared;
    const double w = glm::dot(glm::cross(ab, ap), normal) / areaSquared;
    result.point = glm::vec3(da + v * ab + w * ac);
    result.barycentrics = {float(1.0 - v - w), float(v), float(w)};
    result.region = ClosestRegion::Interior;
    return result;
}

void closestPointsSegments(const glm::vec3& p0,
                           const glm::vec3& p1,
                           const glm::vec3& q0,
                           const glm::vec3& q1,
                           glm::vec3& pointP,
                           glm::vec3& pointQ,
                           float& s,
                           float& t) {
    const glm::dvec3 d1 = glm::dvec3(p1) - glm::dvec3(p0);
    const glm::dvec3 d2 = glm::dvec3(q1) - glm::dvec3(q0);
    const glm::dvec3 r = glm::dvec3(p0) - glm::dvec3(q0);
    const double a = glm::dot(d1, d1);
    const double e = glm::dot(d2, d2);
    const double f = glm::dot(d2, r);
    double ds, dt;
    if (a == 0.0 && e == 0.0) {
        ds = dt = 0.0;
    } else if (a == 0.0) {
        ds = 0.0;
        dt = glm::clamp(f / e, 0.0, 1.0);
    } else {
        const double c = glm::dot(d1, r);
        if (e == 0.0) {
            dt = 0.0;
            ds = glm::clamp(-c / a, 0.0, 1.0);
        } else {
            const double b = glm::dot(d1, d2);
            const auto normal = glm::cross(d1, d2);
            const double denominator = glm::dot(normal, normal);
            ds = denominator > 0.0
                ? glm::clamp(glm::dot(glm::cross(-r, d2), normal) / denominator, 0.0, 1.0)
                : 0.0;
            dt = (b * ds + f) / e;
            if (dt < 0.0) {
                dt = 0.0;
                ds = glm::clamp(-c / a, 0.0, 1.0);
            } else if (dt > 1.0) {
                dt = 1.0;
                ds = glm::clamp((b - c) / a, 0.0, 1.0);
            }
        }
    }
    s = float(ds); t = float(dt);
    pointP = glm::vec3(glm::dvec3(p0) + d1 * ds);
    pointQ = glm::vec3(glm::dvec3(q0) + d2 * dt);
}

bool validateDescription(const SurfaceDescription& description, std::string& error) {
    if (description.positions.empty()) {
        error = "OGC surface has no vertices.";
        return false;
    }
    if (description.triangles.empty()) {
        error = "OGC surface has no triangles.";
        return false;
    }
    for (const glm::vec3& position : description.positions) {
        if (!finiteVec(position)) {
            error = "OGC surface contains a non-finite vertex.";
            return false;
        }
    }
    for (const glm::uvec3& triangle : description.triangles) {
        if (triangle.x >= description.positions.size()
            || triangle.y >= description.positions.size()
            || triangle.z >= description.positions.size()) {
            error = "OGC triangle index is outside the vertex buffer.";
            return false;
        }
        if (triangle.x == triangle.y || triangle.y == triangle.z
            || triangle.z == triangle.x) {
            error = "OGC surface contains a triangle with repeated vertices.";
            return false;
        }
    }
    if (!description.previousPositions.empty()
        && description.previousPositions.size() != description.positions.size()) {
        error = "OGC previous-position buffer has the wrong size.";
        return false;
    }
    if (description.thickness < 0.0f || description.friction < 0.0f) {
        error = "OGC thickness and friction must be non-negative.";
        return false;
    }
    return true;
}

void buildTopology(Surface& surface) {
    const std::size_t vertexCount = surface.description.positions.size();
    surface.vertexFaces.assign(vertexCount, {});
    surface.vertexEdges.assign(vertexCount, {});
    surface.vertexNeighbors.assign(vertexCount, {});
    surface.edges.clear();
    std::unordered_map<std::uint64_t, std::uint32_t> edgeLookup;

    for (std::uint32_t faceIndex = 0;
         faceIndex < surface.description.triangles.size(); ++faceIndex) {
        const glm::uvec3 face = surface.description.triangles[faceIndex];
        const std::uint32_t ids[3] = {face.x, face.y, face.z};
        for (std::uint32_t id : ids) surface.vertexFaces[id].push_back(faceIndex);
        const std::uint32_t pairs[3][3] = {
            {ids[0], ids[1], ids[2]},
            {ids[1], ids[2], ids[0]},
            {ids[0], ids[2], ids[1]}
        };
        for (const auto& pair : pairs) {
            const std::uint64_t key = edgeKey(pair[0], pair[1]);
            auto found = edgeLookup.find(key);
            if (found == edgeLookup.end()) {
                Edge edge;
                edge.a = std::min(pair[0], pair[1]);
                edge.b = std::max(pair[0], pair[1]);
                edge.oppositeA = static_cast<int>(pair[2]);
                const std::uint32_t index = static_cast<std::uint32_t>(surface.edges.size());
                surface.edges.push_back(edge);
                edgeLookup.emplace(key, index);
            } else {
                Edge& edge = surface.edges[found->second];
                if (edge.oppositeB < 0) edge.oppositeB = static_cast<int>(pair[2]);
            }
        }
    }

    for (std::uint32_t edgeIndex = 0; edgeIndex < surface.edges.size(); ++edgeIndex) {
        const Edge& edge = surface.edges[edgeIndex];
        surface.vertexEdges[edge.a].push_back(edgeIndex);
        surface.vertexEdges[edge.b].push_back(edgeIndex);
        surface.vertexNeighbors[edge.a].push_back(edge.b);
        surface.vertexNeighbors[edge.b].push_back(edge.a);
    }
    surface.responses.assign(vertexCount, {});
    surface.boundAnchors = surface.description.positions;
}

int edgeForFaceRegion(const Surface& surface,
                      std::uint32_t faceIndex,
                      ClosestRegion region) {
    const glm::uvec3 face = surface.description.triangles[faceIndex];
    std::uint32_t a = 0;
    std::uint32_t b = 0;
    if (region == ClosestRegion::AB) { a = face.x; b = face.y; }
    else if (region == ClosestRegion::BC) { a = face.y; b = face.z; }
    else if (region == ClosestRegion::AC) { a = face.x; b = face.z; }
    else return -1;
    for (std::uint32_t edgeIndex : surface.vertexEdges[a]) {
        const Edge& edge = surface.edges[edgeIndex];
        if ((edge.a == a && edge.b == b) || (edge.a == b && edge.b == a)) {
            return static_cast<int>(edgeIndex);
        }
    }
    return -1;
}

bool checkVertexFeasible(const glm::vec3& point,
                         const Surface& surface,
                         std::uint32_t vertex,
                         float epsilon) {
    const glm::dvec3 origin(surface.description.positions[vertex]);
    const glm::dvec3 delta = glm::dvec3(point) - origin;
    const double relaxed = -double(epsilon) - kAbsoluteRelaxation;
    for (std::uint32_t neighbor : surface.vertexNeighbors[vertex]) {
        const glm::dvec3 towardOrigin = origin - glm::dvec3(surface.description.positions[neighbor]);
        if (glm::dot(delta, towardOrigin) < relaxed) return false;
    }
    return true;
}

bool checkEdgeFeasible(const glm::vec3& point,
                       const Surface& surface,
                       std::uint32_t edgeIndex,
                       float epsilon) {
    const Edge& edge = surface.edges[edgeIndex];
    const glm::dvec3 a(surface.description.positions[edge.a]);
    const glm::dvec3 b(surface.description.positions[edge.b]);
    const glm::dvec3 p(point), ab = b - a;
    const double lengthSquared = glm::dot(ab, ab);
    if (lengthSquared == 0.0) return false;
    const double relaxed = double(epsilon) + kAbsoluteRelaxation;
    if (glm::dot(p - a, ab) < -relaxed) return false;
    if (glm::dot(p - b, ab) > relaxed) return false;
    const int opposites[2] = {edge.oppositeA, edge.oppositeB};
    for (int opposite : opposites) {
        if (opposite < 0) continue;
        const glm::dvec3 other(surface.description.positions[static_cast<std::size_t>(opposite)]);
        const double t = glm::dot(ab, other - a) / lengthSquared;
        const glm::dvec3 foot = a + t * ab;
        if (glm::dot(other - foot, p - foot) > relaxed) return false;
    }
    return true;
}

bool checkFeasible(const glm::vec3& point,
                   const Surface& surface,
                   std::uint32_t faceIndex,
                   ClosestRegion region,
                   float epsilon) {
    const glm::uvec3 face = surface.description.triangles[faceIndex];
    switch (region) {
        case ClosestRegion::Interior: return true;
        case ClosestRegion::A: return checkVertexFeasible(point, surface, face.x, epsilon);
        case ClosestRegion::B: return checkVertexFeasible(point, surface, face.y, epsilon);
        case ClosestRegion::C: return checkVertexFeasible(point, surface, face.z, epsilon);
        case ClosestRegion::AB:
        case ClosestRegion::BC:
        case ClosestRegion::AC: {
            const int edge = edgeForFaceRegion(surface, faceIndex, region);
            return edge >= 0 && checkEdgeFeasible(point, surface, static_cast<std::uint32_t>(edge), epsilon);
        }
        default: return false;
    }
}

PrimitiveKey primitiveKey(const Surface& surface,
                          SurfaceHandle handle,
                          std::uint32_t faceIndex,
                          ClosestRegion region) {
    const glm::uvec3 face = surface.description.triangles[faceIndex];
    switch (region) {
        case ClosestRegion::A: return {handle, 0, face.x};
        case ClosestRegion::B: return {handle, 0, face.y};
        case ClosestRegion::C: return {handle, 0, face.z};
        case ClosestRegion::AB:
        case ClosestRegion::BC:
        case ClosestRegion::AC: return {handle, 1, static_cast<std::uint32_t>(std::max(0, edgeForFaceRegion(surface, faceIndex, region)))};
        default: return {handle, 2, faceIndex};
    }
}

bool faceContainsVertex(const glm::uvec3& face, std::uint32_t vertex) {
    return face.x == vertex || face.y == vertex || face.z == vertex;
}

glm::vec3 previousAt(const Surface& surface, std::uint32_t index) {
    if (surface.description.previousPositions.size() == surface.description.positions.size()) {
        return surface.description.previousPositions[index];
    }
    return surface.description.positions[index];
}

void contactDerivatives(const Settings& settings,
                        float distance,
                        float thickness,
                        float& lambda,
                        float& stiffness) {
    const float adjusted = std::max(distance - thickness, kEpsilon);
    const float radius = settings.contactRadius;
    if (adjusted >= radius) {
        lambda = stiffness = 0.0f;
        return;
    }
    const float tau = radius * 0.5f;
    if (adjusted < tau) {
        const float k2 = 0.5f * tau * tau * settings.contactStiffness;
        lambda = k2 / adjusted;
        stiffness = k2 / (adjusted * adjusted);
    } else {
        lambda = settings.contactStiffness * (radius - adjusted);
        stiffness = settings.contactStiffness;
    }
}

void addResponse(const Surface& surface,
                 AccumulatedResponse* responses,
                 std::uint32_t vertex,
                 float coefficient,
                 const glm::vec3& normal,
                 float lambda,
                 float stiffness,
                 const glm::vec3& tangentialForce,
                 float tangentialStiffness) {
    // Kinematic bodies need the equal/opposite reaction for force feedback too;
    // their prescribed coordinates are still never integrated by this module.
    if (surface.description.motion == SurfaceMotion::Static || vertex >= surface.responses.size()) return;
    AccumulatedResponse& response = responses[vertex];
    const double weight = coefficient;
    const glm::dvec3 direction(normal);
    const glm::dmat3 projection = glm::outerProduct(direction, direction);
    response.force += weight * (double(lambda) * direction + glm::dvec3(tangentialForce));
    response.hessian += weight * weight
        * (double(stiffness) * projection
           + double(tangentialStiffness) * (glm::dmat3(1.0) - projection));
    response.contactCount += 1;
    response.active = true;
}

void updateMinimum(float& destination, float distance) {
    if (std::isfinite(distance)) destination = std::min(destination, distance);
}

} // namespace

struct State {
    struct PairCache {
        SurfaceHandle a, b;
        std::vector<glm::vec3> anchorA, anchorB;
        std::vector<std::pair<std::uint32_t, std::uint32_t>> facesA, facesB, edges;
        bool dirty = true;
    };
    std::vector<PairCache> pairCaches;
    struct Partial {
        std::vector<AccumulatedResponse> responses;
        std::vector<ContactPoint> contacts;
        std::vector<std::size_t> featureStamps;
        StepStatistics statistics;
        std::exception_ptr error;
    };
    std::vector<Partial> partials;
    bool candidatesValid = false;
    bool candidatesSuppressed = false;
    float candidateRadius = 0.0f;
    std::vector<std::vector<std::uint32_t>> nearbyEdges;
    mutable std::mutex mutex;
    std::unordered_map<SurfaceHandle, Surface> surfaces;
    std::vector<ContactPoint> contacts;
    Bvh faceBvh;
    Bvh edgeBvh;
    SurfaceHandle nextHandle = 1;
    std::size_t truncatedDisplacements = 0;
    bool topologyDirty = true;
};

bool Initialize(Context& context) {
    if (!context.state) context.state = std::make_shared<State>();
    context.initialized = true;
    context.lastError.clear();
    return true;
}

void Reset(Context& context) {
    context.state = std::make_shared<State>();
    context.statistics = {};
    context.lastError.clear();
    context.initialized = true;
}

SurfaceHandle RegisterSurface(Context& context,
                              const SurfaceDescription& description) {
    if (!context.initialized) Initialize(context);
    std::string error;
    if (!validateDescription(description, error)) {
        context.lastError = error;
        return InvalidSurfaceHandle;
    }
    Surface surface;
    surface.description = description;
    if (surface.description.previousPositions.empty()) {
        surface.description.previousPositions = surface.description.positions;
    }
    buildTopology(surface);
    std::lock_guard<std::mutex> lock(context.state->mutex);
    const SurfaceHandle handle = context.state->nextHandle++;
    context.state->surfaces.emplace(handle, std::move(surface));
    context.state->topologyDirty = true;
    context.lastError.clear();
    return handle;
}

bool RemoveSurface(Context& context, SurfaceHandle handle) {
    if (!context.state) return false;
    std::lock_guard<std::mutex> lock(context.state->mutex);
    const bool removed = context.state->surfaces.erase(handle) != 0;
    if (removed) context.state->topologyDirty = true;
    if (!removed) context.lastError = "OGC surface handle was not found.";
    return removed;
}

bool UpdateSurface(Context& context,
                   SurfaceHandle handle,
                   const std::vector<glm::vec3>& positions,
                   const std::vector<glm::vec3>* previousPositions) {
    if (!context.state) return false;
    std::lock_guard<std::mutex> lock(context.state->mutex);
    auto found = context.state->surfaces.find(handle);
    if (found == context.state->surfaces.end()) {
        context.lastError = "OGC surface handle was not found.";
        return false;
    }
    Surface& surface = found->second;
    if (positions.size() != surface.description.positions.size()) {
        context.lastError = "OGC update cannot change vertex count; register a new surface for topology changes.";
        return false;
    }
    if (previousPositions && previousPositions->size() != positions.size()) {
        context.lastError = "OGC previous-position update has the wrong size.";
        return false;
    }
    for (const glm::vec3& position : positions) {
        if (!finiteVec(position)) {
            context.lastError = "OGC update contains a non-finite vertex.";
            return false;
        }
    }
    surface.description.previousPositions = previousPositions
        ? *previousPositions : surface.description.positions;
    surface.geometryDirty |= positions != surface.description.positions;
    surface.description.positions = positions;
    context.lastError.clear();
    return true;
}

bool SetElementContactFilter(Context& context, SurfaceHandle handle,
                             const std::vector<std::uint32_t>& elements,
                             const std::vector<glm::uvec2>& excludedPairs) {
    if (!context.state) return false;
    std::lock_guard<std::mutex> lock(context.state->mutex);
    auto found = context.state->surfaces.find(handle);
    if (found == context.state->surfaces.end()) return false;
    auto& surface = found->second;
    if (!elements.empty() && elements.size() != surface.description.positions.size()) return false;
    if (elements.empty() && surface.elements.empty()) return false;
    std::unordered_set<std::uint64_t> exclusions;
    exclusions.reserve(excludedPairs.size());
    for (const auto pair : excludedPairs) {
        const auto key = (std::uint64_t(std::min(pair.x,pair.y)) << 32u) | std::max(pair.x,pair.y);
        exclusions.insert(key);
    }
    const bool labelsChanged = !elements.empty() && elements != surface.elements;
    // Callers may supply the same filter every projection. Only a change in
    // canonical element labels or pair membership invalidates cached queries;
    // pair order, duplicate entries and reversed endpoints have no semantics.
    if (!labelsChanged && exclusions == surface.excludedElements) return true;
    if (labelsChanged) surface.elements = elements;
    surface.excludedElements = std::move(exclusions);
    context.state->candidatesValid = false;
    context.state->candidatesSuppressed = false;
    return true;
}
#if defined(WORMWOOD_OGC_CACHE_CHECK)
// Available only to the standalone native regression executable. Runtime API
// and production binaries expose no cache-state inspection entry point.
std::array<std::size_t, 3> CandidateStateForCheck(const Context& context) {
    std::lock_guard<std::mutex> lock(context.state->mutex);
    return {std::size_t(context.state->candidatesValid),
            std::size_t(context.state->candidatesSuppressed),
            context.state->pairCaches.size()};
}
std::array<double, 7> TriangleForCheck(glm::vec3 p, glm::vec3 a, glm::vec3 b, glm::vec3 c) {
    const auto r = closestPointTriangle(p,a,b,c);
    return {r.point.x,r.point.y,r.point.z,r.barycentrics.x,r.barycentrics.y,r.barycentrics.z,double(r.region)};
}
std::array<double, 8> SegmentsForCheck(glm::vec3 a, glm::vec3 b, glm::vec3 c, glm::vec3 d) {
    glm::vec3 p,q; float s,t;
    closestPointsSegments(a,b,c,d,p,q,s,t);
    return {p.x,p.y,p.z,q.x,q.y,q.z,s,t};
}
glm::vec3 NormalizeForCheck(glm::vec3 v) { return safeNormalize(v, glm::vec3(0,1,0)); }
#endif
bool SetActiveVertices(Context& context, SurfaceHandle handle,
                       const std::vector<std::uint8_t>& active) {
    if (!context.state) return false;
    std::lock_guard<std::mutex> lock(context.state->mutex);
    auto found = context.state->surfaces.find(handle);
    if (found == context.state->surfaces.end()
        || active.size() != found->second.description.positions.size()) return false;
    found->second.activeVertices = active;
    context.state->candidatesValid = false;
    context.state->candidatesSuppressed = false;
    return true;
}
bool SetMovableVertices(Context& context, SurfaceHandle handle, const std::vector<std::uint8_t>& mask) {
    if (!context.state) return false;
    std::lock_guard<std::mutex> lock(context.state->mutex);
    auto found = context.state->surfaces.find(handle);
    if (found == context.state->surfaces.end() || mask.size() != found->second.description.positions.size()) return false;
    found->second.movableVertices = mask;
    context.state->candidatesValid = false;
    context.state->candidatesSuppressed = false;
    return true;
}
bool BuildContacts(Context& context) {
    if (!context.initialized) Initialize(context);
    if (!context.state) return false;
    std::lock_guard<std::mutex> lock(context.state->mutex);
    State& state = *context.state;
    state.contacts.clear();
    StepStatistics statistics;
    statistics.surfaceCount = state.surfaces.size();

    if (context.settings.contactRadius <= 0.0f
        || context.settings.queryRadius < context.settings.contactRadius
        || context.settings.conservativeRelaxation <= 0.0f
        || context.settings.conservativeRelaxation >= 0.5f) {
        context.lastError = "OGC settings require queryRadius >= contactRadius > 0 and 0 < conservativeRelaxation < 0.5.";
        return false;
    }

    Bvh& faceBvh = state.faceBvh;
    Bvh& edgeBvh = state.edgeBvh;
    const auto broadStart = std::chrono::steady_clock::now();
    // Reuse broad-phase candidates, never forces or contact bounds. With every
    // vertex within margin of its anchor, two primitives can approach by at
    // most 2*margin. Querying that enlarged region cannot miss a new contact.
    // Fractured turf/rock still benefits from the same conservative neighbour
    // cache. Filter topology exclusions while constructing the lists, not on
    // every one of the twenty coupled queries in a simulation frame.
    bool cacheCandidates = context.settings.cacheCandidates && state.surfaces.size() <= 8
        && (!state.candidatesSuppressed || state.topologyDirty);
    const float margin = context.settings.queryRadius * 2.0f;
    bool rebuildCandidates = state.topologyDirty || !state.candidatesValid
        || state.candidateRadius != context.settings.queryRadius;
    if (cacheCandidates) {
        if (rebuildCandidates) {
            state.pairCaches.clear();
            std::vector<SurfaceHandle> handles;
            for (const auto& [handle, surface] : state.surfaces) handles.push_back(handle);
            std::sort(handles.begin(), handles.end());
            for (std::size_t a = 0; a < handles.size(); ++a) {
                for (std::size_t b = a; b < handles.size(); ++b) {
                    const auto& first = state.surfaces.at(handles[a]);
                    const auto& second = state.surfaces.at(handles[b]);
                    if ((a == b && !first.description.selfCollision) || !collisionEnabled(first, second)
                        || (!canMove(first) && !canMove(second))) continue;
                    State::PairCache pair;
                    pair.a = handles[a]; pair.b = handles[b];
                    state.pairCaches.push_back(std::move(pair));
                }
            }
        }
        auto moved = [&](SurfaceHandle handle, const std::vector<glm::vec3>& anchors) {
            const auto& positions = state.surfaces.at(handle).description.positions;
            if (positions.size() != anchors.size()) return true;
            for (std::size_t i = 0; i < positions.size(); ++i) {
                const auto delta = positions[i] - anchors[i];
                if (glm::dot(delta, delta) >= margin * margin) return true;
            }
            return false;
        };
        // Moving the tool invalidates only its pairs. It must not re-query all
        // soil/ground and turf/soil neighbours on each actuator increment.
        for (auto& pair : state.pairCaches) {
            pair.dirty = pair.dirty || moved(pair.a, pair.anchorA) || moved(pair.b, pair.anchorB);
            rebuildCandidates |= pair.dirty;
        }
    }
    for (auto& [handle, surface] : state.surfaces) {
        const std::size_t vertexCount = surface.description.positions.size();
        surface.responseOffset = statistics.vertexCount;
        surface.featureOffset = statistics.vertexCount + statistics.edgeCount + statistics.triangleCount;
        surface.responses.assign(vertexCount, {});
        surface.boundAnchors = surface.description.positions;
        for (VertexResponse& response : surface.responses) {
            response.conservativeBound = context.settings.queryRadius
                * context.settings.conservativeRelaxation;
        }
        statistics.vertexCount += vertexCount;
        statistics.triangleCount += surface.description.triangles.size();
        statistics.edgeCount += surface.edges.size();
    }
    auto updateFacePrimitive = [&](PrimitiveRef& primitive, const Surface& surface) {
        const glm::uvec3 face = surface.description.triangles[primitive.primitive];
        primitive.bounds = {};
        primitive.bounds.include(surface.description.positions[face.x]);
        primitive.bounds.include(surface.description.positions[face.y]);
        primitive.bounds.include(surface.description.positions[face.z]);
        primitive.center = (surface.description.positions[face.x]
                          + surface.description.positions[face.y]
                          + surface.description.positions[face.z]) / 3.0f;
    };
    auto updateEdgePrimitive = [&](PrimitiveRef& primitive, const Surface& surface) {
        const Edge& edge = surface.edges[primitive.primitive];
        primitive.bounds = {};
        primitive.bounds.include(surface.description.positions[edge.a]);
        primitive.bounds.include(surface.description.positions[edge.b]);
        primitive.center = (surface.description.positions[edge.a]
                          + surface.description.positions[edge.b]) * 0.5f;
    };
    if (state.topologyDirty) {
        faceBvh.primitives.clear();
        edgeBvh.primitives.clear();
        for (auto& [handle, surface] : state.surfaces) {
            surface.edgePrimitives.clear();
            surface.facePrimitives.clear();
            for (std::uint32_t faceIndex = 0;
                 faceIndex < surface.description.triangles.size(); ++faceIndex) {
                PrimitiveRef primitive;
                primitive.surface = handle;
                primitive.primitive = faceIndex;
                updateFacePrimitive(primitive, surface);
                surface.facePrimitives.push_back(static_cast<std::uint32_t>(faceBvh.primitives.size()));
                faceBvh.primitives.push_back(primitive);
            }
            for (std::uint32_t edgeIndex = 0;
                 edgeIndex < surface.edges.size(); ++edgeIndex) {
                PrimitiveRef primitive;
                primitive.surface = handle;
                primitive.primitive = edgeIndex;
                updateEdgePrimitive(primitive, surface);
                surface.edgePrimitives.push_back(static_cast<std::uint32_t>(edgeBvh.primitives.size()));
                edgeBvh.primitives.push_back(primitive);
            }
        }
        faceBvh.build();
        edgeBvh.build();
        state.topologyDirty = false;
    } else {
        for (auto& [handle, surface] : state.surfaces) {
            if (!surface.geometryDirty) continue;
            for (auto i : surface.facePrimitives) updateFacePrimitive(faceBvh.primitives[i], surface);
            for (auto i : surface.edgePrimitives) updateEdgePrimitive(edgeBvh.primitives[i], surface);
        }
        if (!cacheCandidates || rebuildCandidates) {
            faceBvh.refit();
            edgeBvh.refit();
        }
    }
    for (auto& [handle, surface] : state.surfaces) surface.geometryDirty = false;
    if (cacheCandidates && rebuildCandidates) {
        const float radius = context.settings.queryRadius + 2.0f * margin + 1e-6f;
        std::size_t pairs = 0;
        for (const auto& pair : state.pairCaches) {
            if (!pair.dirty) pairs += pair.facesA.size() + pair.facesB.size() + pair.edges.size();
        }
        bool overflow = pairs > context.settings.maximumCandidatePairs;
        for (auto& pair : state.pairCaches) {
            if (overflow) break;
            if (!pair.dirty) continue;
            const auto& first = state.surfaces.at(pair.a);
            const auto& second = state.surfaces.at(pair.b);
            pair.anchorA = first.description.positions;
            pair.anchorB = second.description.positions;
            pair.facesA.clear(); pair.facesB.clear(); pair.edges.clear();
            auto vertices = [&](SurfaceHandle from, SurfaceHandle to, auto& list) {
                const auto& source = state.surfaces.at(from);
                const auto& target = state.surfaces.at(to);
                for (std::uint32_t i = 0; i < source.description.positions.size() && !overflow; ++i) {
                    if (!source.activeVertices.empty() && !source.activeVertices[i]) continue;
                    faceBvh.querySphere(source.description.positions[i], radius, InvalidSurfaceHandle,
                        [&](const PrimitiveRef& ref) {
                            if (overflow) return;
                            if (!movingFacePair(source, i, target, target.description.triangles[ref.primitive])) return;
                            if (from == to) {
                                const auto face = target.description.triangles[ref.primitive];
                                if (faceContainsVertex(face, i) || !elementContactEnabled(source, i, face.x)) return;
                            }
                            if (++pairs > context.settings.maximumCandidatePairs) { overflow = true; return; }
                            list.emplace_back(i, static_cast<std::uint32_t>(&ref - faceBvh.primitives.data()));
                        }, to);
                }
            };
            vertices(pair.a, pair.b, pair.facesA);
            if (pair.a != pair.b) vertices(pair.b, pair.a, pair.facesB);
            for (auto i : first.edgePrimitives) {
                if (overflow) break;
                const auto& query = edgeBvh.primitives[i];
                const auto& a = first.edges[query.primitive];
                auto bounds = query.bounds; bounds.expand(radius);
                edgeBvh.queryAabb(bounds, InvalidSurfaceHandle, [&](const PrimitiveRef& ref) {
                    if (overflow) return;
                    const auto j = static_cast<std::uint32_t>(&ref - edgeBvh.primitives.data());
                    const auto& b = second.edges[ref.primitive];
                    if (!movingEdgePair(first, a, second, b)) return;
                    if (pair.a == pair.b && (j <= i || !elementContactEnabled(first, a.a, b.a)
                        || a.a == b.a || a.a == b.b || a.b == b.a || a.b == b.b)) return;
                    if (++pairs > context.settings.maximumCandidatePairs) { overflow = true; return; }
                    pair.edges.emplace_back(std::min(i,j), std::max(i,j));
                }, pair.b);
            }
            pair.dirty = false;
        }
        state.candidateRadius = context.settings.queryRadius;
        state.candidatesValid = !overflow;
        state.candidatesSuppressed = overflow;
        if (overflow) {
            // Dense fixtures fall back to complete streaming queries; no contact
            // is dropped and no overflowing rebuild is retried every batch.
            cacheCandidates = false;
            state.pairCaches.clear();
            for (auto& [handle, surface] : state.surfaces) surface.nearbyFaces.clear();
            state.nearbyEdges.clear();
        } else {
            for (auto& [handle, surface] : state.surfaces) {
                surface.nearbyFaces.resize(surface.description.positions.size());
                for (auto& list : surface.nearbyFaces) list.clear();
            }
            state.nearbyEdges.resize(edgeBvh.primitives.size());
            for (auto& list : state.nearbyEdges) list.clear();
            for (const auto& pair : state.pairCaches) {
                auto& a = state.surfaces.at(pair.a).nearbyFaces;
                auto& b = state.surfaces.at(pair.b).nearbyFaces;
                for (auto [i,j] : pair.facesA) a[i].push_back(j);
                for (auto [i,j] : pair.facesB) b[i].push_back(j);
                for (auto [i,j] : pair.edges) state.nearbyEdges[i].push_back(j);
            }
        }
    } else if (!cacheCandidates) { state.candidatesValid = false; }
    statistics.broadPhaseMilliseconds = std::chrono::duration<double, std::milli>(
        std::chrono::steady_clock::now() - broadStart).count();

    const auto narrowStart = std::chrono::steady_clock::now();
    // Apple runs these independent query ranges on its persistent worker pool.
    // Each range owns its response buffer; reduction occurs only after all
    // ranges finish. No worker writes the shared surface state or contacts.
    // Streaming self-contact queries are read-only BVH walks too. Each worker
    // still owns its response/contact buffers, so fracture must not switch a
    // previously parallel material scene back to a single CPU worker.
    const std::size_t workers = context.settings.parallelQueries && statistics.vertexCount >= 384 ? 4 : 1;
    auto& partials = state.partials;
    partials.resize(workers);
    for (auto& part : partials) {
        part.responses.assign(statistics.vertexCount, {});
        part.contacts.clear();
        part.featureStamps.clear();
        part.statistics = {};
        part.error = {};
        for (auto& response : part.responses) response.conservativeBound = context.settings.queryRadius * context.settings.conservativeRelaxation;
    }
    const std::size_t featureCount = statistics.vertexCount + statistics.edgeCount + statistics.triangleCount;
    auto solveRange = [&](std::size_t worker) {
    auto& part = partials[worker];
    auto& statistics = part.statistics;
    std::vector<PrimitiveKey> emitted;
    emitted.reserve(std::min(std::size_t(32), context.settings.maximumContactsPerPrimitive));
    auto featureIndex = [&](const PrimitiveKey& key) {
        const auto& surface = state.surfaces.at(key.surface);
        return surface.featureOffset + key.primitive
            + (key.type >= 1 ? surface.description.positions.size() : 0)
            + (key.type >= 2 ? surface.edges.size() : 0);
    };
    for (auto sourceIterator = state.surfaces.begin();
         sourceIterator != state.surfaces.end(); ++sourceIterator) {
        const SurfaceHandle sourceHandle = sourceIterator->first;
        Surface& source = sourceIterator->second;
        auto* sourceResponses = part.responses.data() + source.responseOffset;
        for (std::uint32_t vertexIndex = static_cast<std::uint32_t>(worker);
             vertexIndex < source.description.positions.size(); vertexIndex += workers) {
            if (!source.activeVertices.empty() && !source.activeVertices[vertexIndex]) continue;
            if (cacheCandidates && source.nearbyFaces[vertexIndex].empty()) continue;
            const glm::vec3 point = source.description.positions[vertexIndex];
            emitted.clear();
            bool stamped = false;
            const std::size_t stamp = source.responseOffset + vertexIndex + 1;
            auto alreadyEmitted = [&](const PrimitiveKey& key) {
                if (!stamped) {
                    if (std::find(emitted.begin(), emitted.end(), key) != emitted.end()) return true;
                    if (emitted.size() < context.settings.maximumContactsPerPrimitive) {
                        emitted.push_back(key);
                        return false;
                    }
                    // Dense or finely triangulated contacts use a complete,
                    // topology-sized marker table instead of dropping pairs.
                    // Every feature has one slot, independent of query density.
                    if (part.featureStamps.empty()) {
                        part.featureStamps.assign(featureCount, 0);
                    }
                    for (const auto& previous : emitted) part.featureStamps[featureIndex(previous)] = stamp;
                    stamped = true;
                }
                auto& seen = part.featureStamps[featureIndex(key)];
                if (seen == stamp) return true;
                seen = stamp;
                return false;
            };
            auto visitFace = [&](const PrimitiveRef& reference) {
                    if (!reference.bounds.overlapsSphere(point, context.settings.queryRadius)) return;
                    Surface& target = state.surfaces.at(reference.surface);
                    auto* targetResponses = part.responses.data() + target.responseOffset;
                    if (!collisionEnabled(source, target)) return;
                    if (sourceHandle == reference.surface) {
                        if (!source.description.selfCollision) return;
                        const glm::uvec3 adjacent = target.description.triangles[reference.primitive];
                        if (faceContainsVertex(adjacent, vertexIndex) || !elementContactEnabled(source, vertexIndex, adjacent.x)) return;
                    }
                    const glm::uvec3 face = target.description.triangles[reference.primitive];
                    if (!movingFacePair(source, vertexIndex, target, face)) return;
                    const ClosestTriangleResult closest = closestPointTriangle(
                        point,
                        target.description.positions[face.x],
                        target.description.positions[face.y],
                        target.description.positions[face.z]);
                    if (closest.region == ClosestRegion::Invalid) return;
                    const float distance = safeLength(point - closest.point);
                    updateMinimum(sourceResponses[vertexIndex].conservativeBound,
                                  std::max(0.0f, distance - source.description.thickness
                                                      - target.description.thickness)
                                      * context.settings.conservativeRelaxation);
                    for (std::uint32_t targetVertex : {face.x, face.y, face.z}) {
                        if (targetVertex < target.responses.size()) {
                            updateMinimum(targetResponses[targetVertex].conservativeBound,
                                          std::max(0.0f, distance - source.description.thickness
                                                              - target.description.thickness)
                                              * context.settings.conservativeRelaxation);
                        }
                    }
                    if (distance > context.settings.queryRadius) return;
                    if (!checkFeasible(point, target, reference.primitive,
                                       closest.region,
                                       context.settings.feasibleRegionEpsilon)) return;
                    const float thickness = source.description.thickness
                        + target.description.thickness;
                    float lambda = 0.0f;
                    float stiffness = 0.0f;
                    contactDerivatives(context.settings, distance, thickness,
                                       lambda, stiffness);
                    if (lambda <= 0.0f) return;
                    const PrimitiveKey key = primitiveKey(target, reference.surface,
                                                          reference.primitive, closest.region);
                    if (alreadyEmitted(key)) return;
                    const glm::vec3 faceNormal = safeNormalize(
                        glm::cross(glm::dvec3(target.description.positions[face.y])
                                     - glm::dvec3(target.description.positions[face.x]),
                                   glm::dvec3(target.description.positions[face.z])
                                     - glm::dvec3(target.description.positions[face.x])),
                        glm::vec3(0.0f, 1.0f, 0.0f));
                    const glm::vec3 normal = safeNormalize(point - closest.point,
                                                           faceNormal);
                    const glm::vec3 sourceDelta = point - previousAt(source, vertexIndex);
                    const glm::vec3 targetPrevious =
                        closest.barycentrics.x * previousAt(target, face.x)
                      + closest.barycentrics.y * previousAt(target, face.y)
                      + closest.barycentrics.z * previousAt(target, face.z);
                    const glm::vec3 relativeDelta = sourceDelta
                        - (closest.point - targetPrevious);
                    const glm::vec3 tangent = relativeDelta
                        - normal * glm::dot(relativeDelta, normal);
                    const float tangentLength = safeLength(tangent);
                    const float friction = 0.5f * (source.description.friction
                                                  + target.description.friction);
                    const float smoothLength = std::max(tangentLength,
                        context.settings.frictionEpsilon);
                    const glm::vec3 frictionForce = tangentLength > kEpsilon
                        ? -friction * lambda * tangent / smoothLength
                        : glm::vec3(0.0f);
                    const float frictionStiffness = friction * lambda / smoothLength;

                    addResponse(source, sourceResponses, vertexIndex, 1.0f, normal, lambda,
                                stiffness, frictionForce, frictionStiffness);
                    addResponse(target, targetResponses, face.x, -closest.barycentrics.x, normal,
                                lambda, stiffness, frictionForce, frictionStiffness);
                    addResponse(target, targetResponses, face.y, -closest.barycentrics.y, normal,
                                lambda, stiffness, frictionForce, frictionStiffness);
                    addResponse(target, targetResponses, face.z, -closest.barycentrics.z, normal,
                                lambda, stiffness, frictionForce, frictionStiffness);

                    ContactPoint contact;
                    contact.kind = ContactKind::VertexFace;
                    contact.surfaceA = sourceHandle;
                    contact.surfaceB = reference.surface;
                    contact.primitiveA = vertexIndex;
                    contact.primitiveB = reference.primitive;
                    contact.pointA = point;
                    contact.pointB = closest.point;
                    contact.normal = normal;
                    contact.weights = glm::vec4(-closest.barycentrics, 1.0f);
                    contact.distance = distance;
                    part.contacts.push_back(contact);
                    ++statistics.vertexFaceContacts;
                };
            if (cacheCandidates) {
                for (auto index : source.nearbyFaces[vertexIndex]) visitFace(faceBvh.primitives[index]);
            } else faceBvh.querySphere(point, context.settings.queryRadius,
                source.description.selfCollision ? InvalidSurfaceHandle : sourceHandle, visitFace);
        }
    }

    // Each leaf contains each primitive exactly once, and candidateIndex must
    // exceed primitiveIndex. No pair can repeat; a per-pair hash allocation is
    // unnecessary, including for candidates which turn out to be far apart.
    for (std::size_t primitiveIndex = worker;
         primitiveIndex < edgeBvh.primitives.size(); primitiveIndex += workers) {
        if (cacheCandidates && state.nearbyEdges[primitiveIndex].empty()) continue;
        const PrimitiveRef& query = edgeBvh.primitives[primitiveIndex];
        Surface& first = state.surfaces.at(query.surface);
        auto* firstResponses = part.responses.data() + first.responseOffset;
        const Edge& firstEdge = first.edges[query.primitive];
        Aabb search = query.bounds;
        search.expand(context.settings.queryRadius);
        auto visitEdge = [&](const PrimitiveRef& candidate) {
            if (!candidate.bounds.overlaps(search)) return;
            const std::size_t candidateIndex = static_cast<std::size_t>(&candidate - edgeBvh.primitives.data());
            if (candidateIndex <= primitiveIndex) return;
            Surface& second = state.surfaces.at(candidate.surface);
            auto* secondResponses = part.responses.data() + second.responseOffset;
            if (!collisionEnabled(first, second)) return;
            if (!canMove(first) && !canMove(second)) return;
            const Edge& secondEdge = second.edges[candidate.primitive];
            if (!movingEdgePair(first, firstEdge, second, secondEdge)) return;
            if (query.surface == candidate.surface) {
                if (!first.description.selfCollision || !elementContactEnabled(first, firstEdge.a, secondEdge.a)) return;
                if (firstEdge.a == secondEdge.a || firstEdge.a == secondEdge.b
                    || firstEdge.b == secondEdge.a || firstEdge.b == secondEdge.b) return;
            }
            glm::vec3 pointA;
            glm::vec3 pointB;
            float s = 0.0f;
            float t = 0.0f;
            closestPointsSegments(first.description.positions[firstEdge.a],
                                  first.description.positions[firstEdge.b],
                                  second.description.positions[secondEdge.a],
                                  second.description.positions[secondEdge.b],
                                  pointA, pointB, s, t);
            const float distance = safeLength(pointA - pointB);
            if (distance > context.settings.queryRadius) return;
            const float safeDistance = std::max(0.0f, distance
                - first.description.thickness - second.description.thickness)
                * context.settings.conservativeRelaxation;
            for (std::uint32_t vertex : {firstEdge.a, firstEdge.b}) {
                updateMinimum(firstResponses[vertex].conservativeBound, safeDistance);
            }
            for (std::uint32_t vertex : {secondEdge.a, secondEdge.b}) {
                updateMinimum(secondResponses[vertex].conservativeBound, safeDistance);
            }
            const glm::dvec3 firstDirection = glm::dvec3(first.description.positions[firstEdge.b])
                - glm::dvec3(first.description.positions[firstEdge.a]);
            const glm::dvec3 secondDirection = glm::dvec3(second.description.positions[secondEdge.b])
                - glm::dvec3(second.description.positions[secondEdge.a]);
            const double firstLength2 = glm::dot(firstDirection, firstDirection);
            const double secondLength2 = glm::dot(secondDirection, secondDirection);
            if (firstLength2 == 0.0 || secondLength2 == 0.0) return;
            const double parallelMeasure = glm::dot(glm::cross(firstDirection, secondDirection),
                                                   glm::cross(firstDirection, secondDirection))
                / (firstLength2 * secondLength2);
            if (parallelMeasure <= kParallelMeasureEpsilon) return;
            const bool feasible =
                (glm::dot(glm::dvec3(pointB) - glm::dvec3(first.description.positions[firstEdge.a]), firstDirection) >= 0.0
                 && glm::dot(glm::dvec3(first.description.positions[firstEdge.b]) - glm::dvec3(pointB), firstDirection) >= 0.0)
                || (glm::dot(glm::dvec3(pointA) - glm::dvec3(second.description.positions[secondEdge.a]), secondDirection) >= 0.0
                    && glm::dot(glm::dvec3(second.description.positions[secondEdge.b]) - glm::dvec3(pointA), secondDirection) >= 0.0);
            if (!feasible) return;

            float lambda = 0.0f;
            float stiffness = 0.0f;
            contactDerivatives(context.settings, distance,
                               first.description.thickness + second.description.thickness,
                               lambda, stiffness);
            if (lambda <= 0.0f) return;
            const glm::vec3 fallbackNormal = safeNormalize(
                glm::cross(firstDirection, secondDirection),
                glm::vec3(0.0f, 1.0f, 0.0f));
            const glm::vec3 normal = safeNormalize(pointA - pointB, fallbackNormal);
            const glm::vec3 previousA = (1.0f - s) * previousAt(first, firstEdge.a)
                                      + s * previousAt(first, firstEdge.b);
            const glm::vec3 previousB = (1.0f - t) * previousAt(second, secondEdge.a)
                                      + t * previousAt(second, secondEdge.b);
            const glm::vec3 relativeDelta = (pointA - previousA) - (pointB - previousB);
            const glm::vec3 tangent = relativeDelta
                - normal * glm::dot(relativeDelta, normal);
            const float tangentLength = safeLength(tangent);
            const float friction = 0.5f * (first.description.friction
                                          + second.description.friction);
            const float smoothLength = std::max(tangentLength,
                                                context.settings.frictionEpsilon);
            const glm::vec3 frictionForce = tangentLength > kEpsilon
                ? -friction * lambda * tangent / smoothLength
                : glm::vec3(0.0f);
            const float frictionStiffness = friction * lambda / smoothLength;
            const float coefficients[4] = {1.0f - s, s, -1.0f + t, -t};
            addResponse(first, firstResponses, firstEdge.a, coefficients[0], normal, lambda,
                        stiffness, frictionForce, frictionStiffness);
            addResponse(first, firstResponses, firstEdge.b, coefficients[1], normal, lambda,
                        stiffness, frictionForce, frictionStiffness);
            addResponse(second, secondResponses, secondEdge.a, coefficients[2], normal, lambda,
                        stiffness, frictionForce, frictionStiffness);
            addResponse(second, secondResponses, secondEdge.b, coefficients[3], normal, lambda,
                        stiffness, frictionForce, frictionStiffness);

            ContactPoint contact;
            contact.kind = ContactKind::EdgeEdge;
            contact.surfaceA = query.surface;
            contact.surfaceB = candidate.surface;
            contact.primitiveA = query.primitive;
            contact.primitiveB = candidate.primitive;
            contact.pointA = pointA;
            contact.pointB = pointB;
            contact.normal = normal;
            contact.weights = glm::vec4(coefficients[0], coefficients[1],
                                        coefficients[2], coefficients[3]);
            contact.distance = distance;
            part.contacts.push_back(contact);
            ++statistics.edgeEdgeContacts;
        };
        if (cacheCandidates) {
            for (auto index : state.nearbyEdges[primitiveIndex]) visitEdge(edgeBvh.primitives[index]);
        } else edgeBvh.queryAabb(search, first.description.selfCollision ? InvalidSurfaceHandle : query.surface, visitEdge);
    }

    };
#if defined(__APPLE__)
    if (workers > 1) {
        struct Work { decltype(solveRange)* solve; std::vector<State::Partial>* partials; } work{&solveRange, &partials};
        dispatch_apply_f(workers, dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), &work,
            [](void* opaque, std::size_t worker) {
                auto& work = *static_cast<Work*>(opaque);
                try { (*work.solve)(worker); } catch (...) { (*work.partials)[worker].error = std::current_exception(); }
            });
    } else solveRange(0);
#else
    for (std::size_t worker = 0; worker < workers; ++worker) solveRange(worker);
#endif
    for (const auto& part : partials) {
        if (part.error) std::rethrow_exception(part.error);
        statistics.vertexFaceContacts += part.statistics.vertexFaceContacts;
        statistics.edgeEdgeContacts += part.statistics.edgeEdgeContacts;
        statistics.contactBufferOverflows += part.statistics.contactBufferOverflows;
        state.contacts.insert(state.contacts.end(), part.contacts.begin(), part.contacts.end());
    }
    // Candidate traversal and worker assignment need not sum cancelling
    // contacts in the same order. Retain double precision through both levels
    // of accumulation, rounding only once at the public response boundary.
    for (auto& [handle, surface] : state.surfaces) {
        for (std::size_t i = 0; i < surface.responses.size(); ++i) {
            auto& out = surface.responses[i];
            glm::dvec3 force(0.0);
            glm::dmat3 hessian(0.0);
            for (const auto& part : partials) {
                const auto& in = part.responses[surface.responseOffset + i];
                force += in.force;
                hessian += in.hessian;
                out.conservativeBound = std::min(out.conservativeBound, in.conservativeBound);
                out.contactCount += in.contactCount;
                out.active = out.active || in.active;
            }
            out.force = glm::vec3(force);
            out.hessian = glm::mat3(hessian);
        }
    }
    statistics.narrowPhaseMilliseconds = std::chrono::duration<double, std::milli>(
        std::chrono::steady_clock::now() - narrowStart).count();
    statistics.truncatedDisplacements = state.truncatedDisplacements;
    context.statistics = statistics;
    context.lastError.clear();
    return true;
}

bool GetVertexResponses(const Context& context, SurfaceHandle handle, std::vector<VertexResponse>& output) {
    if (!context.state) return false;
    std::lock_guard<std::mutex> lock(context.state->mutex);
    const auto found = context.state->surfaces.find(handle);
    if (found == context.state->surfaces.end()) return false;
    output = found->second.responses;
    return true;
}

VertexResponse GetVertexResponse(const Context& context,
                                 SurfaceHandle handle,
                                 std::uint32_t vertexIndex) {
    if (!context.state) return {};
    std::lock_guard<std::mutex> lock(context.state->mutex);
    const auto found = context.state->surfaces.find(handle);
    if (found == context.state->surfaces.end()
        || vertexIndex >= found->second.responses.size()) return {};
    return found->second.responses[vertexIndex];
}

FilteredDisplacement FilterVertexPosition(Context& context,
                                           SurfaceHandle handle,
                                           std::uint32_t vertexIndex,
                                           const glm::vec3& proposedPosition) {
    FilteredDisplacement result;
    result.position = proposedPosition;
    if (!context.state) return result;
    std::lock_guard<std::mutex> lock(context.state->mutex);
    auto found = context.state->surfaces.find(handle);
    if (found == context.state->surfaces.end()
        || vertexIndex >= found->second.responses.size()) {
        context.lastError = "OGC surface or vertex was not found while filtering displacement.";
        return result;
    }
    Surface& surface = found->second;
    const glm::vec3 anchor = surface.boundAnchors[vertexIndex];
    const glm::vec3 displacement = proposedPosition - anchor;
    const float length = safeLength(displacement);
    const float bound = std::max(0.0f,
        surface.responses[vertexIndex].conservativeBound);
    if (length > bound && length > kEpsilon) {
        result.position = anchor + displacement * (bound / length);
        result.truncated = true;
        result.contactsNeedRefresh = true;
        ++context.state->truncatedDisplacements;
        context.statistics.truncatedDisplacements = context.state->truncatedDisplacements;
    }
    return result;
}

std::vector<ContactPoint> CopyContacts(const Context& context) {
    if (!context.state) return {};
    std::lock_guard<std::mutex> lock(context.state->mutex);
    return context.state->contacts;
}

StepStatistics GetStatistics(const Context& context) {
    return context.statistics;
}

bool ContainsSurface(const Context& context, SurfaceHandle handle) {
    if (!context.state) return false;
    std::lock_guard<std::mutex> lock(context.state->mutex);
    return context.state->surfaces.find(handle) != context.state->surfaces.end();
}

std::string LastError(const Context& context) {
    return context.lastError;
}

} // namespace OffsetGeometricContact
