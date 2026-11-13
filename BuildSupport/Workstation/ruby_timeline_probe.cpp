// Copied finite stationary timeline diagnostic; parent owns compilation/runs.
// Production timeline IDs and variable mode1 spans are checked before dispatch.
// Alpha must be exporter timeline cache-use counts (integer 0..2).
// Finite, headless wgpu-native 27 split convex-bake/lighting-resolve probe.
// Warm timings exclude the one startup bake. Cold timings include a new bake
// before every frame; baseline disables the same shader's cache through plan.
// Optional resolve includes a separate lighting pass in every timed frame.
// resolve-once is explicitly limited to this immutable warm fixture.
// If cs_ruby_response_general exists, panel bake and canonical fallback/control
// bake run as separate passes. Both are included in intrinsic-bake timings.
// Build/run is deliberately external; the canonical probes are unchanged.
#include <wgpu.h>

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <limits>
#include <memory>
#include <mutex>
#include <sstream>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>
#include <utility>

using Clock = std::chrono::steady_clock;
static constexpr uint64_t maximum_buffer_bytes = 128ull * 1024 * 1024;
static constexpr uint64_t maximum_shader_bytes = 8ull * 1024 * 1024;
static constexpr uint64_t cache_bytes = 120ull * 1024 * 1024;
static constexpr uint32_t maximum_entries = 20, entry_rows = 8;
static constexpr uint32_t descriptor_rows = 1 + maximum_entries * entry_rows;
static constexpr uint32_t mapped_targets = 16644 + 4096;
static constexpr uint64_t plan_rows = descriptor_rows + (mapped_targets + 3) / 4;
static constexpr uint64_t plan_bytes = plan_rows * 16;
static double milliseconds(Clock::time_point start, Clock::time_point end) {
    return std::chrono::duration<double, std::milli>(end - start).count();
}
static WGPUStringView view(const std::string &text) { return {text.data(), text.size()}; }
static WGPUStringView literal(const char *text) { return {text, std::strlen(text)}; }
static std::string text(WGPUStringView value) {
    if (!value.data) return {};
    return {value.data, value.length == WGPU_STRLEN ? std::strlen(value.data) : value.length};
}
static std::string quoted(const std::string &value) {
    std::ostringstream out;
    out << '"';
    for (unsigned char c : value) {
        if (c == '"' || c == '\\') out << '\\' << c;
        else if (c == '\n') out << "\\n";
        else if (c == '\r') out << "\\r";
        else if (c == '\t') out << "\\t";
        else if (c < 32) out << "\\u" << std::hex << std::setw(4) << std::setfill('0') << unsigned(c) << std::dec;
        else out << c;
    }
    out << '"';
    return out.str();
}
static std::vector<char> read_file(const std::string &path, uint64_t maximum) {
    std::ifstream file(path, std::ios::binary | std::ios::ate);
    if (!file) throw std::runtime_error("Cannot open " + path);
    auto length = file.tellg();
    if (length <= 0) throw std::runtime_error("Empty input: " + path);
    if (static_cast<uint64_t>(length) > maximum ||
        static_cast<uint64_t>(length) > std::numeric_limits<size_t>::max() ||
        static_cast<uint64_t>(length) > static_cast<uint64_t>(std::numeric_limits<std::streamsize>::max()))
        throw std::runtime_error("Input exceeds bounded read size: " + path);
    std::vector<char> bytes(static_cast<size_t>(length));
    file.seekg(0);
    file.read(bytes.data(), static_cast<std::streamsize>(bytes.size()));
    if (!file) throw std::runtime_error("Cannot read " + path);
    return bytes;
}
static void write_file(const std::string &path, const void *data, size_t size) {
    if (size > maximum_buffer_bytes ||
        size > static_cast<uint64_t>(std::numeric_limits<std::streamsize>::max()))
        throw std::runtime_error("Output exceeds bounded write size: " + path);
    std::ofstream file(path, std::ios::binary);
    if (!file) throw std::runtime_error("Cannot create " + path);
    file.write(static_cast<const char *>(data), static_cast<std::streamsize>(size));
    if (!file) throw std::runtime_error("Cannot write " + path);
}
static uint32_t integer(const char *arg, uint32_t maximum) {
    std::string source(arg);
    size_t consumed = 0;
    auto value = std::stoull(source, &consumed);
    if (consumed != source.size() || value == 0 || value > maximum)
        throw std::runtime_error("Invalid positive integer: " + source);
    return static_cast<uint32_t>(value);
}

// Callback storage outlives GPU handles even on a timeout/error path. The
// callbacks publish their strings/handles before the release-store to done.
struct Completion {
    std::atomic<bool> done{false};
    bool success = false;
    std::string message;
    WGPUAdapter adapter = nullptr;
    WGPUDevice device = nullptr;
};
static std::vector<std::unique_ptr<Completion>> completions;
static Completion *completion() {
    completions.emplace_back(std::make_unique<Completion>());
    return completions.back().get();
}
static std::atomic<bool> gpu_failed{false};
static std::mutex error_mutex;
static std::string gpu_error;
static void report_gpu_error(const std::string &message) {
    {
        std::lock_guard<std::mutex> lock(error_mutex);
        gpu_error = message;
        std::cerr << "GPU error: " << message << '\n';
    }
    gpu_failed.store(true, std::memory_order_release);
}
static void check_gpu_error() {
    if (gpu_failed.load(std::memory_order_acquire)) {
        std::lock_guard<std::mutex> lock(error_mutex);
        throw std::runtime_error(gpu_error);
    }
}
static void finish(Completion *state, bool success, WGPUStringView message = {}) {
    state->success = success;
    state->message = text(message);
    state->done.store(true, std::memory_order_release);
}
static void await(Completion *state, WGPUInstance instance, WGPUDevice device,
                  const char *operation, double timeout_seconds = 60.) {
    (void)instance;
    auto start = Clock::now();
    while (!state->done.load(std::memory_order_acquire)) {
        // wgpu-native's device polling drives queue/map completion. Every
        // callback here allows spontaneous delivery, including adapter/device
        // creation; do not depend on optional browser-style instance futures.
        if (device) wgpuDevicePoll(device, false, nullptr);
        check_gpu_error();
        if (milliseconds(start, Clock::now()) > timeout_seconds * 1000.)
            throw std::runtime_error(std::string(operation) + " callback timed out");
        if (!state->done.load(std::memory_order_acquire))
            std::this_thread::sleep_for(std::chrono::microseconds(250));
    }
    check_gpu_error();
    if (!state->success)
        throw std::runtime_error(std::string(operation) + " failed: " + state->message);
}

struct Context {
    WGPUInstance instance = nullptr;
    WGPUAdapter adapter = nullptr;
    WGPUDevice device = nullptr;
    WGPUQueue queue = nullptr;
    WGPUShaderModule shader = nullptr;
    WGPUBindGroupLayout bind_layout = nullptr;
    WGPUPipelineLayout pipeline_layout = nullptr;
    WGPUComputePipeline pipeline = nullptr;
    WGPUComputePipeline bake_pipeline = nullptr;
    WGPUComputePipeline general_bake_pipeline = nullptr;
    WGPUComputePipeline resolve_pipeline = nullptr;
    WGPUBindGroup bind_group = nullptr;
    WGPUBindGroup bake_bind_group = nullptr;
    WGPUQuerySet queries = nullptr;
    std::vector<WGPUBuffer> buffers;
    ~Context() {
        if (bind_group) wgpuBindGroupRelease(bind_group);
        if (bake_bind_group) wgpuBindGroupRelease(bake_bind_group);
        if (pipeline) wgpuComputePipelineRelease(pipeline);
        if (bake_pipeline) wgpuComputePipelineRelease(bake_pipeline);
        if (general_bake_pipeline) wgpuComputePipelineRelease(general_bake_pipeline);
        if (resolve_pipeline) wgpuComputePipelineRelease(resolve_pipeline);
        if (pipeline_layout) wgpuPipelineLayoutRelease(pipeline_layout);
        if (bind_layout) wgpuBindGroupLayoutRelease(bind_layout);
        if (shader) wgpuShaderModuleRelease(shader);
        if (queries) wgpuQuerySetRelease(queries);
        for (auto buffer : buffers) wgpuBufferRelease(buffer);
        if (queue) wgpuQueueRelease(queue);
        if (device) { wgpuDeviceDestroy(device); wgpuDeviceRelease(device); }
        if (adapter) wgpuAdapterRelease(adapter);
        if (instance) wgpuInstanceRelease(instance);
    }
    WGPUBuffer buffer(uint64_t size, WGPUBufferUsage usage, const char *label) {
        if (size == 0 || size > maximum_buffer_bytes || size % 4)
            throw std::runtime_error(std::string("Invalid bounded buffer size: ") + label);
        WGPUBufferDescriptor descriptor{};
        descriptor.label = literal(label);
        descriptor.size = size;
        descriptor.usage = usage;
        auto result = wgpuDeviceCreateBuffer(device, &descriptor);
        if (!result) throw std::runtime_error(std::string("Creating buffer: ") + label);
        buffers.push_back(result);
        return result;
    }
    void pop_error_scope(const char *operation) {
        auto state = completion();
        WGPUPopErrorScopeCallbackInfo callback{};
        callback.mode = WGPUCallbackMode_AllowSpontaneous;
        callback.userdata1 = state;
        callback.callback = [](WGPUPopErrorScopeStatus status, WGPUErrorType error,
                               WGPUStringView message, void *data, void *) {
            finish(static_cast<Completion *>(data), status == WGPUPopErrorScopeStatus_Success &&
                   error == WGPUErrorType_NoError, message);
        };
        wgpuDevicePopErrorScope(device, callback);
        await(state, instance, device, operation);
    }
    void wait_queue() {
        auto state = completion();
        WGPUQueueWorkDoneCallbackInfo callback{};
        callback.mode = WGPUCallbackMode_AllowSpontaneous;
        callback.userdata1 = state;
        callback.callback = [](WGPUQueueWorkDoneStatus status, void *data, void *) {
            finish(static_cast<Completion *>(data), status == WGPUQueueWorkDoneStatus_Success);
        };
        wgpuQueueOnSubmittedWorkDone(queue, callback);
        await(state, instance, device, "queue completion");
    }
    std::vector<char> readback(WGPUBuffer buffer, size_t size) {
        if (size == 0 || size > maximum_buffer_bytes || size % 4)
            throw std::runtime_error("Invalid bounded readback size");
        auto state = completion();
        WGPUBufferMapCallbackInfo callback{};
        callback.mode = WGPUCallbackMode_AllowSpontaneous;
        callback.userdata1 = state;
        callback.callback = [](WGPUMapAsyncStatus status, WGPUStringView message, void *data, void *) {
            finish(static_cast<Completion *>(data), status == WGPUMapAsyncStatus_Success, message);
        };
        wgpuBufferMapAsync(buffer, WGPUMapMode_Read, 0, size, callback);
        await(state, instance, device, "buffer mapping");
        const void *mapped = wgpuBufferGetConstMappedRange(buffer, 0, size);
        if (!mapped) throw std::runtime_error("Mapped range is null");
        std::vector<char> result(size);
        std::memcpy(result.data(), mapped, size);
        wgpuBufferUnmap(buffer);
        return result;
    }
};

struct Timing {
    double encode_ms = 0, submit_complete_ms = 0, cpu_ms = 0;
    double gpu_ms = std::numeric_limits<double>::quiet_NaN();
    double bake_gpu_ms = 0, resolve_gpu_ms = 0, shade_gpu_ms = std::numeric_limits<double>::quiet_NaN();
    double first_bake_gpu_ms = 0, general_bake_gpu_ms = 0;
};
static std::string compute_entry(uint32_t width, uint32_t height, uint32_t group,
                                 bool invocation_hook) {
    std::ostringstream out;
    out << "\n// Headless diagnostic only: full-scene shade after optional cache bake.\n"
        << "struct WorkstationComputeOutput { pixels: array<vec4<f32>> };\n"
        << "@group(0) @binding(4) var<storage,read_write> workstationComputeOutput: WorkstationComputeOutput;\n"
        << "@compute @workgroup_size(" << group << "," << group << ",1)\n"
        << "fn cs_main(@builtin(global_invocation_id) id:vec3<u32>, @builtin(local_invocation_index) localIndex:u32) {\n"
        << " if(id.x >= " << width << "u || id.y >= " << height << "u){return;}\n"
        << (invocation_hook ? " workstationProbeInvocation(localIndex);\n" : "")
        << " let screen=vec2<f32>((f32(id.x)+0.5)*2.0/" << width << ".0-1.0,"
        << "1.0-(f32(id.y)+0.5)*2.0/" << height << ".0);\n";
    out << " workstationComputeOutput.pixels[id.y*" << width << "u+id.x]=workstationShade(screen);\n";
    out << "}\n";
    return out.str();
}

int main(int argc, char **argv) {
    std::string prefix;
    try {
        if (argc < 8 || argc > 10) throw std::runtime_error(
            "usage: ruby_timeline_probe shader.wgsl frame.bin plan.bin width height output-prefix iterations [baseline|cold|warm] [resolve|resolve-once]");
        uint32_t width = integer(argv[4], 16384), height = integer(argv[5], 16384);
        uint32_t iterations = integer(argv[7], 1000);
        constexpr uint32_t group = 8;
        const std::string mode = argc >= 9 ? argv[8] : "warm";
        if (mode != "baseline" && mode != "cold" && mode != "warm")
            throw std::runtime_error("Mode must be baseline, cold or warm");
        const std::string resolve_mode = argc >= 10 ? argv[9] : "none";
        const bool moving_frame = argc == 12;
        if (moving_frame && (mode != "warm" || resolve_mode != "resolve"))
            throw std::runtime_error("Moving fixture requires warm startup and per-frame resolve");
        if (resolve_mode != "none" && resolve_mode != "resolve" && resolve_mode != "resolve-once")
            throw std::runtime_error("Lighting mode must be resolve or resolve-once");
        const bool resolve_requested = resolve_mode != "none";
        const bool resolve_once = resolve_mode == "resolve-once";
        if (resolve_once && mode != "warm")
            throw std::runtime_error("resolve-once requires the immutable warm fixture");
        const uint64_t output_rows = uint64_t(width) * height;
        const uint64_t output_bytes = output_rows * 4 * sizeof(float);
        // Arguments are bounded before multiplication, so this cannot overflow
        // uint64_t. Keep both GPU and CPU readback allocations within 128 MiB.
        if (output_bytes > maximum_buffer_bytes || output_bytes > std::numeric_limits<size_t>::max())
            throw std::runtime_error("Probe output exceeds the 128 MiB limit");
        prefix = argv[6];
        auto parent = std::filesystem::path(prefix).parent_path();
        if (!parent.empty()) std::filesystem::create_directories(parent);
        auto shader_bytes = read_file(argv[1], maximum_shader_bytes);
        std::string source(shader_bytes.begin(), shader_bytes.end());
        if (source.find("fn cs_main(") != std::string::npos)
            throw std::runtime_error("Pass canonical WGSL; the probe appends its compute entry itself");
        if (source.find("fn cs_ruby_response(") == std::string::npos)
            throw std::runtime_error("Shader must include cs_ruby_response bake entry");
        const bool split_bake = source.find("fn cs_ruby_response_general(") != std::string::npos;
        const bool has_resolver = source.find("fn cs_ruby_resolve(") != std::string::npos;
        if (resolve_requested && !has_resolver)
            throw std::runtime_error("Resolve mode requires cs_ruby_resolve shader entry");
        if (has_resolver && !resolve_requested && mode != "baseline")
            throw std::runtime_error("Transformed shader requires an explicit lighting resolve mode");
        bool invocation_hook = source.find("fn workstationProbeInvocation(") != std::string::npos;
        source += compute_entry(width, height, group, invocation_hook);
        write_file(prefix + ".compute.wgsl", source.data(), source.size());
        auto frame = read_file(argv[2], maximum_buffer_bytes);
        if (frame.size() < 256 * 16 || frame.size() % 16)
            throw std::runtime_error("frame.bin must contain complete RGBA32F optical rows");
        for (size_t offset = 0; offset < frame.size(); offset += sizeof(float)) {
            float value;
            std::memcpy(&value, frame.data() + offset, sizeof(value));
            if (!std::isfinite(value)) throw std::runtime_error("Nonfinite optical-frame value");
        }
        float resolution[2];
        std::memcpy(resolution, frame.data() + 8 * 16, sizeof(resolution));
        if (resolution[0] != static_cast<float>(width) || resolution[1] != static_cast<float>(height))
            throw std::runtime_error("frame row8 resolution differs from width/height; refusing to alter sampling");
        auto plan = read_file(argv[3], plan_bytes);
        if (plan.size() != plan_bytes)
            throw std::runtime_error("plan.bin must contain 5346 RGBA32F rows, including the complete target-to-slot map");
        std::vector<float> plan_values(plan.size() / sizeof(float));
        std::memcpy(plan_values.data(), plan.data(), plan.size());
        for (float value : plan_values)
            if (!std::isfinite(value)) throw std::runtime_error("Nonfinite response-plan value");
        const float count = plan_values[0];
        if (count < 0 || count > maximum_entries || count != std::floor(count))
            throw std::runtime_error("Plan entry count is outside 0..20");
        const uint32_t entry_count = static_cast<uint32_t>(count);
        const bool plan_enabled = plan_values[1] >= .5f && entry_count > 0;
        plan_values[1] = mode == "baseline" || !plan_enabled ? 0.f : 1.f;
        uint32_t active_entry_count = 0;
        std::vector<std::pair<uint64_t,uint64_t>> response_spans;
        for (uint32_t i = 0; i < entry_count; ++i) {
            const size_t descriptor_index = (1 + i * entry_rows) * 4;
            const size_t state_index = descriptor_index + 3;
            const float state = plan_values[state_index];
            // Slots 0..3 belong to panels even when those panels are hidden;
            // entry_count is a dispatch extent, not a dense active-entry count.
            if (state == 0.f) continue;
            if (state != 1.f && state != 2.f)
                throw std::runtime_error("Active plan descriptors must be ready (1) or bake (2)");
            ++active_entry_count;
            const float target = plan_values[descriptor_index];
            const float edge = plan_values[descriptor_index + 1];
            const float base = plan_values[descriptor_index + 2];
            const bool timeline = target >= 700000.f && target < 704096.f;
            if (target < 0 || (target >= 16644.f && !timeline) || target != std::floor(target))
                throw std::runtime_error("Plan descriptor target is outside production ranges");
            if (base < 0 || base != std::floor(base) || base > float(cache_bytes / 16))
                throw std::runtime_error("Plan descriptor base is outside bounded response storage");
            const float pixel_mode = plan_values[descriptor_index + 4 * 4 + 3];
            uint64_t span = 0;
            if (pixel_mode == 1.f) {
                const float w = plan_values[descriptor_index + 4 * 4];
                const float h = plan_values[descriptor_index + 4 * 4 + 1];
                const float stride = plan_values[descriptor_index + 4 * 4 + 2];
                const float allocated = plan_values[descriptor_index + 6 * 4];
                const float x = plan_values[descriptor_index + 5 * 4];
                const float y = plan_values[descriptor_index + 5 * 4 + 1];
                const bool original_glass = target >= 4.f + 4.f*256.f && target < 4.f + 10.f*256.f;
                if ((!timeline && !original_glass) || (timeline && uint32_t(target-700000.f)%16u != 0u)
                    || edge != 64.f || stride != 108.f || w < 1.f || h < 1.f
                    || w != std::floor(w) || h != std::floor(h) || double(w)*h > 16384.
                    || allocated < 1.f || allocated != std::floor(allocated)
                    || allocated > float(cache_bytes / 16) || double(w)*h*108. > allocated
                    || base < float(4u*128u*128u*60u)
                    || x < 0.f || y < 0.f || x != std::floor(x) || y != std::floor(y)
                    || double(x)+w > width || double(y)+h > height)
                    throw std::runtime_error("Invalid exact-pixel descriptor shape/allocation/rectangle");
                span = uint64_t(allocated);
            } else {
                if (pixel_mode != 0.f || timeline || (edge != 64.f && edge != 128.f))
                    throw std::runtime_error("Legacy descriptors cannot contain timeline/pixel modes");
                span = uint64_t(edge)*uint64_t(edge)*60u;
            }
            if (span > cache_bytes/16 - uint64_t(base))
                throw std::runtime_error("Plan descriptor response span exceeds bounded cache storage");
            response_spans.emplace_back(uint64_t(base),uint64_t(base)+span);
            // This process owns a fresh cache, so even an already-ready saved
            // descriptor must be baked once before its first shading pass.
            plan_values[state_index] = 2.f;
        }
        std::sort(response_spans.begin(),response_spans.end());
        for (size_t i=1; i<response_spans.size(); ++i)
            if (response_spans[i-1].second > response_spans[i].first)
                throw std::runtime_error("Active production response allocations overlap");
        for (uint32_t target = 0; plan_enabled && target < mapped_targets; ++target) {
            const float slot = plan_values[descriptor_rows * 4 + target];
            if (slot < -1.f || slot != std::floor(slot) || slot >= float(entry_count))
                throw std::runtime_error("Target-to-slot map contains an invalid cache slot");
            if (slot >= 0) {
                const size_t descriptor_index = (1 + uint32_t(slot) * entry_rows) * 4;
                const uint32_t component = target < 16644u ? target : 700000u+(target-16644u);
                if (plan_values[descriptor_index + 3] == 0.f || plan_values[descriptor_index] != float(component))
                    throw std::runtime_error("Target-to-slot map does not match an active response descriptor");
            }
        }
        const bool bake_enabled = mode != "baseline" && plan_enabled && active_entry_count > 0;
        const bool resolve_enabled = resolve_requested && bake_enabled;
        std::memcpy(plan.data(), plan_values.data(), plan.size());

        Context context;
        WGPUInstanceExtras extra{};
        extra.chain.sType = static_cast<WGPUSType>(WGPUSType_InstanceExtras);
        extra.backends = WGPUInstanceBackend_Metal;
        WGPUInstanceDescriptor instance_descriptor{};
        instance_descriptor.nextInChain = &extra.chain;
        context.instance = wgpuCreateInstance(&instance_descriptor);
        if (!context.instance) throw std::runtime_error("Cannot create Metal WebGPU instance");
        auto adapter_state = completion();
        WGPURequestAdapterOptions adapter_options{};
        adapter_options.featureLevel = WGPUFeatureLevel_Core;
        adapter_options.powerPreference = WGPUPowerPreference_HighPerformance;
        adapter_options.backendType = WGPUBackendType_Metal;
        WGPURequestAdapterCallbackInfo adapter_callback{};
        adapter_callback.mode = WGPUCallbackMode_AllowSpontaneous;
        adapter_callback.userdata1 = adapter_state;
        adapter_callback.callback = [](WGPURequestAdapterStatus status, WGPUAdapter adapter,
                                      WGPUStringView message, void *data, void *) {
            auto state = static_cast<Completion *>(data);
            state->adapter = adapter;
            finish(state, status == WGPURequestAdapterStatus_Success && adapter, message);
        };
        wgpuInstanceRequestAdapter(context.instance, &adapter_options, adapter_callback);
        await(adapter_state, context.instance, nullptr, "adapter request");
        context.adapter = adapter_state->adapter;
        WGPUAdapterInfo adapter_info{};
        if (wgpuAdapterGetInfo(context.adapter, &adapter_info) != WGPUStatus_Success)
            throw std::runtime_error("Cannot get adapter info");
        std::string adapter_name = text(adapter_info.device);
        std::string adapter_description = text(adapter_info.description);
        auto backend = adapter_info.backendType;
        wgpuAdapterInfoFreeMembers(adapter_info);
        if (backend != WGPUBackendType_Metal) throw std::runtime_error("Requested Metal, received another backend");
        bool timestamps = wgpuAdapterHasFeature(context.adapter, WGPUFeatureName_TimestampQuery);
        WGPUFeatureName timestamp_feature = WGPUFeatureName_TimestampQuery;
        WGPUDeviceDescriptor device_descriptor{};
        device_descriptor.label = literal("Workstation exact-kernel compute probe");
        device_descriptor.defaultQueue.label = literal("Workstation probe queue");
        device_descriptor.requiredFeatureCount = timestamps ? 1 : 0;
        device_descriptor.requiredFeatures = timestamps ? &timestamp_feature : nullptr;
        device_descriptor.uncapturedErrorCallbackInfo.callback =
            [](WGPUDevice const *, WGPUErrorType, WGPUStringView message, void *, void *) {
                report_gpu_error(text(message));
            };
        device_descriptor.deviceLostCallbackInfo.mode = WGPUCallbackMode_AllowSpontaneous;
        device_descriptor.deviceLostCallbackInfo.callback =
            [](WGPUDevice const *, WGPUDeviceLostReason reason, WGPUStringView message, void *, void *) {
                if (reason != WGPUDeviceLostReason_Destroyed && reason != WGPUDeviceLostReason_InstanceDropped)
                    report_gpu_error("Device lost: " + text(message));
            };
        auto device_state = completion();
        WGPURequestDeviceCallbackInfo device_callback{};
        device_callback.mode = WGPUCallbackMode_AllowSpontaneous;
        device_callback.userdata1 = device_state;
        device_callback.callback = [](WGPURequestDeviceStatus status, WGPUDevice device,
                                     WGPUStringView message, void *data, void *) {
            auto state = static_cast<Completion *>(data);
            state->device = device;
            finish(state, status == WGPURequestDeviceStatus_Success && device, message);
        };
        wgpuAdapterRequestDevice(context.adapter, &device_descriptor, device_callback);
        await(device_state, context.instance, nullptr, "device request");
        context.device = device_state->device;
        context.queue = wgpuDeviceGetQueue(context.device);
        WGPULimits limits{};
        if (wgpuDeviceGetLimits(context.device, &limits) != WGPUStatus_Success)
            throw std::runtime_error("Cannot get device limits");
        const uint64_t largest_buffer = std::max({uint64_t(frame.size()), output_bytes, cache_bytes});
        if (largest_buffer > limits.maxStorageBufferBindingSize || largest_buffer > limits.maxBufferSize ||
            limits.maxStorageBuffersPerShaderStage < 5)
            throw std::runtime_error("Probe input/output exceeds device storage-buffer limits");
        float timestamp_period = timestamps ? wgpuQueueGetTimestampPeriod(context.queue) : 0.f;
        if (timestamps && !(timestamp_period > 0)) throw std::runtime_error("Invalid timestamp period");

        auto dummy = context.buffer(96, WGPUBufferUsage_Storage | WGPUBufferUsage_CopyDst, "Unused vertex appearance");
        auto optical = context.buffer(frame.size(), WGPUBufferUsage_Storage | WGPUBufferUsage_CopyDst, "Complete optical frame");
        auto cache = context.buffer(cache_bytes, WGPUBufferUsage_Storage, "Ruby optical response cache");
        auto bake_plan = context.buffer(plan_bytes, WGPUBufferUsage_Storage | WGPUBufferUsage_CopyDst, "Ruby bake plan");
        auto ready_plan = context.buffer(plan_bytes, WGPUBufferUsage_Storage | WGPUBufferUsage_CopyDst, "Ruby ready plan");
        auto output = context.buffer(output_bytes, WGPUBufferUsage_Storage | WGPUBufferUsage_CopySrc, "Compute RGBA32F output");
        auto output_readback = context.buffer(output_bytes, WGPUBufferUsage_CopyDst | WGPUBufferUsage_MapRead, "Image readback");
        wgpuQueueWriteBuffer(context.queue, optical, 0, frame.data(), frame.size());
        wgpuQueueWriteBuffer(context.queue, bake_plan, 0, plan.data(), plan.size());
        for (uint32_t i = 0; i < entry_count; ++i) {
            const size_t state_index = (1 + i * entry_rows) * 4 + 3;
            if (plan_values[state_index] == 2.f) plan_values[state_index] = 1.f;
        }
        wgpuQueueWriteBuffer(context.queue, ready_plan, 0, plan_values.data(), plan.size());
        WGPUBindGroupLayoutEntry layout_entries[5]{};
        uint64_t sizes[5] = {96, frame.size(), cache_bytes, plan_bytes, output_bytes};
        WGPUBuffer handles[5] = {dummy, optical, cache, ready_plan, output};
        WGPUBindGroupEntry bindings[5]{};
        for (uint32_t i = 0; i < 5; ++i) {
            layout_entries[i].binding = i;
            layout_entries[i].visibility = WGPUShaderStage_Compute;
            layout_entries[i].buffer.type = (i == 2 || i == 4) ? WGPUBufferBindingType_Storage : WGPUBufferBindingType_ReadOnlyStorage;
            layout_entries[i].buffer.minBindingSize = sizes[i];
            bindings[i].binding = i;
            bindings[i].buffer = handles[i];
            bindings[i].size = sizes[i];
        }
        WGPUBindGroupLayoutDescriptor bind_layout_descriptor{};
        bind_layout_descriptor.entryCount = 5;
        bind_layout_descriptor.entries = layout_entries;
        context.bind_layout = wgpuDeviceCreateBindGroupLayout(context.device, &bind_layout_descriptor);
        WGPUPipelineLayoutDescriptor pipeline_layout_descriptor{};
        pipeline_layout_descriptor.bindGroupLayoutCount = 1;
        pipeline_layout_descriptor.bindGroupLayouts = &context.bind_layout;
        context.pipeline_layout = wgpuDeviceCreatePipelineLayout(context.device, &pipeline_layout_descriptor);
        WGPUBindGroupDescriptor bind_descriptor{};
        bind_descriptor.layout = context.bind_layout;
        bind_descriptor.entryCount = 5;
        bind_descriptor.entries = bindings;
        context.bind_group = wgpuDeviceCreateBindGroup(context.device, &bind_descriptor);
        bindings[3].buffer = bake_plan;
        context.bake_bind_group = wgpuDeviceCreateBindGroup(context.device, &bind_descriptor);
        if (!context.bind_layout || !context.pipeline_layout || !context.bind_group || !context.bake_bind_group)
            throw std::runtime_error("Null response-cache pipeline layout or bind group");
        auto compile_start = Clock::now();
        wgpuDevicePushErrorScope(context.device, WGPUErrorFilter_Validation);
        WGPUShaderSourceWGSL wgsl{};
        wgsl.chain.sType = WGPUSType_ShaderSourceWGSL;
        wgsl.code = view(source);
        WGPUShaderModuleDescriptor shader_descriptor{};
        shader_descriptor.nextInChain = &wgsl.chain;
        shader_descriptor.label = literal("Canonical workstation plus compute entry");
        context.shader = wgpuDeviceCreateShaderModule(context.device, &shader_descriptor);
        context.pop_error_scope("shader creation");
        if (!context.shader) throw std::runtime_error("Null shader module");
        wgpuDevicePushErrorScope(context.device, WGPUErrorFilter_Validation);
        WGPUComputePipelineDescriptor pipeline_descriptor{};
        pipeline_descriptor.layout = context.pipeline_layout;
        pipeline_descriptor.compute.module = context.shader;
        pipeline_descriptor.compute.entryPoint = literal("cs_main");
        context.pipeline = wgpuDeviceCreateComputePipeline(context.device, &pipeline_descriptor);
        context.pop_error_scope("pipeline creation");
        if (!context.pipeline) throw std::runtime_error("Null compute pipeline");
        wgpuDevicePushErrorScope(context.device, WGPUErrorFilter_Validation);
        pipeline_descriptor.compute.entryPoint = literal("cs_ruby_response");
        context.bake_pipeline = wgpuDeviceCreateComputePipeline(context.device, &pipeline_descriptor);
        context.pop_error_scope("bake pipeline creation");
        if (!context.bake_pipeline) throw std::runtime_error("Null bake compute pipeline");
        if (split_bake) {
            wgpuDevicePushErrorScope(context.device, WGPUErrorFilter_Validation);
            pipeline_descriptor.compute.entryPoint = literal("cs_ruby_response_general");
            context.general_bake_pipeline = wgpuDeviceCreateComputePipeline(context.device, &pipeline_descriptor);
            context.pop_error_scope("general bake pipeline creation");
            if (!context.general_bake_pipeline) throw std::runtime_error("Null general bake compute pipeline");
        }
        if (resolve_requested) {
            wgpuDevicePushErrorScope(context.device, WGPUErrorFilter_Validation);
            pipeline_descriptor.compute.entryPoint = literal("cs_ruby_resolve");
            context.resolve_pipeline = wgpuDeviceCreateComputePipeline(context.device, &pipeline_descriptor);
            context.pop_error_scope("lighting resolve pipeline creation");
            if (!context.resolve_pipeline) throw std::runtime_error("Null lighting resolve compute pipeline");
        }
        double compile_ms = milliseconds(compile_start, Clock::now());
        WGPUBuffer query_resolve = nullptr, query_readback = nullptr;
        if (timestamps) {
            WGPUQuerySetDescriptor query_descriptor{};
            query_descriptor.type = WGPUQueryType_Timestamp;
            query_descriptor.count = 8;
            context.queries = wgpuDeviceCreateQuerySet(context.device, &query_descriptor);
            query_resolve = context.buffer(256, WGPUBufferUsage_QueryResolve | WGPUBufferUsage_CopySrc, "Timestamp resolution");
            query_readback = context.buffer(64, WGPUBufferUsage_CopyDst | WGPUBufferUsage_MapRead, "Timestamp readback");
        }
        context.wait_queue(); // Initial uploads are outside all timed dispatches.
        auto dispatch = [&](bool do_bake, bool do_shade) {
            const bool do_general_bake = do_bake && split_bake;
            const bool do_resolve = resolve_enabled && (do_bake || (do_shade && !resolve_once));
            auto start = Clock::now();
            WGPUCommandEncoder encoder = wgpuDeviceCreateCommandEncoder(context.device, nullptr);
            uint32_t query_count = 0;
            // Pass kinds: 0 = first intrinsic bake, 1 = resolve, 2 = shade,
            // 3 = canonical controls/rejected panels. Separate passes order
            // row29 writes before the second pipeline's fallback-flag reads.
            auto encode_pass = [&](uint32_t kind) {
                WGPUComputePassDescriptor pass_descriptor{};
                WGPUComputePassTimestampWrites writes{};
                if (timestamps) {
                    writes.querySet = context.queries;
                    writes.beginningOfPassWriteIndex = query_count++;
                    writes.endOfPassWriteIndex = query_count++;
                    pass_descriptor.timestampWrites = &writes;
                }
                auto pass = wgpuCommandEncoderBeginComputePass(encoder, &pass_descriptor);
                const auto pipeline = kind == 0 ? context.bake_pipeline :
                    (kind == 3 ? context.general_bake_pipeline :
                    (kind == 1 ? context.resolve_pipeline : context.pipeline));
                wgpuComputePassEncoderSetPipeline(pass, pipeline);
                wgpuComputePassEncoderSetBindGroup(pass, 0, (kind == 0 || kind == 3) ? context.bake_bind_group : context.bind_group, 0, nullptr);
                if (kind != 2)
                    wgpuComputePassEncoderDispatchWorkgroups(pass, 16, 16, entry_count);
                else
                    wgpuComputePassEncoderDispatchWorkgroups(pass, (width + group - 1) / group, (height + group - 1) / group, 1);
                wgpuComputePassEncoderEnd(pass);
                wgpuComputePassEncoderRelease(pass);
            };
            if (do_bake) encode_pass(0);
            if (do_general_bake) encode_pass(3);
            if (do_resolve) encode_pass(1);
            if (do_shade) encode_pass(2);
            if (timestamps) {
                wgpuCommandEncoderResolveQuerySet(encoder, context.queries, 0, query_count, query_resolve, 0);
                wgpuCommandEncoderCopyBufferToBuffer(encoder, query_resolve, 0, query_readback, 0, query_count * 8);
            }
            auto command = wgpuCommandEncoderFinish(encoder, nullptr);
            wgpuCommandEncoderRelease(encoder);
            auto submit_start = Clock::now();
            wgpuQueueSubmit(context.queue, 1, &command);
            wgpuCommandBufferRelease(command);
            context.wait_queue();
            auto complete = Clock::now();
            Timing timing;
            timing.encode_ms = milliseconds(start, submit_start);
            timing.submit_complete_ms = milliseconds(submit_start, complete);
            timing.cpu_ms = milliseconds(start, complete);
            if (timestamps) {
                auto bytes = context.readback(query_readback, query_count * 8);
                uint64_t values[8]{};
                std::memcpy(values, bytes.data(), bytes.size());
                for (uint32_t i = 1; i < query_count; ++i)
                    if (values[i] < values[i - 1]) throw std::runtime_error("Timestamp query went backwards");
                const double unit_ms = timestamp_period / 1.e6;
                timing.gpu_ms = double(values[query_count - 1] - values[0]) * unit_ms;
                timing.first_bake_gpu_ms = do_bake ? double(values[1] - values[0]) * unit_ms : 0.;
                timing.general_bake_gpu_ms = do_general_bake ? double(values[3] - values[2]) * unit_ms : 0.;
                const uint32_t resolve_start = (do_bake ? 2 : 0) + (do_general_bake ? 2 : 0);
                // Include the gap between the two intrinsic passes as well
                // as both kernels; do not report only the cheaper panel pass.
                timing.bake_gpu_ms = do_bake ? double(values[resolve_start - 1] - values[0]) * unit_ms : 0.;
                timing.resolve_gpu_ms = do_resolve ? double(values[resolve_start + 1] - values[resolve_start]) * unit_ms : 0.;
                const uint32_t shade_start = resolve_start + (do_resolve ? 2 : 0);
                timing.shade_gpu_ms = do_shade ? double(values[shade_start + 1] - values[shade_start]) * unit_ms : 0.;
            }
            return timing;
        };
        Timing startup_bake;
        const bool startup_bake_performed = mode == "warm" && bake_enabled;
        if (startup_bake_performed) {
            startup_bake = dispatch(true, false);
            std::cout << "{\"phase\":\"startup_bake\",\"cpu_ms\":" << startup_bake.cpu_ms << ",\"gpu_ms\":";
            if (timestamps) std::cout << startup_bake.gpu_ms; else std::cout << "null";
            std::cout << "}\n" << std::flush;
        }
        std::vector<Timing> timings;
        if (moving_frame) {
            auto moved_frame = read_file(argv[10], maximum_buffer_bytes);
            auto moved_plan = read_file(argv[11], plan_bytes);
            if (moved_frame.size() != frame.size() || moved_plan.size() != plan.size())
                throw std::runtime_error("Moving fixture changed buffer dimensions");
            for (const auto *bytes : {&moved_frame, &moved_plan}) {
                for (size_t offset = 0; offset < bytes->size(); offset += sizeof(float)) {
                    float value; std::memcpy(&value, bytes->data()+offset, sizeof(value));
                    if (!std::isfinite(value)) throw std::runtime_error("Moving fixture is nonfinite");
                }
            }
            std::vector<float> moved_values(moved_plan.size()/sizeof(float));
            std::memcpy(moved_values.data(), moved_plan.data(), moved_plan.size());
            if (moved_values[0] != float(entry_count) || moved_values[1] != 1.f)
                throw std::runtime_error("Moving fixture changed active dispatch or disabled cache");
            for (uint32_t i=0; i<entry_count; ++i) {
                const size_t at=(1+i*entry_rows)*4;
                if (moved_values[at+3] != plan_values[at+3])
                    throw std::runtime_error("Moving plan must reuse every previously baked entry");
                for (size_t channel=0; channel<3; ++channel)
                    if (moved_values[at+channel] != plan_values[at+channel])
                        throw std::runtime_error("Moving plan changed cache target, edge or address");
            }
            const size_t map_start=descriptor_rows*4;
            if (!std::equal(moved_values.begin()+map_start, moved_values.end(), plan_values.begin()+map_start))
                throw std::runtime_error("Moving plan changed target-to-slot mapping");
            float moved_resolution[2];
            std::memcpy(moved_resolution, moved_frame.data()+8*16, sizeof(moved_resolution));
            if (moved_resolution[0] != float(width) || moved_resolution[1] != float(height))
                throw std::runtime_error("Moving frame changed drawable size");
            // Reuse the resident intrinsic cache; only poses/lights and the
            // corresponding ready descriptors change after the startup bake.
            wgpuQueueWriteBuffer(context.queue, optical, 0, moved_frame.data(), moved_frame.size());
            wgpuQueueWriteBuffer(context.queue, ready_plan, 0, moved_plan.data(), moved_plan.size());
            context.wait_queue();
        }
        Timing warmup;
        for (uint32_t iteration = 0; iteration <= iterations; ++iteration) {
            Timing timing = dispatch(mode == "cold" && bake_enabled, true);
            if (iteration == 0) warmup = timing;
            else timings.push_back(timing);
            std::cout << "{\"phase\":" << quoted(iteration == 0 ? "warmup" : "pass")
                      << ",\"index\":" << iteration << ",\"cpu_ms\":" << timing.cpu_ms
                      << ",\"gpu_ms\":";
            if (timestamps) std::cout << timing.gpu_ms; else std::cout << "null";
            std::cout << ",\"bake_gpu_ms\":";
            if (timestamps) std::cout << timing.bake_gpu_ms; else std::cout << "null";
            std::cout << ",\"first_bake_gpu_ms\":";
            if (timestamps) std::cout << timing.first_bake_gpu_ms; else std::cout << "null";
            std::cout << ",\"general_bake_gpu_ms\":";
            if (timestamps) std::cout << timing.general_bake_gpu_ms; else std::cout << "null";
            std::cout << ",\"resolve_gpu_ms\":";
            if (timestamps) std::cout << timing.resolve_gpu_ms; else std::cout << "null";
            std::cout << ",\"shade_gpu_ms\":";
            if (timestamps) std::cout << timing.shade_gpu_ms; else std::cout << "null";
            std::cout << "}\n" << std::flush;
        }
        auto encoder = wgpuDeviceCreateCommandEncoder(context.device, nullptr);
        wgpuCommandEncoderCopyBufferToBuffer(encoder, output, 0, output_readback, 0, output_bytes);
        auto command = wgpuCommandEncoderFinish(encoder, nullptr);
        wgpuCommandEncoderRelease(encoder);
        wgpuQueueSubmit(context.queue, 1, &command);
        wgpuCommandBufferRelease(command);
        auto rgba = context.readback(output_readback, output_bytes);
        write_file(prefix + ".rgba32f", rgba.data(), rgba.size());
        std::vector<unsigned char> rgb(size_t(width) * height * 3);
        size_t nonfinite = 0;
        size_t negative = 0;
        uint64_t timeline_cache_samples = 0, timeline_cached_pixels = 0;
        float minimum = std::numeric_limits<float>::infinity();
        float maximum = -minimum;
        for (size_t row = 0; row < static_cast<size_t>(output_rows); ++row) {
            float color[4];
            std::memcpy(color, rgba.data() + row * 16, 16);
            if (!std::isfinite(color[3]) || color[3] < 0.f || color[3] > 2.f || color[3] != std::floor(color[3]))
                throw std::runtime_error("Timeline alpha is not an integer primary cache-use count");
            timeline_cache_samples += uint64_t(color[3]);
            timeline_cached_pixels += color[3] > 0.f ? 1u : 0u;
            for (size_t channel = 0; channel < 4; ++channel) {
                float value = color[channel];
                if (!std::isfinite(value)) { ++nonfinite; value = 0; }
                else {
                    if (value < 0) ++negative;
                    if (channel < 3) {
                        minimum = std::min(minimum, value);
                        maximum = std::max(maximum, value);
                    }
                }
                if (channel < 3)
                    rgb[row * 3 + channel] = static_cast<unsigned char>(std::lround(std::clamp(value, 0.f, 1.f) * 255));
            }
        }
        {
            std::ostringstream header;
            header << "P6\n" << width << ' ' << height << "\n255\n";
            const auto header_bytes = header.str();
            if (rgb.size() > maximum_buffer_bytes - header_bytes.size())
                throw std::runtime_error("PPM output exceeds bounded write size");
            std::ofstream ppm(prefix + ".ppm", std::ios::binary);
            ppm.write(header_bytes.data(), static_cast<std::streamsize>(header_bytes.size()));
            ppm.write(reinterpret_cast<const char *>(rgb.data()), static_cast<std::streamsize>(rgb.size()));
            if (!ppm) throw std::runtime_error("Cannot write PPM output");
        }
        auto median = [&](bool gpu) {
            std::vector<double> values;
            for (const auto &t : timings) values.push_back(gpu ? t.gpu_ms : t.cpu_ms);
            std::sort(values.begin(), values.end());
            size_t count = values.size();
            return count % 2 ? values[count / 2] : (values[count / 2 - 1] + values[count / 2]) * .5;
        };
        std::ostringstream json;
        json << std::setprecision(10) << "{\n  \"status\":\"completed\",\n  \"backend\":\"Metal\",\n"
             << "  \"adapter\":" << quoted(adapter_name) << ",\n  \"adapter_description\":" << quoted(adapter_description)
             << ",\n  \"wgpu_native_version\":" << wgpuGetVersion()
             << ",\n  \"width\":" << width << ",\n  \"height\":" << height
             << ",\n  \"mode\":" << quoted(mode)
             << ",\n  \"lighting_resolve_mode\":" << quoted(resolve_mode)
             << ",\n  \"split_intrinsic_bake\":" << (split_bake ? "true" : "false")
             << ",\n  \"intrinsic_bake_passes\":" << (split_bake ? 2 : 1)
             << ",\n  \"entry_count\":" << entry_count
             << ",\n  \"active_entry_count\":" << active_entry_count
             << ",\n  \"timeline_cache_samples\":" << timeline_cache_samples
             << ",\n  \"timeline_cached_pixels\":" << timeline_cached_pixels
             << ",\n  \"alpha_semantics\":\"actual timeline primary cache-use count, integer0..2\""
             << ",\n  \"cache_bytes\":" << cache_bytes
             << ",\n  \"plan_bytes\":" << plan.size()
             << ",\n  \"held_cache_moving_fixture\":" << (moving_frame ? "true" : "false")
             << ",\n  \"rows_per_pixel\":1"
             << ",\n  \"output_rows\":" << output_rows
             << ",\n  \"output_bytes\":" << output_bytes
             << ",\n  \"ppm_written\":true"
             << ",\n  \"workgroup\":[" << group << "," << group << ",1],\n  \"invocation_hook\":" << (invocation_hook ? "true" : "false")
             << ",\n  \"measured_iterations\":" << iterations
             << ",\n  \"unmeasured_warmups\":1,\n  \"timestamp_supported\":" << (timestamps ? "true" : "false")
             << ",\n  \"timestamp_period_ns\":" << timestamp_period
             << ",\n  \"shader_pipeline_compile_ms\":" << compile_ms
             << ",\n  \"frame_bytes\":" << frame.size()
             << ",\n  \"startup_bake_performed\":" << (startup_bake_performed ? "true" : "false")
             << ",\n  \"startup_bake_cpu_ms\":" << startup_bake.cpu_ms
             << ",\n  \"startup_bake_gpu_ms\":";
        if (timestamps && startup_bake_performed) json << startup_bake.gpu_ms; else json << "null";
        json << ",\n  \"startup_intrinsic_bake_gpu_ms\":";
        if (timestamps && startup_bake_performed) json << startup_bake.bake_gpu_ms; else json << "null";
        json << ",\n  \"startup_first_bake_gpu_ms\":";
        if (timestamps && startup_bake_performed) json << startup_bake.first_bake_gpu_ms; else json << "null";
        json << ",\n  \"startup_general_bake_gpu_ms\":";
        if (timestamps && startup_bake_performed) json << startup_bake.general_bake_gpu_ms; else json << "null";
        json << ",\n  \"startup_lighting_resolve_gpu_ms\":";
        if (timestamps && startup_bake_performed) json << startup_bake.resolve_gpu_ms; else json << "null";
        json
             << ",\n  \"warmup_cpu_ms\":" << warmup.cpu_ms
             << ",\n  \"median_cpu_ms\":" << median(false) << ",\n  \"median_gpu_ms\":";
        if (timestamps) json << median(true); else json << "null";
        json << ",\n  \"nonfinite_output_channels\":" << nonfinite
             << ",\n  \"negative_output_channels\":" << negative;
        json << ",\n  \"rgb_min\":" << (std::isfinite(minimum) ? minimum : 0)
             << ",\n  \"rgb_max\":" << (std::isfinite(maximum) ? maximum : 0)
             << ",\n  \"output_format\":\"top-left origin, tightly packed little-endian RGBA32F, display-encoded workstationShade output\",\n";
        json
             << "  \"timing_scope\":\"GPU timestamps bracket all enabled per-frame passes: intrinsic bake, optional general bake, lighting resolve and shading. bake_gpu_ms includes both intrinsic passes and their inter-pass gap when split; first_bake_gpu_ms and general_bake_gpu_ms report the individual kernels. Resolve mode includes lighting on every frame; resolve-once resolves only during the excluded warm startup and is valid only for this immutable fixture. Separate bake, resolve and shade pass timings are included; total includes inter-pass overhead. Warm startup and one warmup frame are excluded from medians. CPU includes encoding, submission, timestamp resolve/copy and completion wait; excludes query mapping, image readback and compilation.\",\n"
             << "  \"passes\":[\n";
        for (size_t i = 0; i < timings.size(); ++i) {
            auto t = timings[i];
            json << "    {\"encode_ms\":" << t.encode_ms << ",\"submit_complete_ms\":" << t.submit_complete_ms
                 << ",\"cpu_ms\":" << t.cpu_ms << ",\"gpu_ms\":";
            if (timestamps) json << t.gpu_ms; else json << "null";
            json << ",\"bake_gpu_ms\":";
            if (timestamps) json << t.bake_gpu_ms; else json << "null";
            json << ",\"first_bake_gpu_ms\":";
            if (timestamps) json << t.first_bake_gpu_ms; else json << "null";
            json << ",\"general_bake_gpu_ms\":";
            if (timestamps) json << t.general_bake_gpu_ms; else json << "null";
            json << ",\"resolve_gpu_ms\":";
            if (timestamps) json << t.resolve_gpu_ms; else json << "null";
            json << ",\"shade_gpu_ms\":";
            if (timestamps) json << t.shade_gpu_ms; else json << "null";
            json << '}' << (i + 1 < timings.size() ? "," : "") << '\n';
        }
        json << "  ]\n}\n";
        auto report = json.str();
        write_file(prefix + ".json", report.data(), report.size());
        if (nonfinite) throw std::runtime_error("Nonfinite shader output; inspect saved report/raw output");
        std::cout << "COMPUTE_PROBE_OK=" << prefix << ".json\n";
        return 0;
    } catch (const std::exception &error) {
        std::cerr << "COMPUTE_PROBE_FAILED: " << error.what() << '\n';
        if (!prefix.empty()) {
            auto report = "{\"status\":\"failed\",\"error\":" + quoted(error.what()) + "}\n";
            try { write_file(prefix + ".error.json", report.data(), report.size()); } catch (...) {}
        }
        return 1;
    }
}
