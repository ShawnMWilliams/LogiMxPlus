<#
.SYNOPSIS
    Install LogiMxPlus.exe (the Rust build) for the current user, start it at login, and
    start it now. Nothing else needs to be installed.

.PARAMETER Exe
    Path to a LogiMxPlus.exe to install. Default: download the latest GitHub release.

.EXAMPLE
    .\scripts\install-windows.ps1
    .\scripts\install-windows.ps1 -Exe .\target\release\logimxplus.exe
#>
param(
    [string]$Exe = ""
)
$ErrorActionPreference = "Stop"
$dest = Join-Path $env:LOCALAPPDATA "Programs\LogiMxPlus"
$target = Join-Path $dest "LogiMxPlus.exe"

# Stop any running copy (including older Python-based installs) so files can be replaced.
$old = Get-CimInstance Win32_Process |
    Where-Object { $_.Name -eq 'LogiMxPlus.exe' -or $_.Name -eq 'logimxplusw.exe' -or
                   ($_.Name -like 'python*' -and $_.CommandLine -match 'logimxplus') }
foreach ($p in $old) { Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue }
foreach ($p in $old) { Wait-Process -Id $p.ProcessId -Timeout 10 -ErrorAction SilentlyContinue }

# Remove an older uv-based install, if present.
# (Best effort: Windows PowerShell treats uv's stderr chatter as an error under "Stop".)
$uv = (Get-Command uv -ErrorAction SilentlyContinue).Source
if ($uv) {
    try {
        $ErrorActionPreference = "Continue"
        if ((& $uv tool list 2>&1 | Out-String) -match '(?m)^logimxplus ') {
            & $uv tool uninstall logimxplus 2>&1 | Out-Null
        }
    } catch {
        Write-Output "note: could not check for an old uv install: $_"
    } finally {
        $ErrorActionPreference = "Stop"
    }
}

New-Item -ItemType Directory -Force -Path $dest | Out-Null
if ($Exe) {
    Copy-Item -Path $Exe -Destination $target -Force
} else {
    $url = "https://github.com/ShawnMWilliams/LogiMxPlus/releases/latest/download/LogiMxPlus.exe"
    Invoke-WebRequest -Uri $url -OutFile $target -UseBasicParsing
}

# Register the login entry (it points at $target), then start it now.
Start-Process -FilePath $target -ArgumentList "install" -Wait -WindowStyle Hidden
Start-Process -FilePath $target -ArgumentList "run" -WindowStyle Hidden
$cmd = (Get-ItemProperty "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run").LogiMxPlus
Write-Output "LogiMxPlus installed to $target and started. Login entry: $cmd"
