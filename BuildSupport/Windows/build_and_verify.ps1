param([ValidateRange(0,10)][int]$SoakSeconds=0,[ValidateRange(1,20)][int]$SoakRepeats=2)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$root=(Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $root
New-Item -ItemType Directory -Force output | Out-Null
$taskName=$null
$success=$false
try {
 & cmd.exe /d /c 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File BuildSupport\Windows\setup.ps1 > output\windows-setup.log 2>&1'
 if($LASTEXITCODE -ne 0){throw 'Toolchain setup failed; see windows-setup.log'}
 . (Join-Path $PSScriptRoot 'environment.ps1')
 # Fresh snapshots share dependency artifacts, but must never reuse a package
 # executable compiled with another CARGO_MANIFEST_DIR or source revision.
 & cmd.exe /d /c 'cargo clean -p wormwood > output\windows-package-clean.log 2>&1'
 if($LASTEXITCODE -ne 0){throw 'Package cache cleanup failed'}
 & cmd.exe /d /c 'cargo test --locked > output\windows-tests.log 2>&1'
 if($LASTEXITCODE -ne 0){throw 'Windows tests failed; see windows-tests.log'}
 & cmd.exe /d /c 'cargo build --locked > output\windows-build.log 2>&1'
 if($LASTEXITCODE -ne 0){throw 'Windows build failed; see windows-build.log'}
 $id=(Get-Content source-manifest.json -Raw|ConvertFrom-Json).fingerprint
 $taskName='WormwoodVerify-'+$id.Substring(0,16)+'-'+[guid]::NewGuid().ToString('N').Substring(0,8)
 $action=New-ScheduledTaskAction -Execute 'powershell.exe' -Argument ('-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "'+(Join-Path $PSScriptRoot 'verify.ps1')+'" -SoakSeconds '+$SoakSeconds+' -SoakRepeats '+$SoakRepeats) -WorkingDirectory $root
 $principal=New-ScheduledTaskPrincipal -UserId ([System.Security.Principal.WindowsIdentity]::GetCurrent().Name) -LogonType Interactive -RunLevel Limited
 $settings=New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds (180+3*$SoakRepeats*($SoakSeconds+12)))
 Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Settings $settings | Out-Null
 Start-ScheduledTask -TaskName $taskName
 $deadline=(Get-Date).AddSeconds(120+3*$SoakRepeats*($SoakSeconds+12))
 while(!(Test-Path output\windows-verification.json)){
  if((Get-Date) -gt $deadline){throw 'Interactive verification timed out'}
  Start-Sleep -Milliseconds 500
 }
 $result=Get-Content output\windows-verification.json -Raw | ConvertFrom-Json
 if(!$result.passed){throw ('Native verification failed: '+$result.error)}
 $success=$true
} catch {
 $_.ToString() | Set-Content output\windows-driver-error.txt
 Write-Output $_.ToString()
} finally {
 if($taskName){Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue}
 Compress-Archive -Path output\* -DestinationPath windows-results.zip -Force
}
if(!$success){exit 1}
