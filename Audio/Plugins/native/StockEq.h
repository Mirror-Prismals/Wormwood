#pragma once

#include <array>
#include <atomic>
#include <cstddef>
#include <utility>

// The legacy type/file names remain stable for session and host integration.
// Each U-Shaping instance is one region bounded by two transition frequencies.
struct LShapingBiquadMemory {
    double z1 = 0.0;
    double z2 = 0.0;
};

struct LShapingDspPathState {
    std::array<LShapingBiquadMemory, 8> left{}, right{};
    std::array<LShapingBiquadMemory, 8> previousLeft{}, previousRight{};
    std::array<double, 2> smoothLogHz{};
    std::array<int, 2> orders{{2, 2}}, previousOrders{{2, 2}};
    double smoothGainDb = 0.0;
    double wet = 1.0;
    double slopeFade = 1.0;
    float sampleRate = 0.0f;
    bool initialized = false;
};

struct LShapingBandParameters {
    std::atomic<float> frequencyHz{1000.0f};
    std::atomic<int> slopeDbOct{12};

    LShapingBandParameters() = default;
    LShapingBandParameters(const LShapingBandParameters&) = delete;
    LShapingBandParameters& operator=(const LShapingBandParameters&) = delete;
    LShapingBandParameters(LShapingBandParameters&& other) noexcept { *this = std::move(other); }
    LShapingBandParameters& operator=(LShapingBandParameters&& other) noexcept {
        frequencyHz.store(other.frequencyHz.load(std::memory_order_relaxed), std::memory_order_relaxed);
        slopeDbOct.store(other.slopeDbOct.load(std::memory_order_relaxed), std::memory_order_relaxed);
        return *this;
    }
};

struct LShapingTrackState {
    std::atomic<bool> bypassed{false};
    std::atomic<float> gainDb{0.0f};
    std::array<LShapingBandParameters, 2> bands{};
    LShapingDspPathState realtime{}, offline{};

    LShapingTrackState() {
        bands[0].frequencyHz.store(180.0f, std::memory_order_relaxed);
        bands[1].frequencyHz.store(6000.0f, std::memory_order_relaxed);
    }
    LShapingTrackState(const LShapingTrackState&) = delete;
    LShapingTrackState& operator=(const LShapingTrackState&) = delete;
    LShapingTrackState(LShapingTrackState&& other) noexcept { *this = std::move(other); }
    LShapingTrackState& operator=(LShapingTrackState&& other) noexcept {
        bypassed.store(other.bypassed.load(std::memory_order_relaxed), std::memory_order_relaxed);
        gainDb.store(other.gainDb.load(std::memory_order_relaxed), std::memory_order_relaxed);
        for (size_t i = 0; i < bands.size(); ++i) bands[i] = std::move(other.bands[i]);
        realtime = other.realtime;
        offline = other.offline;
        return *this;
    }
};

#include "json.hpp"
namespace LShapingSystemLogic {
enum class ProcessingPath : uint8_t { Realtime = 0, Offline = 1 };
void ProcessStereo(LShapingTrackState&, float*, float*, int, float, ProcessingPath);
void ProcessMono(LShapingTrackState&, float*, int, float, ProcessingPath);
void ResetPath(LShapingTrackState&, ProcessingPath);
void ResponseCurveDb(const LShapingTrackState&, const float*, float*, int, float);
float ResponseDb(const LShapingTrackState&, float, float);
nlohmann::json Serialize(const LShapingTrackState&);
void Deserialize(LShapingTrackState&, const nlohmann::json&);
}
