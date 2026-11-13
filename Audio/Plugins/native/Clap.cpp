#include "Host/ClapHost.h"
#include "Host/Vst2Host.h"
#include "Host/Vst3Host.h"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <iostream>
#include <limits>
#include <unordered_set>

#if defined(_WIN32)
#include <windows.h>
#else
#include <dlfcn.h>
#endif

namespace ClapSystemLogic {
namespace {
    constexpr uint32_t kMaxEventBytes = 128;

    uint64_t currentThreadHash() {
        return static_cast<uint64_t>(std::hash<std::thread::id>{}(std::this_thread::get_id()));
    }
    double nowSeconds() {
        using Clock = std::chrono::steady_clock;
        return std::chrono::duration<double>(Clock::now().time_since_epoch()).count();
    }

    bool descriptorHasFeature(const clap_plugin_descriptor_t* desc, const char* feature) {
        if (!desc || !desc->features || !feature) return false;
        for (const char* const* it = desc->features; *it; ++it) {
            if (std::strcmp(*it, feature) == 0) return true;
        }
        return false;
    }

    std::vector<std::filesystem::path> clapSearchRoots(const std::string& executableDir) {
        std::vector<std::filesystem::path> roots;
        const std::filesystem::path userClap = std::filesystem::path(executableDir) / "USER_CLAP";
        roots.push_back(userClap);
        return roots;
    }

    std::vector<std::filesystem::path> scanClapBundles(const std::vector<std::filesystem::path>& roots) {
        std::vector<std::filesystem::path> bundles;
        std::unordered_set<std::string> seen;
        for (const auto& root : roots) {
            std::error_code ec;
            if (!std::filesystem::exists(root, ec) || !std::filesystem::is_directory(root, ec)) continue;
            std::filesystem::recursive_directory_iterator it(root,
                std::filesystem::directory_options::skip_permission_denied, ec);
            const std::filesystem::recursive_directory_iterator end;
            for (; it != end; it.increment(ec)) {
                if (ec) { ec.clear(); continue; }
                const auto& path = it->path();
                if (path.extension() != ".clap") continue;
#if defined(__APPLE__)
                if (!it->is_directory(ec)) continue;
                it.disable_recursion_pending();
#else
                if (!it->is_regular_file(ec)) continue;
#endif
                std::string canonical = std::filesystem::weakly_canonical(path, ec).string();
                if (canonical.empty()) canonical = path.string();
                if (seen.insert(canonical).second) bundles.push_back(path);
            }
        }
        std::sort(bundles.begin(), bundles.end());
        return bundles;
    }

    std::filesystem::path resolveClapBinary(const std::filesystem::path& bundle) {
#if defined(__APPLE__)
        const auto macosDir = bundle / "Contents/MacOS";
        std::error_code ec;
        auto preferred = macosDir / bundle.stem();
        if (std::filesystem::is_regular_file(preferred, ec)) return preferred;
        if (!std::filesystem::is_directory(macosDir, ec)) return {};
        for (const auto& entry : std::filesystem::directory_iterator(macosDir, ec)) {
            if (ec) break;
            if (entry.is_regular_file(ec)) return entry.path();
        }
        return {};
#else
        return bundle;
#endif
    }

    void* openDynamicLibrary(const std::filesystem::path& binary) {
#if defined(_WIN32)
        return reinterpret_cast<void*>(LoadLibraryW(binary.wstring().c_str()));
#else
        return dlopen(binary.c_str(), RTLD_NOW | RTLD_LOCAL);
#endif
    }

    void closeDynamicLibrary(void* handle) {
        if (!handle) return;
#if defined(_WIN32)
        FreeLibrary(reinterpret_cast<HMODULE>(handle));
#else
        dlclose(handle);
#endif
    }

    void* findDynamicSymbol(void* handle, const char* name) {
        if (!handle) return nullptr;
#if defined(_WIN32)
        return reinterpret_cast<void*>(GetProcAddress(reinterpret_cast<HMODULE>(handle), name));
#else
        return dlsym(handle, name);
#endif
    }
    uint32_t CLAP_ABI inputEventsSize(const clap_input_events_t* list) {
        auto* plugin = list ? static_cast<ClapPlugin*>(list->ctx) : nullptr;
        return plugin ? static_cast<uint32_t>(plugin->inputEvents.size()) : 0u;
    }

    const clap_event_header_t* CLAP_ABI inputEventsGet(const clap_input_events_t* list, uint32_t index) {
        auto* plugin = list ? static_cast<ClapPlugin*>(list->ctx) : nullptr;
        if (!plugin || index >= plugin->inputEvents.size()) return nullptr;
        return plugin->inputEvents[index].header();
    }

    bool CLAP_ABI outputEventsPush(const clap_output_events_t* list, const clap_event_header_t* event) {
        auto* plugin = list ? static_cast<ClapPlugin*>(list->ctx) : nullptr;
        if (!plugin || !event) return false;
        if (event->space_id == CLAP_CORE_EVENT_SPACE_ID
            && (event->type == CLAP_EVENT_PARAM_VALUE
                || event->type == CLAP_EVENT_PARAM_GESTURE_BEGIN
                || event->type == CLAP_EVENT_PARAM_GESTURE_END)) {
            plugin->stateDirty.store(true, std::memory_order_relaxed);
        }
        return true;
    }

    template <typename T>
    T* appendEvent(ClapPlugin& plugin, uint16_t type, uint32_t time = 0) {
        static_assert(sizeof(T) <= kMaxEventBytes, "CLAP event storage too small");
        plugin.inputEvents.emplace_back();
        T* event = reinterpret_cast<T*>(plugin.inputEvents.back().bytes.data());
        std::memset(event, 0, sizeof(T));
        event->header.size = sizeof(T);
        event->header.time = time;
        event->header.space_id = CLAP_CORE_EVENT_SPACE_ID;
        event->header.type = type;
        return event;
    }
    void refreshParameters(ClapPlugin& plugin) {
        plugin.parameterInfo.clear();
        if (!plugin.plugin || !plugin.params) return;
        const uint32_t count = plugin.params->count(plugin.plugin);
        if (count > 4096) throw std::runtime_error("CLAP parameter count exceeds 4096");
        plugin.parameterInfo.reserve(count);
        for (uint32_t i = 0; i < count; ++i) {
            clap_param_info_t info{};
            if (!plugin.params->get_info(plugin.plugin, i, &info)) continue;
            ClapParamInfo out;
            out.id = info.id;
            out.flags = info.flags;
            out.cookie = info.cookie;
            out.name = info.name;
            out.module = info.module;
            out.minValue = info.min_value;
            out.maxValue = info.max_value;
            out.defaultValue = info.default_value;
            plugin.parameterInfo.push_back(std::move(out));
        }
        std::vector<uint32_t> ids;
        for (const auto& info : plugin.parameterInfo) ids.push_back(info.id);
        plugin.pendingParameters.configure(ids);
        plugin.inputEvents.reserve(ids.size() + 256);
    }

    void preparePortBuffer(ClapPortBuffer& port, uint32_t channels, int blockSize) {
        port.channels = channels;
        port.storage.assign(static_cast<size_t>(channels) * static_cast<size_t>(std::max(1, blockSize)), 0.0f);
        port.pointers.resize(channels, nullptr);
        for (uint32_t c = 0; c < channels; ++c) {
            port.pointers[c] = port.storage.data() + static_cast<size_t>(c) * static_cast<size_t>(std::max(1, blockSize));
        }
    }

    bool refreshAudioPorts(ClapPlugin& plugin, int blockSize) {
        plugin.inputBusses = 0;
        plugin.outputBusses = 0;
        plugin.inputChannels = 0;
        plugin.outputChannels = 0;
        if (!plugin.audioPorts || !plugin.plugin) return false;
        plugin.inputBusses = static_cast<int>(plugin.audioPorts->count(plugin.plugin, true));
        plugin.outputBusses = static_cast<int>(plugin.audioPorts->count(plugin.plugin, false));
        if (plugin.outputBusses <= 0) return false;
        if (!plugin.isInstrument && plugin.inputBusses <= 0) return false;

        plugin.inputPortBuffers.resize(static_cast<size_t>(plugin.inputBusses));
        plugin.outputPortBuffers.resize(static_cast<size_t>(plugin.outputBusses));
        plugin.inputAudioBuffers.resize(static_cast<size_t>(plugin.inputBusses));
        plugin.outputAudioBuffers.resize(static_cast<size_t>(plugin.outputBusses));

        for (int i = 0; i < plugin.inputBusses; ++i) {
            clap_audio_port_info_t info{};
            if (!plugin.audioPorts->get(plugin.plugin, static_cast<uint32_t>(i), true, &info)) return false;
            if (i == 0) plugin.inputChannels = static_cast<int>(std::min<uint32_t>(info.channel_count, 2));
            preparePortBuffer(plugin.inputPortBuffers[static_cast<size_t>(i)], info.channel_count, blockSize);
        }
        for (int i = 0; i < plugin.outputBusses; ++i) {
            clap_audio_port_info_t info{};
            if (!plugin.audioPorts->get(plugin.plugin, static_cast<uint32_t>(i), false, &info)) return false;
            if (i == 0) plugin.outputChannels = static_cast<int>(std::min<uint32_t>(info.channel_count, 2));
            preparePortBuffer(plugin.outputPortBuffers[static_cast<size_t>(i)], info.channel_count, blockSize);
        }
        return plugin.outputChannels > 0;
    }

    void refreshNoteDialect(ClapPlugin& plugin) {
        plugin.noteDialect = 0;
        plugin.supportedNoteDialects = 0;
        if (!plugin.notePorts || !plugin.plugin || plugin.notePorts->count(plugin.plugin, true) == 0) return;
        clap_note_port_info_t info{};
        if (!plugin.notePorts->get(plugin.plugin, 0, true, &info)) return;
        plugin.supportedNoteDialects = info.supported_dialects;
        constexpr uint32_t supportedByHost = CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI | CLAP_NOTE_DIALECT_MIDI_MPE;
        if (info.preferred_dialect && (info.preferred_dialect & supportedByHost)
            && (info.supported_dialects & info.preferred_dialect)) {
            plugin.noteDialect = info.preferred_dialect;
        } else if (info.supported_dialects & CLAP_NOTE_DIALECT_CLAP) {
            plugin.noteDialect = CLAP_NOTE_DIALECT_CLAP;
        } else if (info.supported_dialects & CLAP_NOTE_DIALECT_MIDI) {
            plugin.noteDialect = CLAP_NOTE_DIALECT_MIDI;
        } else if (info.supported_dialects & CLAP_NOTE_DIALECT_MIDI_MPE) {
            plugin.noteDialect = CLAP_NOTE_DIALECT_MIDI_MPE;
        }
    }
    ClapPlugin* fromHost(const clap_host_t* host) {
        return host ? static_cast<ClapPlugin*>(host->host_data) : nullptr;
    }

    void CLAP_ABI hostLog(const clap_host_t* host, clap_log_severity severity, const char* msg) {
        ClapPlugin* p = fromHost(host);
        std::ostream& stream = (severity >= CLAP_LOG_ERROR) ? std::cerr : std::cout;
        stream << "CLAP";
        if (p) stream << " [" << p->name << "]";
        stream << ": " << (msg ? msg : "") << std::endl;
    }

    bool CLAP_ABI hostIsMainThread(const clap_host_t* host) {
        ClapPlugin* p = fromHost(host);
        return p && p->mainThreadHash == currentThreadHash();
    }

    bool CLAP_ABI hostIsAudioThread(const clap_host_t* host) {
        ClapPlugin* p = fromHost(host);
        return p && p->audioThreadHash.load(std::memory_order_relaxed) == currentThreadHash();
    }

    void CLAP_ABI hostStateDirty(const clap_host_t* host) {
        if (ClapPlugin* p = fromHost(host)) p->stateDirty.store(true, std::memory_order_relaxed);
    }
    void CLAP_ABI hostParamsRescan(const clap_host_t* host, clap_param_rescan_flags) {
        if (ClapPlugin* p = fromHost(host)) p->callbackRequested.store(true, std::memory_order_relaxed);
    }
    void CLAP_ABI hostParamsClear(const clap_host_t*, clap_id, clap_param_clear_flags) {}
    void CLAP_ABI hostParamsRequestFlush(const clap_host_t* host) {
        if (ClapPlugin* p = fromHost(host)) p->paramFlushRequested.store(true, std::memory_order_relaxed);
    }

    void CLAP_ABI hostGuiResizeHintsChanged(const clap_host_t*) {}
    bool CLAP_ABI hostGuiRequestResize(const clap_host_t* host, uint32_t width, uint32_t height) {
        ClapPlugin* p = fromHost(host);
        if (!p) return false;
        p->guiResizeWidth.store(width, std::memory_order_relaxed);
        p->guiResizeHeight.store(height, std::memory_order_relaxed);
        p->guiResizePending.store(true, std::memory_order_release);
        return true;
    }
    bool CLAP_ABI hostGuiRequestShow(const clap_host_t* host) {
        if (ClapPlugin* p = fromHost(host)) { p->guiVisibilityRequest.store(1, std::memory_order_relaxed); return true; }
        return false;
    }
    bool CLAP_ABI hostGuiRequestHide(const clap_host_t* host) {
        if (ClapPlugin* p = fromHost(host)) { p->guiVisibilityRequest.store(-1, std::memory_order_relaxed); return true; }
        return false;
    }
    void CLAP_ABI hostGuiClosed(const clap_host_t* host, bool wasDestroyed) {
        if (ClapPlugin* p = fromHost(host)) {
            p->uiVisible = false;
            if (wasDestroyed) p->guiClosedByPlugin.store(true, std::memory_order_relaxed);
        }
    }

    bool CLAP_ABI hostRegisterTimer(const clap_host_t* host, uint32_t periodMs, clap_id* timerId) {
        ClapPlugin* p = fromHost(host);
        if (!p || !timerId) return false;
        periodMs = std::max<uint32_t>(1, periodMs);
        ClapTimerRegistration timer;
        timer.id = p->nextTimerId++;
        timer.periodMs = periodMs;
        timer.nextFireSeconds = nowSeconds() + static_cast<double>(periodMs) / 1000.0;
        p->timers.push_back(timer);
        *timerId = timer.id;
        return true;
    }

    bool CLAP_ABI hostUnregisterTimer(const clap_host_t* host, clap_id timerId) {
        ClapPlugin* p = fromHost(host);
        if (!p) return false;
        auto it = std::find_if(p->timers.begin(), p->timers.end(), [&](const auto& t) { return t.id == timerId; });
        if (it == p->timers.end()) return false;
        p->timers.erase(it);
        return true;
    }
    bool CLAP_ABI hostAudioPortsFlagSupported(const clap_host_t*, uint32_t flags) {
        constexpr uint32_t supported = CLAP_AUDIO_PORTS_RESCAN_NAMES;
        return (flags & ~supported) == 0;
    }
    void CLAP_ABI hostAudioPortsRescan(const clap_host_t* host, uint32_t flags) {
        ClapPlugin* p = fromHost(host);
        if (!p) return;
        if (flags & ~CLAP_AUDIO_PORTS_RESCAN_NAMES) p->restartRequested.store(true, std::memory_order_relaxed);
    }
    uint32_t CLAP_ABI hostNoteDialects(const clap_host_t*) {
        return CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI | CLAP_NOTE_DIALECT_MIDI_MPE;
    }
    void CLAP_ABI hostNotePortsRescan(const clap_host_t* host, uint32_t) {
        if (ClapPlugin* p = fromHost(host)) p->restartRequested.store(true, std::memory_order_relaxed);
    }
    void CLAP_ABI hostLatencyChanged(const clap_host_t* host) {
        if (ClapPlugin* p = fromHost(host)) p->restartRequested.store(true, std::memory_order_relaxed);
    }

    const clap_host_log_t kHostLog{hostLog};
    const clap_host_thread_check_t kHostThreadCheck{hostIsMainThread, hostIsAudioThread};
    const clap_host_state_t kHostState{hostStateDirty};
    const clap_host_params_t kHostParams{hostParamsRescan, hostParamsClear, hostParamsRequestFlush};
    const clap_host_gui_t kHostGui{hostGuiResizeHintsChanged, hostGuiRequestResize,
                                   hostGuiRequestShow, hostGuiRequestHide, hostGuiClosed};
    const clap_host_timer_support_t kHostTimer{hostRegisterTimer, hostUnregisterTimer};
    const clap_host_audio_ports_t kHostAudioPorts{hostAudioPortsFlagSupported, hostAudioPortsRescan};
    const clap_host_note_ports_t kHostNotePorts{hostNoteDialects, hostNotePortsRescan};
    const clap_host_latency_t kHostLatency{hostLatencyChanged};

    const void* CLAP_ABI hostGetExtension(const clap_host_t*, const char* id) {
        if (!id) return nullptr;
        if (std::strcmp(id, CLAP_EXT_LOG) == 0) return &kHostLog;
        if (std::strcmp(id, CLAP_EXT_THREAD_CHECK) == 0) return &kHostThreadCheck;
        if (std::strcmp(id, CLAP_EXT_STATE) == 0) return &kHostState;
        if (std::strcmp(id, CLAP_EXT_PARAMS) == 0) return &kHostParams;
        if (std::strcmp(id, CLAP_EXT_GUI) == 0) return &kHostGui;
        if (std::strcmp(id, CLAP_EXT_TIMER_SUPPORT) == 0) return &kHostTimer;
        if (std::strcmp(id, CLAP_EXT_AUDIO_PORTS) == 0) return &kHostAudioPorts;
        if (std::strcmp(id, CLAP_EXT_NOTE_PORTS) == 0) return &kHostNotePorts;
        if (std::strcmp(id, CLAP_EXT_LATENCY) == 0) return &kHostLatency;
        return nullptr;
    }

    void CLAP_ABI hostRequestRestart(const clap_host_t* host) {
        if (ClapPlugin* p = fromHost(host)) p->restartRequested.store(true, std::memory_order_relaxed);
    }
    void CLAP_ABI hostRequestProcess(const clap_host_t* host) {
        if (ClapPlugin* p = fromHost(host)) p->processRequested.store(true, std::memory_order_relaxed);
    }
    void CLAP_ABI hostRequestCallback(const clap_host_t* host) {
        if (ClapPlugin* p = fromHost(host)) p->callbackRequested.store(true, std::memory_order_release);
    }
    bool loadClapLibrary(Vst3Context& ctx, const std::filesystem::path& bundle) {
        const std::filesystem::path binary = resolveClapBinary(bundle);
        if (binary.empty()) return false;
        void* handle = openDynamicLibrary(binary);
        if (!handle) {
            std::cerr << "CLAP: failed to load " << bundle << std::endl;
            return false;
        }
        auto* entry = static_cast<const clap_plugin_entry_t*>(findDynamicSymbol(handle, "clap_entry"));
        if (!entry || !clap_version_is_compatible(entry->clap_version)) {
            closeDynamicLibrary(handle);
            return false;
        }
        const std::string bundlePath = bundle.string();
        if (!entry->init(bundlePath.c_str())) {
            closeDynamicLibrary(handle);
            return false;
        }
        auto* factory = static_cast<const clap_plugin_factory_t*>(entry->get_factory(CLAP_PLUGIN_FACTORY_ID));
        if (!factory) {
            entry->deinit();
            closeDynamicLibrary(handle);
            return false;
        }
        auto library = std::make_unique<ClapLibrary>();
        library->path = bundlePath;
        library->binaryPath = binary.string();
        library->handle = handle;
        library->entry = entry;
        library->factory = factory;
        library->initialized = true;
        ctx.clapLibraries.push_back(std::move(library));
        return true;
    }
    ClapLibrary* findLibrary(Vst3Context& ctx, const std::string& path) {
        for (auto& library : ctx.clapLibraries) {
            if (library && library->path == path) return library.get();
        }
        return nullptr;
    }

    void configureHost(ClapPlugin& plugin) {
        plugin.mainThreadHash = currentThreadHash();
        plugin.host.clap_version = CLAP_VERSION;
        plugin.host.host_data = &plugin;
        plugin.host.name = "Salamander";
        plugin.host.vendor = "Salamander";
        plugin.host.url = "";
        plugin.host.version = "0.1";
        plugin.host.get_extension = hostGetExtension;
        plugin.host.request_restart = hostRequestRestart;
        plugin.host.request_process = hostRequestProcess;
        plugin.host.request_callback = hostRequestCallback;
        plugin.inputEventList.ctx = &plugin;
        plugin.inputEventList.size = inputEventsSize;
        plugin.inputEventList.get = inputEventsGet;
        plugin.outputEventList.ctx = &plugin;
        plugin.outputEventList.try_push = outputEventsPush;
    }

    void queryExtensions(ClapPlugin& plugin) {
        auto get = [&](const char* id) { return plugin.plugin->get_extension(plugin.plugin, id); };
        plugin.audioPorts = static_cast<const clap_plugin_audio_ports_t*>(get(CLAP_EXT_AUDIO_PORTS));
        plugin.notePorts = static_cast<const clap_plugin_note_ports_t*>(get(CLAP_EXT_NOTE_PORTS));
        plugin.params = static_cast<const clap_plugin_params_t*>(get(CLAP_EXT_PARAMS));
        plugin.state = static_cast<const clap_plugin_state_t*>(get(CLAP_EXT_STATE));
        plugin.gui = static_cast<const clap_plugin_gui_t*>(get(CLAP_EXT_GUI));
        plugin.timerSupport = static_cast<const clap_plugin_timer_support_t*>(get(CLAP_EXT_TIMER_SUPPORT));
    }
    void shutdownInstance(ClapPlugin& plugin) {
        if (plugin.vst2) { Vst2Host::Destroy(plugin); return; }
        if (!plugin.plugin) return;
        if (plugin.guiCreated && plugin.gui) {
            if (plugin.uiVisible) plugin.gui->hide(plugin.plugin);
            plugin.gui->destroy(plugin.plugin);
            plugin.guiCreated = false;
        }
        if (plugin.uiWindow) {
            Vst3UI_CloseWindow(plugin.uiWindow);
            plugin.uiWindow = nullptr;
        }
        plugin.uiVisible = false;
        if (plugin.processing) {
            plugin.plugin->stop_processing(plugin.plugin);
            plugin.processing = false;
        }
        if (plugin.active) {
            plugin.plugin->deactivate(plugin.plugin);
            plugin.active = false;
        }
        plugin.plugin->destroy(plugin.plugin);
        plugin.plugin = nullptr;
    }

    bool activateInstance(Vst3Context& ctx, ClapPlugin& plugin) {
        if (!refreshAudioPorts(plugin, ctx.blockSize)) return false;
        refreshNoteDialect(plugin);
        refreshParameters(plugin);
        if (!plugin.plugin->activate(plugin.plugin, ctx.sampleRate, 1u,
                                     static_cast<uint32_t>(std::max(1, ctx.blockSize)))) return false;
        plugin.active = true;
        if (!plugin.plugin->start_processing(plugin.plugin)) {
            plugin.plugin->deactivate(plugin.plugin);
            plugin.active = false;
            return false;
        }
        plugin.processing = true;
        return true;
    }
} // namespace

void Initialize(Vst3Context& ctx, const std::string& executableDir, const std::string& userRoot) {
    if constexpr (!SalamanderPlatform::desktopPluginHosting) return;
    Vst2Host::Discover(ctx, executableDir, userRoot);
    for (const auto& bundle : scanClapBundles(clapSearchRoots(userRoot))) {
        if (std::none_of(ctx.clapLibraries.begin(), ctx.clapLibraries.end(), [&](const auto& library){return library && library->path==bundle.string();})) (void)loadClapLibrary(ctx, bundle);
    }
    for (const auto& library : ctx.clapLibraries) {
        if (!library || !library->factory) continue;
        const uint32_t count = library->factory->get_plugin_count(library->factory);
        for (uint32_t i = 0; i < count; ++i) {
            const clap_plugin_descriptor_t* desc = library->factory->get_plugin_descriptor(library->factory, i);
            if (!desc || !desc->id || !desc->name || !clap_version_is_compatible(desc->clap_version)) continue;
            Vst3AvailablePlugin available;
            available.format = HostedPluginFormat::Clap;
            available.name = desc->name;
            available.vendor = desc->vendor ? desc->vendor : "";
            available.uniqueId = desc->id;
            available.modulePath = library->path;
            available.isInstrument = descriptorHasFeature(desc, CLAP_PLUGIN_FEATURE_INSTRUMENT);
            ctx.availablePlugins.push_back(std::move(available));
        }
    }
    std::sort(ctx.availablePlugins.begin(), ctx.availablePlugins.end(), [](const auto& a, const auto& b) {
        if (a.name != b.name) return a.name < b.name;
        return static_cast<int>(a.format) < static_cast<int>(b.format);
    });
    std::cout << "CLAP: discovered " << ctx.clapLibraries.size() << " module(s)." << std::endl;
}
ClapPlugin* CreatePlugin(Vst3Context& ctx, const Vst3AvailablePlugin& available, bool instrument) {
    if constexpr (!SalamanderPlatform::desktopPluginHosting) return nullptr;
    if (available.format == HostedPluginFormat::Vst2) return Vst2Host::Create(ctx, available, instrument);
    if (available.format != HostedPluginFormat::Clap || available.uniqueId.empty()) return nullptr;
    ClapLibrary* library = findLibrary(ctx, available.modulePath);
    if (!library || !library->factory) return nullptr;

    auto owned = std::make_unique<ClapPlugin>();
    ClapPlugin& plugin = *owned;
    plugin.name = available.name;
    plugin.id = available.uniqueId;
    plugin.vendor = available.vendor;
    plugin.path = available.modulePath;
    plugin.owner = &ctx;
    plugin.inputEvents.reserve(512);
    plugin.tuningVoices.reserve(128);
    plugin.library = library;
    plugin.isInstrument = instrument;
    configureHost(plugin);

    plugin.plugin = library->factory->create_plugin(library->factory, &plugin.host, plugin.id.c_str());
    if (!plugin.plugin || !plugin.plugin->init(plugin.plugin)) {
        if (plugin.plugin) plugin.plugin->destroy(plugin.plugin);
        return nullptr;
    }
    queryExtensions(plugin);
    plugin.inputEvents.reserve(256);
    if (!activateInstance(ctx, plugin)) {
        shutdownInstance(plugin);
        return nullptr;
    }
    ctx.clapPlugins.push_back(std::move(owned));
    return ctx.clapPlugins.back().get();
}

void DestroyPlugin(Vst3Context& ctx, ClapPlugin* plugin) {
    if (!plugin) return;
    for (auto it = ctx.clapPlugins.begin(); it != ctx.clapPlugins.end(); ++it) {
        if (it->get() != plugin) continue;
        shutdownInstance(**it);
        ctx.clapPlugins.erase(it);
        return;
    }
}
const ClapParamInfo* FindParam(const ClapPlugin& plugin, clap_id id) {
    for (const auto& param : plugin.parameterInfo) if (param.id == id) return &param;
    return nullptr;
}

double GetNormalizedParam(const ClapPlugin& plugin, clap_id id) {
    if (plugin.vst2) return Vst2Host::GetParameter(plugin, id);
    if (!plugin.params || !plugin.plugin) return 0.0;
    double value = 0.0;
    if (!plugin.params->get_value(plugin.plugin, id, &value)) return 0.0;
    const ClapParamInfo* info = FindParam(plugin, id);
    if (!info || info->maxValue <= info->minValue) return 0.0;
    return std::clamp((value - info->minValue) / (info->maxValue - info->minValue), 0.0, 1.0);
}

bool QueueNormalizedParam(ClapPlugin& plugin, clap_id id, double normalized) {
    if (plugin.vst2) return Vst2Host::SetParameter(plugin, id, normalized);
    const ClapParamInfo* info = FindParam(plugin, id);
    if (!info || (info->flags & CLAP_PARAM_IS_READONLY)) return false;
    if (!plugin.pendingParameters.publish(id, normalized)) return false;
    plugin.stateDirty.store(true, std::memory_order_relaxed);
    return true;
}
namespace {
    struct OutputStateStream { std::vector<uint8_t>* bytes = nullptr; };
    int64_t CLAP_ABI stateWrite(const clap_ostream_t* stream, const void* buffer, uint64_t size) {
        auto* state = stream ? static_cast<OutputStateStream*>(stream->ctx) : nullptr;
        if (!state || !state->bytes || (!buffer && size > 0)) return -1;
        const auto* first = static_cast<const uint8_t*>(buffer);
        state->bytes->insert(state->bytes->end(), first, first + size);
        return static_cast<int64_t>(size);
    }
    struct InputStateStream { const uint8_t* bytes = nullptr; size_t size = 0; size_t offset = 0; };
    int64_t CLAP_ABI stateRead(const clap_istream_t* stream, void* buffer, uint64_t size) {
        auto* state = stream ? static_cast<InputStateStream*>(stream->ctx) : nullptr;
        if (!state || (!buffer && size > 0)) return -1;
        const size_t remaining = state->offset < state->size ? state->size - state->offset : 0;
        const size_t count = std::min<size_t>(remaining, static_cast<size_t>(size));
        if (count) std::memcpy(buffer, state->bytes + state->offset, count);
        state->offset += count;
        return static_cast<int64_t>(count);
    }
}

bool SaveState(const ClapPlugin& plugin, std::vector<uint8_t>& out) {
    if (plugin.vst2) return Vst2Host::SaveState(plugin, out);
    out.clear();
    if (!plugin.plugin || !plugin.state) return false;
    OutputStateStream data{&out};
    clap_ostream_t stream{&data, stateWrite};
    return plugin.state->save(plugin.plugin, &stream);
}
bool LoadState(ClapPlugin& plugin, const uint8_t* data, size_t size) {
    if (plugin.vst2) return Vst2Host::LoadState(plugin, data, size);
    if (!plugin.plugin || !plugin.state || (!data && size > 0)) return false;
    InputStateStream input{data, size, 0};
    clap_istream_t stream{&input, stateRead};
    if (!plugin.state->load(plugin.plugin, &stream)) return false;
    refreshParameters(plugin);
    plugin.stateDirty.store(false, std::memory_order_relaxed);
    return true;
}

namespace {
    void drainPendingParams(ClapPlugin& plugin) {
        plugin.pendingParameters.consumeIndexed([&](size_t index,uint32_t id,double normalized) {
            const auto& info=plugin.parameterInfo[index];
            auto* event = appendEvent<clap_event_param_value_t>(plugin, CLAP_EVENT_PARAM_VALUE);
            event->param_id = id;
            event->cookie = info.cookie;
            event->note_id = -1;
            event->port_index = -1;
            event->channel = -1;
            event->key = -1;
            event->value = info.minValue + normalized * (info.maxValue - info.minValue);
        });
    }
}
namespace {
    void prepareProcessBuffers(ClapPlugin& plugin, int numFrames,
                               const float* inputLeft, const float* inputRight) {
        for (size_t p = 0; p < plugin.inputPortBuffers.size(); ++p) {
            auto& port = plugin.inputPortBuffers[p];
            std::fill(port.storage.begin(), port.storage.end(), 0.0f);
            if (p == 0 && inputLeft && port.channels > 0) {
                std::copy(inputLeft, inputLeft + numFrames, port.pointers[0]);
                if (port.channels > 1) {
                    const float* right = inputRight ? inputRight : inputLeft;
                    std::copy(right, right + numFrames, port.pointers[1]);
                }
            }
            auto& audio = plugin.inputAudioBuffers[p];
            audio.data32 = port.pointers.data();
            audio.data64 = nullptr;
            audio.channel_count = port.channels;
            audio.latency = 0;
            audio.constant_mask = 0;
        }
        for (size_t p = 0; p < plugin.outputPortBuffers.size(); ++p) {
            auto& port = plugin.outputPortBuffers[p];
            std::fill(port.storage.begin(), port.storage.end(), 0.0f);
            auto& audio = plugin.outputAudioBuffers[p];
            audio.data32 = port.pointers.data();
            audio.data64 = nullptr;
            audio.channel_count = port.channels;
            audio.latency = 0;
            audio.constant_mask = 0;
        }
    }
    clap_event_transport_t makeTransport(const Vst3Context& ctx, int64_t sampleOffset, bool playing) {
        clap_event_transport_t transport{};
        transport.header.size = sizeof(transport);
        transport.header.space_id = CLAP_CORE_EVENT_SPACE_ID;
        transport.header.type = CLAP_EVENT_TRANSPORT;
        transport.flags = CLAP_TRANSPORT_HAS_SECONDS_TIMELINE;
        if (playing) transport.flags |= CLAP_TRANSPORT_IS_PLAYING;
        if (false) {
            transport.flags |= CLAP_TRANSPORT_IS_RECORDING;
        }
        const double rate = ctx.sampleRate > 0.0f ? ctx.sampleRate : 44100.0;
        const uint64_t timelineSample = static_cast<uint64_t>(std::max<int64_t>(0, sampleOffset));
        const double seconds = static_cast<double>(timelineSample) / rate;
        transport.song_pos_seconds = static_cast<clap_sectime>(std::llround(seconds * CLAP_SECTIME_FACTOR));
        // Vst3System selects the shared musical map before entering this chain.
        // Rubato reports fractional local tempo and the mapped quarter position.
        transport.flags |= CLAP_TRANSPORT_HAS_TEMPO | CLAP_TRANSPORT_HAS_BEATS_TIMELINE
                        | CLAP_TRANSPORT_HAS_TIME_SIGNATURE;
        transport.song_pos_beats = static_cast<clap_beattime>(std::llround(
            ctx.processClockQuarterNotes * CLAP_BEATTIME_FACTOR));
        transport.tempo = ctx.processClockBpm;
        transport.tempo_inc = 0.0;
        transport.tsig_num = static_cast<uint16_t>(std::clamp(ctx.processClockNumerator, 1, 32));
        transport.tsig_denom = static_cast<uint16_t>(std::clamp(ctx.processClockDenominator, 1, 32));
        return transport;
    }

    bool runProcess(Vst3Context& ctx, ClapPlugin& plugin, int numFrames,
                    int64_t sampleOffset, bool playing) {
        if (!plugin.plugin || !plugin.active || !plugin.processing || numFrames <= 0) return false;
        if (numFrames > ctx.blockSize) return false;
        clap_event_transport_t transport = makeTransport(ctx, sampleOffset, playing);
        clap_process_t process{};
        process.steady_time = ctx.continuousSamples;
        process.frames_count = static_cast<uint32_t>(numFrames);
        process.transport = &transport;
        process.audio_inputs = plugin.inputAudioBuffers.empty() ? nullptr : plugin.inputAudioBuffers.data();
        process.audio_outputs = plugin.outputAudioBuffers.empty() ? nullptr : plugin.outputAudioBuffers.data();
        process.audio_inputs_count = static_cast<uint32_t>(plugin.inputAudioBuffers.size());
        process.audio_outputs_count = static_cast<uint32_t>(plugin.outputAudioBuffers.size());
        process.in_events = &plugin.inputEventList;
        process.out_events = &plugin.outputEventList;
        plugin.audioThreadHash.store(currentThreadHash(), std::memory_order_relaxed);
        const clap_process_status status = plugin.plugin->process(plugin.plugin, &process);
        return status != CLAP_PROCESS_ERROR;
    }

    void copyMainOutput(const ClapPlugin& plugin, float* left, float* right, int numFrames) {
        if (!left || !right || plugin.outputPortBuffers.empty()) return;
        const auto& port = plugin.outputPortBuffers.front();
        if (port.channels == 0 || port.pointers.empty()) return;
        std::copy(port.pointers[0], port.pointers[0] + numFrames, left);
        if (port.channels > 1) std::copy(port.pointers[1], port.pointers[1] + numFrames, right);
        else std::copy(port.pointers[0], port.pointers[0] + numFrames, right);
    }

    void appendAutomationEvents(Vst3Context&, ClapPlugin&, int64_t) {}
} // namespace

bool ProcessEffect(Vst3Context& ctx, ClapPlugin& plugin,
                   const float* inputLeft, const float* inputRight,
                   float* outputLeft, float* outputRight,
                   int numFrames, int64_t sampleOffset, bool playing) {
    if (!inputLeft || !outputLeft || !outputRight || numFrames <= 0) return false;
    if (plugin.vst2) {
        appendAutomationEvents(ctx, plugin, sampleOffset);
        return Vst2Host::Process(plugin, inputLeft, inputRight, outputLeft, outputRight, numFrames);
    }
    const float* right = inputRight ? inputRight : inputLeft;
    if (plugin.bypassed.load(std::memory_order_relaxed)) {
        std::copy(inputLeft, inputLeft + numFrames, outputLeft);
        std::copy(right, right + numFrames, outputRight);
        return true;
    }
    plugin.inputEvents.clear();

    drainPendingParams(plugin);
    appendAutomationEvents(ctx, plugin, sampleOffset);
    prepareProcessBuffers(plugin, numFrames, inputLeft, right);
    if (!runProcess(ctx, plugin, numFrames, sampleOffset, playing)) {
        std::copy(inputLeft, inputLeft + numFrames, outputLeft);
        std::copy(right, right + numFrames, outputRight);
        return false;
    }
    copyMainOutput(plugin, outputLeft, outputRight, numFrames);
    plugin.processRequested.store(false, std::memory_order_relaxed);
    return true;


}

namespace {
    void appendHeldNoteEvent(ClapPlugin& plugin, bool noteOn, int note, float velocity) {
        if (plugin.noteDialect & CLAP_NOTE_DIALECT_CLAP) {
            auto* event = appendEvent<clap_event_note_t>(plugin,
                noteOn ? CLAP_EVENT_NOTE_ON : CLAP_EVENT_NOTE_OFF);
            event->note_id = note + 1;
            event->port_index = 0;
            event->channel = 0;
            event->key = static_cast<int16_t>(note);
            event->velocity = noteOn ? std::clamp<double>(velocity, 0.0, 1.0) : 0.0;
            return;
        }
        if (!(plugin.noteDialect & (CLAP_NOTE_DIALECT_MIDI | CLAP_NOTE_DIALECT_MIDI_MPE))) return;
        auto* event = appendEvent<clap_event_midi_t>(plugin, CLAP_EVENT_MIDI);
        event->port_index = 0;
        event->data[0] = static_cast<uint8_t>(noteOn ? 0x90 : 0x80);
        event->data[1] = static_cast<uint8_t>(std::clamp(note, 0, 127));
        event->data[2] = static_cast<uint8_t>(noteOn
            ? std::lround(std::clamp(velocity, 0.0f, 1.0f) * 127.0f) : 0);
    }
}

bool ProcessInstrument(Vst3Context& ctx, ClapPlugin& plugin,
                       float* outputLeft, float* outputRight,
                       int numFrames, int64_t sampleOffset, bool playing,
                       const std::array<float, 128>& desiredHeldVelocities,
                       std::array<float, 128>& lastHeldVelocities,
                       const std::vector<Microtonal::Voice>* tunedNotes, int tuningMode) {
    if (!outputLeft || !outputRight || numFrames <= 0) return false;
    plugin.inputEvents.clear();

    drainPendingParams(plugin);
    appendAutomationEvents(ctx, plugin, sampleOffset);
    const bool bypassed = plugin.bypassed.load(std::memory_order_relaxed);
        const auto tuningOutput = TuningOutput(plugin, tuningMode);
        std::vector<Microtonal::Voice> mpeNotes;
        if (tuningOutput == Microtonal::Output::Mpe) {
            mpeNotes = Microtonal::WithMpeNotes(desiredHeldVelocities, tunedNotes, plugin.tuningVoices, sampleOffset);
            tunedNotes = &mpeNotes;
        }
    constexpr float eps = 0.0001f;
    for (int note = 0; note < 128; ++note) {
        const float previous = lastHeldVelocities[static_cast<size_t>(note)];
        const float desired = bypassed || tuningOutput == Microtonal::Output::Mpe ? 0.0f : desiredHeldVelocities[static_cast<size_t>(note)];
        const bool wasHeld = previous > eps;
        const bool isHeld = desired > eps;
        if (wasHeld != isHeld) appendHeldNoteEvent(plugin, isHeld, note, desired);
        lastHeldVelocities[static_cast<size_t>(note)] = isHeld ? std::clamp(desired, 0.0f, 1.0f) : 0.0f;
    }
    auto nativeEvent = [&](bool on, const Microtonal::ActiveVoice& voice) {
        auto* event = appendEvent<clap_event_note_t>(plugin, on ? CLAP_EVENT_NOTE_ON : CLAP_EVENT_NOTE_OFF);
        event->note_id = voice.eventId; event->port_index = 0; event->channel = 0;
        event->key = voice.key; event->velocity = on ? voice.velocity : 0;
        if (on) {
            auto* expression = appendEvent<clap_event_note_expression_t>(plugin, CLAP_EVENT_NOTE_EXPRESSION);
            expression->expression_id = CLAP_NOTE_EXPRESSION_TUNING;
            expression->note_id = voice.eventId; expression->port_index = 0; expression->channel = 0;
            expression->key = voice.key; expression->value = voice.cents / 100.0;
        }
    };
    auto midiEvent = [&](int status, int data1, int data2) {
        auto* event = appendEvent<clap_event_midi_t>(plugin, CLAP_EVENT_MIDI);
        event->port_index = 0; event->data[0] = status; event->data[1] = data1; event->data[2] = data2;
    };
    Microtonal::Dispatch(plugin.tuningVoices, plugin.nextTuningVoiceId, tunedNotes,
        tuningOutput, bypassed, nativeEvent, midiEvent, &plugin.tuningMpeZoneActive);
    if (ctx.processPitchBend >= 0.) {
        const double semitones=bypassed?0.:(ctx.processPitchBend-0.5)*4.;
        if(plugin.noteDialect==CLAP_NOTE_DIALECT_CLAP){
            auto expression=[&](int key,double value){
                auto* e=appendEvent<clap_event_note_expression_t>(plugin,CLAP_EVENT_NOTE_EXPRESSION);
                e->expression_id=CLAP_NOTE_EXPRESSION_TUNING;e->note_id=-1;e->port_index=0;e->channel=0;e->key=key;e->value=value;
            };
            for(int key=0;key<128;++key)if(lastHeldVelocities[key]>eps)expression(key,semitones);
            for(const auto& voice:plugin.tuningVoices)expression(voice.key,voice.cents/100.+semitones);
        }else{
            const int bend=std::clamp(int(std::lround((0.5+semitones/4.)*16384.)),0,16383);
            midiEvent(0xe0,bend&127,bend>>7);
        }
    }

    prepareProcessBuffers(plugin, numFrames, nullptr, nullptr);
    const bool processed = runProcess(ctx, plugin, numFrames, sampleOffset, playing);
    if (!processed || bypassed) {
        std::fill(outputLeft, outputLeft + numFrames, 0.0f);
        std::fill(outputRight, outputRight + numFrames, 0.0f);
        return false;
    }
    copyMainOutput(plugin, outputLeft, outputRight, numFrames);
    plugin.processRequested.store(false, std::memory_order_relaxed);
    return true;
}

namespace {
    void defaultEditorSetParam(void* userData, int32_t paramId, double normalized) {
        auto* plugin = static_cast<ClapPlugin*>(userData);
        if (!plugin) return;
        (void)QueueNormalizedParam(*plugin, static_cast<clap_id>(static_cast<uint32_t>(paramId)), normalized);
    }

    bool openDefaultEditor(ClapPlugin& plugin) {
        std::vector<Vst3DefaultEditorParam> params;
        params.reserve(std::min<size_t>(plugin.parameterInfo.size(), 128));
        for (const auto& info : plugin.parameterInfo) {
            if ((info.flags & CLAP_PARAM_IS_HIDDEN) || params.size() >= 128) continue;
            Vst3DefaultEditorParam p{};
            p.id = static_cast<int32_t>(info.id);
            p.value = GetNormalizedParam(plugin, info.id);
            std::snprintf(p.title, sizeof(p.title), "%s", info.name.empty() ? "Parameter" : info.name.c_str());
            params.push_back(p);
        }
        const std::string title = plugin.vst2 ? Vst2Host::EditorTitle(plugin) : plugin.name;
        plugin.uiWindow = Vst3UI_CreateDefaultEditorWindow(title.c_str(), params.data(),
            static_cast<int>(params.size()), defaultEditorSetParam, &plugin);
        if (!plugin.uiWindow) return false;
        Vst3UI_ShowWindow(plugin.uiWindow);
        plugin.uiVisible = true;
        plugin.usingDefaultEditor = true;
        if (plugin.vst2) Vst2Host::ConfigureEditor(plugin);
        return true;
    }

    bool openNativeEditor(ClapPlugin& plugin) {
        if (!plugin.gui || !plugin.plugin) return false;
#if defined(__APPLE__)
        const char* api = CLAP_WINDOW_API_COCOA;
#elif defined(_WIN32)
        const char* api = CLAP_WINDOW_API_WIN32;
#else
        const char* api = CLAP_WINDOW_API_X11;
#endif
        if (!plugin.gui->is_api_supported(plugin.plugin, api, false)) return false;
        if (!plugin.gui->create(plugin.plugin, api, false)) return false;
        plugin.guiCreated = true;
        uint32_t width = 640, height = 480;
        (void)plugin.gui->get_size(plugin.plugin, &width, &height);
        plugin.uiWindow = Vst3UI_CreateWindow(plugin.name.c_str(), static_cast<int>(width), static_cast<int>(height));
        if (!plugin.uiWindow) return false;
        void* parent = Vst3UI_GetContentView(plugin.uiWindow);
        if (!parent) return false;
        clap_window_t window{};
        window.api = api;
#if defined(__APPLE__)
        window.cocoa = parent;
#elif defined(_WIN32)
        window.win32 = parent;
#else
        window.x11 = static_cast<clap_xwnd>(reinterpret_cast<uintptr_t>(parent));
#endif
        if (!plugin.gui->set_parent(plugin.plugin, &window)) return false;
        if (!plugin.gui->show(plugin.plugin)) return false;
        Vst3UI_ShowWindow(plugin.uiWindow);
        plugin.uiVisible = true;
        plugin.usingDefaultEditor = false;
        return true;
    }
}

void OpenPluginUI(ClapPlugin& plugin) {
    if (plugin.uiWindow) {
        if (!Vst3UI_IsWindowVisible(plugin.uiWindow)) {
            if (plugin.guiCreated && plugin.gui) (void)plugin.gui->show(plugin.plugin);
            Vst3UI_ShowWindow(plugin.uiWindow);
            plugin.uiVisible = true;
        }
        plugin.uiVisible = Vst3UI_IsWindowVisible(plugin.uiWindow);
        return;
    }
    if (!openNativeEditor(plugin)) {
        if (plugin.guiCreated && plugin.gui) {
            plugin.gui->destroy(plugin.plugin);
            plugin.guiCreated = false;
        }
        if (plugin.uiWindow) {
            Vst3UI_CloseWindow(plugin.uiWindow);
            plugin.uiWindow = nullptr;
        }
        (void)openDefaultEditor(plugin);
    }
}

void UpdateMainThread(Vst3Context& ctx) {
    const double now = nowSeconds();
    for (auto& owned : ctx.clapPlugins) {
        if (owned && owned->vst2) { Vst2Host::Update(*owned); continue; }
        if (!owned || !owned->plugin) continue;
        ClapPlugin& plugin = *owned;
        if (plugin.callbackRequested.exchange(false, std::memory_order_acq_rel)) {
            refreshParameters(plugin);
            if (plugin.plugin->on_main_thread) plugin.plugin->on_main_thread(plugin.plugin);
        }
        if (plugin.guiResizePending.exchange(false, std::memory_order_acq_rel) && plugin.uiWindow) {
            const uint32_t width = plugin.guiResizeWidth.load(std::memory_order_relaxed);
            const uint32_t height = plugin.guiResizeHeight.load(std::memory_order_relaxed);
            Vst3UI_SetWindowSize(plugin.uiWindow, static_cast<int>(width), static_cast<int>(height));
        }
        const int visibility = plugin.guiVisibilityRequest.exchange(0, std::memory_order_relaxed);
        if (visibility > 0) OpenPluginUI(plugin);
        else if (visibility < 0 && plugin.uiWindow) {
            if (plugin.guiCreated && plugin.gui) (void)plugin.gui->hide(plugin.plugin);
            Vst3UI_HideWindow(plugin.uiWindow);
            plugin.uiVisible = false;
        }
        if (plugin.guiClosedByPlugin.exchange(false, std::memory_order_relaxed)) {
            if (plugin.guiCreated && plugin.gui) {
                plugin.gui->destroy(plugin.plugin);
                plugin.guiCreated = false;
            }
            if (plugin.uiWindow) {
                Vst3UI_CloseWindow(plugin.uiWindow);
                plugin.uiWindow = nullptr;
            }
        }
        if (plugin.timerSupport) {
            for (auto& timer : plugin.timers) {
                if (now < timer.nextFireSeconds) continue;
                plugin.timerSupport->on_timer(plugin.plugin, timer.id);
                timer.nextFireSeconds = now + static_cast<double>(timer.periodMs) / 1000.0;
            }
        }
        if (plugin.restartRequested.exchange(false, std::memory_order_acq_rel)) {
            plugin.audioThreadHash.store(currentThreadHash(), std::memory_order_relaxed);
            if (plugin.processing) { plugin.plugin->stop_processing(plugin.plugin); plugin.processing = false; }
            if (plugin.active) { plugin.plugin->deactivate(plugin.plugin); plugin.active = false; }
            if (!activateInstance(ctx, plugin)) {
                std::cerr << "CLAP: restart failed for " << plugin.name << std::endl;
            }
        }
    }
}

void Cleanup(Vst3Context& ctx) {
    while (!ctx.clapPlugins.empty()) DestroyPlugin(ctx, ctx.clapPlugins.back().get());
    for (auto& library : ctx.clapLibraries) {
        if (!library) continue;
        if (library->initialized && library->entry) library->entry->deinit();
        library->initialized = false;
        closeDynamicLibrary(library->handle);
        library->handle = nullptr;
        library->entry = nullptr;
        library->factory = nullptr;
    }
    ctx.clapLibraries.clear();
}

} // namespace ClapSystemLogic
