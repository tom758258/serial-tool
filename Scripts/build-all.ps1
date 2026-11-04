Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$desktopDir = Join-Path $repoRoot "apps\desktop"
$cliExe = Join-Path $repoRoot "target\release\serial-tool.exe"
$desktopExe = Join-Path $desktopDir "src-tauri\target\release\Serial Tool.exe"
$outputDir = Join-Path $repoRoot "dist\windows-x64"

# Never leave a previous successful bundle behind after a failed build.
if (Test-Path -LiteralPath $outputDir) {
    Remove-Item -LiteralPath $outputDir -Recurse -Force
}

Push-Location $repoRoot
try {
    Write-Host "Building CLI / Worker..."
    & cargo build --release -p serial-tool-cli --locked
    if ($LASTEXITCODE -ne 0) { throw "CLI build failed (exit $LASTEXITCODE)." }

    Write-Host "Building Tauri Desktop..."
    Push-Location $desktopDir
    try {
        & npm.cmd ci
        if ($LASTEXITCODE -ne 0) { throw "npm ci failed (exit $LASTEXITCODE)." }

        & npm.cmd run tauri build -- --no-bundle
        if ($LASTEXITCODE -ne 0) { throw "Desktop build failed (exit $LASTEXITCODE)." }
    }
    finally {
        Pop-Location
    }

    foreach ($exe in @($cliExe, $desktopExe)) {
        if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
            throw "Expected EXE not found: $exe"
        }
        if ((Get-Item -LiteralPath $exe).Length -eq 0) {
            throw "Expected EXE is empty: $exe"
        }
    }

    Write-Host "Checking CLI startup and manifest..."
    & $cliExe --help | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "CLI --help failed (exit $LASTEXITCODE)." }

    $manifestJson = & $cliExe manifest --json
    if ($LASTEXITCODE -ne 0) { throw "CLI manifest failed (exit $LASTEXITCODE)." }
    $null = ConvertFrom-Json -InputObject ($manifestJson -join "`n") -ErrorAction Stop

    New-Item -ItemType Directory -Path $outputDir -Force | Out-Null
    Copy-Item -LiteralPath $cliExe -Destination (Join-Path $outputDir "serial-tool.exe")
    Copy-Item -LiteralPath $desktopExe -Destination (Join-Path $outputDir "Serial Tool.exe")

    foreach ($name in @("serial-tool.exe", "Serial Tool.exe")) {
        $outputExe = Join-Path $outputDir $name
        if (-not (Test-Path -LiteralPath $outputExe -PathType Leaf)) {
            throw "Output EXE not found: $outputExe"
        }
        if ((Get-Item -LiteralPath $outputExe).Length -eq 0) {
            throw "Output EXE is empty: $outputExe"
        }
    }

    Write-Host "Windows CLI and Desktop EXEs: $outputDir"
}
finally {
    Pop-Location
}
