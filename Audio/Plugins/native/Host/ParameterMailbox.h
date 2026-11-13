#pragma once
#include <atomic>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <memory>
#include <stdexcept>
#include <vector>

// The editor publishes one latest value per declared parameter. Intermediate
// UI edits before the next block have the same sample offset, so retaining the
// last value is also the VST3 queue's semantics. A full event ring must never
// discard the knob's final position. Configure only while processing is stopped.
class NormalizedParameterMailbox {
    struct Slot { uint32_t id = 0; std::atomic<uint64_t> value{0}; };
    std::unique_ptr<Slot[]> slots;
    size_t count = 0;
    static constexpr uint64_t dirty = uint64_t(1) << 63;
public:
    static_assert(std::atomic<uint64_t>::is_always_lock_free,
                  "Parameter publication requires lock-free 64-bit atomics");
    void configure(const std::vector<uint32_t>& ids) {
        if (ids.size() > 4096) throw std::runtime_error("Plugin parameter count exceeds 4096");
        bool unchanged=ids.size()==count;
        for(size_t i=0;unchanged&&i<count;++i)unchanged=ids[i]==slots[i].id;
        if(unchanged)return;
        auto next = std::make_unique<Slot[]>(ids.size());
        for (size_t i = 0; i < ids.size(); ++i) {
            next[i].id = ids[i];
            for(size_t j=0;j<count;++j)if(slots[j].id==ids[i]) {
                next[i].value.store(slots[j].value.load(std::memory_order_relaxed),std::memory_order_relaxed);
                break;
            }
        }
        slots = std::move(next); count = ids.size();
    }
    size_t size() const { return count; }
    bool publish(uint32_t id, double value) {
        if (!std::isfinite(value) || value < 0. || value > 1.) return false;
        // Normalized values cannot use the sign bit; canonicalize negative zero.
        if (value == 0.) value = 0.;
        uint64_t bits; std::memcpy(&bits, &value, sizeof(bits));
        for (size_t i = 0; i < count; ++i) if (slots[i].id == id) {
            slots[i].value.store(bits | dirty, std::memory_order_release);
            return true;
        }
        return false;
    }
    template<class Consume> void consumeIndexed(Consume consume) {
        for (size_t i = 0; i < count; ++i) {
            auto bits = slots[i].value.exchange(0, std::memory_order_acquire);
            if (!(bits & dirty)) continue;
            bits &= ~dirty;
            double value; std::memcpy(&value, &bits, sizeof(value));
            consume(i, slots[i].id, value);
        }
    }
    template<class Consume> void consume(Consume consume) {
        consumeIndexed([&](size_t,uint32_t id,double value){consume(id,value);});
    }
};
