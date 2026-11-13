#include "Host/Vst2EditorBridge.h"
#include <condition_variable>
#include <mutex>
#include <optional>
#include <thread>

struct Vst2EditorBridge::Shared {
    std::mutex mutex;
    std::condition_variable wake;
    bool stopped = false, busy = true, failed = false;
    std::optional<Json> request;
    std::optional<Reply> reply;
};

Vst2EditorBridge::Vst2EditorBridge(const std::string& python, const std::string& script, Json open)
    : shared(std::make_shared<Shared>()) {
    auto state = shared;
    // The thread retains only transport state, never a plugin or UI pointer.
    // Closing the window never waits for an emulated editor to finish loading.
    std::thread([state, python, script, request = std::move(open)]() mutable {
        Vst2Worker worker;
        bool first = true;
        for (;;) {
            Reply reply;
            reply.sent = request;
            try {
                if (first && !worker.Start(python, script, reply.error)) {}
                else worker.Exchange(request, {}, reply.result, reply.png, first ? 60000 : 5000, reply.error);
            } catch (const std::exception& e) { reply.error = e.what(); worker.Abort(); }
            first = false;
            std::unique_lock<std::mutex> lock(state->mutex);
            if (state->stopped) break;
            state->failed = !reply.error.empty();
            state->reply = std::move(reply);
            if (state->failed) break;
            state->wake.wait(lock, [&] { return state->stopped || state->request.has_value(); });
            if (state->stopped) break;
            request = std::move(*state->request);
            state->request.reset();
        }
        std::string ignored;
        if (worker.Running()) worker.Close(ignored);
    }).detach();
}

Vst2EditorBridge::~Vst2EditorBridge() {
    { std::lock_guard<std::mutex> lock(shared->mutex); shared->stopped = true; }
    shared->wake.notify_one();
}

bool Vst2EditorBridge::Submit(Json request) {
    std::lock_guard<std::mutex> lock(shared->mutex);
    if (shared->busy || shared->failed || shared->stopped) return false;
    shared->busy = true;
    shared->request = std::move(request);
    shared->wake.notify_one();
    return true;
}

bool Vst2EditorBridge::Poll(Reply& reply) {
    std::lock_guard<std::mutex> lock(shared->mutex);
    if (!shared->reply) return false;
    reply = std::move(*shared->reply);
    shared->reply.reset();
    shared->busy = false;
    return true;
}
