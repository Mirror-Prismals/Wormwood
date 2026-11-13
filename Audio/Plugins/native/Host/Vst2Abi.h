#pragma once

#include <cstddef>
#include <cstdint>

// VST2's public binary layout. No SDK implementation or platform loader lives
// here. Guest addresses must never be cast to host pointers by the emulator.
namespace Vst2Abi {
struct Effect;
using Callback = intptr_t (*)(Effect*, int32_t, int32_t, intptr_t, void*, float);
using Process = void (*)(Effect*, float**, float**, int32_t);
using SetParameter = void (*)(Effect*, int32_t, float);
using GetParameter = float (*)(Effect*, int32_t);
using ProcessDouble = void (*)(Effect*, double**, double**, int32_t);

struct Effect {
    int32_t magic;
    Callback dispatcher;
    Process process;
    SetParameter setParameter;
    GetParameter getParameter;
    int32_t numPrograms, numParams, numInputs, numOutputs, flags;
    intptr_t reserved1, reserved2;
    int32_t initialDelay, realQualities, offQualities;
    float ioRatio;
    void* object;
    void* user;
    int32_t uniqueId, version;
    Process processReplacing;
    ProcessDouble processDoubleReplacing;
    char future[56];
};

inline constexpr int32_t Magic = 0x56737450;
inline constexpr int32_t CanReplacing = 1 << 4;
inline constexpr int32_t ProgramChunks = 1 << 5;
inline constexpr int32_t IsSynth = 1 << 8;
enum Opcode : int32_t {
    Open = 0, Close = 1, SetProgram = 2, GetProgram = 3,
    SetSampleRate = 10, SetBlockSize = 11, MainsChanged = 12,
    GetChunk = 23, SetChunk = 24, ProcessEvents = 25,
    GetEffectName = 45, GetVendorString = 47, GetProductString = 48,
    GetVstVersion = 58, StartProcess = 71, StopProcess = 72
};
enum HostOpcode : int32_t {
    Automate = 0, Version = 1, CurrentId = 2, Idle = 3,
    GetTime = 7, GetSampleRate = 16, GetBlockSize = 17,
    GetCurrentProcessLevel = 23, GetVendorStringHost = 32,
    GetProductStringHost = 33, GetVendorVersion = 34, CanDo = 37
};

// The initial compatibility worker implements only the Mac x86_64 ABI.
#if INTPTR_MAX == INT64_MAX
static_assert(sizeof(Effect) == 192);
static_assert(offsetof(Effect, dispatcher) == 8);
static_assert(offsetof(Effect, numParams) == 44);
static_assert(offsetof(Effect, object) == 96);
static_assert(offsetof(Effect, processReplacing) == 120);
#endif
} // namespace Vst2Abi
