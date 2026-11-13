#pragma once
#include <algorithm>
#include <cstddef>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace PluginInserts {
// Cardinal Vst3System.cpp::AddPluginToMidiTrack creates the replacement
// before retiring the old instrument. Effects append in their existing order.
// Kept independent of native SDKs so the ownership transition is testable.
template<class Insert, class Create, class Destroy>
void assign(std::vector<Insert>& inserts, const std::string& track,
            bool instrument, std::size_t capacity, Create create, Destroy destroy) {
    auto previous = std::find_if(inserts.begin(), inserts.end(), [&](const auto& p) {
        return instrument && p.track == track && p.instrument();
    });
    const auto replacement = static_cast<std::size_t>(previous - inserts.begin());
    if (replacement == inserts.size() && inserts.size() >= capacity)
        throw std::runtime_error("Plugin insert capacity reached (128)");
    // Allocate before invoking a plug-in factory. An allocation failure must
    // leave the live instrument, editor, effects and other tracks untouched.
    if (replacement == inserts.size()) inserts.reserve(inserts.size() + 1);
    Insert pending = create(); // throws on failure; old instrument still lives
    if (replacement < inserts.size()) {
        std::swap(inserts[replacement], pending);
        destroy(pending);
        return;
    }
    const auto at = instrument
        ? std::find_if(inserts.begin(), inserts.end(), [&](const auto& p) { return p.track == track; })
        : inserts.end();
    try {
        inserts.insert(at, std::move(pending));
    } catch (...) {
        destroy(pending);
        throw;
    }
}
}
