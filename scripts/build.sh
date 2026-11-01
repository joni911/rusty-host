#!/usr/bin/env bash
# Build helper — makes the Windows GNU toolchain actually usable.
#
# Rust's bundled `rust-mingw` component is incomplete: it ships crt2.o and the
# import libraries but *not* gcc's crtbegin.o/crtend.o, so any crate with a
# build script fails with:
#   ld: cannot find crtbegin.o
# A full mingw-w64 installation supplies them. This script locates one and puts
# it on PATH ahead of Rust's shim, and also exposes Rust's rust-mingw directory
# for dlltool (whose child ld.exe must be findable).
#
# rusty-host avoids C dependencies, but build scripts are unavoidable, so this
# keeps local and CI builds identical.
set -euo pipefail

export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"

if [[ "$(uname -s)" == *MINGW* || "$(uname -s)" == *MSYS* ]]; then
  export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable-x86_64-pc-windows-gnu}"

  # 1. Prefer a real mingw-w64 installation (scoop, msys2, chocolatey, winlibs).
  for candidate in \
      "${MINGW_ROOT:-}" \
      "$HOME/scoop/apps/mingw-winlibs/current" \
      /c/msys64/ucrt64 \
      /c/msys64/mingw64 \
      /c/tools/mingw64 \
      /mingw64 ; do
    [[ -n "$candidate" && -x "$candidate/bin/x86_64-w64-mingw32-gcc.exe" ]] || continue
    export PATH="$candidate/bin:$PATH"
    MINGW_FOUND="$candidate"
    break
  done

  # 2. Rust's own rust-mingw directory: supplies dlltool and import libraries.
  SYSROOT="${RUSTUP_HOME:-$HOME/.rustup}/toolchains/$RUSTUP_TOOLCHAIN/lib/rustlib/x86_64-pc-windows-gnu"
  [[ -d "$SYSROOT/lib/self-contained" ]] && export PATH="$SYSROOT/lib/self-contained:$SYSROOT/bin/self-contained:$PATH"

  # Point rustc at the real gcc, not Rust's shim.
  if [[ -n "${MINGW_FOUND:-}" ]]; then
    CC_WIN="$(cygpath -w "$MINGW_FOUND/bin/x86_64-w64-mingw32-gcc.exe" 2>/dev/null || echo "$MINGW_FOUND/bin/x86_64-w64-mingw32-gcc.exe")"
    export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="$CC_WIN"
    export CC_x86_64_pc_windows_gnu="$CC_WIN"
    # Rust's bundled dlltool.exe cannot spawn its own ld.exe (CreateProcess
    # fails), so any crate generating import libraries breaks. Prefer the real
    # mingw one, which works.
    if [[ -x "$MINGW_FOUND/bin/dlltool.exe" ]]; then
      DLLTOOL_WIN="$(cygpath -w "$MINGW_FOUND/bin/dlltool.exe" 2>/dev/null || echo "$MINGW_FOUND/bin/dlltool.exe")"
      export DLLTOOL="$DLLTOOL_WIN"
    fi
  fi

  if [[ -z "${MINGW_FOUND:-}" ]]; then
    echo "warning: no complete mingw-w64 found; build scripts may fail on crtbegin.o" >&2
    echo "         install one (e.g. scoop install mingw-winlibs) or set MINGW_ROOT" >&2
  fi
fi

exec cargo "$@"
