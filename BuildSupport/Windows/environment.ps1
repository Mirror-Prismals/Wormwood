$ErrorActionPreference='Stop'
$cache=Join-Path $HOME 'WormwoodTools'
$tools=Get-Content (Join-Path $cache 'toolchain.json') -Raw | ConvertFrom-Json
$prefix=Join-Path $tools.msys_root 'mingw64'
$bin=Join-Path $prefix 'bin'
$wgpuHeader=Get-ChildItem $tools.wgpu -Recurse -Filter wgpu.h | Select-Object -First 1
$wgpuLibrary=Get-ChildItem $tools.wgpu -Recurse -Filter wgpu_native.dll | Select-Object -First 1
if(!$wgpuHeader -or !$wgpuLibrary){throw 'Pinned wgpu-native SDK incomplete'}
$env:PATH="$bin;C:\Program Files\JACK2;$env:PATH"
$env:LIBCLANG_PATH=$bin
$env:CARGO_TARGET_DIR=Join-Path $cache 'cargo-target'
$env:WORMWOOD_NATIVE_PREFIX=$prefix.Replace('\','/')
$env:WORMWOOD_WEBGPU_INCLUDE=$wgpuHeader.DirectoryName.Replace('\','/')
$env:WORMWOOD_GLFW_LIBRARY=Join-Path $bin 'glfw3.dll'
$env:WORMWOOD_WEBGPU_LIBRARY=$wgpuLibrary.FullName
$env:WORMWOOD_VULKAN_LIBRARY='C:\Windows\System32\vulkan-1.dll'
$env:WORMWOOD_JACK_LIBRARY='C:\Windows\libjack64.dll'
$env:WORMWOOD_JACKD='C:\Program Files\JACK2\jackd.exe'
if(!$env:WORMWOOD_JACK_OUTPUT){$env:WORMWOOD_JACK_OUTPUT='ASIO::Focusrite USB ASIO'}
$validator=Join-Path $bin 'glslangValidator.exe'
if(!(Test-Path $validator)){$validator=Join-Path $bin 'glslang.exe'}
$env:WORMWOOD_GLSLANG=$validator

if(!$env:WORMWOOD_JACK_PERIOD){$env:WORMWOOD_JACK_PERIOD="256"}
