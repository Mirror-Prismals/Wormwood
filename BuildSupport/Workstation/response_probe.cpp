// Finite, headless wgpu-native 27 workstation/optical-response probe.
// Build/run is deliberately external. The canonical compute_probe is unchanged.
// Optional final rows-per-pixel > 1 calls responseBake(vec2<f32>, u32), which
// writes pixels[pixelIndex * rowsPerPixel + row] through storage binding 2.
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

using Clock = std::chrono::steady_clock;
static constexpr uint64_t maximum_buffer_bytes = 128ull * 1024 * 1024;
static constexpr uint64_t maximum_shader_bytes = 8ull * 1024 * 1024;
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
    WGPUBindGroup bind_group = nullptr;
    WGPUQuerySet queries = nullptr;
    std::vector<WGPUBuffer> buffers;
    ~Context() {
        if (bind_group) wgpuBindGroupRelease(bind_group);
        if (pipeline) wgpuComputePipelineRelease(pipeline);
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
};
static std::string compute_entry(uint32_t width, uint32_t height, uint32_t group,
                                 bool invocation_hook, bool bake) {
    std::ostringstream out;
    out << "\n// Headless diagnostic only: canonical shade or optical-response bake.\n"
        << "struct WorkstationComputeOutput { pixels: array<vec4<f32>> };\n"
        << "@group(0) @binding(2) var<storage,read_write> workstationComputeOutput: WorkstationComputeOutput;\n"
        << "@compute @workgroup_size(" << group << "," << group << ",1)\n"
        << "fn workstation_compute_main(@builtin(global_invocation_id) id:vec3<u32>, @builtin(local_invocation_index) localIndex:u32) {\n"
        << " if(id.x >= " << width << "u || id.y >= " << height << "u){return;}\n"
        << (invocation_hook ? " workstationProbeInvocation(localIndex);\n" : "")
        << " let screen=vec2<f32>((f32(id.x)+0.5)*2.0/" << width << ".0-1.0,"
        << "1.0-(f32(id.y)+0.5)*2.0/" << height << ".0);\n";
    if (bake)
        out << " responseBake(screen, id.y*" << width << "u+id.x);\n";
    else
        out << " workstationComputeOutput.pixels[id.y*" << width << "u+id.x]=workstationShade(screen);\n";
    out << "}\n";
    return out.str();
}

int main(int argc, char **argv) {
    std::string prefix;
    try {
        if (argc < 7 || argc > 9) throw std::runtime_error(
            "usage: response_probe shader.wgsl frame.bin width height output-prefix iterations [workgroup-side:4|8] [rows-per-pixel:1..128]");
        uint32_t width = integer(argv[3], 16384), height = integer(argv[4], 16384);
        uint32_t iterations = integer(argv[6], 1000);
        uint32_t group = argc >= 8 ? integer(argv[7], 8) : 8;
        if (group != 4 && group != 8) throw std::runtime_error("Workgroup side must be 4 or 8");
        uint32_t rows_per_pixel = argc == 9 ? integer(argv[8], 128) : 1;
        const bool bake = rows_per_pixel > 1;
        const uint64_t output_rows = uint64_t(width) * height * rows_per_pixel;
        const uint64_t output_bytes = output_rows * 4 * sizeof(float);
        // Arguments are bounded before multiplication, so this cannot overflow
        // uint64_t. Keep both GPU and CPU readback allocations within 128 MiB.
        if (output_bytes > maximum_buffer_bytes || output_bytes > std::numeric_limits<size_t>::max())
            throw std::runtime_error("Probe output exceeds the 128 MiB limit");
        prefix = argv[5];
        auto parent = std::filesystem::path(prefix).parent_path();
        if (!parent.empty()) std::filesystem::create_directories(parent);
        auto shader_bytes = read_file(argv[1], maximum_shader_bytes);
        std::string source(shader_bytes.begin(), shader_bytes.end());
        if (source.find("fn workstation_compute_main") != std::string::npos)
            throw std::runtime_error("Pass canonical WGSL; the probe appends its compute entry itself");
        bool invocation_hook = source.find("fn workstationProbeInvocation(") != std::string::npos;
        source += compute_entry(width, height, group, invocation_hook, bake);
        write_file(prefix + ".compute.wgsl", source.data(), source.size());
        auto frame = read_file(argv[2], maximum_buffer_bytes);
        if (frame.size() < 256 * 16 || frame.size() % 16)
            throw std::runtime_error("frame.bin must contain complete RGBA32F optical rows");
        float resolution[2];
        std::memcpy(resolution, frame.data() + 8 * 16, sizeof(resolution));
        if (resolution[0] != static_cast<float>(width) || resolution[1] != static_cast<float>(height))
            throw std::runtime_error("frame row8 resolution differs from width/height; refusing to alter sampling");

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
        if (std::max<uint64_t>(frame.size(), output_bytes) > limits.maxStorageBufferBindingSize ||
            std::max<uint64_t>(frame.size(), output_bytes) > limits.maxBufferSize)
            throw std::runtime_error("Probe input/output exceeds device storage-buffer limits");
        float timestamp_period = timestamps ? wgpuQueueGetTimestampPeriod(context.queue) : 0.f;
        if (timestamps && !(timestamp_period > 0)) throw std::runtime_error("Invalid timestamp period");

        auto dummy = context.buffer(96, WGPUBufferUsage_Storage | WGPUBufferUsage_CopyDst, "Unused vertex appearance");
        auto optical = context.buffer(frame.size(), WGPUBufferUsage_Storage | WGPUBufferUsage_CopyDst, "Complete optical frame");
        auto output = context.buffer(output_bytes, WGPUBufferUsage_Storage | WGPUBufferUsage_CopySrc, "Compute RGBA32F output");
        auto output_readback = context.buffer(output_bytes, WGPUBufferUsage_CopyDst | WGPUBufferUsage_MapRead, "Image readback");
        wgpuQueueWriteBuffer(context.queue, optical, 0, frame.data(), frame.size());
        WGPUBindGroupLayoutEntry layout_entries[3]{};
        uint64_t sizes[3] = {96, frame.size(), output_bytes};
        WGPUBuffer handles[3] = {dummy, optical, output};
        WGPUBindGroupEntry bindings[3]{};
        for (uint32_t i = 0; i < 3; ++i) {
            layout_entries[i].binding = i;
            layout_entries[i].visibility = WGPUShaderStage_Compute;
            layout_entries[i].buffer.type = i == 2 ? WGPUBufferBindingType_Storage : WGPUBufferBindingType_ReadOnlyStorage;
            layout_entries[i].buffer.minBindingSize = sizes[i];
            bindings[i].binding = i;
            bindings[i].buffer = handles[i];
            bindings[i].size = sizes[i];
        }
        WGPUBindGroupLayoutDescriptor bind_layout_descriptor{};
        bind_layout_descriptor.entryCount = 3;
        bind_layout_descriptor.entries = layout_entries;
        context.bind_layout = wgpuDeviceCreateBindGroupLayout(context.device, &bind_layout_descriptor);
        WGPUPipelineLayoutDescriptor pipeline_layout_descriptor{};
        pipeline_layout_descriptor.bindGroupLayoutCount = 1;
        pipeline_layout_descriptor.bindGroupLayouts = &context.bind_layout;
        context.pipeline_layout = wgpuDeviceCreatePipelineLayout(context.device, &pipeline_layout_descriptor);
        WGPUBindGroupDescriptor bind_descriptor{};
        bind_descriptor.layout = context.bind_layout;
        bind_descriptor.entryCount = 3;
        bind_descriptor.entries = bindings;
        context.bind_group = wgpuDeviceCreateBindGroup(context.device, &bind_descriptor);
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
        pipeline_descriptor.compute.entryPoint = literal("workstation_compute_main");
        context.pipeline = wgpuDeviceCreateComputePipeline(context.device, &pipeline_descriptor);
        context.pop_error_scope("pipeline creation");
        if (!context.pipeline) throw std::runtime_error("Null compute pipeline");
        double compile_ms = milliseconds(compile_start, Clock::now());
        WGPUBuffer query_resolve = nullptr, query_readback = nullptr;
        if (timestamps) {
            WGPUQuerySetDescriptor query_descriptor{};
            query_descriptor.type = WGPUQueryType_Timestamp;
            query_descriptor.count = 2;
            context.queries = wgpuDeviceCreateQuerySet(context.device, &query_descriptor);
            query_resolve = context.buffer(256, WGPUBufferUsage_QueryResolve | WGPUBufferUsage_CopySrc, "Timestamp resolution");
            query_readback = context.buffer(16, WGPUBufferUsage_CopyDst | WGPUBufferUsage_MapRead, "Timestamp readback");
        }
        context.wait_queue(); // Initial uploads are outside all timed dispatches.
        std::vector<Timing> timings;
        Timing warmup;
        for (uint32_t iteration = 0; iteration <= iterations; ++iteration) {
            auto start = Clock::now();
            WGPUCommandEncoder encoder = wgpuDeviceCreateCommandEncoder(context.device, nullptr);
            WGPUComputePassDescriptor pass_descriptor{};
            WGPUComputePassTimestampWrites writes{};
            if (timestamps) {
                writes.querySet = context.queries;
                writes.beginningOfPassWriteIndex = 0;
                writes.endOfPassWriteIndex = 1;
                pass_descriptor.timestampWrites = &writes;
            }
            auto pass = wgpuCommandEncoderBeginComputePass(encoder, &pass_descriptor);
            wgpuComputePassEncoderSetPipeline(pass, context.pipeline);
            wgpuComputePassEncoderSetBindGroup(pass, 0, context.bind_group, 0, nullptr);
            wgpuComputePassEncoderDispatchWorkgroups(pass, (width + group - 1) / group, (height + group - 1) / group, 1);
            wgpuComputePassEncoderEnd(pass);
            wgpuComputePassEncoderRelease(pass);
            if (timestamps) {
                wgpuCommandEncoderResolveQuerySet(encoder, context.queries, 0, 2, query_resolve, 0);
                wgpuCommandEncoderCopyBufferToBuffer(encoder, query_resolve, 0, query_readback, 0, 16);
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
                auto bytes = context.readback(query_readback, 16);
                uint64_t values[2];
                std::memcpy(values, bytes.data(), 16);
                if (values[1] < values[0]) throw std::runtime_error("Timestamp query went backwards");
                timing.gpu_ms = double(values[1] - values[0]) * timestamp_period / 1.e6;
            }
            if (iteration == 0) warmup = timing;
            else timings.push_back(timing);
            std::cout << "{\"phase\":" << quoted(iteration == 0 ? "warmup" : "pass")
                      << ",\"index\":" << iteration << ",\"cpu_ms\":" << timing.cpu_ms
                      << ",\"gpu_ms\":";
            if (timestamps) std::cout << timing.gpu_ms; else std::cout << "null";
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
        std::vector<unsigned char> rgb;
        if (!bake) rgb.resize(size_t(width) * height * 3);
        size_t nonfinite = 0;
        size_t negative = 0;
        float minimum = std::numeric_limits<float>::infinity();
        float maximum = -minimum;
        for (size_t row = 0; row < static_cast<size_t>(output_rows); ++row) {
            float color[4];
            std::memcpy(color, rgba.data() + row * 16, 16);
            for (size_t channel = 0; channel < 4; ++channel) {
                float value = color[channel];
                if (!std::isfinite(value)) { ++nonfinite; value = 0; }
                else {
                    if (value < 0) ++negative;
                    // Bake rows are arbitrary signed coefficients, including
                    // their fourth channel. Shade retains its RGB-only range.
                    if (bake || channel < 3) {
                        minimum = std::min(minimum, value);
                        maximum = std::max(maximum, value);
                    }
                }
                if (!bake && channel < 3)
                    rgb[row * 3 + channel] = static_cast<unsigned char>(std::lround(std::clamp(value, 0.f, 1.f) * 255));
            }
        }
        if (!bake) {
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
             << ",\n  \"mode\":" << quoted(bake ? "response_bake" : "shade")
             << ",\n  \"rows_per_pixel\":" << rows_per_pixel
             << ",\n  \"output_rows\":" << output_rows
             << ",\n  \"output_bytes\":" << output_bytes
             << ",\n  \"ppm_written\":" << (bake ? "false" : "true")
             << ",\n  \"workgroup\":[" << group << "," << group << ",1],\n  \"invocation_hook\":" << (invocation_hook ? "true" : "false")
             << ",\n  \"measured_iterations\":" << iterations
             << ",\n  \"unmeasured_warmups\":1,\n  \"timestamp_supported\":" << (timestamps ? "true" : "false")
             << ",\n  \"timestamp_period_ns\":" << timestamp_period
             << ",\n  \"shader_pipeline_compile_ms\":" << compile_ms
             << ",\n  \"frame_bytes\":" << frame.size()
             << ",\n  \"warmup_cpu_ms\":" << warmup.cpu_ms
             << ",\n  \"median_cpu_ms\":" << median(false) << ",\n  \"median_gpu_ms\":";
        if (timestamps) json << median(true); else json << "null";
        json << ",\n  \"nonfinite_output_channels\":" << nonfinite
             << ",\n  \"negative_output_channels\":" << negative;
        if (bake)
            json << ",\n  \"coefficient_min\":" << (std::isfinite(minimum) ? minimum : 0)
                 << ",\n  \"coefficient_max\":" << (std::isfinite(maximum) ? maximum : 0)
                 << ",\n  \"output_format\":\"top-left pixel origin, pixel-major then response row, tightly packed little-endian vec4<f32>, signed raw coefficients; no image conversion\",\n";
        else
            json << ",\n  \"rgb_min\":" << (std::isfinite(minimum) ? minimum : 0)
                 << ",\n  \"rgb_max\":" << (std::isfinite(maximum) ? maximum : 0)
                 << ",\n  \"output_format\":\"top-left origin, tightly packed little-endian RGBA32F, display-encoded workstationShade output\",\n";
        json
             << "  \"timing_scope\":\"GPU timestamps bracket compute pass only; CPU includes encoding, submission, resolve/copy and completion wait, excludes query mapping/image readback/compilation. One warmup excluded.\",\n"
             << "  \"passes\":[\n";
        for (size_t i = 0; i < timings.size(); ++i) {
            auto t = timings[i];
            json << "    {\"encode_ms\":" << t.encode_ms << ",\"submit_complete_ms\":" << t.submit_complete_ms
                 << ",\"cpu_ms\":" << t.cpu_ms << ",\"gpu_ms\":";
            if (timestamps) json << t.gpu_ms; else json << "null";
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
