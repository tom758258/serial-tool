# Windows-only, hardware-free release-candidate acceptance.
# Requires a clean committed HEAD. Does not create a GitHub Release or tag.
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
$runId = (Get-Date -Format "yyyyMMdd-HHmmss") + "-" + [guid]::NewGuid().ToString("N").Substring(0, 8)
$reportDir = Join-Path $repoRoot ("reports\release-acceptance\" + $runId)
New-Item -ItemType Directory -Path $reportDir -Force | Out-Null
$script:Steps = @(
    "git-source", "required-tools", "versions", "required-files", "build-windows",
    "artifacts", "cli-smoke", "stage-release", "checksums", "source-unchanged"
) | ForEach-Object {
    [pscustomobject]@{ name = $_; result = "NOT RUN"; error = $null }
}
$script:Commit = $null
$script:Version = $null
$script:CandidateDir = $null
$script:Ok = $false
$script:FailedStep = $null
$script:Failure = $null
$script:CliExe = Join-Path $repoRoot "dist\windows-x64\serial-tool.exe"

function Invoke-Step {
    param([string]$Name, [scriptblock]$Action)
    $step = $script:Steps | Where-Object { $_.name -eq $Name } | Select-Object -First 1
    Write-Host "[start] $Name"
    try {
        & $Action
        $step.result = "PASS"
        Write-Host "[PASS] $Name"
    } catch {
        $step.result = "FAIL"
        $step.error = $_.Exception.Message
        $script:FailedStep = $Name
        throw
    }
}

function Invoke-Checked {
    param([string]$Executable, [string[]]$Arguments)
    $output = @(& $Executable @Arguments 2>&1)
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0) {
        throw "$Executable $($Arguments -join ' ') failed (exit $exitCode): $($output -join ' ')"
    }
    return ($output -join [Environment]::NewLine).Trim()
}

function Get-CargoVersion {
    param([string]$Path)
    $text = Get-Content -LiteralPath $Path -Raw
    $match = [regex]::Match($text, '(?m)^version\s*=\s*"([^"]+)"')
    if (-not $match.Success) { throw "Missing package version: $Path" }
    return $match.Groups[1].Value
}

try {
    Invoke-Step "git-source" {
        $script:Commit = Invoke-Checked "git" @("-C", $repoRoot, "rev-parse", "HEAD")
        $status = Invoke-Checked "git" @("-C", $repoRoot, "status", "--porcelain", "--untracked-files=all")
        if (-not $script:Commit -or $status) {
            throw "Release acceptance requires a clean, committed Git HEAD."
        }
    }
    Invoke-Step "required-tools" {
        if ($env:OS -ne "Windows_NT") { throw "Windows release acceptance must run on Windows." }
        foreach ($command in @("git", "cargo", "node", "npm.cmd", "powershell.exe")) {
            $null = Get-Command $command -ErrorAction Stop
        }
    }
    Invoke-Step "versions" {
        $script:Version = Get-CargoVersion (Join-Path $repoRoot "crates\serial-tool-core\Cargo.toml")
        foreach ($path in @("crates\serial-tool-cli\Cargo.toml", "apps\desktop\src-tauri\Cargo.toml")) {
            $actual = Get-CargoVersion (Join-Path $repoRoot $path)
            if ($actual -cne $script:Version) { throw "Version mismatch in $path" }
        }
        $package = Get-Content -LiteralPath (Join-Path $repoRoot "apps\desktop\package.json") -Raw | ConvertFrom-Json
        $tauri = Get-Content -LiteralPath (Join-Path $repoRoot "apps\desktop\src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json
        if ($package.version -cne $script:Version -or $tauri.version -cne $script:Version) {
            throw "Desktop package/Tauri versions do not match Rust version $script:Version."
        }
    }
    Invoke-Step "required-files" {
        foreach ($relative in @(
            "LICENSE", "THIRD_PARTY_NOTICES.txt", "licenses\MPL-2.0.txt",
            "crates\serial-tool-cli\help\cli.html",
            "crates\serial-tool-cli\help\cli.zh-TW.html",
            "apps\desktop\public\help\desktop.html",
            "apps\desktop\public\help\desktop.zh-TW.html"
        )) {
            if (-not (Test-Path -LiteralPath (Join-Path $repoRoot $relative) -PathType Leaf)) {
                throw "Missing tracked release/Help file: $relative"
            }
        }
    }
    Invoke-Step "build-windows" {
        & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "build-all.ps1") 2>&1 | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "build-all.ps1 failed (exit $LASTEXITCODE)." }
    }
    Invoke-Step "artifacts" {
        foreach ($file in @($script:CliExe, (Join-Path $repoRoot "dist\windows-x64\Serial Tool.exe"))) {
            if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Missing EXE: $file" }
            if ((Get-Item -LiteralPath $file).Length -eq 0) { throw "Empty EXE: $file" }
        }
    }
    Invoke-Step "cli-smoke" {
        $versionText = Invoke-Checked $script:CliExe @("--version")
        if ($versionText -cne "serial-tool $script:Version") { throw "Unexpected CLI version: $versionText" }

        $helpText = Invoke-Checked $script:CliExe @("--help")
        if (-not $helpText.Contains("user-guide")) { throw "CLI help missing user-guide." }
        $null = Invoke-Checked $script:CliExe @("user-guide", "--help")

        $manifest = (Invoke-Checked $script:CliExe @("manifest", "--json")) | ConvertFrom-Json
        if ($manifest.schema_version -ne 2 -or $manifest.tool_id -cne "serial") {
            throw "Unexpected CLI manifest contract."
        }

        $dryRunText = Invoke-Checked $script:CliExe @(
            "send", "--port", "COM4", "--baud", "115200",
            "--hex", "AA 55", "--dry-run", "--json"
        )
        $dryRun = $dryRunText | ConvertFrom-Json
        if ($dryRun.ok -ne $true -or $dryRun.event -cne "dry_run" -or
            $dryRun.performs_serial_io -ne $false -or $dryRun.tx_hex -cne "aa55") {
            throw "Unexpected dry-run result or I/O safety declaration."
        }
    }
    Invoke-Step "stage-release" {
        $shortCommit = $script:Commit.Substring(0, 8)
        $name = "serial-tool-$script:Version-windows-x64-$shortCommit-$runId"
        $script:CandidateDir = Join-Path $repoRoot ("release\" + $name)
        if (Test-Path -LiteralPath $script:CandidateDir) { throw "Release directory already exists." }
        New-Item -ItemType Directory -Path $script:CandidateDir -Force | Out-Null
        foreach ($name in @("serial-tool.exe", "Serial Tool.exe")) {
            Copy-Item -LiteralPath (Join-Path $repoRoot ("dist\windows-x64\" + $name)) -Destination (Join-Path $script:CandidateDir $name) -ErrorAction Stop
        }
        foreach ($name in @("LICENSE", "THIRD_PARTY_NOTICES.txt")) {
            Copy-Item -LiteralPath (Join-Path $repoRoot $name) -Destination (Join-Path $script:CandidateDir $name) -ErrorAction Stop
        }
        $licenseDir = Join-Path $script:CandidateDir "licenses"
        New-Item -ItemType Directory -Path $licenseDir -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $repoRoot "licenses\MPL-2.0.txt") -Destination (Join-Path $licenseDir "MPL-2.0.txt") -ErrorAction Stop
    }
    Invoke-Step "checksums" {
        $files = @("serial-tool.exe", "Serial Tool.exe", "LICENSE",
            "THIRD_PARTY_NOTICES.txt", "licenses\MPL-2.0.txt")
        $lines = @()
        foreach ($name in $files) {
            $path = Join-Path $script:CandidateDir $name
            $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
            $lines += "$hash  $($name.Replace('\', '/'))"
        }
        $checksumPath = Join-Path $script:CandidateDir "checksums.txt"
        [IO.File]::WriteAllLines($checksumPath, [string[]]$lines, ([System.Text.UTF8Encoding]::new($false)))
        $savedLines = @(Get-Content -LiteralPath $checksumPath)
        if ($savedLines.Count -ne $files.Count) { throw "Checksum file has unexpected entries." }
        foreach ($name in $files) {
            $path = Join-Path $script:CandidateDir $name
            $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
            $entry = "$hash  $($name.Replace('\', '/'))"
            if ($savedLines -cnotcontains $entry) { throw "Checksum mismatch: $name" }
        }
    }
    Invoke-Step "source-unchanged" {
        $finalCommit = Invoke-Checked "git" @("-C", $repoRoot, "rev-parse", "HEAD")
        $status = Invoke-Checked "git" @("-C", $repoRoot, "status", "--porcelain", "--untracked-files=all")
        if ($finalCommit -cne $script:Commit -or $status) {
            throw "Source HEAD or working tree changed during acceptance."
        }
    }
    $script:Ok = $true
} catch {
    $script:Failure = $_.Exception.Message
    Write-Warning $script:Failure
} finally {
    if (-not $script:Ok -and $script:CandidateDir -and (Test-Path -LiteralPath $script:CandidateDir)) {
        Remove-Item -LiteralPath $script:CandidateDir -Force -Recurse -ErrorAction SilentlyContinue
    }
    if (-not $script:Ok) { $script:CandidateDir = $null }
    $report = [ordered]@{
        schema_version = 1
        kind = "serial-tool-release-acceptance"
        ok = $script:Ok
        source_commit = $script:Commit
        project_version = $script:Version
        platform = "windows-x64"
        hardware_touched = $false
        release_dir = $script:CandidateDir
        failed_step = $script:FailedStep
        failure = $script:Failure
        steps = $script:Steps
    }
    $encoding = [System.Text.UTF8Encoding]::new($false)
    [IO.File]::WriteAllText((Join-Path $reportDir "report.json"), ($report | ConvertTo-Json -Depth 6), $encoding)
    $summary = @("# Serial Tool Release Acceptance", "",
        "Result: $(if ($script:Ok) { 'PASS' } else { 'FAIL' })",
        "Commit: $script:Commit",
        "Version: $script:Version",
        "Platform: windows-x64",
        "Hardware touched: false", "",
        "| Step | Result | Error |", "| --- | --- | --- |")
    foreach ($step in $script:Steps) {
        $errorText = [string]$step.error
        $errorText = $errorText.Replace("|", "/").Replace([char]13, " ").Replace([char]10, " ")
        $summary += "| $($step.name) | $($step.result) | $errorText |"
    }
    [IO.File]::WriteAllLines((Join-Path $reportDir "summary.md"), [string[]]$summary, $encoding)
    Write-Host "Acceptance report: $reportDir"
}
if (-not $script:Ok) { exit 1 }
Write-Host "Release candidate: $script:CandidateDir"
