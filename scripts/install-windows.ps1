<#
.SYNOPSIS
    Install LogiMxPlus for the current user, start it at login, and start it now.

.PARAMETER Source
    What to install: a local checkout path, or a git URL (default: the GitHub repo over SSH).

.EXAMPLE
    .\scripts\install-windows.ps1
    .\scripts\install-windows.ps1 -Source C:\repos\ShawnMWilliams\LogiMxPlus
#>
param(
    [string]$Source = "git+ssh://git@github.com/ShawnMWilliams/LogiMxPlus"
)
$ErrorActionPreference = "Stop"

$uv = (Get-Command uv -ErrorAction SilentlyContinue).Source
if (-not $uv) { throw "uv not found on PATH. Install it from https://docs.astral.sh/uv/" }

# Stop a running copy so its files can be replaced.
Get-CimInstance Win32_Process -Filter "Name='pythonw.exe' or Name='python.exe'" |
    Where-Object { $_.CommandLine -match '-m logimxplus run' } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }

& $uv tool install --python 3.12 --force --reinstall $Source
if ($LASTEXITCODE -ne 0) { throw "uv tool install failed ($LASTEXITCODE)" }

$bin = (& $uv tool dir --bin).Trim()
& (Join-Path $bin "logimxplus.exe") install
if ($LASTEXITCODE -ne 0) { throw "logimxplus install failed ($LASTEXITCODE)" }

# Start it now with the same command Windows will run at login.
$cmd = (Get-ItemProperty "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run").LogiMxPlus
$exe = ($cmd -split '"')[1]
if (-not $exe) { $exe = ($cmd -split ' ')[0] }
Start-Process -FilePath $exe -ArgumentList "-m", "logimxplus", "run" -WindowStyle Hidden
Write-Output "LogiMxPlus installed and started: $cmd"
