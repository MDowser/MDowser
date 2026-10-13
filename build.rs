// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `build.rs`: generates the packaged raster icons when they are missing or
//! outdated (`build_support/gen_icons.rs`), Brotli-compresses the two
//! largest embedded fonts (`build_support/gen_fonts.rs`) and embeds the
//! Windows executable resources (`assets/icons/icon.ico` as the shell icon,
//! a `VERSIONINFO` block and the application manifest) into the executables
//! of that platform.
//!
//! Why the icons are generated here: `src/assets.rs` embeds
//! `assets/icons/icon-256.png` with `include_bytes!`, so the files must
//! exist BEFORE the crate compiles. A fresh checkout (or a deleted
//! `assets/icons/`) therefore regenerates them from `assets/icon.svg` with
//! no manual step; `make icons` forces a regeneration. The output is
//! committed and packaging never generates it.
//!
//! Why the Windows resources are embedded here: the taskbar, Alt-Tab and the
//! shell read the icon from the PE resource of the `.exe`, not from the
//! window at runtime (`main` also calls `with_icon`, which only covers the
//! window/taskbar while running). The `VERSIONINFO` block is what Explorer's
//! Details tab, the task manager's Apps list and installer metadata read
//! (file/product description, version, copyright); without it those fields
//! are blank. The manifest declares per-monitor DPI awareness, the supported
//! Windows versions, the UTF-8 active code page, long-path awareness and the
//! segment heap, so the window is not bitmap-scaled on a HiDPI display.
//!
//! - Windows targets only for the resources: every other platform gets a
//!   no-op (the icon ships in the `.app`/`.deb`/window through other
//!   channels).
//! - GNU targets: `windres -O coff`, linked through `rustc-link-arg-bin`.
//!   Works with mingw-w64 (the dist script installs it) and with the
//!   windows-gnu toolchains that ship MinGW binutils.
//! - MSVC targets: `rc.exe` from the Windows SDK (CI's Windows runner and
//!   any Developer Command Prompt have it; `RC`/`RC_PATH` override).
//! - Never fails the build: without a resource compiler the executable is
//!   built without an embedded icon and a `cargo:warning` explains why.

#[path = "build_support/gen_fonts.rs"]
mod gen_fonts;
#[path = "build_support/gen_icons.rs"]
mod gen_icons;

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR set by cargo"));

    // Icons FIRST: `src/assets.rs` embeds them at compile time and the
    // Windows resource below reads `icon.ico`. Generation is skipped when
    // the committed files exist and `assets/icon.svg` did not change.
    println!("cargo:rerun-if-changed=assets/icon.svg");
    println!("cargo:rerun-if-changed=build_support/gen_icons.rs");
    gen_icons::ensure(manifest, &out);
    for name in gen_icons::OUTPUT_NAMES {
        println!("cargo:rerun-if-changed=assets/icons/{name}");
    }

    // Font compression: `src/assets.rs` embeds the two largest faces as
    // Brotli blobs generated here (skipped while the stamp matches).
    println!("cargo:rerun-if-changed=build_support/gen_fonts.rs");
    for (input, _) in gen_fonts::INPUTS {
        println!("cargo:rerun-if-changed={input}");
    }
    gen_fonts::ensure(manifest, &out);

    let ico = manifest.join("assets").join("icons").join("icon.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    println!("cargo:rerun-if-changed=build.rs");
    // The version written into the VERSIONINFO block comes from the crate
    // version: a version bump must relink the resource, not keep the old one.
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");
    // Resource-compiler selection is part of the rebuild fingerprint:
    // switching `RC`/`RC_PATH` or the SDK location with a warm target
    // directory would otherwise keep linking the stale icon object.
    for var in ["RC", "RC_PATH", "ProgramFiles(x86)", "ProgramFiles"] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    let target_var = env::var("TARGET").unwrap_or_default().replace('-', "_");
    println!("cargo:rerun-if-env-changed=RC_{target_var}");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if !ico.is_file() {
        warn("assets/icons/icon.ico is missing; the .exe ships without embedded resources");
        return;
    }
    let rc = out.join("mdowser-icon.rc");
    // Forward slashes: accepted by windres and rc.exe, and neither treats
    // them as escape sequences inside the quoted path.
    let path = ico.to_string_lossy().replace('\\', "/");
    // Application manifest (RT_MANIFEST): resource type 24, id 1. Written
    // next to the `.rc`, which references it by absolute path.
    let manifest_file = out.join("mdowser.manifest");
    if let Err(e) = fs::write(&manifest_file, MANIFEST_XML) {
        warn(&format!("cannot write {}: {e}", manifest_file.display()));
        return;
    }
    let manifest_path = manifest_file.to_string_lossy().replace('\\', "/");
    let version = env!("CARGO_PKG_VERSION");
    let (quad, display) = version_parts(version);
    // Every string value is ASCII on purpose: rc.exe reads the `.rc` in the
    // system ANSI code page unless a `#pragma code_page` is present, so a
    // non-ASCII byte would be mangled or rejected. The `VERSIONINFO` strings
    // are what Explorer's Details tab and installers display.
    let rc_text = format!(
        "1 ICON \"{path}\"\n\
         \n\
         // Application manifest (DPI awareness, supported Windows versions,\n\
         // UTF-8 code page, long paths, segment heap). RT_MANIFEST = 24, id 1.\n\
         1 24 \"{manifest_path}\"\n\
         \n\
         1 VERSIONINFO\n\
         FILEVERSION {quad}\n\
         PRODUCTVERSION {quad}\n\
         FILEFLAGSMASK 0x3fL\n\
         FILEFLAGS 0x0L\n\
         FILEOS 0x40004L\n\
         FILETYPE 0x1L\n\
         FILESUBTYPE 0x0L\n\
         BEGIN\n\
             BLOCK \"StringFileInfo\"\n\
             BEGIN\n\
                 BLOCK \"040904b0\"\n\
                 BEGIN\n\
                     VALUE \"CompanyName\", \"MDowser\"\n\
                     VALUE \"FileDescription\", \"MDowser\"\n\
                     VALUE \"FileVersion\", \"{display}\"\n\
                     VALUE \"InternalName\", \"mdowser\"\n\
                     VALUE \"LegalCopyright\", \"Copyright (c) 2026 MDowser - MPL-2.0\"\n\
                     VALUE \"OriginalFilename\", \"mdowser.exe\"\n\
                     VALUE \"ProductName\", \"MDowser\"\n\
                     VALUE \"ProductVersion\", \"{display}\"\n\
                 END\n\
             END\n\
             BLOCK \"VarFileInfo\"\n\
             BEGIN\n\
                 VALUE \"Translation\", 0x409, 1200\n\
             END\n\
         END\n"
    );
    if let Err(e) = fs::write(&rc, rc_text) {
        warn(&format!("cannot write {}: {e}", rc.display()));
        return;
    }
    let resource = if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        msvc(&rc, &out)
    } else {
        gnu(&rc, &out)
    };
    match resource {
        Ok(resource) => println!("cargo:rustc-link-arg-bin=mdowser={}", resource.display()),
        Err(reason) => warn(&format!("Windows resources skipped: {reason}")),
    }
}

/// `cargo:warning` helper (always visible, never fatal).
fn warn(message: &str) {
    println!("cargo:warning=mdowser build: {message}");
}

/// Windows application manifest embedded as `RT_MANIFEST` (id 1).
///
/// - `requestedExecutionLevel asInvoker`: never prompt for elevation.
/// - `dpiAware`/`dpiAwareness PerMonitorV2`: the window is rendered at the
///   real device resolution instead of being bitmap-scaled by the DWM.
/// - `supportedOS` GUIDs: Windows 7 through 10/11, which is what makes the
///   OS report the correct version to the app (and what enables the modern
///   common-controls behaviour).
/// - `longPathAware` + `activeCodePage UTF-8`: the local-file reader accepts
///   paths beyond `MAX_PATH` and ANSI APIs see UTF-8.
/// - `heapType SegmentHeap`: the more efficient NT heap used by modern apps.
///
/// The `assemblyIdentity` version is the MANIFEST's own identity, unrelated to
/// the product version above (Windows only uses it for side-by-side lookup).
const MANIFEST_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="dev.mdowser.app" version="1.0.0.0" processorArchitecture="*"/>
  <description>MDowser</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <!-- Windows 10 and 11 -->
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
      <!-- Windows 8.1 -->
      <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/>
      <!-- Windows 8 -->
      <supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}"/>
      <!-- Windows 7 -->
      <supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
      <heapType xmlns="http://schemas.microsoft.com/SMI/2020/WindowsSettings">SegmentHeap</heapType>
    </windowsSettings>
  </application>
</assembly>
"#;

/// `FILEVERSION` quad (`a,b,c,d`, four `u16`) and the display string for a
/// Cargo version like `0.4.4`.
///
/// A pre-release/build suffix (`1.2.3-rc.1`, `1.2.3+build`) is kept in the
/// string Windows shows but dropped from the numeric quad, which must be four
/// plain integers; missing components become `0`.
fn version_parts(version: &str) -> (String, String) {
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let mut quad = [0u16; 4];
    for (slot, part) in quad.iter_mut().zip(core.split('.')) {
        *slot = part.parse().unwrap_or(0);
    }
    let quad = quad
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(",");
    (quad, version.to_string())
}

/// Resource-compiles `rc` with mingw's `windres` into a COFF object.
fn gnu(rc: &Path, out: &Path) -> Result<PathBuf, String> {
    let windres = find_windres()?;
    let obj = out.join("mdowser-icon.o");
    let output = Command::new(&windres)
        // `-O coff`: the object ld can link (a .res is link.exe-only).
        .args(["-O", "coff"])
        .arg(rc)
        .arg("-o")
        .arg(&obj)
        .output()
        .map_err(|e| format!("{}: {e}", windres.to_string_lossy()))?;
    if !output.status.success() {
        return Err(format!(
            "windres failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(obj)
}

/// Resource-compiles `rc` with the Windows SDK's `rc.exe` into a `.res`.
fn msvc(rc: &Path, out: &Path) -> Result<PathBuf, String> {
    let tool = find_rc()?;
    let res = out.join("mdowser-icon.res");
    // `.res` files are valid LINK inputs; winresource links them this way.
    let output = Command::new(&tool)
        .arg("/nologo")
        .arg(format!("/fo{}", res.display()))
        .arg(rc)
        .output()
        .map_err(|e| format!("{}: {e}", tool.to_string_lossy()))?;
    if !output.status.success() {
        return Err(format!(
            "rc.exe failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(res)
}

/// MinGW prefix for the target arch (`x86_64-w64-mingw32`, `i686-w64-mingw32`).
fn mingw_prefix() -> Result<&'static str, String> {
    match env::var("CARGO_CFG_TARGET_ARCH")
        .unwrap_or_default()
        .as_str()
    {
        "x86_64" => Ok("x86_64-w64-mingw32"),
        "x86" => Ok("i686-w64-mingw32"),
        "aarch64" => Ok("aarch64-w64-mingw32"),
        other => Err(format!("no MinGW prefix known for arch {other}")),
    }
}

/// `windres` from `RC`/`RC_<target>` overrides or from the MinGW prefix.
fn find_windres() -> Result<OsString, String> {
    let target = env::var("TARGET").unwrap_or_default().replace('-', "_");
    for var in [format!("RC_{target}"), "RC".to_string()] {
        if let Ok(path) = env::var(&var) {
            return Ok(path.into());
        }
    }
    let prefixed = format!("{}-windres", mingw_prefix()?);
    for candidate in [prefixed.as_str(), "windres"] {
        if command_exists(candidate) {
            return Ok(candidate.into());
        }
    }
    Err(format!(
        "{prefixed} not found (install mingw-w64 or set RC)"
    ))
}

/// `rc.exe` from `RC`/`RC_PATH`/`RC_<target>`, `where`, or the Windows SDK.
fn find_rc() -> Result<OsString, String> {
    let target = env::var("TARGET").unwrap_or_default().replace('-', "_");
    for var in [
        format!("RC_{target}"),
        "RC_PATH".to_string(),
        "RC".to_string(),
    ] {
        if let Ok(path) = env::var(&var) {
            return Ok(path.into());
        }
    }
    if cfg!(windows) {
        if let Ok(output) = Command::new("where").arg("rc.exe").output() {
            if output.status.success() {
                if let Some(first) = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                {
                    return Ok(first.into());
                }
            }
        }
    }
    let sub = match env::var("CARGO_CFG_TARGET_ARCH")
        .unwrap_or_default()
        .as_str()
    {
        "x86_64" => "x64",
        "x86" => "x86",
        "aarch64" => "arm64",
        other => return Err(format!("no Windows SDK bin directory for arch {other}")),
    };
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Ok(root) = env::var(var) {
            let bin = Path::new(&root).join("Windows Kits").join("10").join("bin");
            if let Some(rc) = newest_sdk_rc(&bin, sub) {
                return Ok(rc.into());
            }
        }
    }
    Err("rc.exe not found (install the Windows SDK or set RC)".to_string())
}

/// Newest `rc.exe` under a `Windows Kits/10/bin` root: version directories
/// sort numerically (lexicographic sorting would rank 10.0.9 above 10.0.10).
fn newest_sdk_rc(bin: &Path, sub: &str) -> Option<PathBuf> {
    let mut best: Option<(Vec<u32>, PathBuf)> = None;
    for entry in fs::read_dir(bin).ok()?.flatten() {
        let version: Vec<u32> = entry
            .file_name()
            .to_string_lossy()
            .split('.')
            .filter_map(|part| part.parse().ok())
            .collect();
        let rc = entry.path().join(sub).join("rc.exe");
        if !rc.is_file() {
            continue;
        }
        let newer = match &best {
            Some((best_version, _)) => version > *best_version,
            None => true,
        };
        if newer {
            best = Some((version, rc));
        }
    }
    best.map(|(_, rc)| rc)
}

/// True for a regular file that could actually be executed on this host: on
/// Unix a non-executable file named `windres` is not a tool, and accepting
/// it silently downgraded the build to "no icon" with a warning.
fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path
            .metadata()
            .map(|meta| meta.permissions().mode() & 0o111 == 0)
            .unwrap_or(true)
        {
            return false;
        }
    }
    true
}

/// PATH lookup without spawning a process (Windows also honors `.exe`).
fn command_exists(command: &str) -> bool {
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&paths).any(|dir| {
        is_executable_file(&dir.join(command))
            || (cfg!(windows) && is_executable_file(&dir.join(format!("{command}.exe"))))
    })
}
