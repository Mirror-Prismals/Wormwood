#pragma once
#include "Host/Vst2Worker.h"
#include <memory>

// Main-thread client; the independent thread owns every blocking worker call.
class Vst2EditorBridge {
public:
    using Json = Vst2Worker::Json;
    struct Reply {
        Json result, sent;
        std::vector<uint8_t> png;
        std::string error;
    };
    Vst2EditorBridge(const std::string& python, const std::string& script, Json open);
    ~Vst2EditorBridge();
    Vst2EditorBridge(const Vst2EditorBridge&) = delete;
    Vst2EditorBridge& operator=(const Vst2EditorBridge&) = delete;
    bool Submit(Json request);
    bool Poll(Reply& reply);
private:
    struct Shared;
    std::shared_ptr<Shared> shared;
};
