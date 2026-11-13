#pragma once
#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>
struct ClapPlugin;
struct Vst3Context;
struct Vst3AvailablePlugin;
struct Vst2HostedInstance;
// VST2 platform backend using the existing shared insert/parameter presentation
// container. No CLAP binary or CLAP ABI is involved in a VST2 instance.
namespace Vst2Host {
void Discover(Vst3Context&, const std::string& root, const std::string& userRoot);
ClapPlugin* Create(Vst3Context&, const Vst3AvailablePlugin&, bool instrument);
void Update(ClapPlugin&);
std::string EditorTitle(const ClapPlugin&);
void ConfigureEditor(ClapPlugin&);
void Destroy(ClapPlugin&);
bool Process(ClapPlugin&, const float*, const float*, float*, float*, int);
bool SetParameter(ClapPlugin&, uint32_t, double);
double GetParameter(const ClapPlugin&, uint32_t);
bool SaveState(const ClapPlugin&, std::vector<uint8_t>&);
bool LoadState(ClapPlugin&, const uint8_t*, size_t);
}
