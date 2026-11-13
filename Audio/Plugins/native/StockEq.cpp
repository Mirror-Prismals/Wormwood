// Cardinal LShapingSystem DSP; only the game Host wrapper is removed.
#include "StockEq.h"
using json = nlohmann::json;
#include <algorithm>
#include <cmath>
#include <complex>

namespace LShapingSystemLogic {
namespace {
constexpr double kPi = 3.14159265358979323846;
constexpr std::array<int, 7> kSlopes{{6, 12, 18, 24, 30, 36, 48}};
struct Coeff { double b0=1, b1=0, b2=0, a1=0, a2=0; };
using Coefficients = std::array<Coeff, 8>;

float finiteClamp(float value, float low, float high, float fallback) {
    return std::clamp(std::isfinite(value) ? value : fallback, low, high);
}
int orderFor(int slope) {
    int nearest = 12;
    for (int option : kSlopes)
        if (std::abs(double(slope) - option) < std::abs(double(slope) - nearest)) nearest = option;
    return nearest / 6;
}
std::array<double, 2> frequencies(const LShapingTrackState& s, float sr) {
    const float upper = std::min(20000.0f, sr * 0.475f);
    double low = finiteClamp(s.bands[0].frequencyHz.load(std::memory_order_relaxed), 20, upper / 1.01f, 180);
    double high = finiteClamp(s.bands[1].frequencyHz.load(std::memory_order_relaxed), float(low * 1.01), upper, 6000);
    return {{low, high}};
}
Coeff normalize(double b0, double b1, double b2, double a0, double a1, double a2) {
    return {b0/a0, b1/a0, b2/a0, a1/a0, a2/a0};
}

// A Butterworth pole/zero shelf of order N, bilinear-transformed after
// prewarping the midpoint. Pole and zero radii straddle that midpoint.
// Complementary high shelves (+gain at low edge, -gain at high edge)
// return to unity outside the band. Orders change transition steepness;
// no resonant Q boost is used. This is our design, not Meyer's algorithm.
void edge(Coefficients& c, int offset, double hz, double db, int order, float sr) {
    const double w = std::tan(kPi * hz / sr);
    const double ratio = std::pow(10.0, db / (40.0 * order));
    const double pole = w * ratio, zero = w / ratio;
    int slot = offset;
    if (order % 2) {
        const double g = pole / zero;
        c[slot++] = normalize(g*(1+zero), g*(zero-1), 0, 1+pole, pole-1, 0);
    }
    for (int i = 0; i < order / 2; ++i) {
        const double damping = 2 * std::sin(kPi * (2*i+1) / (2*order));
        const double g = (pole*pole)/(zero*zero);
        c[slot++] = normalize(g*(1+damping*zero+zero*zero), 2*g*(zero*zero-1),
                              g*(1-damping*zero+zero*zero), 1+damping*pole+pole*pole,
                              2*(pole*pole-1), 1-damping*pole+pole*pole);
    }
}
Coefficients coefficients(const std::array<double, 2>& hz, double gain,
                          const std::array<int, 2>& orders, float sr) {
    Coefficients c{};
    if (std::abs(gain) < 1e-8) return c;
    edge(c, 0, hz[0], gain, orders[0], sr);
    edge(c, 4, hz[1], -gain, orders[1], sr);
    return c;
}
double filter(double x, const Coefficients& c, std::array<LShapingBiquadMemory, 8>& memory) {
    for (size_t i = 0; i < c.size(); ++i) {
        const auto& a = c[i]; auto& m = memory[i];
        const double y = a.b0*x + m.z1;
        m.z1 = a.b1*x - a.a1*y + m.z2;
        m.z2 = a.b2*x - a.a2*y;
        if (std::abs(m.z1) < 1e-24) m.z1 = 0;
        if (std::abs(m.z2) < 1e-24) m.z2 = 0;
        x = y;
    }
    if (!std::isfinite(x)) { memory = {}; return 0; }
    return x;
}
LShapingDspPathState& pathFor(LShapingTrackState& s, ProcessingPath path) {
    return path == ProcessingPath::Offline ? s.offline : s.realtime;
}
void process(LShapingTrackState& s, float* l, float* r, int frames, float sr, ProcessingPath path) {
    if (!l || frames <= 0 || !std::isfinite(sr) || sr < 100) return;
    auto& p = pathFor(s, path);
    const auto targetHz = frequencies(s, sr);
    const double targetGain = finiteClamp(s.gainDb.load(std::memory_order_relaxed), -24, 24, 0);
    const double targetWet = s.bypassed.load(std::memory_order_relaxed) ? 0 : 1;
    const std::array<int, 2> orders{{orderFor(s.bands[0].slopeDbOct.load(std::memory_order_relaxed)),
                                    orderFor(s.bands[1].slopeDbOct.load(std::memory_order_relaxed))}};
    if (!p.initialized || p.sampleRate != sr) {
        p = {};
        p.smoothLogHz = {{std::log(targetHz[0]), std::log(targetHz[1])}};
        p.smoothGainDb = targetGain; p.wet = targetWet;
        p.orders = orders; p.sampleRate = sr; p.initialized = true;
    }
    if (orders != p.orders && p.slopeFade >= 1) {
        p.previousOrders = p.orders; p.previousLeft = p.left; p.previousRight = p.right;
        p.orders = orders; p.left = {}; p.right = {}; p.slopeFade = 0;
    }
    const double wetStep = 1.0 / (0.01 * sr);
    const double fadeStep = 1.0 / (0.02 * sr);
    const std::array<double, 2> targetLogHz{{std::log(targetHz[0]), std::log(targetHz[1])}};
    // Short sub-blocks while moving; settled controls need one coefficient set
    // per host buffer, even when many shaping instances are stacked.
    for (int start = 0; start < frames;) {
        const bool settled = std::abs(targetGain - p.smoothGainDb) < 1e-7
            && std::abs(targetLogHz[0] - p.smoothLogHz[0]) < 1e-8
            && std::abs(targetLogHz[1] - p.smoothLogHz[1]) < 1e-8;
        if (settled) { p.smoothLogHz = targetLogHz; p.smoothGainDb = targetGain; }
        const int count = settled ? frames - start : std::min(32, frames - start);
        const double alpha = -std::expm1(-double(count) / (0.018 * sr));
        for (int i = 0; i < 2; ++i) p.smoothLogHz[i] += alpha * (targetLogHz[i] - p.smoothLogHz[i]);
        p.smoothGainDb += alpha * (targetGain - p.smoothGainDb);
        const std::array<double, 2> hz{{std::exp(p.smoothLogHz[0]), std::exp(p.smoothLogHz[1])}};
        const auto c = coefficients(hz, p.smoothGainDb, p.orders, sr);
        const auto old = p.slopeFade < 1 ? coefficients(hz, p.smoothGainDb, p.previousOrders, sr) : c;
        for (int i = start; i < start + count; ++i) {
            p.wet += std::clamp(targetWet - p.wet, -wetStep, wetStep);
            auto sample = [&](float dry, auto& current, auto& previous) {
                double wet = filter(dry, c, current);
                if (p.slopeFade < 1) {
                    const double before = filter(dry, old, previous);
                    wet = before + p.slopeFade * (wet - before);
                }
                return float(dry + p.wet * (wet - dry));
            };
            l[i] = sample(l[i], p.left, p.previousLeft);
            if (r) r[i] = sample(r[i], p.right, p.previousRight);
            p.slopeFade = std::min(1.0, p.slopeFade + fadeStep);
        }
        start += count;
    }
}
float readNumber(const json& value, const char* key, float fallback) {
    auto it = value.find(key);
    if (it == value.end() || !it->is_number()) return fallback;
    const float result = it->get<float>();
    return std::isfinite(result) ? result : fallback;
}
} // namespace

void ResetPath(LShapingTrackState& s, ProcessingPath p) { pathFor(s, p) = {}; }
void ProcessStereo(LShapingTrackState& s, float* l, float* r, int n, float sr, ProcessingPath p) {
    if (r) process(s, l, r, n, sr, p);
}
void ProcessMono(LShapingTrackState& s, float* x, int n, float sr, ProcessingPath p) { process(s, x, nullptr, n, sr, p); }
void ResponseCurveDb(const LShapingTrackState& s, const float* hz, float* result, int count, float sr) {
    if (!hz || !result || count <= 0) return;
    if (s.bypassed.load(std::memory_order_relaxed) || !std::isfinite(sr) || sr < 100) {
        std::fill(result, result + count, 0.0f); return;
    }
    const auto c = coefficients(frequencies(s, sr), finiteClamp(s.gainDb.load(std::memory_order_relaxed), -24, 24, 0),
        {{orderFor(s.bands[0].slopeDbOct.load(std::memory_order_relaxed)), orderFor(s.bands[1].slopeDbOct.load(std::memory_order_relaxed))}}, sr);
    for (int i = 0; i < count; ++i) {
        const double w = 2*kPi*finiteClamp(hz[i], 0, sr*.5f, 1000)/sr;
        const auto z = std::polar(1.0, -w); double db = 0;
        for (const auto& a : c) {
            const auto h = (a.b0 + a.b1*z + a.b2*z*z) / (1.0 + a.a1*z + a.a2*z*z);
            db += 20*std::log10(std::max(1e-12, std::abs(h)));
        }
        result[i] = float(std::clamp(db, -48.0, 48.0));
    }
}
float ResponseDb(const LShapingTrackState& s, float hz, float sr) {
    float result = 0;
    ResponseCurveDb(s, &hz, &result, 1, sr);
    return result;
}

json Serialize(const LShapingTrackState& s) {
    const auto hz = frequencies(s, 48000);
    json out = {{"version", 2}, {"bypassed", s.bypassed.load(std::memory_order_relaxed)},
                {"gain_db", finiteClamp(s.gainDb.load(std::memory_order_relaxed), -24, 24, 0)}, {"edges", json::array()}};
    for (int i = 0; i < 2; ++i) out["edges"].push_back({{"frequency_hz", hz[i]},
        {"slope_db_oct", 6*orderFor(s.bands[i].slopeDbOct.load(std::memory_order_relaxed))}});
    return out;
}
void Deserialize(LShapingTrackState& s, const json& v) {
    if (!v.is_object()) return;
    s.bypassed.store(v.contains("bypassed") && v["bypassed"].is_boolean() && v["bypassed"].get<bool>(), std::memory_order_relaxed);
    // Old independent shelf gains have no unique single-band equivalent.
    // Keep their transition positions but reset the replacement band to unity.
    const bool modern = v.contains("edges") && v["edges"].is_array();
    s.gainDb.store(modern ? finiteClamp(readNumber(v, "gain_db", 0), -24, 24, 0) : 0, std::memory_order_relaxed);
    auto edges = v.find(modern ? "edges" : "poles");
    for (int i = 0; i < 2; ++i) {
        const json e = edges != v.end() && edges->is_array() && i < int(edges->size()) && (*edges)[i].is_object() ? (*edges)[i] : json::object();
        s.bands[i].frequencyHz.store(finiteClamp(readNumber(e, "frequency_hz", i ? 6000 : 180), 20, 20000, i ? 6000 : 180), std::memory_order_relaxed);
        s.bands[i].slopeDbOct.store(6*orderFor(int(finiteClamp(readNumber(e, "slope_db_oct", 12), 6, 48, 12))), std::memory_order_relaxed);
    }
    const auto hz = frequencies(s, 48000);
    for (int i = 0; i < 2; ++i) s.bands[i].frequencyHz.store(float(hz[i]), std::memory_order_relaxed);
    ResetPath(s, ProcessingPath::Realtime); ResetPath(s, ProcessingPath::Offline);
}

} // namespace LShapingSystemLogic
