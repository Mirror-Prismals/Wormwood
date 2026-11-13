#pragma once
#include "Microtonal.h"
#include "ParameterMailbox.h"

#include <array>
#include <atomic>
#include <cstdint>
#include <cstddef>
#include <memory>
#include <string>
#include <thread>
#include <vector>

#include <clap/clap.h>

struct Vst2HostedInstance;
struct DawContext;
struct Vst3Context;
struct Vst3AvailablePlugin;
struct Vst3UiWindow;

enum class HostedPluginFormat : uint8_t {
    Vst3 = 0,
    Clap = 1,
    AudioUnit = 2,
    Vst2 = 3,
};

struct ClapLibrary {
    std::string path;
    std::string binaryPath;
    void* handle = nullptr;
    const clap_plugin_entry_t* entry = nullptr;
    const clap_plugin_factory_t* factory = nullptr;
    bool initialized = false;
};
struct ClapParamInfo {
    clap_id id = CLAP_INVALID_ID;
    uint32_t flags = 0;
    void* cookie = nullptr;
    std::string name;
    std::string module;
    double minValue = 0.0;
    double maxValue = 1.0;
    double defaultValue = 0.0;
};

struct ClapPendingParam {
    clap_id id = CLAP_INVALID_ID;
    void* cookie = nullptr;
    double value = 0.0;
};

struct ClapEventStorage {
    alignas(8) std::array<std::byte, 128> bytes{};

    clap_event_header_t* header() {
        return reinterpret_cast<clap_event_header_t*>(bytes.data());
    }
    const clap_event_header_t* header() const {
        return reinterpret_cast<const clap_event_header_t*>(bytes.data());
    }
};
struct ClapPortBuffer {
    uint32_t channels = 0;
    std::vector<float> storage;
    std::vector<float*> pointers;
};

struct ClapTimerRegistration {
    clap_id id = CLAP_INVALID_ID;
    uint32_t periodMs = 0;
    double nextFireSeconds = 0.0;
};

struct ClapPlugin {
    std::vector<Microtonal::ActiveVoice> tuningVoices;
    int nextTuningVoiceId = 1024;
    bool tuningMpeZoneActive = false;
    HostedPluginFormat format = HostedPluginFormat::Clap;
    std::shared_ptr<Vst2HostedInstance> vst2;
    std::string name;
    std::string id;
    std::string vendor;
    std::string path;
    Vst3Context* owner = nullptr;
    ClapLibrary* library = nullptr;
    const clap_plugin_t* plugin = nullptr;
    clap_host_t host{};

    const clap_plugin_audio_ports_t* audioPorts = nullptr;
    const clap_plugin_note_ports_t* notePorts = nullptr;
    const clap_plugin_params_t* params = nullptr;
    const clap_plugin_state_t* state = nullptr;
    const clap_plugin_gui_t* gui = nullptr;
    const clap_plugin_timer_support_t* timerSupport = nullptr;
    Vst3UiWindow* uiWindow = nullptr;
    bool guiCreated = false;
    bool uiVisible = false;
    bool usingDefaultEditor = false;
    bool isInstrument = false;
    bool active = false;
    bool processing = false;
    std::atomic<bool> bypassed{false};

    int inputChannels = 0;
    int outputChannels = 0;
    int inputBusses = 0;
    int outputBusses = 0;
    uint32_t noteDialect = CLAP_NOTE_DIALECT_CLAP;
    uint32_t supportedNoteDialects = 0;

    std::vector<ClapPortBuffer> inputPortBuffers;
    std::vector<ClapPortBuffer> outputPortBuffers;
    std::vector<clap_audio_buffer_t> inputAudioBuffers;
    std::vector<clap_audio_buffer_t> outputAudioBuffers;
    std::vector<ClapParamInfo> parameterInfo;

    std::vector<ClapEventStorage> inputEvents;
    clap_input_events_t inputEventList{};
    clap_output_events_t outputEventList{};

    NormalizedParameterMailbox pendingParameters;
    std::atomic<bool> callbackRequested{false};
    std::atomic<bool> restartRequested{false};
    std::atomic<bool> processRequested{false};
    std::atomic<bool> paramFlushRequested{false};
    std::atomic<bool> stateDirty{false};
    std::atomic<bool> guiResizePending{false};
    std::atomic<uint32_t> guiResizeWidth{0};
    std::atomic<uint32_t> guiResizeHeight{0};
    std::atomic<int> guiVisibilityRequest{0};
    std::atomic<bool> guiClosedByPlugin{false};

    std::vector<ClapTimerRegistration> timers;
    clap_id nextTimerId = 1;
    uint64_t mainThreadHash = 0;
    std::atomic<uint64_t> audioThreadHash{0};
};

namespace ClapSystemLogic {
    void Initialize(Vst3Context& ctx, const std::string& executableDir, const std::string& userRoot);
    void Cleanup(Vst3Context& ctx);
    void UpdateMainThread(Vst3Context& ctx);

    ClapPlugin* CreatePlugin(Vst3Context& ctx,
                             const Vst3AvailablePlugin& available,
                             bool instrument);
    void DestroyPlugin(Vst3Context& ctx, ClapPlugin* plugin);
    void OpenPluginUI(ClapPlugin& plugin);
    bool ProcessEffect(Vst3Context& ctx,
                       ClapPlugin& plugin,
                       const float* inputLeft,
                       const float* inputRight,
                       float* outputLeft,
                       float* outputRight,
                       int numFrames,
                       int64_t sampleOffset,
                       bool playing);

    bool ProcessInstrument(Vst3Context& ctx,
                           ClapPlugin& plugin,
                           float* outputLeft,
                           float* outputRight,
                           int numFrames,
                           int64_t sampleOffset,
                           bool playing,
                           const std::array<float, 128>& desiredHeldVelocities,
                           std::array<float, 128>& lastHeldVelocities,
                           const std::vector<Microtonal::Voice>* tunedNotes = nullptr, int tuningMode = 0);

    bool QueueNormalizedParam(ClapPlugin& plugin, clap_id id, double normalized);
    double GetNormalizedParam(const ClapPlugin& plugin, clap_id id);
    bool SaveState(const ClapPlugin& plugin, std::vector<uint8_t>& out);
    bool LoadState(ClapPlugin& plugin, const uint8_t* data, size_t size);
    const ClapParamInfo* FindParam(const ClapPlugin& plugin, clap_id id);
}

namespace ClapSystemLogic {
    inline Microtonal::Output TuningOutput(const ClapPlugin& plugin, int mode) { return Microtonal::ResolveOutput(mode, (plugin.supportedNoteDialects & CLAP_NOTE_DIALECT_CLAP), (plugin.supportedNoteDialects & CLAP_NOTE_DIALECT_MIDI_MPE) || (mode == 2 && (plugin.supportedNoteDialects & CLAP_NOTE_DIALECT_MIDI))); }
}
