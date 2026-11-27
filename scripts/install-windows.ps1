<#
.SYNOPSIS
    Install LogiMxPlus for the current user, start it at login, and start it now.

.PARAMETER Source
    What to install: a local checkout path, or a git URL (default: the GitHub repo).

.EXAMPLE
    .\scripts\install-windows.ps1
    .\scripts\install-windows.ps1 -Source .
#>
param(
    [string]$Source = "git+https://github.com/ShawnMWilliams/LogiMxPlus"
)
$ErrorActionPreference = "Stop"

$uv = (Get-Command uv -ErrorAction SilentlyContinue).Source
if (-not $uv) { throw "uv not found on PATH. Install it from https://docs.astral.sh/uv/" }

# Stop a running copy so its files can be replaced.
Get-CimInstance Win32_Process -Filter "Name='pythonw.exe' or Name='python.exe' or Name='logimxplusw.exe'" |
    Where-Object { $_.CommandLine -match 'logimxplus' } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }

& $uv tool install --python 3.12 --force --reinstall $Source
if ($LASTEXITCODE -ne 0) { throw "uv tool install failed ($LASTEXITCODE)" }

$bin = (& $uv tool dir --bin).Trim()
$env:PATH = "$bin;$env:PATH"  # so the login entry uses the named logimxplusw.exe launcher
& (Join-Path $bin "logimxplus.exe") install
if ($LASTEXITCODE -ne 0) { throw "logimxplus install failed ($LASTEXITCODE)" }

# Start it now; it shows in Task Manager as logimxplusw.exe.
Start-Process -FilePath (Join-Path $bin "logimxplusw.exe") -ArgumentList "run" -WindowStyle Hidden
$cmd = (Get-ItemProperty "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run").LogiMxPlus
Write-Output "LogiMxPlus installed and started. Login entry: $cmd"
