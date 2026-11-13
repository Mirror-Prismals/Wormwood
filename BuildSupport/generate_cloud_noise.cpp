// Original cloud noise generator, isolated from world/terrain wrappers.
// Preserves the 64^3 periodic RGBA volume, seven mip levels and gutters.
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <fstream>
#include <vector>
namespace WormwoodCloud {
constexpr int NoiseSide=64,NoiseTile=66,NoiseAtlasSide=528,NoiseAtlasHeight=723;
constexpr std::array<int,7> NoiseMipOffset{{0,528,664,700,710,716,720}};
inline int wrap(int x, int size) { return (x % size + size) % size; }
inline float clamp01(float x) { return std::max(0.0f, std::min(1.0f, x)); }
inline unsigned char byte(float x) { return static_cast<unsigned char>(std::round(clamp01(x) * 255.0f)); }
inline uint32_t hash(uint32_t x) {
    x ^= x >> 16u; x *= 0x7feb352du; x ^= x >> 15u;
    x *= 0x846ca68bu; return x ^ (x >> 16u);
}
inline uint32_t lattice(int x, int y, int z, int period, uint32_t seed) {
    return hash(static_cast<uint32_t>(wrap(x, period)) * 73856093u
        ^ static_cast<uint32_t>(wrap(y, period)) * 19349663u
        ^ static_cast<uint32_t>(wrap(z, period)) * 83492791u ^ seed);
}
inline float unit(uint32_t x) { return static_cast<float>(x & 0x00ffffffu) / 16777216.0f; }
inline float value(float x, float y, float z, int period, uint32_t seed) {
    const int ix = static_cast<int>(std::floor(x)), iy = static_cast<int>(std::floor(y)), iz = static_cast<int>(std::floor(z));
    const auto fade = [](float v) { return v * v * v * (v * (v * 6.0f - 15.0f) + 10.0f); };
    const float fx = fade(x - ix), fy = fade(y - iy), fz = fade(z - iz);
    float result = 0.0f;
    for (int dz = 0; dz < 2; ++dz) for (int dy = 0; dy < 2; ++dy) for (int dx = 0; dx < 2; ++dx) {
        result += unit(lattice(ix + dx, iy + dy, iz + dz, period, seed))
            * (dx ? fx : 1.0f - fx) * (dy ? fy : 1.0f - fy) * (dz ? fz : 1.0f - fz);
    }
    return result;
}
inline float worley(float x, float y, float z, int period, uint32_t seed) {
    const int ix = static_cast<int>(std::floor(x)), iy = static_cast<int>(std::floor(y)), iz = static_cast<int>(std::floor(z));
    float nearest = 10.0f;
    for (int dz = -1; dz <= 1; ++dz) for (int dy = -1; dy <= 1; ++dy) for (int dx = -1; dx <= 1; ++dx) {
        const uint32_t h = lattice(ix + dx, iy + dy, iz + dz, period, seed);
        const float px = ix + dx + unit(h) - x;
        const float py = iy + dy + unit(hash(h + 31u)) - y;
        const float pz = iz + dz + unit(hash(h + 73u)) - z;
        nearest = std::min(nearest, px * px + py * py + pz * pz);
    }
    return clamp01(1.0f - std::sqrt(nearest) * 0.90f);
}

inline std::vector<unsigned char> BuildNoiseAtlas() {
    std::vector<unsigned char> volume(NoiseSide * NoiseSide * NoiseSide * 4);
    for (int z = 0; z < NoiseSide; ++z) for (int y = 0; y < NoiseSide; ++y) for (int x = 0; x < NoiseSide; ++x) {
        const float px = (x + 0.5f) / NoiseSide, py = (y + 0.5f) / NoiseSide, pz = (z + 0.5f) / NoiseSide;
        const float base = worley(px * 4, py * 4, pz * 4, 4, 137u);
        const float medium = worley(px * 8, py * 8, pz * 8, 8, 931u);
        const float fine = worley(px * 16, py * 16, pz * 16, 16, 4129u);
        const float broad = value(px * 4, py * 4, pz * 4, 4, 1231u) * 0.67f
            + value(px * 8, py * 8, pz * 8, 8, 713u) * 0.33f;
        const size_t i = static_cast<size_t>((z * NoiseSide + y) * NoiseSide + x) * 4;
        volume[i] = byte(base * 0.67f + broad * 0.33f);
        volume[i + 1] = byte(medium);
        volume[i + 2] = byte(fine);
        volume[i + 3] = byte(broad);
    }
    std::vector<unsigned char> atlas(NoiseAtlasSide * NoiseAtlasHeight * 4);
    int side = NoiseSide;
    for (int level = 0; level < 7; ++level) {
        const int tile = side + 2, columns = std::min(8, side);
        for (int z = 0; z < side; ++z) for (int y = 0; y < tile; ++y) for (int x = 0; x < tile; ++x) {
            const int sx = wrap(x - 1, side), sy = wrap(y - 1, side);
            const size_t source = static_cast<size_t>((z * side + sy) * side + sx) * 4;
            const size_t dest = static_cast<size_t>(((NoiseMipOffset[level] + z / columns * tile + y) * NoiseAtlasSide)
                + z % columns * tile + x) * 4;
            std::copy_n(volume.data() + source, 4, atlas.data() + dest);
        }
        if (side > 1) {
            const int nextSide = side / 2;
            std::vector<unsigned char> next(nextSide * nextSide * nextSide * 4);
            for (int z = 0; z < nextSide; ++z) for (int y = 0; y < nextSide; ++y) for (int x = 0; x < nextSide; ++x) {
                for (int channel = 0; channel < 4; ++channel) {
                    unsigned sum = 0;
                    for (int dz = 0; dz < 2; ++dz) for (int dy = 0; dy < 2; ++dy) for (int dx = 0; dx < 2; ++dx)
                        sum += volume[(((z * 2 + dz) * side + y * 2 + dy) * side + x * 2 + dx) * 4 + channel];
                    next[((z * nextSide + y) * nextSide + x) * 4 + channel] = static_cast<unsigned char>((sum + 4u) / 8u);
                }
            }
            volume = std::move(next); side = nextSide;
        }
    }
    return atlas;
}

}
int main(int argc,char**argv){if(argc!=2)return 2;const auto rgba=WormwoodCloud::BuildNoiseAtlas();std::ofstream out(argv[1],std::ios::binary);const uint32_t width=528,height=723;out.write(reinterpret_cast<const char*>(&width),4);out.write(reinterpret_cast<const char*>(&height),4);out.write(reinterpret_cast<const char*>(rgba.data()),rgba.size());return out.good()?0:1;}
