#!/usr/bin/env bash
# Build helper — pins the toolchain and fixes the GNU-toolchain PATH on Windows.
#
# Why this exists: the bundled `dlltool.exe` shipped with the GNU toolchain needs
# its own directory on PATH so it can spawn `ld.exe`. Without it, crates that
# generate import libraries (anything depending on `windows-sys`) fail with
#   dlltool could not create import library ... CreateProcess
#
# rusty-host's dependency graph is deliberately free of such crates today, but
# this keeps CI and contributors on an identical environment.
set -euo pipefail

# cargo/rustup live here when the shell is not a login shell
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"

if [[ "$(uname -s)" == *MINGW* || "$(uname -s)" == *MSYS* ]]; then
  TC="${RUSTUP_HOME:-$HOME/.rustup}/toolchains/stable-x86_64-pc-windows-gnu/lib/rustlib/x86_64-pc-windows-gnu/bin"
  if [[ -d "$TC/self-contained" ]]; then
    export PATH="$TC/self-contained:$TC:$PATH"
  fi
  export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable-x86_64-pc-windows-gnu}"
fi

exec cargo "$@"
