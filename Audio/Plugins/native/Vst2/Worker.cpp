#include "Host/Vst2Worker.h"

#include <algorithm>
#include <chrono>
#include <cstring>
#include <limits>
#if defined(__APPLE__) || defined(__linux__)
#include <cerrno>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <spawn.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>
extern char** environ;
#endif

struct Vst2Worker::Impl {
    int socket = -1;
    int pid = -1;
    uint64_t sequence = 0;
};

Vst2Worker::Vst2Worker() : impl(std::make_unique<Impl>()) {}
Vst2Worker::~Vst2Worker() { Abort(); }
bool Vst2Worker::Running() const noexcept { return impl->pid > 0; }

void Vst2Worker::Abort() noexcept {
#if defined(__APPLE__) || defined(__linux__)
    if (impl->socket >= 0) ::close(impl->socket);
    if (impl->pid > 0) {
        // The PID remains ours until reaped; no PID-reuse race with another job.
        ::kill(impl->pid, SIGKILL);
        while (::waitpid(impl->pid, nullptr, 0) < 0 && errno == EINTR) {}
    }
#endif
    impl->socket = impl->pid = -1;
}

bool Vst2Worker::Start(const std::string& python, const std::string& script, std::string& error) {
    Abort();
    error.clear();
#if defined(__APPLE__) || defined(__linux__)
    int sockets[2];
    if (::socketpair(AF_UNIX, SOCK_STREAM, 0, sockets) != 0) {
        error = "Cannot create VST2 worker channel"; return false;
    }
    for (int fd : sockets) {
        if (fd < 3 || ::fcntl(fd, F_SETFD, FD_CLOEXEC) < 0) {
            ::close(sockets[0]); ::close(sockets[1]);
            error = "Cannot protect VST2 worker descriptors"; return false;
        }
    }
#ifdef SO_NOSIGPIPE
    int enabled = 1;
    if (::setsockopt(sockets[0], SOL_SOCKET, SO_NOSIGPIPE, &enabled, sizeof(enabled)) != 0) {
        ::close(sockets[0]); ::close(sockets[1]); error = "Cannot suppress worker SIGPIPE"; return false;
    }
#endif
    posix_spawn_file_actions_t actions;
    int status = ::posix_spawn_file_actions_init(&actions);
    if (status != 0) {
        ::close(sockets[0]); ::close(sockets[1]); error = "Cannot initialize worker spawn"; return false;
    }
    status = ::posix_spawn_file_actions_adddup2(&actions, sockets[1], STDIN_FILENO);
    if (!status) status = ::posix_spawn_file_actions_adddup2(&actions, sockets[1], STDOUT_FILENO);
    if (!status) status = ::posix_spawn_file_actions_adddup2(&actions, STDERR_FILENO, STDERR_FILENO);
    if (!status) status = ::posix_spawn_file_actions_addclose(&actions, sockets[0]);
    if (!status) status = ::posix_spawn_file_actions_addclose(&actions, sockets[1]);
    char* argv[] = {const_cast<char*>(python.c_str()), const_cast<char*>(script.c_str()), nullptr};
    pid_t child = -1;
    posix_spawnattr_t attributes;
    const int attributeStatus = ::posix_spawnattr_init(&attributes);
    if (!status) status = attributeStatus;
#ifdef POSIX_SPAWN_CLOEXEC_DEFAULT
    if (!status) status = ::posix_spawnattr_setflags(&attributes, POSIX_SPAWN_CLOEXEC_DEFAULT);
#endif
    if (!status) status = ::posix_spawn(&child, python.c_str(), &actions, &attributes, argv, environ);
    if (!attributeStatus) ::posix_spawnattr_destroy(&attributes);
    ::posix_spawn_file_actions_destroy(&actions);
    ::close(sockets[1]);
    if (status != 0) {
        ::close(sockets[0]); error = "Cannot launch VST2 worker: " + std::string(std::strerror(status)); return false;
    }
    impl->pid = child; impl->socket = sockets[0]; impl->sequence = 0;
    const int flags = ::fcntl(impl->socket, F_GETFL, 0);
    if (flags < 0 || ::fcntl(impl->socket, F_SETFL, flags | O_NONBLOCK) < 0) {
        error = "Cannot configure bounded VST2 channel"; Abort(); return false;
    }
    return true;
#else
    error = "VST2 compatibility worker is unavailable on this platform"; return false;
#endif
}

bool Vst2Worker::Exchange(Json request, const std::vector<uint8_t>& input,
                         Json& result, std::vector<uint8_t>& output,
                         int timeoutMs, std::string& error) {
    result = Json(); output.clear(); error.clear();
    if (!Running() || timeoutMs <= 0 || timeoutMs > 120000 || !request.is_object()) {
        error = "Invalid VST2 worker exchange"; return false;
    }
#if defined(__APPLE__) || defined(__linux__)
    constexpr size_t maxJson = 8 * 1024 * 1024, maxAudio = 1024 * 1024;
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(timeoutMs);
    auto transfer = [&](uint8_t* data, size_t count, bool writing) {
        while (count) {
            auto remaining = std::chrono::duration_cast<std::chrono::milliseconds>(deadline - std::chrono::steady_clock::now()).count();
            if (remaining <= 0) { error = "VST2 worker timed out"; return false; }
            pollfd fd{impl->socket, static_cast<short>(writing ? POLLOUT : POLLIN), 0};
            const int ready = ::poll(&fd, 1, static_cast<int>(remaining));
            if (ready < 0 && errno == EINTR) continue;
            if (ready == 0) { error = "VST2 worker timed out"; return false; }
            if (ready < 0 || (fd.revents & (POLLERR | POLLNVAL))) { error = "VST2 worker channel failed"; return false; }
            int sendFlags = 0;
#ifdef MSG_NOSIGNAL
            sendFlags = MSG_NOSIGNAL;
#endif
            const auto n = writing ? ::send(impl->socket, data, count, sendFlags) : ::recv(impl->socket, data, count, 0);
            if (n < 0 && (errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR)) continue;
            if (n <= 0) { error = "VST2 worker exited"; return false; }
            data += n; count -= static_cast<size_t>(n);
        }
        return true;
    };
    auto put32 = [](uint8_t* p, uint32_t value) { for (int i = 0; i < 4; ++i) p[i] = static_cast<uint8_t>(value >> (8 * i)); };
    auto get32 = [](const uint8_t* p) { uint32_t value = 0; for (int i = 0; i < 4; ++i) value |= uint32_t(p[i]) << (8 * i); return value; };
    try {
        request["version"] = 1; request["id"] = ++impl->sequence;
        auto control = request.dump();
        if (control.size() > maxJson || input.size() > maxAudio) { error = "VST2 message exceeds bounds"; return false; }
        std::vector<uint8_t> packet(8 + control.size() + input.size());
        put32(packet.data(), static_cast<uint32_t>(control.size()));
        put32(packet.data() + 4, static_cast<uint32_t>(input.size()));
        std::memcpy(packet.data() + 8, control.data(), control.size());
        if (!input.empty()) std::memcpy(packet.data() + 8 + control.size(), input.data(), input.size());
        uint8_t header[8];
        if (!transfer(packet.data(), packet.size(), true) || !transfer(header, 8, false)) { Abort(); return false; }
        const auto controlSize = get32(header), audioSize = get32(header + 4);
        if (!controlSize || controlSize > maxJson || audioSize > maxAudio) {
            error = "Invalid VST2 response bounds"; Abort(); return false;
        }
        std::vector<uint8_t> body(controlSize);
        output.resize(audioSize);
        if (!transfer(body.data(), body.size(), false) || !transfer(output.data(), output.size(), false)) { output.clear(); Abort(); return false; }
        auto response = Json::parse(body.begin(), body.end());
        if (response.at("version") != 1 || response.at("id") != impl->sequence) {
            error = "VST2 response identity mismatch"; output.clear(); Abort(); return false;
        }
        if (response.at("ok") != true) {
            error = response.value("error", "VST2 worker rejected request"); output.clear(); Abort(); return false;
        }
        result = response.at("result");
        return true;
    } catch (const std::exception& exc) {
        error = std::string("VST2 protocol failure: ") + exc.what(); output.clear(); Abort(); return false;
    }
#else
    error = "VST2 worker unavailable"; return false;
#endif
}

bool Vst2Worker::Close(std::string& error) {
    if (!Running()) return true;
    Json result;
    std::vector<uint8_t> output;
    const bool ok = Exchange({{"op", "close"}}, {}, result, output, 5000, error);
    Abort();
    return ok;
}
