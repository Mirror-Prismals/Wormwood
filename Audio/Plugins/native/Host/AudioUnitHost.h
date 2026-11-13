#pragma once
#include "Microtonal.h"

#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <string>
#include <vector>

struct BaseSystem;
struct Vst3Context;
struct Vst3AvailablePlugin;
struct Vst3UiWindow;

struct AudioUnitParamInfo {
    uint32_t id = 0;
    std::string name;
    std::string units;
    float minValue = 0.0f;
    float maxValue = 1.0f;
    float defaultValue = 0.0f;
    bool readable = true;
    bool writable = true;
};

// The native AudioComponent/AudioUnit handles remain opaque in the shared
// header so Cardinal's non-Apple builds do not acquire Apple SDK dependencies.
struct AudioUnitPlugin {
    std::vector<Microtonal::ActiveVoice> tuningVoices;
    int nextTuningVoiceId = 1024;
    bool tuningMpeZoneActive = false;
    std::string name;
    std::string vendor;
    std::string uniqueId;
    uint32_t componentType = 0;
    uint32_t componentSubtype = 0;
    uint32_t componentManufacturer = 0;
    void* component = nullptr;
    void* instance = nullptr;
    Vst3Context* owner = nullptr;
    Vst3UiWindow* uiWindow = nullptr;
    bool uiVisible = false;
    bool isInstrument = false;
    bool initialized = false;
    int inputChannels = 0;
    int outputChannels = 0;
    std::atomic<bool> bypassed{false};
    std::vector<AudioUnitParamInfo> parameterInfo;
    std::vector<float> interleavedInput;
    std::vector<float> interleavedOutput;
    const float* renderInputLeft = nullptr;
    const float* renderInputRight = nullptr;
    int renderInputFrames = 0;
};

namespace AudioUnitSystemLogic {
    void InitializeBackend(Vst3Context& ctx, const std::string& executableDir);
    void CleanupBackend(Vst3Context& ctx);
    AudioUnitPlugin* CreatePlugin(Vst3Context& ctx,
                                  const Vst3AvailablePlugin& available,
                                  bool instrument);
    void DestroyPlugin(Vst3Context& ctx, AudioUnitPlugin* plugin);
    bool ProcessEffect(Vst3Context& ctx, AudioUnitPlugin& plugin,
                       const float* inputLeft, const float* inputRight,
                       float* outputLeft, float* outputRight, int numFrames,
                       int64_t sampleOffset, bool playing);
    bool ProcessInstrument(Vst3Context& ctx, AudioUnitPlugin& plugin,
                           float* outputLeft, float* outputRight, int numFrames,
                           int64_t sampleOffset, bool playing,
                           const std::array<float, 128>& desiredHeldVelocities,
                           std::array<float, 128>& lastHeldVelocities,
                           const std::vector<Microtonal::Voice>* tunedNotes = nullptr, int tuningMode = 0);
    void OpenPluginUI(AudioUnitPlugin& plugin);
    bool SetNormalizedParam(AudioUnitPlugin& plugin, uint32_t id, double normalized);
    double GetNormalizedParam(const AudioUnitPlugin& plugin, uint32_t id);
    bool SaveState(const AudioUnitPlugin& plugin, std::vector<uint8_t>& out);
    bool LoadState(AudioUnitPlugin& plugin, const uint8_t* data, size_t size);
}

namespace AudioUnitSystemLogic {
    inline Microtonal::Output TuningOutput(const AudioUnitPlugin& plugin, int mode) { return Microtonal::ResolveOutput(mode, false, mode == 2); }
}
namespace AudioUnitSystemLogic {void Reset(AudioUnitPlugin&);void Reconfigure(Vst3Context&,AudioUnitPlugin&);}
