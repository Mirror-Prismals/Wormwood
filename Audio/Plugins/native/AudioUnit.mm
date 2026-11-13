#include "Host/Vst3Host.h"

#if defined(__APPLE__)
#import <AudioToolbox/AudioToolbox.h>
#import <AudioUnit/AudioUnit.h>
#import <CoreFoundation/CoreFoundation.h>
#import <mach-o/dyld.h>

#include <algorithm>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <iostream>
#include <limits>

namespace AudioUnitSystemLogic {
namespace {
struct RegisteredUserAudioUnit {
    AudioComponentDescription description{};
    AudioComponent component = nullptr;
    std::string name;
};

struct UserAudioUnitBundle {
    std::string path;
    CFBundleRef bundle = nullptr;
    std::vector<RegisteredUserAudioUnit> components;
};

// AudioComponentRegister has process lifetime semantics and no matching
// unregister call. Retain each user-selected bundle for the remainder of the
// process so its registered factory pointer can never dangle.
std::vector<UserAudioUnitBundle> gUserAudioUnitBundles;

AudioUnit asUnit(const AudioUnitPlugin& plugin) {
    return reinterpret_cast<AudioUnit>(plugin.instance);
}

std::string fourCC(uint32_t value) {
    char text[5] = {
        static_cast<char>((value >> 24) & 0xff), static_cast<char>((value >> 16) & 0xff),
        static_cast<char>((value >> 8) & 0xff), static_cast<char>(value & 0xff), 0
    };
    for (int i = 0; i < 4; ++i) if (text[i] < 32 || text[i] > 126) text[i] = '?';
    return std::string(text, 4);
}

std::string componentId(const AudioComponentDescription& d) {
    return "au:" + fourCC(d.componentType) + ":" + fourCC(d.componentSubType)
        + ":" + fourCC(d.componentManufacturer);
}

uint32_t fourCCFromString(CFStringRef value) {
    if (!value) return 0;
    char text[5]{};
    if (!CFStringGetCString(value, text, sizeof(text), kCFStringEncodingMacRoman)
        || std::strlen(text) != 4) return 0;
    return (static_cast<uint32_t>(static_cast<uint8_t>(text[0])) << 24)
        | (static_cast<uint32_t>(static_cast<uint8_t>(text[1])) << 16)
        | (static_cast<uint32_t>(static_cast<uint8_t>(text[2])) << 8)
        | static_cast<uint32_t>(static_cast<uint8_t>(text[3]));
}

CFStringRef dictionaryString(CFDictionaryRef dictionary, CFStringRef key) {
    if (!dictionary) return nullptr;
    CFTypeRef value = CFDictionaryGetValue(dictionary, key);
    return value && CFGetTypeID(value) == CFStringGetTypeID()
        ? static_cast<CFStringRef>(value) : nullptr;
}

UInt32 dictionaryUInt32(CFDictionaryRef dictionary, CFStringRef key) {
    if (!dictionary) return 0;
    CFTypeRef value = CFDictionaryGetValue(dictionary, key);
    if (!value || CFGetTypeID(value) != CFNumberGetTypeID()) return 0;
    int64_t result = 0;
    return CFNumberGetValue(static_cast<CFNumberRef>(value), kCFNumberSInt64Type, &result)
        ? static_cast<UInt32>(result) : 0;
}

bool dictionaryBool(CFDictionaryRef dictionary, CFStringRef key) {
    if (!dictionary) return false;
    CFTypeRef value = CFDictionaryGetValue(dictionary, key);
    return value && CFGetTypeID(value) == CFBooleanGetTypeID()
        && CFBooleanGetValue(static_cast<CFBooleanRef>(value));
}

std::string stringFromCF(CFStringRef value) {
    if (!value) return {};
    char text[1024]{};
    return CFStringGetCString(value, text, sizeof(text), kCFStringEncodingUTF8)
        ? std::string(text) : std::string{};
}

std::string executableDirectory() {
    uint32_t size = 0;
    _NSGetExecutablePath(nullptr, &size);
    if (size == 0) return std::filesystem::current_path().string();
    std::string path(size, '\0');
    if (_NSGetExecutablePath(path.data(), &size) != 0)
        return std::filesystem::current_path().string();
    return std::filesystem::path(path.c_str()).parent_path().string();
}

void appendUserComponent(Vst3Context& ctx, AudioComponent component,
                         const AudioComponentDescription& description,
                         const std::string& name, const std::string& bundlePath) {
    if (!component) return;
    Vst3AvailablePlugin available;
    available.format = HostedPluginFormat::AudioUnit;
    available.name = name.empty() ? componentId(description) : name;
    const size_t separator = available.name.find(':');
    available.vendor = separator == std::string::npos
        ? fourCC(description.componentManufacturer)
        : available.name.substr(0, separator);
    available.uniqueId = componentId(description);
    available.modulePath = bundlePath;
    available.isInstrument = description.componentType == kAudioUnitType_MusicDevice
        || description.componentType == kAudioUnitType_Generator;
    available.audioUnitType = description.componentType;
    available.audioUnitSubtype = description.componentSubType;
    available.audioUnitManufacturer = description.componentManufacturer;
    available.audioUnitComponent = component;
    const bool duplicate = std::any_of(ctx.availablePlugins.begin(), ctx.availablePlugins.end(),
        [&](const Vst3AvailablePlugin& existing) {
            return existing.format == HostedPluginFormat::AudioUnit
                && existing.uniqueId == available.uniqueId;
        });
    if (!duplicate) ctx.availablePlugins.push_back(std::move(available));
}

size_t registerUserBundle(Vst3Context& ctx, const std::filesystem::path& path) {
    std::error_code ec;
    std::string canonical = std::filesystem::weakly_canonical(path, ec).string();
    if (canonical.empty()) canonical = path.string();
    auto retained = std::find_if(gUserAudioUnitBundles.begin(), gUserAudioUnitBundles.end(),
        [&](const UserAudioUnitBundle& entry) { return entry.path == canonical; });
    if (retained != gUserAudioUnitBundles.end()) {
        for (const RegisteredUserAudioUnit& registered : retained->components) {
            appendUserComponent(ctx, registered.component, registered.description,
                                registered.name, retained->path);
        }
        return retained->components.size();
    }
    CFBundleRef bundle = nullptr;
    if (!bundle) {
        CFURLRef url = CFURLCreateFromFileSystemRepresentation(kCFAllocatorDefault,
            reinterpret_cast<const UInt8*>(canonical.data()), canonical.size(), true);
        if (!url) return 0;
        bundle = CFBundleCreate(kCFAllocatorDefault, url);
        CFRelease(url);
        if (!bundle) return 0;
        CFErrorRef loadError = nullptr;
        if (!CFBundleLoadExecutableAndReturnError(bundle, &loadError)) {
            std::cerr << "AudioUnit: failed to load " << canonical;
            if (loadError) {
                CFStringRef description = CFErrorCopyDescription(loadError);
                std::cerr << ": " << stringFromCF(description);
                if (description) CFRelease(description);
                CFRelease(loadError);
            }
            std::cerr << std::endl;
            CFRelease(bundle);
            return 0;
        }
        gUserAudioUnitBundles.push_back({canonical, bundle, {}});
    }

    CFDictionaryRef info = CFBundleGetInfoDictionary(bundle);
    CFTypeRef componentsValue = info ? CFDictionaryGetValue(info, CFSTR("AudioComponents")) : nullptr;
    if (!componentsValue || CFGetTypeID(componentsValue) != CFArrayGetTypeID()) {
        std::cerr << "AudioUnit: no AudioComponents metadata in " << canonical << std::endl;
        return 0;
    }
    CFArrayRef components = static_cast<CFArrayRef>(componentsValue);
    size_t registered = 0;
    for (CFIndex i = 0; i < CFArrayGetCount(components); ++i) {
        CFTypeRef item = CFArrayGetValueAtIndex(components, i);
        if (!item || CFGetTypeID(item) != CFDictionaryGetTypeID()) continue;
        CFDictionaryRef metadata = static_cast<CFDictionaryRef>(item);
        CFStringRef factoryName = dictionaryString(metadata, CFSTR("factoryFunction"));
        AudioComponentDescription description{
            fourCCFromString(dictionaryString(metadata, CFSTR("type"))),
            fourCCFromString(dictionaryString(metadata, CFSTR("subtype"))),
            fourCCFromString(dictionaryString(metadata, CFSTR("manufacturer"))), 0, 0
        };
        if (!factoryName || description.componentType == 0 || description.componentSubType == 0
            || description.componentManufacturer == 0) continue;
        if (dictionaryBool(metadata, CFSTR("sandboxSafe")))
            description.componentFlags |= kAudioComponentFlag_SandboxSafe;
        auto factory = reinterpret_cast<AudioComponentFactoryFunction>(
            CFBundleGetFunctionPointerForName(bundle, factoryName));
        if (!factory) {
            std::cerr << "AudioUnit: missing factory " << stringFromCF(factoryName)
                      << " in " << canonical << std::endl;
            continue;
        }
        CFStringRef registeredName = dictionaryString(metadata, CFSTR("name"));
        if (!registeredName) registeredName = CFBundleGetIdentifier(bundle);
        AudioComponent component = AudioComponentRegister(&description, registeredName,
            dictionaryUInt32(metadata, CFSTR("version")), factory);
        if (!component) continue;
        appendUserComponent(ctx, component, description, stringFromCF(registeredName), canonical);
        gUserAudioUnitBundles.back().components.push_back(
            {description, component, stringFromCF(registeredName)});
        ++registered;
    }
    return registered;
}

AudioStreamBasicDescription floatStereo(double sampleRate) {
    AudioStreamBasicDescription format{};
    format.mSampleRate = sampleRate;
    format.mFormatID = kAudioFormatLinearPCM;
    format.mFormatFlags = kAudioFormatFlagsNativeFloatPacked | kAudioFormatFlagIsNonInterleaved;
    format.mBytesPerPacket = sizeof(float);
    format.mFramesPerPacket = 1;
    format.mBytesPerFrame = sizeof(float);
    format.mChannelsPerFrame = 2;
    format.mBitsPerChannel = 32;
    return format;
}

struct StereoBufferList {
    AudioBufferList list;
    AudioBuffer second;
};

StereoBufferList outputList(float* left, float* right, int frames) {
    StereoBufferList result{};
    result.list.mNumberBuffers = 2;
    result.list.mBuffers[0].mNumberChannels = 1;
    result.list.mBuffers[0].mDataByteSize = static_cast<UInt32>(frames * sizeof(float));
    result.list.mBuffers[0].mData = left;
    result.second.mNumberChannels = 1;
    result.second.mDataByteSize = static_cast<UInt32>(frames * sizeof(float));
    result.second.mData = right;
    return result;
}

OSStatus inputRenderCallback(void* refCon, AudioUnitRenderActionFlags*,
                             const AudioTimeStamp*, UInt32, UInt32 frameCount,
                             AudioBufferList* ioData) {
    auto* plugin = static_cast<AudioUnitPlugin*>(refCon);
    if (!plugin || !ioData) return kAudio_ParamError;
    const UInt32 frames = std::min<UInt32>(frameCount,
        static_cast<UInt32>(std::max(0, plugin->renderInputFrames)));
    for (UInt32 buffer = 0; buffer < ioData->mNumberBuffers; ++buffer) {
        float* dst = static_cast<float*>(ioData->mBuffers[buffer].mData);
        if (!dst) continue;
        const float* src = buffer == 0 ? plugin->renderInputLeft : plugin->renderInputRight;
        if (!src) src = plugin->renderInputLeft;
        if (src) std::copy(src, src + frames, dst);
        else std::fill(dst, dst + frames, 0.0f);
        if (frames < frameCount) std::fill(dst + frames, dst + frameCount, 0.0f);
        ioData->mBuffers[buffer].mDataByteSize = frameCount * sizeof(float);
    }
    return noErr;
}

void refreshParameters(AudioUnitPlugin& plugin) {
    plugin.parameterInfo.clear();
    AudioUnit unit = asUnit(plugin);
    if (!unit) return;
    UInt32 size = 0;
    Boolean writable = false;
    if (AudioUnitGetPropertyInfo(unit, kAudioUnitProperty_ParameterList,
            kAudioUnitScope_Global, 0, &size, &writable) != noErr || size == 0) return;
    std::vector<AudioUnitParameterID> ids(size / sizeof(AudioUnitParameterID));
    if (AudioUnitGetProperty(unit, kAudioUnitProperty_ParameterList,
            kAudioUnitScope_Global, 0, ids.data(), &size) != noErr) return;
    for (AudioUnitParameterID id : ids) {
        AudioUnitParameterInfo info{};
        UInt32 infoSize = sizeof(info);
        if (AudioUnitGetProperty(unit, kAudioUnitProperty_ParameterInfo,
                kAudioUnitScope_Global, id, &info, &infoSize) != noErr) continue;
        AudioUnitParamInfo out;
        out.id = id;
        if (info.flags & kAudioUnitParameterFlag_HasCFNameString) {
            if (info.cfNameString) {
                char name[256]{};
                if (CFStringGetCString(info.cfNameString, name, sizeof(name), kCFStringEncodingUTF8)) out.name = name;
            }
        } else out.name = info.name;
        if (out.name.empty()) out.name = "Parameter " + std::to_string(id);
        out.minValue = info.minValue;
        out.maxValue = info.maxValue;
        out.defaultValue = info.defaultValue;
        out.readable = (info.flags & kAudioUnitParameterFlag_IsReadable) != 0;
        out.writable = (info.flags & kAudioUnitParameterFlag_IsWritable) != 0;
        plugin.parameterInfo.push_back(std::move(out));
    }
}

bool configure(AudioUnitPlugin& plugin, Vst3Context& ctx) {
    AudioUnit unit = asUnit(plugin);
    if (!unit) return false;
    UInt32 maximumFrames = static_cast<UInt32>(std::max(1, ctx.blockSize));
    (void)AudioUnitSetProperty(unit, kAudioUnitProperty_MaximumFramesPerSlice,
                              kAudioUnitScope_Global, 0, &maximumFrames, sizeof(maximumFrames));
    AudioStreamBasicDescription format = floatStereo(ctx.sampleRate);
    if (!plugin.isInstrument) {
        if (AudioUnitSetProperty(unit, kAudioUnitProperty_StreamFormat,
                kAudioUnitScope_Input, 0, &format, sizeof(format)) != noErr) return false;
        AURenderCallbackStruct callback{inputRenderCallback, &plugin};
        if (AudioUnitSetProperty(unit, kAudioUnitProperty_SetRenderCallback,
                kAudioUnitScope_Input, 0, &callback, sizeof(callback)) != noErr) return false;
        plugin.inputChannels = 2;
    }
    if (AudioUnitSetProperty(unit, kAudioUnitProperty_StreamFormat,
            kAudioUnitScope_Output, 0, &format, sizeof(format)) != noErr) return false;
    plugin.outputChannels = 2;
    if (AudioUnitInitialize(unit) != noErr) return false;
    plugin.initialized = true;
    refreshParameters(plugin);
    return true;
}

void defaultEditorSetParam(void* userData, int32_t id, double normalized) {
    auto* plugin = static_cast<AudioUnitPlugin*>(userData);
    if (plugin) (void)SetNormalizedParam(*plugin, static_cast<uint32_t>(id), normalized);
}
}

void InitializeBackend(Vst3Context& ctx, const std::string& executableDir) {
    ctx.availablePlugins.erase(std::remove_if(ctx.availablePlugins.begin(), ctx.availablePlugins.end(),
        [](const Vst3AvailablePlugin& available) {
            return available.format == HostedPluginFormat::AudioUnit;
        }), ctx.availablePlugins.end());
    const std::filesystem::path userAudioUnit = std::filesystem::path(executableDir)
        / "USER_AUDIOUNIT";
    std::error_code ec;
    size_t discovered = 0;
    if (std::filesystem::is_directory(userAudioUnit, ec)) {
        std::filesystem::recursive_directory_iterator it(userAudioUnit,
            std::filesystem::directory_options::skip_permission_denied, ec);
        const std::filesystem::recursive_directory_iterator end;
        for (; it != end; it.increment(ec)) {
            if (ec) { ec.clear(); continue; }
            if (it->path().extension() != ".component" || !it->is_directory(ec)) continue;
            const std::filesystem::path bundle = it->path();
            it.disable_recursion_pending();
            discovered += registerUserBundle(ctx, bundle);
        }
    }
    std::sort(ctx.availablePlugins.begin(), ctx.availablePlugins.end(), [](const auto& a, const auto& b) {
        if (a.name != b.name) return a.name < b.name;
        return static_cast<int>(a.format) < static_cast<int>(b.format);
    });
    ctx.browserCacheBuilt = false;
    std::cout << "AudioUnit: discovered " << discovered << " user component(s) in "
              << userAudioUnit << "." << std::endl;
}

AudioUnitPlugin* CreatePlugin(Vst3Context& ctx, const Vst3AvailablePlugin& available, bool instrument) {
    if (available.format != HostedPluginFormat::AudioUnit) return nullptr;
    AudioComponent component = reinterpret_cast<AudioComponent>(available.audioUnitComponent);
    if (!component) return nullptr;
    AudioUnit instance = nullptr;
    if (AudioComponentInstanceNew(component, &instance) != noErr || !instance) return nullptr;
    auto owned = std::make_unique<AudioUnitPlugin>();
    AudioUnitPlugin& plugin = *owned;
    plugin.name = available.name;
    plugin.vendor = available.vendor;
    plugin.uniqueId = available.uniqueId;
    plugin.componentType = available.audioUnitType;
    plugin.componentSubtype = available.audioUnitSubtype;
    plugin.componentManufacturer = available.audioUnitManufacturer;
    plugin.component = component;
    plugin.instance = instance;
    plugin.owner = &ctx;
    plugin.isInstrument = instrument;
    if (!configure(plugin, ctx)) {
        AudioComponentInstanceDispose(instance);
        return nullptr;
    }
    AudioUnitPlugin* result = owned.get();
    ctx.audioUnitPlugins.push_back(std::move(owned));
    return result;
}

void DestroyPlugin(Vst3Context& ctx, AudioUnitPlugin* plugin) {
    if (!plugin) return;
    if (plugin->uiWindow) Vst3UI_CloseWindow(plugin->uiWindow);
    AudioUnit unit = asUnit(*plugin);
    if (unit) {
        if (plugin->initialized) AudioUnitUninitialize(unit);
        AudioComponentInstanceDispose(unit);
    }
    ctx.audioUnitPlugins.erase(std::remove_if(ctx.audioUnitPlugins.begin(), ctx.audioUnitPlugins.end(),
        [&](const std::unique_ptr<AudioUnitPlugin>& owned) { return owned.get() == plugin; }),
        ctx.audioUnitPlugins.end());
}

bool ProcessEffect(Vst3Context&, AudioUnitPlugin& plugin,
                   const float* inputLeft, const float* inputRight,
                   float* outputLeft, float* outputRight, int numFrames,
                   int64_t sampleOffset, bool) {
    if (!plugin.initialized || !outputLeft || !outputRight || numFrames <= 0) return false;
    if (plugin.bypassed.load(std::memory_order_relaxed)) return false;
    plugin.renderInputLeft = inputLeft;
    plugin.renderInputRight = inputRight ? inputRight : inputLeft;
    plugin.renderInputFrames = numFrames;
    auto buffers = outputList(outputLeft, outputRight, numFrames);
    AudioTimeStamp timestamp{};
    timestamp.mFlags = kAudioTimeStampSampleTimeValid;
    timestamp.mSampleTime = static_cast<Float64>(sampleOffset);
    AudioUnitRenderActionFlags flags = 0;
    const OSStatus status = AudioUnitRender(asUnit(plugin), &flags, &timestamp, 0,
                                            static_cast<UInt32>(numFrames), &buffers.list);
    plugin.renderInputLeft = plugin.renderInputRight = nullptr;
    plugin.renderInputFrames = 0;
    return status == noErr;
}

bool ProcessInstrument(Vst3Context& ctx, AudioUnitPlugin& plugin,
                       float* outputLeft, float* outputRight, int numFrames,
                       int64_t sampleOffset, bool,
                       const std::array<float,128>& desired,
                       std::array<float,128>& previous,
                       const std::vector<Microtonal::Voice>* tunedNotes, int tuningMode) {
    if (!plugin.initialized || !outputLeft || !outputRight || numFrames <= 0) return false;
    const bool bypassed = plugin.bypassed.load(std::memory_order_relaxed);
        const auto tuningOutput = TuningOutput(plugin, tuningMode);
        std::vector<Microtonal::Voice> mpeNotes;
        if (tuningOutput == Microtonal::Output::Mpe) {
            mpeNotes = Microtonal::WithMpeNotes(desired, tunedNotes, plugin.tuningVoices, sampleOffset);
            tunedNotes = &mpeNotes;
        }
    AudioUnit unit = asUnit(plugin);
    if (ctx.processPitchBend >= 0.) {
        const int bend=std::clamp(int(std::lround(ctx.processPitchBend*16384.)),0,16383);
        MusicDeviceMIDIEvent(unit,0xe0,bend&127,bend>>7,0);
    }

    for (int note = 0; note < 128; ++note) {
        const bool wasHeld = previous[static_cast<size_t>(note)] > 0.0001f;
        const float velocity = bypassed || tuningOutput == Microtonal::Output::Mpe ? 0.0f : std::clamp(desired[static_cast<size_t>(note)], 0.0f, 1.0f);
        const bool held = velocity > 0.0001f;
        if (wasHeld && !held) MusicDeviceMIDIEvent(unit, 0x80, static_cast<UInt32>(note), 0, 0);
        else if (!wasHeld && held) MusicDeviceMIDIEvent(unit, 0x90, static_cast<UInt32>(note),
            static_cast<UInt32>(std::lround(velocity * 127.0f)), 0);
        previous[static_cast<size_t>(note)] = held ? velocity : 0.0f;
    }
    Microtonal::Dispatch(plugin.tuningVoices, plugin.nextTuningVoiceId, tunedNotes,
        tuningOutput, bypassed,
        [](bool, const Microtonal::ActiveVoice&) {},
        [&](int status, int a, int b) { MusicDeviceMIDIEvent(unit, status, a, b, 0); }, &plugin.tuningMpeZoneActive);
    auto buffers = outputList(outputLeft, outputRight, numFrames);
    AudioTimeStamp timestamp{};
    timestamp.mFlags = kAudioTimeStampSampleTimeValid;
    timestamp.mSampleTime = static_cast<Float64>(sampleOffset);
    AudioUnitRenderActionFlags flags = 0;
    const OSStatus status = AudioUnitRender(unit, &flags, &timestamp, 0,
                                            static_cast<UInt32>(numFrames), &buffers.list);
    if (status != noErr || bypassed) {
        std::fill(outputLeft, outputLeft + numFrames, 0.0f);
        std::fill(outputRight, outputRight + numFrames, 0.0f);
        return false;
    }
    return true;
}

bool SetNormalizedParam(AudioUnitPlugin& plugin, uint32_t id, double normalized) {
    auto it = std::find_if(plugin.parameterInfo.begin(), plugin.parameterInfo.end(),
                           [&](const AudioUnitParamInfo& info) { return info.id == id; });
    if (it == plugin.parameterInfo.end() || !it->writable) return false;
    const float value = it->minValue + static_cast<float>(std::clamp(normalized, 0.0, 1.0))
        * (it->maxValue - it->minValue);
    return AudioUnitSetParameter(asUnit(plugin), id, kAudioUnitScope_Global, 0, value, 0) == noErr;
}

double GetNormalizedParam(const AudioUnitPlugin& plugin, uint32_t id) {
    auto it = std::find_if(plugin.parameterInfo.begin(), plugin.parameterInfo.end(),
                           [&](const AudioUnitParamInfo& info) { return info.id == id; });
    if (it == plugin.parameterInfo.end()) return 0.0;
    AudioUnitParameterValue value = it->defaultValue;
    if (AudioUnitGetParameter(asUnit(plugin), id, kAudioUnitScope_Global, 0, &value) != noErr)
        value = it->defaultValue;
    const double range = static_cast<double>(it->maxValue - it->minValue);
    return range > 0.0 ? std::clamp((value - it->minValue) / range, 0.0, 1.0) : 0.0;
}

bool SaveState(const AudioUnitPlugin& plugin, std::vector<uint8_t>& out) {
    out.clear();
    CFPropertyListRef state = nullptr;
    UInt32 size = sizeof(state);
    if (AudioUnitGetProperty(asUnit(plugin), kAudioUnitProperty_ClassInfo,
            kAudioUnitScope_Global, 0, &state, &size) != noErr || !state) return false;
    CFErrorRef error = nullptr;
    CFDataRef data = CFPropertyListCreateData(kCFAllocatorDefault, state,
        kCFPropertyListBinaryFormat_v1_0, 0, &error);
    CFRelease(state);
    if (error) CFRelease(error);
    if (!data) return false;
    const UInt8* bytes = CFDataGetBytePtr(data);
    const CFIndex length = CFDataGetLength(data);
    if (bytes && length > 0) out.assign(bytes, bytes + length);
    CFRelease(data);
    return !out.empty();
}

bool LoadState(AudioUnitPlugin& plugin, const uint8_t* data, size_t size) {
    if (!data || size == 0 || size > static_cast<size_t>(std::numeric_limits<CFIndex>::max())) return false;
    CFDataRef serialized = CFDataCreate(kCFAllocatorDefault, data, static_cast<CFIndex>(size));
    if (!serialized) return false;
    CFErrorRef error = nullptr;
    CFPropertyListRef state = CFPropertyListCreateWithData(kCFAllocatorDefault, serialized,
        kCFPropertyListImmutable, nullptr, &error);
    CFRelease(serialized);
    if (error) CFRelease(error);
    if (!state) return false;
    const OSStatus status = AudioUnitSetProperty(asUnit(plugin), kAudioUnitProperty_ClassInfo,
        kAudioUnitScope_Global, 0, &state, sizeof(state));
    CFRelease(state);
    if (status == noErr) refreshParameters(plugin);
    return status == noErr;
}

void OpenPluginUI(AudioUnitPlugin& plugin) {
    if (plugin.uiWindow) {
        Vst3UI_ShowWindow(plugin.uiWindow);
        plugin.uiVisible = true;
        return;
    }
    std::vector<Vst3DefaultEditorParam> params;
    params.reserve(std::min<size_t>(plugin.parameterInfo.size(), 128));
    for (const auto& info : plugin.parameterInfo) {
        if (!info.readable || params.size() >= 128) continue;
        Vst3DefaultEditorParam param{};
        param.id = static_cast<int32_t>(info.id);
        param.value = GetNormalizedParam(plugin, info.id);
        std::snprintf(param.title, sizeof(param.title), "%s", info.name.c_str());
        std::snprintf(param.units, sizeof(param.units), "%s", info.units.c_str());
        params.push_back(param);
    }
    plugin.uiWindow = Vst3UI_CreateDefaultEditorWindow(plugin.name.c_str(), params.data(),
        static_cast<int>(params.size()), defaultEditorSetParam, &plugin);
    if (plugin.uiWindow) {
        Vst3UI_ShowWindow(plugin.uiWindow);
        plugin.uiVisible = true;
    }
}

void CleanupBackend(Vst3Context& ctx) {
    while (!ctx.audioUnitPlugins.empty()) DestroyPlugin(ctx, ctx.audioUnitPlugins.back().get());
}

}
#endif
#if defined(__APPLE__)
namespace AudioUnitSystemLogic {
void Reset(AudioUnitPlugin& p){if(asUnit(p))AudioUnitReset(asUnit(p),kAudioUnitScope_Global,0);}
void Reconfigure(Vst3Context& ctx,AudioUnitPlugin& p){
    if(p.initialized){AudioUnitUninitialize(asUnit(p));p.initialized=false;}
    if(!configure(p,ctx))throw std::runtime_error("Audio Unit rejected sample rate");
}
}
#endif
