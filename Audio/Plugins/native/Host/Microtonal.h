#pragma once
#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdint>
#include <functional>
#include <string>
#include <vector>

// Canonical pitch is an ordinary MIDI anchor plus a cents offset. EDO degrees
// are an editing coordinate, never an overloaded MIDI key number.
namespace Microtonal {
constexpr int kMaxEdo = 4096; // Bounds grid allocation, not pitch resolution.
constexpr double kPitchEpsilon = 1e-7;
enum class Output { Off, Native, Mpe };
struct Voice {
    uint64_t id = 0;
    int key = 0;
    double cents = 0;
    float velocity = 0;
    uint64_t onset = 0;
};
struct ActiveVoice : Voice {
    int eventId = 0;
    int channel = 0;
    Output output = Output::Off;
};
inline int ValidEdo(int edo) { return std::clamp(edo, 1, kMaxEdo); }
template<class Note> inline double Pitch(const Note& note) { return note.pitch + note.tuningCents / 100.0; }
inline double DegreePitch(int degree, int edo) { return degree * 12.0 / ValidEdo(edo); }
template<class Note> inline int Degree(const Note& note, int edo) {
    return static_cast<int>(std::llround(Pitch(note) * ValidEdo(edo) / 12.0));
}
template<class Note> inline void SetPitch(Note& note, double pitch) {
    pitch = std::clamp(std::isfinite(pitch) ? pitch : 0.0, 0.0, 127.0);
    note.pitch = static_cast<int>(std::lround(pitch));
    note.tuningCents = (pitch - note.pitch) * 100.0;
    if (std::abs(note.tuningCents) < kPitchEpsilon * 100) note.tuningCents = 0;
}
template<class Note> inline void SetDegree(Note& note, int degree, int edo) {
    edo = ValidEdo(edo);
    degree = std::clamp(degree, 0, static_cast<int>(std::floor(127.0 * edo / 12.0)));
    SetPitch(note, DegreePitch(degree, edo));
}
template<class Note> inline bool IsMicrotonal(const Note& note) {
    return std::abs(Pitch(note) - std::round(Pitch(note))) > kPitchEpsilon;
}
template<class Clip> inline bool NeedsRetuning(const Clip& clip, int edo) {
    for (const auto& note : clip.notes)
        if (std::abs(Pitch(note) - DegreePitch(Degree(note, edo), edo)) > kPitchEpsilon) return true;
    return false;
}
template<class Clip> inline void Retune(Clip& clip, int edo, bool clear) {
    clip.edo = ValidEdo(edo);
    if (clear) clip.notes.clear();
    else for (auto& note : clip.notes) SetDegree(note, Degree(note, clip.edo), clip.edo);
}
// mode: 0 automatic, 1 explicitly declared native tuning, 2 explicitly
// declared MPE, 3 ordinary notes only. No silent nearest-semitone fallback.
inline Output ResolveOutput(int mode, bool nativeSupport, bool mpeSupport) {
    if (mode == 3) return Output::Off;
    if (mode == 1) return nativeSupport ? Output::Native : Output::Off;
    if (mode == 2) return mpeSupport ? Output::Mpe : Output::Off;
    return nativeSupport ? Output::Native : mpeSupport ? Output::Mpe : Output::Off;
}
template<class Palette> inline auto Color(double pitch, const Palette& palette) {
    const int lower = static_cast<int>(std::floor(pitch));
    const float fraction = static_cast<float>(pitch - lower);
    const size_t a = static_cast<size_t>((lower % 12 + 12) % 12), b = (a + 1) % 12;
    return palette[a] * (1.0f - fraction) + palette[b] * fraction;
}
inline std::vector<Voice> WithMpeNotes(const std::array<float,128>& ordinary,
    const std::vector<Voice>* tuned, const std::vector<ActiveVoice>& active, int64_t sampleOffset) {
    std::vector<Voice> result = tuned ? *tuned : std::vector<Voice>{};
    for (int key=0;key<128;++key) {
        if (ordinary[static_cast<size_t>(key)] <= .0001f) continue;
        const uint64_t id = (1ULL<<63) + static_cast<uint64_t>(key);
        const auto previous = std::find_if(active.begin(),active.end(),[&](const auto& v){return v.id==id;});
        const uint64_t onset = previous==active.end() ? static_cast<uint64_t>(std::max<int64_t>(0,sampleOffset)) : previous->onset;
        result.push_back({id,key,0,ordinary[static_cast<size_t>(key)],onset});
    }
    return result;
}
inline int PitchBend(double cents) {
    // Member channel RPN config below establishes +/- 48 semitones.
    return std::clamp(static_cast<int>(std::lround(8192.0 + cents * 8192.0 / 4800.0)), 0, 16383);
}
template<class MidiBytes> inline void ConfigureMpeChannel(int channel, MidiBytes emit) {
    emit(0xb0 | channel, 101, 0); emit(0xb0 | channel, 100, 0);
    emit(0xb0 | channel, 6, 48); emit(0xb0 | channel, 38, 0);
    emit(0xb0 | channel, 101, 127); emit(0xb0 | channel, 100, 127);
}
// Each plugin owns its voice state. Stops, bypasses and mode switches send
// note-offs before recycling IDs/channels. Excess MPE voices use the newest
// 15 most recently started notes consistently, avoiding steal/retrigger churn per callback.
template<class NoteEvent, class MidiBytes>
inline void Dispatch(std::vector<ActiveVoice>& active, int& nextId,
    const std::vector<Voice>* desired, Output output, bool bypassed,
    NoteEvent noteEvent, MidiBytes midiBytes, bool* mpeZoneActive = nullptr) {
    auto stop = [&](const ActiveVoice& voice) {
        if (voice.output == Output::Native) noteEvent(false, voice);
        else if (voice.output == Output::Mpe) {
            midiBytes(0x80 | voice.channel, voice.key, 0);
            midiBytes(0xe0 | voice.channel, 0, 64);
        }
    };
    std::vector<Voice> selected;
    if (desired && output == Output::Mpe && desired->size() > 15) {
        selected = *desired;
        std::sort(selected.begin(), selected.end(), [](const auto& a, const auto& b) {
            return a.onset != b.onset ? a.onset > b.onset : a.id > b.id;
        });
        selected.resize(15); desired = &selected;
    }
    const size_t count = desired && !bypassed && output != Output::Off ? desired->size() : 0;
    constexpr size_t first = 0;
    auto matches = [](const Voice& a, const Voice& b) {
        return a.id == b.id && a.key == b.key && std::abs(a.cents - b.cents) < 1e-5 && std::abs(a.velocity - b.velocity) < 1e-5;
    };
    for (auto it = active.begin(); it != active.end();) {
        bool keep = it->output == output;
        bool found = false;
        if (keep) for (size_t i = first; i < count; ++i) if (matches(*it, (*desired)[i])) { found = true; break; }
        if (!found) { stop(*it); it = active.erase(it); } else ++it;
    }
    if (mpeZoneActive && *mpeZoneActive && output != Output::Mpe) {
        midiBytes(0xb0,101,0); midiBytes(0xb0,100,6); midiBytes(0xb0,6,0);
        midiBytes(0xb0,101,127); midiBytes(0xb0,100,127); *mpeZoneActive = false;
    }
    for (size_t i = first; i < count; ++i) {
        const auto& wanted = (*desired)[i];
        if (std::any_of(active.begin(), active.end(), [&](const auto& v) { return matches(v, wanted); })) continue;
        ActiveVoice voice;
        static_cast<Voice&>(voice) = wanted;
        voice.output = output;
        if (nextId <= 0 || nextId == INT32_MAX) nextId = 1024;
        while (std::any_of(active.begin(), active.end(), [&](const auto& v) { return v.eventId == nextId; }))
            if (++nextId == INT32_MAX) nextId = 1024;
        voice.eventId = nextId++;
        if (output == Output::Native) noteEvent(true, voice);
        else {
            if (mpeZoneActive ? !*mpeZoneActive : active.empty()) {
                midiBytes(0xb0,101,0); midiBytes(0xb0,100,6); midiBytes(0xb0,6,15);
                midiBytes(0xb0,101,127); midiBytes(0xb0,100,127);
                if (mpeZoneActive) *mpeZoneActive = true;
            }
            voice.channel = 1;
            while (std::any_of(active.begin(), active.end(), [&](const auto& v) { return v.channel == voice.channel; })) ++voice.channel;
            ConfigureMpeChannel(voice.channel, midiBytes);
            const int bend = PitchBend(voice.cents);
            midiBytes(0xe0 | voice.channel, bend & 127, bend >> 7);
            midiBytes(0x90 | voice.channel, voice.key, std::clamp(static_cast<int>(std::lround(voice.velocity * 127)), 1, 127));
        }
        active.push_back(voice);
    }
}
template<class Track> inline void Collect(const Track& track, uint64_t sample,
    std::array<float, 128>& standard, std::vector<Voice>& tuned, int transpose = 0, uint64_t salt = 0) {
    for (size_t ci = 0; ci < track.clips.size(); ++ci) {
        const auto& clip = track.clips[ci];
        if (!clip.length || sample < clip.startSample || sample - clip.startSample >= clip.length) continue;
        const auto local = sample - clip.startSample;
        for (size_t ni = 0; ni < clip.notes.size(); ++ni) {
            const auto& note = clip.notes[ni];
            if (!note.length || local < note.startSample || local - note.startSample >= note.length) continue;
            const int key = note.pitch + transpose;
            const float velocity = std::clamp(note.velocity, 0.0f, 1.0f);
            if (key < 0 || key > 127 || velocity <= 0) continue;
            if (IsMicrotonal(note)) tuned.push_back({salt + ((ci + 1) << 32) + ni, key, note.tuningCents, velocity, clip.startSample + note.startSample});
            else standard[static_cast<size_t>(key)] = std::max(standard[static_cast<size_t>(key)], velocity);
        }
    }
}
}

struct MicrotonalContext {
    bool menuOpen = false;
    bool confirmOpen = false;
    bool ownsPointer = false;
    bool mouseWasDown = false;
    bool undoWasDown = false, redoWasDown = false;
    int targetTrack = -1, targetClip = -1, requestedEdo = 12;
    int openingEdo = 12;
    uint64_t openingClipStart = 0;
    std::string edoText = "12", error;
    std::atomic<uint64_t> unsupportedSerial{0};
    std::atomic<int> unsupportedTrack{-1};
    uint64_t displayedSerial = 0;
    double noticeUntil = 0;
};

namespace Microtonal {
template<class Track> inline void ReportUnsupported(MicrotonalContext* context, Track& track, bool blocked, int trackIndex) {
    const bool previous = track.microtonalUnsupportedActive.exchange(blocked, std::memory_order_relaxed);
    if (blocked && !previous && context) {
        context->unsupportedTrack.store(trackIndex, std::memory_order_relaxed);
        context->unsupportedSerial.fetch_add(1, std::memory_order_relaxed);
    }
}
inline void Transpose(std::vector<Voice>& voices, int semitones) {
    for (auto& voice : voices) voice.key += semitones;
    voices.erase(std::remove_if(voices.begin(), voices.end(), [](const auto& v) { return v.key < 0 || v.key > 127; }), voices.end());
}
}
