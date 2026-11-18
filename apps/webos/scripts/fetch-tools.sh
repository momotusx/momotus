#!/usr/bin/env bash
# Fetches the prebuilt tools the webOS core's build uses (plans/webos.md
# section 2) into .tools at the repository root, each checked against a pinned
# SHA-256, and links them into .tools/bin: wasm-bindgen 0.2.129 (the
# lockfile's), binaryen version_133 (wasm-opt) and wasm-tools 1.263.0, plus
# fnm 1.39.0, zig 0.15.2 and cargo-zigbuild 0.23.4 on Linux (the native core's
# build, build-native.sh; on macOS all three come from Homebrew or cargo).
# Linux x86_64 and macOS arm64. Nothing is compiled. A tool already in place
# is kept.
set -euo pipefail
cd "$(dirname "$0")/../../.."
tools="$PWD/.tools"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)
    bindgen="wasm-bindgen-0.2.129-x86_64-unknown-linux-musl"
    bindgen_sha=82d12bb940e2d4e72e0d5605387fc1b8ca179044e012b620f0ce4e7440e8320e
    binaryen="binaryen-version_133-x86_64-linux"
    binaryen_sha=2dc9c7813f5375db93d96ead4b78222fcc3e2677bbb832297af4797782a37489
    wasmtools="wasm-tools-1.263.0-x86_64-linux"
    wasmtools_sha=21c036c1740a0552124c79c6e2170cb61c5b016005671a6d5bc06cf642165431
    fnm_zip="fnm-linux"
    fnm_sha=7807664f39d39fc518da1c35ba0181e4b3267603c4b1dedeb4b5fc6ae440a224
    zig="zig-x86_64-linux-0.15.2"
    zig_sha=02aa270f183da276e5b5920b1dac44a63f1a49e55050ebde3aecc9eb82f93239
    zigbuild="cargo-zigbuild-x86_64-unknown-linux-musl"
    zigbuild_sha=9e3cf73485edbd45905c8aadbc0fdf869c7ddc3848f0c898229f2680db52e44b
    ;;
  Darwin-arm64)
    bindgen="wasm-bindgen-0.2.129-aarch64-apple-darwin"
    bindgen_sha=81d4a23d56b3c3eb8187658329116d50e0b228a93b343825fb71f70179051cd1
    binaryen="binaryen-version_133-arm64-macos"
    binaryen_sha=ad66da82ac13f163e424b1643f16c6dfcccc98b5966296b43e52d3cab04f84a8
    wasmtools="wasm-tools-1.263.0-aarch64-macos"
    wasmtools_sha=b568f04746a699c4068b8d751779b60b501cf2cab6d259a3bc973eb5eeb1f62b
    fnm_zip=
    zig=
    ;;
  *)
    echo "fetch-tools.sh: no prebuilt tools listed for $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}

# fetch <url> <sha256> <file>: downloaded into .tools/dl and checked.
fetch() {
  local file="$tools/dl/$3"
  mkdir -p "$tools/dl"
  [ -f "$file" ] || curl -fsSL --retry 3 -o "$file.part" "$1"
  [ -f "$file" ] || mv "$file.part" "$file"
  local got
  got=$(sha256 "$file")
  if [ "$got" != "$2" ]; then
    rm -f "$file"
    echo "fetch-tools.sh: $3 has sha256 $got, want $2" >&2
    exit 1
  fi
}

# unpack <archive name> <dir it holds>
unpack() {
  [ -d "$tools/$2" ] || tar -xzf "$tools/dl/$1" -C "$tools"
}

gh=https://github.com
fetch "$gh/wasm-bindgen/wasm-bindgen/releases/download/0.2.129/$bindgen.tar.gz" "$bindgen_sha" "$bindgen.tar.gz"
unpack "$bindgen.tar.gz" "$bindgen"
fetch "$gh/WebAssembly/binaryen/releases/download/version_133/$binaryen.tar.gz" "$binaryen_sha" "$binaryen.tar.gz"
unpack "$binaryen.tar.gz" binaryen-version_133
fetch "$gh/bytecodealliance/wasm-tools/releases/download/v1.263.0/$wasmtools.tar.gz" "$wasmtools_sha" "$wasmtools.tar.gz"
unpack "$wasmtools.tar.gz" "$wasmtools"

mkdir -p "$tools/bin"
ln -sf "../$bindgen/wasm-bindgen" "$tools/bin/wasm-bindgen"
ln -sf ../binaryen-version_133/bin/wasm-opt "$tools/bin/wasm-opt"
ln -sf "../$wasmtools/wasm-tools" "$tools/bin/wasm-tools"
if [ -n "$fnm_zip" ]; then
  fetch "$gh/Schniz/fnm/releases/download/v1.39.0/$fnm_zip.zip" "$fnm_sha" "$fnm_zip.zip"
  [ -x "$tools/bin/fnm" ] || unzip -oq "$tools/dl/$fnm_zip.zip" -d "$tools/bin"
  chmod +x "$tools/bin/fnm"
fi
if [ -n "$zig" ]; then
  fetch "https://ziglang.org/download/0.15.2/$zig.tar.xz" "$zig_sha" "$zig.tar.xz"
  [ -d "$tools/$zig" ] || tar -xJf "$tools/dl/$zig.tar.xz" -C "$tools"
  ln -sf "../$zig/zig" "$tools/bin/zig"
  fetch "$gh/rust-cross/cargo-zigbuild/releases/download/v0.23.4/$zigbuild.tar.xz" "$zigbuild_sha" "$zigbuild.tar.xz"
  [ -d "$tools/$zigbuild" ] || tar -xJf "$tools/dl/$zigbuild.tar.xz" -C "$tools"
  ln -sf "../$zigbuild/cargo-zigbuild" "$tools/bin/cargo-zigbuild"
  "$tools/bin/zig" version
  "$tools/bin/cargo-zigbuild" --help >/dev/null
fi
# macOS quarantines downloaded binaries.
if [ "$(uname -s)" = Darwin ]; then xattr -dr com.apple.quarantine "$tools" 2>/dev/null || true; fi
"$tools/bin/wasm-bindgen" --version
"$tools/bin/wasm-opt" --version
"$tools/bin/wasm-tools" --version
