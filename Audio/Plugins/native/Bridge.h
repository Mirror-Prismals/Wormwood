#pragma once
#include <cstdint>

// Shared by native implementation and diagnostics; Rust declares the same ABI.
extern "C" void* ww_plugins_create();
extern "C" void ww_plugins_destroy(void*);
extern "C" const char* ww_plugins_command(void*, const char*);
// Preparation-thread parameter route: no controller/UI or JSON calls.
extern "C" bool ww_plugins_automate(void*, uint64_t, uint32_t, double);
extern "C" bool ww_plugins_process(void*, const char*, float*, float*, int,
    double, int64_t, double, double, int, int, const float*);
extern "C" bool ww_plugins_process_timeline(void*, const char*, float*, float*, int,
    double, int64_t, double, double, int, int, const float*, int);
extern "C" bool ww_plugins_process_chain_timeline(void*, const char*, float*, float*, int,
    double, int64_t, double, double, int, int, const float*, const char*, int);

// Five doubles per tuned voice: stable ID, anchor key, cents, velocity, onset.
extern "C" bool ww_plugins_process_tuned_timeline(void*, const char*, float*, float*, int,
    double, int64_t, double, double, int, int, const float*, int, const double*, int);

// Standard channel-1 pitch bend in semitones, +/-2; independent of note tuning.
extern "C" bool ww_plugins_process_bent_timeline(void*, const char*, float*, float*, int,
    double, int64_t, double, double, int, int, const float*, int, const double*, int, double);
