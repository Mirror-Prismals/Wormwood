#include "Host/Vst2AudioBridge.h"
#include <array>
#include <chrono>
#include <cmath>
#include <cstring>
#include <cstdio>
#include <thread>
#include <vector>
#include <mutex>
#include <map>

struct Vst2AudioBridge::Impl {
    enum : unsigned { Free, Queued, Working, Done };
    struct Slot {
        std::atomic<unsigned> state{Free};
        std::vector<float> input, output;
        uint64_t sequence = 0;
    };
    static constexpr int Capacity = 8;
    static constexpr int Delay = 2;
    std::array<Slot, Capacity> slots;
    std::array<std::vector<float>, Delay + 1> dry;
    std::vector<float> streamInput, streamOutput;
    int streamPosition = 0;
    std::unique_ptr<std::atomic<float>[]> values;
    std::unique_ptr<std::atomic<bool>[]> dirty;
    std::unique_ptr<Vst2Worker> worker;
    std::thread thread;
    std::mutex metadataMutex;
    std::string failureDetail;
    std::map<int, Vst2Worker::Json> metadata;
    std::atomic<bool> stopping{false}, bypassed{false};
    std::atomic<uint64_t> completed{0}, late{0};
    std::atomic<const char*> fault{nullptr};
    void Fail(const char* reason) noexcept {
        const char* empty = nullptr;
        fault.compare_exchange_strong(empty, reason, std::memory_order_release);
    }
    uint64_t submitted = 0;
    int frames = 0, parameters = 0;

    void Run() noexcept {
        try {
            std::vector<uint8_t> input(static_cast<size_t>(frames) * 2 * sizeof(float)), output;
            Vst2Worker::Json result;
            while (!stopping.load(std::memory_order_acquire) && !fault.load(std::memory_order_acquire)) {
                // Input blocks may be dropped during temporary overload. Pick
                // the oldest published block instead of waiting for a gap.
                Slot* selected = nullptr;
                for (auto& candidate : slots) {
                    if (candidate.state.load(std::memory_order_acquire) == Queued &&
                        (!selected || candidate.sequence < selected->sequence)) selected = &candidate;
                }
                if (!selected) {
                    std::this_thread::sleep_for(std::chrono::microseconds(100));
                    continue;
                }
                auto& slot = *selected;
                slot.state.store(Working, std::memory_order_release);
                Vst2Worker::Json updates = Vst2Worker::Json::array();
                for (int i = 0; i < parameters; ++i) {
                    if (dirty[i].exchange(false, std::memory_order_acq_rel))
                        updates.push_back({i, values[i].load(std::memory_order_acquire)});
                }
                std::memcpy(input.data(), slot.input.data(), input.size());
                std::string error;
                if (!worker->Exchange({{"op", "process"}, {"frames", frames}, {"parameters", updates}, {"parameter_metadata", true}},
                                      input, result, output, 2000, error) || output.size() != input.size()) {
                    std::fprintf(stderr, "VST2 worker: %s\n", error.empty() ? "invalid audio response size" : error.c_str());
                    { std::lock_guard<std::mutex> lock(metadataMutex); failureDetail = error.empty() ? "Invalid audio response size" : error; }
                    Fail("worker request failed; see terminal"); break;
                }
                std::memcpy(slot.output.data(), output.data(), output.size());
                for (float value : slot.output) if (!std::isfinite(value)) {
                    Fail("worker returned nonfinite audio"); return;
                }
                slot.state.store(Done, std::memory_order_release);
                completed.fetch_add(1, std::memory_order_release);
                if (result.contains("parameter_info")) {
                    std::lock_guard<std::mutex> lock(metadataMutex);
                    for (const auto& item : result.at("parameter_info")) {
                        int id = item.at("id").get<int>();
                        if (id >= 0 && id < parameters) metadata[id] = item;
                    }
                }
            }
        } catch (...) {
            Fail("worker thread exception");
        }
    }
};

Vst2AudioBridge::Vst2AudioBridge() : impl(std::make_unique<Impl>()) {
    static_assert(std::atomic<float>::is_always_lock_free);
    static_assert(std::atomic<const char*>::is_always_lock_free);
    static_assert(std::atomic<unsigned>::is_always_lock_free);
    static_assert(std::atomic<uint64_t>::is_always_lock_free);
    static_assert(std::atomic<bool>::is_always_lock_free);
}
Vst2AudioBridge::~Vst2AudioBridge() { Stop(); }

Vst2Worker::Json Vst2AudioBridge::TakeParameterInfo() {
    auto result = Vst2Worker::Json::array();
    std::lock_guard<std::mutex> lock(impl->metadataMutex);
    for (auto& item : impl->metadata) result.push_back(std::move(item.second));
    impl->metadata.clear();
    return result;
}

bool Vst2AudioBridge::Start(std::unique_ptr<Vst2Worker> worker, int blockSize, int parameterCount) {
    if (impl->thread.joinable() || !worker || !worker->Running() || blockSize < 1 || blockSize > 8192
        || parameterCount < 0 || parameterCount > 4096) return false;
    impl->frames = blockSize; impl->parameters = parameterCount;
    { std::lock_guard<std::mutex> lock(impl->metadataMutex); impl->metadata.clear(); impl->failureDetail.clear(); }
    impl->streamInput.assign(static_cast<size_t>(blockSize) * 2, 0.0f);
    impl->streamOutput.assign(static_cast<size_t>(blockSize) * 2, 0.0f);
    impl->streamPosition = 0;
    for (auto& slot : impl->slots) {
        slot.input.assign(static_cast<size_t>(blockSize) * 2, 0.0f);
        slot.output.assign(static_cast<size_t>(blockSize) * 2, 0.0f);
        slot.state.store(Impl::Free, std::memory_order_relaxed);
    }
    for (auto& dry : impl->dry) dry.assign(static_cast<size_t>(blockSize) * 2, 0.0f);
    impl->values = std::make_unique<std::atomic<float>[]>(parameterCount);
    impl->dirty = std::make_unique<std::atomic<bool>[]>(parameterCount);
    for (int i = 0; i < parameterCount; ++i) { impl->values[i].store(0); impl->dirty[i].store(false); }
    impl->submitted = 0; impl->completed.store(0);
    impl->fault.store(nullptr); impl->late.store(0); impl->stopping.store(false);
    impl->worker = std::move(worker);
    try { impl->thread = std::thread([this] { impl->Run(); }); }
    catch (...) { impl->worker.reset(); impl->Fail("worker thread could not start"); return false; }
    return true;
}

std::unique_ptr<Vst2Worker> Vst2AudioBridge::Stop() {
    impl->stopping.store(true, std::memory_order_release);
    if (impl->thread.joinable()) impl->thread.join();
    return std::move(impl->worker);
}

bool Vst2AudioBridge::Process(const float* left, const float* right, float* outLeft, float* outRight, int frames) noexcept {
    if (!left || !right || !outLeft || !outRight || frames != impl->frames || frames < 1
        || impl->stopping.load(std::memory_order_acquire)) return false;
    const auto sequence = impl->submitted++;
    const auto bytes = static_cast<size_t>(frames) * sizeof(float);
    auto& dry = impl->dry[sequence % impl->dry.size()];
    std::memcpy(dry.data(), left, bytes);
    std::memcpy(dry.data() + frames, right, bytes);
    if (sequence < Impl::Delay) {
        std::memset(outLeft, 0, bytes); std::memset(outRight, 0, bytes);
    } else {
        const auto expected = sequence - Impl::Delay;
        auto& slot = impl->slots[expected % Impl::Capacity];
        const bool ready = !impl->fault.load(std::memory_order_acquire)
            && slot.state.load(std::memory_order_acquire) == Impl::Done && slot.sequence == expected;
        const auto& data = ready && !impl->bypassed.load(std::memory_order_relaxed) ? slot.output : impl->dry[expected % impl->dry.size()];
        std::memcpy(outLeft, data.data(), bytes);
        std::memcpy(outRight, data.data() + frames, bytes);
        if (ready) slot.state.store(Impl::Free, std::memory_order_release);
        else if (!impl->fault.load(std::memory_order_acquire))
            impl->late.fetch_add(1, std::memory_order_relaxed);
    }
    if (!impl->fault.load(std::memory_order_acquire)) {
        auto& slot = impl->slots[sequence % Impl::Capacity];
        // Done here belongs to an older block whose output deadline passed.
        // Acquire completion before reusing its buffers and sequence.
        const auto state = slot.state.load(std::memory_order_acquire);
        if (state != Impl::Free && state != Impl::Done) {
            // Keep the in-flight buffers untouched. This block uses delayed
            // dry when its deadline arrives; later blocks can resume wet audio.
            // Transport failures and invalid audio still latch real faults.
        } else {
            std::memcpy(slot.input.data(), dry.data(), bytes * 2);
            slot.sequence = sequence;
            slot.state.store(Impl::Queued, std::memory_order_release);
        }
    }
    return !impl->fault.load(std::memory_order_acquire);
}

bool Vst2AudioBridge::ProcessStream(const float* left, const float* right, float* outLeft, float* outRight, int frames) noexcept {
    if (!left || !right || !outLeft || !outRight || frames < 1 || frames > impl->frames
        || impl->stopping.load(std::memory_order_acquire)) return false;
    bool ok = true;
    const int block = impl->frames;
    for (int i = 0; i < frames; ++i) {
        const int position = impl->streamPosition;
        const float l = left[i], r = right[i];
        impl->streamInput[position] = l;
        impl->streamInput[block + position] = r;
        outLeft[i] = impl->streamOutput[position];
        outRight[i] = impl->streamOutput[block + position];
        if (++impl->streamPosition == block) {
            ok = Process(impl->streamInput.data(), impl->streamInput.data() + block,
                         impl->streamOutput.data(), impl->streamOutput.data() + block, block) && ok;
            impl->streamPosition = 0;
        }
    }
    return ok && !impl->fault.load(std::memory_order_acquire);
}

int Vst2AudioBridge::StreamLatencyFrames() const noexcept { return (Impl::Delay + 1) * impl->frames; }

bool Vst2AudioBridge::QueueParameter(int index, float value) noexcept {
    if (index < 0 || index >= impl->parameters || !std::isfinite(value) || value < 0 || value > 1
        || impl->stopping.load(std::memory_order_acquire)) return false;
    impl->values[index].store(value, std::memory_order_release);
    impl->dirty[index].store(true, std::memory_order_release);
    return true;
}
int Vst2AudioBridge::LatencyFrames() const noexcept { return Impl::Delay * impl->frames; }
bool Vst2AudioBridge::Failed() const noexcept { return impl->fault.load(std::memory_order_acquire); }
uint64_t Vst2AudioBridge::CompletedBlocks() const noexcept { return impl->completed.load(std::memory_order_acquire); }

void Vst2AudioBridge::SetBypassed(bool value) noexcept { impl->bypassed.store(value, std::memory_order_relaxed); }

uint64_t Vst2AudioBridge::LateBlocks() const noexcept { return impl->late.load(std::memory_order_relaxed); }
const char* Vst2AudioBridge::FailureReason() const noexcept {
    const char* reason = impl->fault.load(std::memory_order_acquire);
    return reason ? reason : "";
}

std::string Vst2AudioBridge::FailureDetail() const {
    std::lock_guard<std::mutex> lock(impl->metadataMutex);
    return impl->failureDetail.empty() ? FailureReason() : impl->failureDetail;
}
