#include "Host/Vst2Host.h"
#include "Host/Vst2AudioBridge.h"
#include "Host/Vst2EditorBridge.h"
#include "Host/Vst3Host.h"
#include <filesystem>
#include <algorithm>
#include <future>
#include <mutex>
#include <iostream>
#include <cmath>
#include <chrono>
#include <cstdlib>
#include <deque>

struct Vst2HostedInstance {
    using Json = Vst2Worker::Json;
    struct Loaded { std::unique_ptr<Vst2Worker> worker; Json metadata, state; std::string error; };
    std::future<Loaded> loading;
    Vst2AudioBridge bridge;
    std::atomic<bool> ready{false};
    std::unique_ptr<std::atomic<float>[]> values;
    int count = 0, frames = 0;
    std::mutex stateMutex;
    Json pendingState, baseState;
    Json parameterMetadata = Json::array();
    bool loadFinished = false, faultReported = false;
    std::string root, error;
    std::unique_ptr<Vst2EditorBridge> editor;
    Vst3UiWindow* originalWindow = nullptr;
    std::atomic<bool> editorDirty{false};
    bool editorFailed = false;
    bool originalEditorAvailable = false;
    bool editorInputActive = false;
    std::deque<Json> editorMouseQueue;
};

namespace Vst2Host {
namespace {
std::string backendRoot;
void updateOriginalEditor(ClapPlugin& plugin);
void openOriginalEditor(void* userData);
void updateParameterText(ClapPlugin& plugin, const Vst2Worker::Json& item) {
    const int id = item.at("id").get<int>();
    std::string name = item.value("name", "");
    if (name.empty() || name == "<unknown>") name = "Unlabeled parameter " + std::to_string(id + 1);
    const auto display = item.value("display", "");
    const auto units = item.value("units", "");
    const std::string text = name + (display.empty() ? "" : "\n" + display + (units.empty() ? "" : " " + units));
    Vst3UI_UpdateDefaultParameterText(plugin.uiWindow, id, text.c_str());
}
Vst2Worker::Json editorParameters(const Vst2HostedInstance& state) {
    auto parameters = Vst2Worker::Json::array();
    for (int i = 0; i < state.count; ++i) parameters.push_back({i, state.values[i].load(std::memory_order_acquire)});
    return parameters;
}
void showParameters(void* userData) {
    auto& plugin = *static_cast<ClapPlugin*>(userData);
    ClapSystemLogic::OpenPluginUI(plugin);
    if (plugin.uiWindow) Vst3UI_ShowWindow(plugin.uiWindow);
}
void submitEditorMouse(Vst2HostedInstance& state) {
    if (!state.editor || state.editorMouseQueue.empty()) return;
    auto request = state.editorMouseQueue.front();
    // Snapshot only after the preceding reply has been applied to audio, so
    // queued input cannot reset the knob to an older host value.
    request["parameters"] = editorParameters(state);
    if (state.editor->Submit(std::move(request))) state.editorMouseQueue.pop_front();
}
void editorGesture(void* userData, double x, double y, int phase) {
    auto& plugin = *static_cast<ClapPlugin*>(userData);
    auto& state = *plugin.vst2;
    if (!state.editor) return;
    if (phase == 0) {
        if (state.editorInputActive) return;
        state.editorInputActive = true;
        Vst3UI_SetBitmapEditorBusy(state.originalWindow, true);
    } else if (!state.editorInputActive) return;
    const char* kind = phase == 0 ? "down" : phase == 1 ? "drag" : "up";
    Vst2Worker::Json request = {{"op", "mouse"}, {"phase", kind}, {"coordinates", {x, y}}};
    if (phase == 1 && !state.editorMouseQueue.empty() && state.editorMouseQueue.back().value("phase", "") == "drag")
        state.editorMouseQueue.back() = std::move(request);
    else state.editorMouseQueue.push_back(std::move(request));
    submitEditorMouse(state);
}
void openOriginalEditor(void* userData) {
    auto& plugin = *static_cast<ClapPlugin*>(userData);
    auto& state = *plugin.vst2;
    if (state.originalWindow) { Vst3UI_ShowWindow(state.originalWindow); return; }
    if (state.editor || !state.originalEditorAvailable || !state.ready.load(std::memory_order_acquire)) return;
    const auto root = std::filesystem::path(state.root);
    Vst2Worker::Json libraries = Vst2Worker::Json::array();
    const auto legacy = root / "build/vst2-compat/legacy-cpp/cardinal-legacy-cpp.dylib";
    if (std::filesystem::exists(legacy)) libraries.push_back(legacy.string());
    Vst2Worker::Json request = {{"op", "open"}, {"plugin", plugin.path}, {"architecture", "auto"},
        {"optional_x86_64_libraries", libraries},
        {"sample_rate", plugin.owner ? plugin.owner->sampleRate : 48000.}, {"block_size", state.frames},
        {"parameters", editorParameters(state)}, {"startup_seconds", 15}};
    { std::lock_guard<std::mutex> lock(state.stateMutex); request["state"] = state.baseState; }
    // Generic optional mirror of plugin resource paths, not a plugin-specific shim.
    const char* overrideRoot = std::getenv("CARDINAL_VST2_RESOURCE_ROOT");
    const auto resources = overrideRoot && *overrideRoot ? std::filesystem::path(overrideRoot)
                                                        : root / "USER/USER_VST2_RESOURCES";
    if (std::filesystem::is_directory(resources)) request["resource_root"] = resources.string();
    try {
        state.editor = std::make_unique<Vst2EditorBridge>((root / "build/vst2-compat/venv/bin/python").string(),
            (root / "Platform/Vst2Compat/editor_worker.py").string(), std::move(request));
        state.editorFailed = false;
        state.editorInputActive = false;
        state.editorMouseQueue.clear();
    } catch (const std::exception& e) {
        state.editorFailed = true;
        std::cerr << "VST2 original editor: " << e.what() << '\n';
    }
    ConfigureEditor(plugin);
}

std::vector<Vst3EditorMenuItem> decodeEditorMenu(const Vst2Worker::Json& items, size_t& total, unsigned depth = 0) {
    if (!items.is_array() || depth >= 8 || total + items.size() > 2048)
        throw std::runtime_error("Invalid original editor menu size");
    total += items.size();
    std::vector<Vst3EditorMenuItem> result;
    for (const auto& item : items) {
        Vst3EditorMenuItem entry;
        entry.title = item.at("title").get<std::string>();
        if (entry.title.size() > 4096) throw std::runtime_error("Oversized original editor menu title");
        entry.selection = item.at("selection").get<uint32_t>();
        entry.enabled = item.at("enabled").get<bool>();
        entry.checked = item.at("checked").get<bool>();
        entry.separator = item.at("separator").get<bool>();
        if (item.contains("children")) entry.children = decodeEditorMenu(item.at("children"), total, depth + 1);
        result.push_back(std::move(entry));
    }
    return result;
}

void updateOriginalEditor(ClapPlugin& plugin) {
    auto& state = *plugin.vst2;
    if (!state.editor) return;
    if (state.originalWindow && !Vst3UI_IsWindowVisible(state.originalWindow)) {
        Vst3UI_CloseWindow(state.originalWindow); state.originalWindow = nullptr;
        state.editor.reset(); state.editorMouseQueue.clear(); state.editorInputActive = false;
        ConfigureEditor(plugin); return;
    }
    Vst2EditorBridge::Reply reply;
    if (state.editor->Poll(reply)) {
        try {
            if (!reply.error.empty()) throw std::runtime_error(reply.error);
            const auto operation = reply.sent.value("op", "");
            if (operation == "click" || operation == "drag" || operation == "mouse" || operation == "menu") {
                for (const auto& changed : reply.result.at("parameters")) {
                    const int index = changed.at(0).get<int>();
                    const double value = changed.at(1).get<double>();
                    if (index < 0 || index >= state.count || !std::isfinite(value) || value < 0 || value > 1)
                        throw std::runtime_error("Invalid editor parameter response");
                    const float sent = reply.sent.at("parameters").at(index).at(1).get<float>();
                    // A newer generic/automation change wins over an in-flight gesture.
                    if (state.values[index].load(std::memory_order_acquire) != sent) {
                        state.editorDirty.store(true); continue;
                    }
                    SetParameter(plugin, static_cast<uint32_t>(index), value);
                    Vst3UI_UpdateDefaultParameter(plugin.uiWindow, index, value);
                }
            }
            if (operation == "menu" || (operation == "mouse" && reply.sent.value("phase", "") == "up")) state.editorInputActive = false;
            const bool created = !state.originalWindow;
            if (created) state.originalWindow = Vst3UI_CreateBitmapEditor((plugin.name + " - Original UI").c_str(),
                editorGesture, showParameters, &plugin);
            if (!state.originalWindow || !Vst3UI_UpdateBitmapEditor(state.originalWindow, reply.png.data(), reply.png.size()))
                throw std::runtime_error("Invalid original editor bitmap");
            Vst3UI_SetBitmapEditorBusy(state.originalWindow, state.editorInputActive);
            if (created) Vst3UI_ShowWindow(state.originalWindow);
            ConfigureEditor(plugin);
            if (reply.result.contains("menu")) {
                const auto& menu = reply.result.at("menu");
                size_t total = 0;
                auto entries = decodeEditorMenu(menu.at("items"), total);
                const double x = menu.at("x").get<double>(), y = menu.at("y").get<double>();
                if (!std::isfinite(x) || !std::isfinite(y)) throw std::runtime_error("Invalid menu coordinates");
                state.editorMouseQueue.clear(); // Native popup consumes this click's release.
                state.editorInputActive = true;
                const auto selection = Vst3UI_ShowBitmapEditorMenu(state.originalWindow, entries, x, y);
                state.editorMouseQueue.push_back({{"op", "menu"}, {"selection", selection}});
            }
        } catch (const std::exception& e) {
            std::cerr << "VST2 original editor unavailable: " << e.what() << "; generic editor retained\n";
            if (state.originalWindow) { Vst3UI_CloseWindow(state.originalWindow); state.originalWindow = nullptr; }
            state.editor.reset(); state.editorFailed = true;
            state.editorMouseQueue.clear(); state.editorInputActive = false;
            showParameters(&plugin); ConfigureEditor(plugin); return;
        }
    }
    submitEditorMouse(state);
    if (state.originalWindow && !state.editorInputActive && state.editorMouseQueue.empty() && state.editorDirty.exchange(false)) {
        if (!state.editor->Submit({{"op", "frame"}, {"parameters", editorParameters(state)}})) state.editorDirty.store(true);
    }
}
void requireRequest(Vst2Worker& worker, Vst2Worker::Json request, Vst2Worker::Json& result, int timeout = 5000) {
    std::vector<uint8_t> output;
    std::string error;
    if (!worker.Exchange(std::move(request), {}, result, output, timeout, error)) throw std::runtime_error(error);
}
}

void Discover(Vst3Context& ctx, const std::string& root, const std::string& userRoot) {
    if constexpr (!SalamanderPlatform::desktopPluginHosting) return;
    auto resolved = std::filesystem::absolute(root);
    for (auto candidate = resolved; !candidate.empty(); candidate = candidate.parent_path()) {
        if (std::filesystem::exists(candidate / "Platform/Vst2Compat/worker.py")) { resolved = candidate; break; }
        if (candidate == candidate.parent_path()) break;
    }
    backendRoot = resolved.string();
#if defined(__APPLE__) && defined(__aarch64__)
    const auto folder = std::filesystem::path(userRoot) / "USER_VST2";
    std::error_code error;
    for (std::filesystem::directory_iterator it(folder, error), end; !error && it != end; it.increment(error)) {
        const auto path = it->path();
        if (path.extension() != ".vst" || !it->is_directory(error)) continue;
        Vst3AvailablePlugin item;
        item.format = HostedPluginFormat::Vst2;
        item.name = path.stem().string() + " [VST2]";
        item.modulePath = path.string();
        item.uniqueId = "vst2:" + path.filename().string();
        item.vendor = "VST2 compatibility worker";
        item.isInstrument = false; // Actual capability is validated in the isolated loader.
        ctx.availablePlugins.push_back(std::move(item));
    }
#endif
}

ClapPlugin* Create(Vst3Context& ctx, const Vst3AvailablePlugin& available, bool instrument) {
    if constexpr (!SalamanderPlatform::desktopPluginHosting) return nullptr;
    if (instrument || available.format != HostedPluginFormat::Vst2 || ctx.blockSize < 1) return nullptr;
if(!std::filesystem::exists(std::filesystem::path(backendRoot)/"build/vst2-compat/venv/bin/python"))
    throw std::runtime_error("VST2 compatibility worker is not installed. Run BuildSupport/setup_vst2_worker.sh; VST3, CLAP and Audio Units do not require it.");
auto owned = std::make_unique<ClapPlugin>();

    auto& plugin = *owned;
    plugin.format = HostedPluginFormat::Vst2;
    plugin.name = available.name;
    plugin.path = available.modulePath;
    plugin.id = available.uniqueId;
    plugin.vendor = available.vendor;
    plugin.owner = &ctx;
    plugin.vst2 = std::make_shared<Vst2HostedInstance>();
    auto state = plugin.vst2;
    state->root = backendRoot;
    state->frames = ctx.blockSize;
    const std::string path = plugin.path;
    const double rate = ctx.sampleRate;
    // Capturing a weak reference avoids a future/shared-state ownership cycle.
    std::weak_ptr<Vst2HostedInstance> weak = state;
    state->loading = std::async(std::launch::async, [weak, path, rate] {
        Vst2HostedInstance::Loaded loaded;
        auto state = weak.lock();
        if (!state) return loaded;
        try {
            loaded.worker = std::make_unique<Vst2Worker>();
            std::string error;
            const auto root = std::filesystem::path(state->root);
            if (!loaded.worker->Start((root / "build/vst2-compat/venv/bin/python").string(),
                    (root / "Platform/Vst2Compat/worker.py").string(), error)) throw std::runtime_error(error);
            Vst2Worker::Json libraries = Vst2Worker::Json::array();
            auto legacy = root / "build/vst2-compat/legacy-cpp/cardinal-legacy-cpp.dylib";
            if (std::filesystem::exists(legacy)) libraries.push_back(legacy.string());
            Vst2Worker::Json request = {{"op", "open"}, {"plugin", path}, {"architecture", "auto"},
                {"optional_x86_64_libraries", libraries},
                {"sample_rate", rate}, {"block_size", state->frames}, {"experimental", true},
                {"startup_seconds", 15}};
            const char* overrideRoot = std::getenv("CARDINAL_VST2_RESOURCE_ROOT");
            const auto resources = overrideRoot && *overrideRoot ? std::filesystem::path(overrideRoot)
                                                                : root / "USER/USER_VST2_RESOURCES";
            if (std::filesystem::is_directory(resources)) request["resource_root"] = resources.string();
            requireRequest(*loaded.worker, std::move(request), loaded.metadata, 60000);
            if (loaded.metadata.at("inputs") != 2 || loaded.metadata.at("outputs") != 2 || loaded.metadata.at("is_instrument") == true)
                throw std::runtime_error("This VST2 preview supports stereo effects only");
            if (loaded.metadata.at("parameters").size() > 4096) throw std::runtime_error("Too many VST2 parameters");
            Vst2Worker::Json result;
            requireRequest(*loaded.worker, {{"op", "save_state"}}, result);
            loaded.state = result.at("state");
        } catch (const std::exception& e) { loaded.error = e.what(); loaded.worker.reset(); }
        return loaded;
    });
    ctx.clapPlugins.push_back(std::move(owned));
    return ctx.clapPlugins.back().get();
}

void Update(ClapPlugin& plugin) {
    auto& state = *plugin.vst2;
    if (!state.loadFinished) {
        if (!state.loading.valid() || state.loading.wait_for(std::chrono::seconds(0)) != std::future_status::ready) return;
        auto loaded = state.loading.get();
        state.loadFinished = true;
        state.error = loaded.error;
        if (loaded.worker && state.error.empty()) {
            try {
                // Publication occurs under the same track lock as existing host mutation.
                std::unique_lock<std::mutex> trackLock;
                std::lock_guard<std::mutex> stateLock(state.stateMutex);
                if (!state.pendingState.is_null()) {
                    Vst2Worker::Json result;
                    requireRequest(*loaded.worker, {{"op", "load_state"}, {"state", state.pendingState.at("base")}}, result);
                    for (const auto& parameter : state.pendingState.at("parameters"))
                        requireRequest(*loaded.worker, {{"op", "set_parameter"}, {"index", parameter.at("id")}, {"value", parameter.at("value")}}, result);
                    requireRequest(*loaded.worker, {{"op", "parameters"}}, result);
                    loaded.metadata["parameters"] = result.at("parameters");
                    loaded.state = state.pendingState.at("base");
                }
                state.baseState = loaded.state;
                const auto architecture = loaded.metadata.value("architecture", "x86_64");
                state.originalEditorAvailable = architecture == "x86_64" || architecture == "i386";
                state.count = static_cast<int>(loaded.metadata.at("parameters").size());
                state.parameterMetadata = loaded.metadata.at("parameters");
                state.values = std::make_unique<std::atomic<float>[]>(state.count);
                plugin.parameterInfo.clear();
                for (const auto& parameter : loaded.metadata.at("parameters")) {
                    ClapParamInfo info;
                    info.id = parameter.at("id").get<uint32_t>();
                    if (info.id >= static_cast<uint32_t>(state.count)) throw std::runtime_error("Invalid VST2 parameter index");
                    info.name = parameter.value("name", "Parameter " + std::to_string(info.id));
                    info.defaultValue = parameter.at("value");
                    info.minValue = 0; info.maxValue = 1;
                    state.values[info.id].store(static_cast<float>(info.defaultValue));
                    plugin.parameterInfo.push_back(std::move(info));
                }
                if (!state.bridge.Start(std::move(loaded.worker), state.frames, state.count)) throw std::runtime_error("Cannot start VST2 audio bridge");
                plugin.inputChannels = plugin.outputChannels = 2;
                state.ready.store(true, std::memory_order_release);
            } catch (const std::exception& e) { state.error = e.what(); }
        }
        if (plugin.uiWindow) { Vst3UI_CloseWindow(plugin.uiWindow); plugin.uiWindow = nullptr; }
        if (!state.error.empty()) {
            std::cerr << "VST2: " << plugin.name << ": " << state.error << '\n';
            plugin.name += " (load failed)";
        }
        ClapSystemLogic::OpenPluginUI(plugin);
    }
    if (state.ready.load(std::memory_order_acquire) && state.bridge.Failed() && !state.faultReported) {
        state.faultReported = true;
        plugin.name += " (worker fault; dry bypass)";
        std::cerr << "VST2: worker fault for " << plugin.path << ": " << state.bridge.FailureReason() << "; remove/reload to restart\n";
        if (plugin.uiWindow) { Vst3UI_CloseWindow(plugin.uiWindow); plugin.uiWindow = nullptr; }
        ClapSystemLogic::OpenPluginUI(plugin);
    }
    for (const auto& item : state.bridge.TakeParameterInfo()) {
        const int id = item.at("id").get<int>();
        if (id >= 0 && id < state.count) {
            state.parameterMetadata.at(id) = item;
            updateParameterText(plugin, item);
        }
    }
    updateOriginalEditor(plugin);
}

void ConfigureEditor(ClapPlugin& plugin) {
    auto& state = *plugin.vst2;
    for (const auto& item : state.parameterMetadata) updateParameterText(plugin, item);
    if (state.bridge.Failed()) {
        const auto message = std::string("Plugin audio stopped; dry bypass.\n") + state.bridge.FailureDetail()
            + "\nRemove and reload the plugin to restart it.";
        Vst3UI_SetDefaultEditorError(plugin.uiWindow, message.c_str());
    }
    if (state.loadFinished && !state.originalEditorAvailable) {
        Vst3UI_SetEditorAction(plugin.uiWindow, "Original UI unavailable", false, nullptr, nullptr);
        return;
    }
    const bool loading = state.editor && !state.originalWindow;
    Vst3UI_SetEditorAction(plugin.uiWindow, loading ? "Loading original UI..." : state.editorFailed ? "Retry original UI" : "Original UI",
        state.ready.load(std::memory_order_acquire) && !loading, openOriginalEditor, &plugin);
}

std::string EditorTitle(const ClapPlugin& plugin) {
    const auto& state = *plugin.vst2;
    if (!state.loadFinished) return plugin.name + " - loading";
    if (!state.error.empty()) return plugin.name + " - " + state.error;
    if (state.bridge.Failed()) return plugin.name + " - " + state.bridge.FailureReason();
    const double rate = plugin.owner ? plugin.owner->sampleRate : 48000.0;
    return plugin.name + " - " + std::to_string(static_cast<int>(std::ceil(3000.0 * state.frames / rate))) + " ms bridge latency";
}

void Destroy(ClapPlugin& plugin) {
    if (plugin.uiWindow) { Vst3UI_CloseWindow(plugin.uiWindow); plugin.uiWindow = nullptr; }
    auto state = plugin.vst2;
    if (!state) return;
    if (state->originalWindow) { Vst3UI_CloseWindow(state->originalWindow); state->originalWindow = nullptr; }
    state->editor.reset();
    state->ready.store(false, std::memory_order_release);
    // Caller already excludes audio processing with the track lock.
    auto worker = state->bridge.Stop();
    if (worker && worker->Running()) { std::string error; worker->Close(error); }
    if (state->loading.valid()) { auto loaded = state->loading.get(); }
    plugin.vst2.reset();
}

bool Process(ClapPlugin& plugin, const float* left, const float* right, float* outL, float* outR, int frames) {
    auto& state = *plugin.vst2;
    if (!state.ready.load(std::memory_order_acquire) || frames < 1 || frames > state.frames) return false;
    state.bridge.SetBypassed(plugin.bypassed.load(std::memory_order_relaxed));
    (void)state.bridge.ProcessStream(left, right ? right : left, outL, outR, frames);
    return true; // Fault output is deliberately delayed dry, not stale wet samples.
}
bool SetParameter(ClapPlugin& plugin, uint32_t index, double value) {
    auto& state = *plugin.vst2;
    if (!state.ready.load(std::memory_order_acquire) || index >= static_cast<uint32_t>(state.count) || !std::isfinite(value)) return false;
    value = std::clamp(value, 0.0, 1.0);
    if (!state.bridge.QueueParameter(index, static_cast<float>(value))) return false;
    const float previous = state.values[index].exchange(static_cast<float>(value), std::memory_order_acq_rel);
    if (previous != static_cast<float>(value)) state.editorDirty.store(true, std::memory_order_release);
    return true;
}
double GetParameter(const ClapPlugin& plugin, uint32_t index) {
    const auto& state = *plugin.vst2;
    return state.ready.load(std::memory_order_acquire) && index < static_cast<uint32_t>(state.count)
        ? state.values[index].load(std::memory_order_acquire) : 0.0;
}
bool SaveState(const ClapPlugin& plugin, std::vector<uint8_t>& out) {
    auto& state = *plugin.vst2;
    std::lock_guard<std::mutex> lock(state.stateMutex);
    Vst2Worker::Json saved;
    if (!state.ready.load(std::memory_order_acquire)) {
        if (state.pendingState.is_null()) return false;
        saved = state.pendingState;
    } else {
        saved = {{"version", 1}, {"base", state.baseState}, {"parameters", Vst2Worker::Json::array()}};
        for (int i = 0; i < state.count; ++i) saved["parameters"].push_back({{"id", i}, {"value", state.values[i].load()}});
    }
    auto bytes = saved.dump(); out.assign(bytes.begin(), bytes.end()); return true;
}
bool LoadState(ClapPlugin& plugin, const uint8_t* data, size_t size) {
    auto& state = *plugin.vst2;
    if (!data || size > 8 * 1024 * 1024 || state.ready.load(std::memory_order_acquire)) return false;
    try {
        auto saved = Vst2Worker::Json::parse(data, data + size);
        if (saved.at("version") != 1 || !saved.at("base").is_object() || !saved.at("parameters").is_array()) return false;
        std::lock_guard<std::mutex> lock(state.stateMutex);
        state.pendingState = std::move(saved); return true;
    } catch (...) { return false; }
}
}
