#pragma once

#include <cstdint>
#include <memory>
#include <string>
#include <vector>
#include "json.hpp"

// Platform transport for a single isolated VST2 session. Blocking operations
// belong to a dedicated control/processing thread, NEVER the audio callback.
// One owner serializes calls. A failed exchange destroys the worker session.
class Vst2Worker {
public:
    using Json = nlohmann::json;
    Vst2Worker();
    ~Vst2Worker();
    Vst2Worker(const Vst2Worker&) = delete;
    Vst2Worker& operator=(const Vst2Worker&) = delete;

    bool Start(const std::string& python, const std::string& script, std::string& error);
    bool Exchange(Json request, const std::vector<uint8_t>& input,
                  Json& result, std::vector<uint8_t>& output,
                  int timeoutMs, std::string& error);
    bool Close(std::string& error);
    void Abort() noexcept;
    bool Running() const noexcept;

private:
    struct Impl;
    std::unique_ptr<Impl> impl;
};
