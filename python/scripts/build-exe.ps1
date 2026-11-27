<#
.SYNOPSIS
    Build dist\LogiMxPlus.exe: a single, windowless executable with Python bundled.
#>
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)
uv run --with pyinstaller==6.16.0 pyinstaller --noconfirm --clean --onefile --noconsole `
    --name LogiMxPlus --hidden-import hid --version-file packaging/version_info.txt `
    --paths src packaging/entry.py
if ($LASTEXITCODE -ne 0) { throw "pyinstaller failed ($LASTEXITCODE)" }
Get-Item dist\LogiMxPlus.exe | Select-Object FullName, Length
