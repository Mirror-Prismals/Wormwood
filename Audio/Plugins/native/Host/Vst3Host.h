#pragma once
// Cardinal hosting structures, detached from its old document/UI framework.
#include "Microtonal.h"
#include "ClapHost.h"
#include "AudioUnitHost.h"
#include "ParameterMailbox.h"
#include <memory>
#include <string>
#include <vector>
#include "public.sdk/source/vst/hosting/eventlist.h"
#include "public.sdk/source/vst/hosting/hostclasses.h"
#include "public.sdk/source/vst/hosting/module.h"
#include "public.sdk/source/vst/hosting/parameterchanges.h"
#include "public.sdk/source/vst/hosting/plugprovider.h"
#include "public.sdk/source/vst/hosting/processdata.h"
#include "pluginterfaces/gui/iplugview.h"
#include "pluginterfaces/vst/ivstaudioprocessor.h"
#include "pluginterfaces/vst/ivstcomponent.h"
#include "pluginterfaces/vst/ivsteditcontroller.h"
namespace SalamanderPlatform { inline constexpr bool desktopPluginHosting = true; }
struct Vst3Plugin;
struct Vst3UiWindow;
struct SampleGalaxyAudioData;
struct Vst3DefaultEditorParam {
    int32_t id = 0;
    double value = 0.0;
    char title[128]{};
    char units[64]{};
};
using Vst3DefaultEditorSetParamFn = void (*)(void* userData, int32_t paramID, double value);

Vst3UiWindow* Vst3UI_CreateWindow(const char* title, int width, int height);
Vst3UiWindow* Vst3UI_CreateDefaultEditorWindow(const char* title,
                                               const Vst3DefaultEditorParam* params,
                                               int paramCount,
                                               Vst3DefaultEditorSetParamFn setParam,
                                               void* userData);
void Vst3UI_ShowWindow(Vst3UiWindow* window);
void Vst3UI_HideWindow(Vst3UiWindow* window);
// Main-thread native state, including an editor kept open in the Dock.
bool Vst3UI_IsWindowVisible(Vst3UiWindow* window);
void Vst3UI_CloseWindow(Vst3UiWindow* window);
void* Vst3UI_GetContentView(Vst3UiWindow* window);
void Vst3UI_SetWindowSize(Vst3UiWindow* window, int width, int height);
using Vst3EditorActionFn = void (*)(void*);
// phase: 0 mouse down, 1 drag, 2 mouse up; coordinates are bitmap pixels.
using Vst3EditorGestureFn = void (*)(void*, double, double, int);
void Vst3UI_SetEditorAction(Vst3UiWindow*, const char* label, bool enabled, Vst3EditorActionFn, void*);
void Vst3UI_UpdateDefaultParameter(Vst3UiWindow*, int32_t id, double value);
void Vst3UI_SetDefaultEditorError(Vst3UiWindow*, const char* message);
void Vst3UI_UpdateDefaultParameterText(Vst3UiWindow*, int32_t id, const char* text);
Vst3UiWindow* Vst3UI_CreateBitmapEditor(const char*, Vst3EditorGestureFn, Vst3EditorActionFn, void*);
bool Vst3UI_UpdateBitmapEditor(Vst3UiWindow*, const uint8_t*, size_t);
void Vst3UI_SetBitmapEditorBusy(Vst3UiWindow*, bool);

struct Vst3EditorMenuItem {
    std::string title;
    uint32_t selection = 0;
    bool enabled = true, checked = false, separator = false;
    std::vector<Vst3EditorMenuItem> children;
};
uint32_t Vst3UI_ShowBitmapEditorMenu(Vst3UiWindow*, const std::vector<Vst3EditorMenuItem>&, double x, double y);

struct Vst3Plugin {
    std::vector<Microtonal::ActiveVoice> tuningVoices;
    int nextTuningVoiceId = 1024;
    bool tuningMpeZoneActive = false;
    bool supportsNoteTuning = false;
    Steinberg::Vst::ParamID channelPitchBend = Steinberg::Vst::kNoParamId;
    bool supportsMpeMapping = false;
NormalizedParameterMailbox pending;
NormalizedParameterMailbox controllerUpdates;
std::vector<uint32_t> writableParameters;
Steinberg::IPtr<Steinberg::Vst::IComponentHandler> handler;
std::string name;

    VST3::Hosting::Module::Ptr module;
    VST3::Hosting::ClassInfo classInfo;
    Steinberg::IPtr<Steinberg::Vst::PlugProvider> provider;
    Steinberg::IPtr<Steinberg::Vst::IComponent> component;
    Steinberg::IPtr<Steinberg::Vst::IEditController> controller;
    Steinberg::FUnknownPtr<Steinberg::Vst::IAudioProcessor> processor;
    Steinberg::IPtr<Steinberg::IPlugView> view;
    Steinberg::Vst::HostProcessData processData;
    // Stable backing for every declared bus/channel, including sidechains and
    // channels beyond stereo. Main-thread allocation; no callback growth.
    std::vector<std::vector<float>> safeBusBuffers;
    int safeBufferFrames = 0;
    std::atomic<bool> realtimeResetRequested{false};
    std::atomic<Steinberg::int32> restartFlags{0};
    Steinberg::Vst::EventList eventList;
    Steinberg::Vst::ParameterChanges inputParameterChanges;
    Steinberg::Vst::ProcessContext processContext{};
    Vst3UiWindow* uiWindow = nullptr;
    bool uiVisible = false;
    bool usingDefaultEditor = false;
    int inputChannels = 0;
    int outputChannels = 0;
    int inputBusses = 0;
    int outputBusses = 0;
    bool isInstrument = false;
    bool active = false;
    std::atomic<bool> bypassed{false};
};

struct Vst3AvailablePlugin {
    HostedPluginFormat format = HostedPluginFormat::Vst3;
    std::string name;
    std::string vendor;
    std::string uniqueId;
    std::string modulePath;
    VST3::Hosting::Module::Ptr module;
    VST3::Hosting::ClassInfo classInfo;
    bool isInstrument = false;
    uint32_t audioUnitType = 0;
    uint32_t audioUnitSubtype = 0;
    uint32_t audioUnitManufacturer = 0;
    void* audioUnitComponent = nullptr;
};

struct Vst3Context {
    float sampleRate = 48000;
    int blockSize = 2048;
    int64_t continuousSamples = 0;
    double processClockBpm = 120, processClockQuarterNotes = 0;
    double processPitchBend = -1; // normalized channel-1 wheel, -1 skips unchanged zero state
    int processClockNumerator = 4, processClockDenominator = 4;
    bool browserCacheBuilt = false;
    std::vector<std::unique_ptr<Vst3Plugin>> plugins;
    std::vector<std::unique_ptr<ClapPlugin>> clapPlugins;
    std::vector<std::unique_ptr<ClapLibrary>> clapLibraries;
    std::vector<std::unique_ptr<AudioUnitPlugin>> audioUnitPlugins;
    std::vector<Vst3AvailablePlugin> availablePlugins;
    std::vector<VST3::Hosting::Module::Ptr> modules;
    Steinberg::IPtr<Steinberg::Vst::HostApplication> hostApp;
};
namespace Vst3SystemLogic {
    void Discover(Vst3Context&, const std::string&);
    Vst3Plugin* Create(Vst3Context&, const Vst3AvailablePlugin&);
    void Destroy(Vst3Context&, Vst3Plugin*);
    void Vst3_OpenPluginUI(Vst3Plugin&);
    void Parameter(Vst3Plugin&, uint32_t, double);
    bool Process(Vst3Context&, Vst3Plugin&, const float*, const float*, float*, float*, int, int64_t, bool,
        const std::array<float,128>&, std::array<float,128>&, const std::vector<Microtonal::Voice>* = nullptr);
    bool SaveState(Vst3Plugin&, std::vector<uint8_t>&);
    bool LoadState(Vst3Plugin&, const uint8_t*, size_t);
    // Main/control thread, with processing quiesced by the bridge owner.
    bool Service(Vst3Context&, Vst3Plugin&);
    bool Reconfigure(Vst3Context&, Vst3Plugin&);
    bool Automate(Vst3Plugin&, uint32_t, double);
}
