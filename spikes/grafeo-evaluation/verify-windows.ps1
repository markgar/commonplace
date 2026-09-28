$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Executable = Join-Path $Root "grafeo-evaluation.exe"
$Data = Join-Path $env:TEMP ("commonplace-grafeo-" + [guid]::NewGuid())

try {
    New-Item -ItemType Directory -Path $Data | Out-Null
    $env:COMMONPLACE_GRAFEO_DATA_DIR = $Data
    & $Executable
    if ($LASTEXITCODE -ne 0) {
        throw "Grafeo evaluation exited with code $LASTEXITCODE"
    }

    $Artifact = Get-Item $Executable
    $Hash = Get-FileHash -Algorithm SHA256 $Executable
    Write-Output ""
    Write-Output "Windows artifact:"
    Write-Output "  Path: $($Artifact.FullName)"
    Write-Output "  Bytes: $($Artifact.Length)"
    Write-Output "  SHA256: $($Hash.Hash)"

    $Dumpbin = Get-Command dumpbin.exe -ErrorAction SilentlyContinue
    if ($Dumpbin) {
        Write-Output ""
        Write-Output "Dynamic dependencies:"
        & $Dumpbin.Source /dependents $Executable
    } else {
        Write-Warning "dumpbin.exe was not found; dynamic dependencies were not inspected."
    }
}
finally {
    Remove-Item Env:\COMMONPLACE_GRAFEO_DATA_DIR -ErrorAction SilentlyContinue
    if (Test-Path $Data) {
        Remove-Item -Recurse -Force $Data
    }
}
