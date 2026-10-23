param([ValidateRange(0,10)][int]$SoakSeconds=0,[ValidateRange(1,20)][int]$SoakRepeats=2)
# Run on the logged-in desktop, not SSH's noninteractive session.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$root=(Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $root
$output=Join-Path $root 'output'
New-Item -ItemType Directory -Force $output | Out-Null
$result=@{passed=$false;platform='Windows';fingerprint=(Get-Content source-manifest.json -Raw|ConvertFrom-Json).fingerprint;backends=@()}
try {
 . (Join-Path $PSScriptRoot 'environment.ps1')
 $result.toolchain=Get-Content (Join-Path $HOME 'WormwoodTools\toolchain.json') -Raw | ConvertFrom-Json
 $result.gpu=Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion
 $result.session_id=(Get-Process -Id $PID).SessionId
 if($result.session_id -eq 0){throw 'Verification requires the logged-in desktop session'}
 foreach($backend in @('OpenGL','Vulkan','WebGPU')){
  $report=Join-Path $output "av-$backend.json"
  Remove-Item $report -ErrorAction SilentlyContinue
  $process=Start-Process -FilePath (Join-Path $env:CARGO_TARGET_DIR 'debug\wormwood.exe') -ArgumentList @('--av',$backend,'--frames','60') -WorkingDirectory $root -RedirectStandardOutput (Join-Path $output "av-$backend.stdout.log") -RedirectStandardError (Join-Path $output "av-$backend.stderr.log") -PassThru
  $null=$process.Handle
  if(!$process.WaitForExit(25000)){
   # Target only native children of this exact diagnostic invocation.
   $children=Get-CimInstance Win32_Process -Filter ("ParentProcessId="+$process.Id) | Where-Object {$_.Name -eq 'jackd.exe' -and $_.CommandLine -match '\-n Wormwood' -and $_.CreationDate -ge $process.StartTime}
   foreach($child in $children){Stop-Process -Id $child.ProcessId -ErrorAction SilentlyContinue}
   if(!$process.HasExited){$process.Kill()}
   $process.WaitForExit()
   throw "$backend cue timed out"
  }
  $process.WaitForExit()
  if($process.ExitCode -ne 0){throw "$backend failed: $(Get-Content (Join-Path $output "av-$backend.stderr.log") -Raw)"}
  $value=Get-Content $report -Raw | ConvertFrom-Json
  if($value.stage -ne 'Finished' -or $value.entities_remaining -ne 0 -or !$value.audio.loopback_verified -or $value.audio.samples_produced -ne 36000 -or $value.audio.active_xruns -ne 0 -or !$value.presentation.device.readback_verified -or $value.presentation.backend -ne $backend -or $value.presentation.frames -ne 60){throw "$backend verification assertions failed"}
  Copy-Item (Join-Path $output 'jack-server.log') (Join-Path $output "av-$backend.server.log") -ErrorAction SilentlyContinue
  $result.backends+=,$value
 }
 if($SoakSeconds -gt 0){
  $result.soaks=@()
  foreach($backend in @('OpenGL','Vulkan','WebGPU')){
   $report=Join-Path $output "soak-$backend.json"
   Remove-Item $report -ErrorAction SilentlyContinue
   $process=Start-Process -FilePath (Join-Path $env:CARGO_TARGET_DIR 'debug\wormwood.exe') -ArgumentList @('--soak',$backend,'--seconds',"$SoakSeconds",'--repeat',"$SoakRepeats") -WorkingDirectory $root -RedirectStandardOutput (Join-Path $output "soak-$backend.stdout.log") -RedirectStandardError (Join-Path $output "soak-$backend.stderr.log") -PassThru
   $null=$process.Handle
   if(!$process.WaitForExit(1000*($SoakRepeats*($SoakSeconds+12)+10))){
    Get-CimInstance Win32_Process -Filter ("ParentProcessId="+$process.Id) | Where-Object {$_.Name -eq 'jackd.exe' -and $_.CommandLine -match '\-n Wormwood' -and $_.CreationDate -ge $process.StartTime} | ForEach-Object {Stop-Process -Id $_.ProcessId -ErrorAction SilentlyContinue}
    if(!$process.HasExited){$process.Kill()};$process.WaitForExit();throw "$backend soak timed out"
   }
   $process.WaitForExit();if($process.ExitCode -ne 0){throw "$backend soak failed"}
   $value=Get-Content $report -Raw|ConvertFrom-Json
   if(!$value.passed -or !$value.same_process -or $value.runs.Count -ne $SoakRepeats){throw "$backend soak repetition assertion failed"}
   foreach($run in $value.runs){
    $audio=$run.audio;$presentation=$run.presentation
    $callbackSeconds=$audio.callbacks*$audio.buffer_frames/$audio.sample_rate_hz
    if($run.stage -ne 'Finished' -or $run.entities_remaining -ne 0 -or !$audio.loopback_verified -or $audio.active_xruns -ne 0 -or $audio.samples_produced -ne 36000 -or $callbackSeconds -lt ($SoakSeconds-.1) -or !$presentation.device.readback_verified -or $presentation.frames -ne ($SoakSeconds*60) -or $presentation.frame_period_us -ne 16667){throw "$backend soak runtime assertion failed"}
   }
   $result.soaks+=,$value
  }
 }
 $result.passed=$true
} catch {$result.error=$_.ToString()}
$result | ConvertTo-Json -Depth 12 | Set-Content -Encoding UTF8 (Join-Path $output 'windows-verification.json')
if(!$result.passed){exit 1}
