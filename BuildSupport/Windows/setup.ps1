param([string]$MsysRoot='C:\msys64')
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$cache=Join-Path $HOME 'WormwoodTools'
New-Item -ItemType Directory -Force $cache | Out-Null
# Add the required compiler/SDK packages through the existing package manager.
# Do not refresh databases or run a general system upgrade here.
$bash=Join-Path $MsysRoot 'usr\bin\bash.exe'
& $bash -lc 'pacman -S --needed --noconfirm mingw-w64-x86_64-rust mingw-w64-x86_64-clang mingw-w64-x86_64-glslang mingw-w64-x86_64-vulkan-headers mingw-w64-x86_64-glfw'
if($LASTEXITCODE -ne 0){throw 'MSYS2 prerequisite installation failed; inspect the package-manager log. No database refresh/system upgrade was requested.'}
$archive=Join-Path $cache 'wgpu-v27.0.4.0-windows-gnu.zip'
$hash='c0c2dbcef3c6a9933a1a1bf7cbdaaebed61a33c833bacb0269662f91536be8bd'
if(!(Test-Path $archive)){
 Invoke-WebRequest -UseBasicParsing 'https://github.com/gfx-rs/wgpu-native/releases/download/v27.0.4.0/wgpu-windows-x86_64-gnu-release.zip' -OutFile $archive
}
if((Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $hash){throw 'Pinned wgpu-native archive SHA-256 mismatch'}
$wgpu=Join-Path $cache 'wgpu-v27.0.4.0-windows-gnu'
if(!(Test-Path $wgpu)){Expand-Archive $archive -DestinationPath $wgpu}
$bin=Join-Path $MsysRoot 'mingw64\bin'
$env:PATH="$bin;$env:PATH"
& cargo --version
& rustc --version
$packages=& $bash -lc 'pacman -Q mingw-w64-x86_64-rust mingw-w64-x86_64-clang mingw-w64-x86_64-glslang mingw-w64-x86_64-vulkan-headers mingw-w64-x86_64-glfw'
@{msys_root=$MsysRoot;wgpu=$wgpu;wgpu_archive_sha256=$hash;packages=$packages} | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 (Join-Path $cache 'toolchain.json')
