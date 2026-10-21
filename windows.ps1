# Copyright (c) 2026 MDowser
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# windows.ps1: native Windows bootstrap, winget-only, no make required.
# Equivalent to `make` (deps + host release + current-OS dist into dist/).
# The Makefile needs GNU make + /bin/sh, so on PowerShell this script
# calls the same entry points directly: `cargo build` for the binary and
# `scripts/build-dist.sh --host-only` (via Git Bash) for the host .exe zip
# (x86_64, or aarch64 on Windows on ARM).
<#
.SYNOPSIS
    Installs the Windows build toolchain and compiles MDowser.
.DESCRIPTION
    Winget-only, idempotent: Rustup 1.98, MSVC Build Tools, NASM (x86/x86_64
    hosts) and Git (Git Bash: the sh/tar/sha256sum runtime packaging needs),
    Python (with a python3 shim, which build-dist.sh requires) and LLVM
    (llvm-strip for the .exe). Self-elevates through UAC unless
    -NoElevate is passed. Then `cargo build --locked --release` plus
    the host zip in dist/.
.PARAMETER Test
    Also run scripts/build-test.sh (same as `make test`).
.PARAMETER Validate
    Also run scripts/validate.sh (same as `make validate`).
.PARAMETER SkipBuild
    Skip the release build and dist packaging (setup/tests only).
.PARAMETER SkipDist
    Build the release binary but skip dist packaging.
.PARAMETER SkipSetup
    Install nothing, use what is present (like SKIP_SETUP=1).
.PARAMETER NoElevate
    Do not self-elevate via UAC; continue without admin (installs
    may fail, same as the old warning-only behavior).
.EXAMPLE
    powershell -ExecutionPolicy Bypass -File windows.ps1
.EXAMPLE
    powershell -ExecutionPolicy Bypass -File windows.ps1 -Test
#>
[CmdletBinding()]
param(
    [switch]$Test,
    [switch]$Validate,
    [switch]$SkipBuild,
    [switch]$SkipDist,
    [switch]$SkipSetup,
    [switch]$NoElevate
)

$ErrorActionPreference = "Stop"
# At the repo root, the script's own directory IS the checkout root.
$Root = $PSScriptRoot
if (-not (Test-Path (Join-Path $Root "Cargo.toml")) -or
    -not (Test-Path (Join-Path $Root "scripts/build-dist.sh"))) {
    throw "Run from the MDowser checkout (no Cargo.toml/scripts under $Root)"
}
Set-Location $Root

function Has-Cmd($Name) {
    $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

function Has-Winget($Id) {
    # -SkipSetup must work on a machine without winget: never invoke a
    # missing command (PowerShell would throw) while merely probing.
    if (-not (Has-Cmd winget)) { return $false }
    if (-not $script:WingetCache) { $script:WingetCache = @{} }
    if ($script:WingetCache.ContainsKey($Id)) {
        return $script:WingetCache[$Id]
    }
    $out = winget list -e --id $Id 2>$null | Out-String
    $hit = $out -match [regex]::Escape($Id)
    $script:WingetCache[$Id] = [bool]$hit
    return $script:WingetCache[$Id]
}

function Refresh-Path {
    $m = [Environment]::GetEnvironmentVariable("Path", "Machine")
    $u = [Environment]::GetEnvironmentVariable("Path", "User")
    $extra = @(
        "$env:USERPROFILE\.cargo\bin",
        "C:\Program Files\NASM",
        "C:\Program Files\LLVM\bin",
        "C:\Program Files\Git\bin",
        "C:\Program Files\Git\cmd"
    )
    $seen = @{}
    $parts = @()
    foreach ($p in (($m + ";" + $u + ";" + ($extra -join ";")) -split ";")) {
        $p = $p.Trim().TrimEnd("\")
        if ($p -eq "" -or $seen.ContainsKey($p.ToLower())) { continue }
        $seen[$p.ToLower()] = $true
        $parts += $p
    }
    $env:Path = $parts -join ";"
    if (Has-Cmd cargo) {
        $cd = Split-Path (Get-Command cargo).Source
        $env:Path = "$cd;$env:Path"
    }
}

function Ensure-Package($Id, $Cmd, [string]$WingetArgs = "",
    [string]$Probe = "") {
    $present = ($Cmd -and (Has-Cmd $Cmd))
    if ($present -and (-not [string]::IsNullOrEmpty($Probe))) {
        & $Cmd $Probe >$null 2>&1
        $present = ($LASTEXITCODE -eq 0)
    }
    if ($present) {
        Write-Host "OK   $Id ($Cmd present)"
        return
    }
    if ((-not [string]::IsNullOrEmpty($Probe)) -and (Has-Winget $Id)) {
        Write-Warning ("$Id is installed but '$Cmd $Probe' fails: " + `
            "another $Cmd shadows it on PATH (often the Store stub; " + `
            "disable App execution aliases or fix PATH order). Retrying.")
    }
    elseif (Has-Winget $Id) {
        Write-Host "OK   $Id (winget reports installed)"
        return
    }
    if ($SkipSetup) { throw "MISSING $Id (SkipSetup: install it)" }
    Write-Host "==>  winget install $Id"
    $wargs = @("install", "-e", "--id", $Id,
        "--accept-source-agreements", "--accept-package-agreements")
    if (-not [string]::IsNullOrEmpty($WingetArgs)) {
        $wargs += $WingetArgs.Split(" ",
            [StringSplitOptions]::RemoveEmptyEntries)
    }
    winget @wargs
    if ($LASTEXITCODE -ne 0) {
        throw "winget install $Id failed (exit $LASTEXITCODE)"
    }
    $script:WingetCache.Remove($Id)
    Refresh-Path
}

function Get-VsToolsPath {
    # Install path of a VS/BuildTools instance WITH a native C++ toolset,
    # else "". An ARM64 host uses the ARM64 compiler (the x86.x64 component
    # is not guaranteed there), so accept either component.
    $vw = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path $vw)) { return "" }
    if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") {
        $found = & $vw -products * `
            -requiresAny `
            Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
            Microsoft.VisualStudio.Component.VC.Tools.ARM64 `
            -property installationPath 2>$null | Select-Object -First 1
    }
    else {
        $found = & $vw -products * `
            -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
            -property installationPath 2>$null | Select-Object -First 1
    }
    if ([string]::IsNullOrWhiteSpace($found)) { return "" }
    return $found.Trim()
}

function Invoke-VsModify($Instance, $Setup) {
    # Adds the C++ workload to an existing VS/BuildTools instance.
    # (winget cannot do this: with the package present it no-ops with
    # "no applicable update" instead of running the installer.)
    Write-Host "==>  adding C++ workload to $Instance (several GB)"
    Write-Host ("      downloads GBs and takes many minutes: do not " + `
        "close this window or press Ctrl+C, that cancels the install")
    # NOTE: no `--wait` here: unlike the vs_setup bootstrapper used by
    # the winget path, setup.exe rejects it as unknown (exit 87) and
    # does nothing. `--passive` (not `--quiet`): a silent install shows
    # no progress for minutes and looks hung; passive shows progress
    # without needing interaction.
    # VCTools (not the full-IDE desktop workload): the only C++ workload
    # in the Build Tools product graph; anything else aborts with
    # "Cannot find package".
    & $Setup modify --installPath "$Instance" --add `
        Microsoft.VisualStudio.Workload.VCTools --includeRecommended `
        --passive --norestart
    $mrc = $LASTEXITCODE
    # setup.exe may hand the work to a background process (setup itself,
    # vs_installershell, VSIXInstaller) and exit early: while the tools
    # are missing but any of them still runs, wait (up to 30 min)
    # instead of failing immediately.
    $deadline = (Get-Date).AddMinutes(30)
    $watched = @("setup", "vs_installershell", "VSIXInstaller")
    $sawInstaller = $false
    while ([string]::IsNullOrWhiteSpace((Get-VsToolsPath)) -and
        ((Get-Date) -lt $deadline) -and
        (Get-Process -Name $watched -ErrorAction SilentlyContinue)) {
        $sawInstaller = $true
        Write-Host "      installer still running; waiting 60 s..."
        Start-Sleep -Seconds 60
    }
    # Grace period: the tools can register just after the last
    # installer process exits (skip it when nothing ever ran).
    if ($sawInstaller -and
        [string]::IsNullOrWhiteSpace((Get-VsToolsPath))) {
        Write-Host "      installer exited; giving registration 60 s..."
        Start-Sleep -Seconds 60
    }
    # Trust vswhere, not the exit code: setup.exe can exit 0 having
    # changed nothing (usage output, early handoff, cancellation).
    # Without this gate, a no-op modify ends in `link.exe not found`
    # much later in the cargo build.
    if (-not [string]::IsNullOrWhiteSpace((Get-VsToolsPath))) {
        if ($mrc -eq 3010) {
            Write-Warning ("C++ workload added, reboot pending " + `
                "(exit 3010): reboot before building or the compiler " + `
                "may not be found.")
        }
        else {
            Write-Host "OK   C++ workload added"
        }
        return
    }
    throw ("C++ workload still missing after modify (exit $mrc); " + `
        "inspect %TEMP%\dd_setup_*.log or add 'Desktop development " + `
        "with C++' in the Visual Studio Installer UI, reboot, rerun.")
}

function Ensure-Msvc {
    $instDir = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer"
    $vswhere = "$instDir\vswhere.exe"
    $setup = "$instDir\setup.exe"
    $anyVs = ""
    $toolsPath = Get-VsToolsPath
    if (-not [string]::IsNullOrWhiteSpace($toolsPath)) {
        Write-Host "OK   MSVC ($toolsPath)"; return
    }
    if (Test-Path $vswhere) {
        $anyVs = & $vswhere -products * -property installationPath `
            2>$null | Select-Object -First 1
        if ($anyVs) { $anyVs = $anyVs.Trim() }
    }
    elseif (Has-Cmd cl) { Write-Host "OK   MSVC (cl present)"; return }
    if ($SkipSetup) { throw "MISSING MSVC C++ workload (SkipSetup)" }
    # Null-safe check: vswhere yields nothing when no instance exists,
    # and an empty --installPath makes setup.exe print usage instead
    # of working.
    if ((-not [string]::IsNullOrWhiteSpace($anyVs)) -and
        (Test-Path $setup)) {
        Invoke-VsModify $anyVs $setup
    }
    else {
        Write-Host "==>  winget install BuildTools + C++ workload (several GB)"
        $ov = "--wait --quiet --norestart --add Microsoft.VisualStudio.Workload.VCTools " + `
            "--includeRecommended"
        winget install -e --id Microsoft.VisualStudio.2022.BuildTools `
            --accept-source-agreements --accept-package-agreements `
            --override $ov
        # vs_setup returns 3010 on success-with-reboot-pending (the common
        # BuildTools outcome): not a failure, but MSVC is unusable until
        # the reboot happens. -1978335189 (0x8A15002B) means winget found
        # the package already installed and ran nothing: re-check for an
        # instance and modify it instead of failing.
        if ($LASTEXITCODE -eq 3010) {
            Write-Warning ("BuildTools installed, reboot pending " + `
                "(exit 3010): reboot before building or the compiler " + `
                "may not be found.")
        }
        elseif ($LASTEXITCODE -eq -1978335189) {
            $nowVs = ""
            if (Test-Path $vswhere) {
                $nowVs = & $vswhere -products * -property installationPath `
                    2>$null | Select-Object -First 1
                if ($nowVs) { $nowVs = $nowVs.Trim() }
            }
            if ((-not [string]::IsNullOrWhiteSpace($nowVs)) -and
                (Test-Path $setup)) {
                Invoke-VsModify $nowVs $setup
            }
            else {
                throw ("BuildTools present but unusable and no instance " + `
                    "found; add 'Desktop development with C++' in the " + `
                    "Visual Studio Installer UI.")
            }
        }
        elseif ($LASTEXITCODE -ne 0) {
            throw ("BuildTools install failed (exit $LASTEXITCODE); " + `
                "inspect %TEMP%\dd_setup_*.log, reboot and retry, or " + `
                "install the 'Desktop development with C++' workload manually.")
        }
        else {
            Write-Host "OK   BuildTools installed"
        }
    }
    Refresh-Path
}

function Ensure-Python3Shim {
    if (-not (Has-Cmd python)) { return }
    $py = (Get-Command python).Source
    & $py --version >$null 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw ("'$py' is not a working Python (often the Store stub); " + `
            "disable App execution aliases or fix PATH order")
    }
    $dir = Split-Path $py
    $shim = Join-Path $dir "python3.exe"
    if (-not (Test-Path $shim)) {
        if ($SkipSetup) { throw "MISSING python3 shim next to $py" }
        Write-Host "==>  python3 shim at $shim"
        try {
            Copy-Item $py $shim
        }
        catch {
            throw "Cannot write $shim (rerun elevated): $($_.Exception.Message)"
        }
    }
    # The real python dir first: a Store-stub python3.exe in WindowsApps
    # must not shadow the working shim in this session or in Git Bash.
    Refresh-Path
    $env:Path = "$dir;$env:Path"
}

function Get-GitBash {
    $cands = @(
        "C:\Program Files\Git\bin\bash.exe",
        "${env:ProgramFiles}\Git\bin\bash.exe",
        # Per-user installs (winget/Git-for-Windows "only for me") land here.
        "${env:LOCALAPPDATA}\Programs\Git\bin\bash.exe"
    )
    # Derive from the resolved `git` when it is on PATH: covers a custom or
    # portable prefix (D:\Git, a versioned dir) the fixed candidates miss.
    $gitCmd = Get-Command git -ErrorAction SilentlyContinue
    if ($gitCmd -and $gitCmd.Source) {
        $gitDir = Split-Path $gitCmd.Source -Parent
        $cands += (Join-Path (Split-Path $gitDir -Parent) "bin\bash.exe")
        # `cmd\git.exe` -> `<root>\bin\bash.exe` (and the same from `bin\`).
        $cands += (Join-Path $gitDir "bash.exe")
    }
    foreach ($c in $cands) { if ($c -and (Test-Path $c)) { return $c } }
    throw "Git Bash not found (winget install Git.Git first)"
}

if (-not $SkipSetup -and -not (Has-Cmd winget)) {
    throw "winget missing: install App Installer from Microsoft Store"
}
$isAdmin = ([Security.Principal.WindowsPrincipal] `
    [Security.Principal.WindowsIdentity]::GetCurrent() `
    ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin -and -not $NoElevate) {
    # Self-elevate: BuildTools, machine-wide winget packages and the
    # python3 shim under Program Files need admin. Relaunch this script
    # with the same switches through the UAC prompt, then stop here.
    # `-File` takes ONE argument: PowerShell 5.1 joins the array with
    # spaces without quoting, so a checkout under `C:\Users\me\My Projects\`
    # would be split and the elevated run would fail before doing anything.
    $selfArgs = @("-NoProfile", "-NonInteractive", "-ExecutionPolicy",
        "Bypass", "-File", "`"$PSCommandPath`"")
    foreach ($sw in @("Test", "Validate", "SkipBuild", "SkipDist",
        "SkipSetup")) {
        if ((Get-Variable $sw -ValueOnly)) { $selfArgs += "-$sw" }
    }
    $selfExe = [System.Diagnostics.Process]::GetCurrentProcess(
        ).MainModule.FileName
    Write-Host "==>  requesting elevation (UAC) and relaunching"
    try {
        $elevated = Start-Process -FilePath $selfExe `
            -ArgumentList $selfArgs -Verb RunAs -WorkingDirectory $Root `
            -Wait -PassThru
    }
    catch {
        throw "Elevation declined or failed; rerun elevated or use -NoElevate"
    }
    exit $elevated.ExitCode
}
if (-not $isAdmin) {
    Write-Warning "Not admin (-NoElevate): installs may fail."
}

Ensure-Package "Rustlang.Rustup" "rustup"
Ensure-Msvc
# NASM only on x86/x86_64 hosts (rav1d + aws-lc-sys assembly): ARM64 hosts
# assemble with the system toolchain and need none (see docs/BUILDING.md).
# `windows-11-arm` CI runners follow the same rule (no NASM there).
if ($env:PROCESSOR_ARCHITECTURE -ne "ARM64") {
    Ensure-Package "NASM.NASM" "nasm"
} else {
    Write-Host "SKIP  NASM.NASM (ARM64 host needs no NASM)"
}
Ensure-Package "Git.Git" "git"
# Probe, not just presence: Windows ships a Store-stub python.exe that
# exists on PATH but only opens the Store (nonzero exit).
Ensure-Package "Python.Python.3.12" "python" -Probe "--version"
# llvm-strip for the .exe: without it the host zip ships unstripped
# (a SKIP, not an error).
Ensure-Package "LLVM.LLVM" "llvm-strip"
Refresh-Path
if (-not (Has-Cmd llvm-strip)) {
    Write-Warning ("llvm-strip not on PATH: the host zip will ship " + `
        "unstripped (reinstall LLVM or fix PATH order)")
}
Ensure-Python3Shim

$probes = @{ cargo = "--version"; rustc = "--version" }
$probeTargets = @("cargo", "rustc")
# NASM is x86/x86_64-only (see above): do not fail ARM64 hosts for it.
if ($env:PROCESSOR_ARCHITECTURE -ne "ARM64") {
    $probes["nasm"] = "-v"
    $probeTargets += "nasm"
}
foreach ($t in $probeTargets) {
    if (-not (Has-Cmd $t)) { throw "$t not on PATH after setup" }
    & $t $probes[$t] >$null 2>&1
    if ($LASTEXITCODE -ne 0) { throw "$t on PATH but not runnable" }
}
$bash = Get-GitBash
Write-Host "OK   Git Bash ($bash)"
& $bash -lc "command -v sha256sum >/dev/null || command -v shasum >/dev/null"
if ($LASTEXITCODE -ne 0) {
    throw "No SHA-256 tool in Git Bash (need sha256sum/shasum)"
}
& $bash -lc "python3 --version >/dev/null 2>&1"
if ($LASTEXITCODE -ne 0) {
    throw ("python3 unusable in Git Bash (a Store stub may shadow it; " + `
        "disable App execution aliases or fix PATH order)")
}

Write-Host "==>  toolchain"
cargo --version
rustc --version
if ($env:PROCESSOR_ARCHITECTURE -ne "ARM64") { nasm -v }
if (-not $SkipSetup) {
    # The channel is the single source in `rust-toolchain.toml` (the pin
    # cargo itself honors, matching `Cargo.toml` `rust-version`);
    # `-SkipSetup` installs nothing, so it must not run there.
    $toolchainChannel = "1.98"
    $toolchainFile = Join-Path $Root "rust-toolchain.toml"
    $m = Select-String -Path $toolchainFile -Pattern 'channel\s*=\s*"([^"]+)"' |
        Select-Object -First 1
    if ($m -and $m.Matches.Groups[1].Value) {
        $toolchainChannel = $m.Matches.Groups[1].Value
    }
    rustup toolchain install $toolchainChannel
    if ($LASTEXITCODE -ne 0) { throw "rustup toolchain install failed" }
}

if (-not $SkipBuild) {
    # Privacy: release binaries embed the absolute source path of every panic
    # location, including dependency sources under the Cargo registry
    # ($env:USERPROFILE\.cargo\...), so the builder's username and home layout
    # would ship in the .exe (`strip` cannot remove them). Remap the prefixes
    # at compile time (the PowerShell equivalent of scripts/lib.sh
    # `export_remap_rustflags`. CARGO_ENCODED_RUSTFLAGS (0x1f-separated) keeps
    # a profile path containing spaces intact.
    $remapSep = [string][char]0x1F
    $remapFlags = @()
    if ($env:CARGO_ENCODED_RUSTFLAGS) {
        $remapFlags += ($env:CARGO_ENCODED_RUSTFLAGS -split $remapSep)
    } elseif ($env:RUSTFLAGS) {
        $remapFlags += ($env:RUSTFLAGS -split '\s+')
    }
    if ($env:USERPROFILE) {
        foreach ($prefix in @($env:USERPROFILE, ($env:USERPROFILE -replace '\\', '/'))) {
            $remapFlags += "--remap-path-prefix=$prefix=/build"
            $remapFlags += "--remap-path-prefix=$prefix/.cargo=/cargo"
        }
    }
    if ($remapFlags.Count -gt 0) {
        $env:CARGO_ENCODED_RUSTFLAGS = ($remapFlags -join $remapSep)
    }
    Write-Host "==>  cargo build --locked --release (first build " + `
        "pays ~90 s for the Brotli font pass, then cached)"
    cargo build --locked --release
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed (exit $LASTEXITCODE)"
    }
    $exe = Join-Path $Root "target\release\mdowser.exe"
    if (-not (Test-Path $exe)) { throw "Build produced no $exe" }
    Write-Host "OK   $exe"
}

$bashArgs = @()
if ($Test) { $bashArgs += "./scripts/build-test.sh" }
# build-dist.sh compiles the release binary itself, so -SkipBuild cannot
# honestly pair with a dist run: both flags suppress packaging.
if (-not $SkipDist -and -not $SkipBuild) {
    $bashArgs += "./scripts/build-dist.sh --host-only"
}
if ($Validate) { $bashArgs += "./scripts/validate.sh" }
if ($bashArgs.Count -eq 0) { Write-Host "OK: setup only."; return }
if ($SkipSetup) { $env:SKIP_SETUP = "1" }

# Path via env (never interpolated): survives spaces and apostrophes.
$env:MDOWSER_ROOT = $Root
$unix = (& $bash -lc 'cygpath "$MDOWSER_ROOT"').Trim()
if ([string]::IsNullOrWhiteSpace($unix)) {
    throw "cygpath failed in Git Bash for '$Root'"
}
$unix = $unix -replace "'", "'\\''"
foreach ($step in $bashArgs) {
    Write-Host "==>  bash $step"
    & $bash -lc "cd '$unix' && $step"
    if ($LASTEXITCODE -ne 0) { throw "FAILED: $step (exit $LASTEXITCODE)" }
}

Write-Host ""
$dist = Join-Path $Root "dist"
if (Test-Path $dist) {
    Write-Host "dist/ ready:"
    Get-ChildItem $dist -File | Format-Table Name, Length
}
else {
    Write-Host "(no dist/; SkipDist was set or the dist step did not run)"
}
