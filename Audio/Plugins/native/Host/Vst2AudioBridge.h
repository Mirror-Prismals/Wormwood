#pragma once

#include "Vst2Worker.h"
#include <atomic>
#include <cstddef>
#include <memory>

// Fixed stereo blocks, two-block pipeline latency. Configure/start/stop on a
// control thread. Process and QueueParameter allocate nothing and never do IPC.
// A missed deadline returns equally delayed dry for that block and can recover.
// A full queue drops input and recovers; worker failure still latches a fault.
class Vst2AudioBridge {
public:
    Vst2AudioBridge();
    ~Vst2AudioBridge();
    Vst2AudioBridge(const Vst2AudioBridge&) = delete;
    Vst2AudioBridge& operator=(const Vst2AudioBridge&) = delete;

    // Takes ownership of an already-open worker. Control-thread only, stopped.
    bool Start(std::unique_ptr<Vst2Worker> worker, int blockSize, int parameterCount);
    // Audio callbacks must be quiescent before Stop/destruction.
    std::unique_ptr<Vst2Worker> Stop();
    bool Process(const float* left, const float* right, float* outLeft, float* outRight, int frames) noexcept;
    // Alternative streaming entry point for host sub-blocks up to blockSize.
    // Aggregation adds one block: total latency is StreamLatencyFrames().
    // Do not mix Process and ProcessStream within one started session.
    bool ProcessStream(const float* left, const float* right, float* outLeft, float* outRight, int frames) noexcept;
    int StreamLatencyFrames() const noexcept;
    bool QueueParameter(int index, float value) noexcept;
    int LatencyFrames() const noexcept;
    bool Failed() const noexcept;
    void SetBypassed(bool value) noexcept;
    uint64_t CompletedBlocks() const noexcept;
    uint64_t LateBlocks() const noexcept;
    const char* FailureReason() const noexcept;
    std::string FailureDetail() const; // Main/control thread only.
    // Main/control-thread only: metadata returned by the IPC thread.
    Vst2Worker::Json TakeParameterInfo();

private:
    struct Impl;
    std::unique_ptr<Impl> impl;
};
