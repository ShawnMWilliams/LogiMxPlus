<#
.SYNOPSIS
    Install LogiMxPlus.exe for the current user, start it at login, and start it now.
    No Python needed.

.PARAMETER Exe
    Path to a LogiMxPlus.exe to install. Default: download the latest GitHub release.

.EXAMPLE
    .\scripts\install-windows.ps1
    .\scripts\install-windows.ps1 -Exe .\dist\LogiMxPlus.exe
#>
param(
    [string]$Exe = ""
)
$ErrorActionPreference = "Stop"
$dest = Join-Path $env:LOCALAPPDATA "Programs\LogiMxPlus"
$target = Join-Path $dest "LogiMxPlus.exe"

# Stop any running copy (including older Python-based installs) so files can be replaced.
Get-CimInstance Win32_Process |
    Where-Object { $_.Name -eq 'LogiMxPlus.exe' -or $_.Name -eq 'logimxplusw.exe' -or
                   ($_.Name -like 'python*' -and $_.CommandLine -match 'logimxplus') } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }

# Remove an older uv-based install, if present.
$uv = (Get-Command uv -ErrorAction SilentlyContinue).Source
if ($uv -and ((& $uv tool list 2>$null) -match '^logimxplus ')) {
    & $uv tool uninstall logimxplus | Out-Null
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
