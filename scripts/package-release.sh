#!/usr/bin/env bash
# Package a natively built executable; never relabel a foreign architecture.
set -euo pipefail
cd "$(dirname "$0")/.."
target="${1:-$(rustc -vV | awk '/^host:/ {print $2}')}"
case "$target" in
  x86_64-unknown-linux-gnu) platform=linux-x86_64 ;;
  x86_64-apple-darwin) platform=macos-intel ;;
  aarch64-apple-darwin) platform=macos-apple-silicon ;;
  *) echo "Unsupported release target: $target" >&2; exit 1 ;;
esac
binary="target/$target/release/gtuim"
if [[ ! -f "$binary" ]]; then
  echo "Build first: cargo build --locked --release --target $target" >&2
  exit 1
fi
# CLI smoke test also rejects a binary that cannot execute on the build host.
"$binary" --version
version="$("$binary" --version | awk '{print $2}')"
name="gtuim-$version-$platform"
mkdir -p dist
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/$name"
cp "$binary" "$stage/$name/gtuim"
chmod 755 "$stage/$name/gtuim"
cp LICENSE "$stage/$name/LICENSE"
cp docs/distribution.md "$stage/$name/README.md"
cp "$binary" "dist/$name"
chmod 755 "dist/$name"
tar -czf "dist/$name.tar.gz" -C "$stage" "$name"
if command -v sha256sum >/dev/null; then
  (cd dist && sha256sum "$name" "$name.tar.gz" > "$name.sha256")
else
  (cd dist && shasum -a 256 "$name" "$name.tar.gz" > "$name.sha256")
fi
printf 'Created dist/%s and dist/%s.tar.gz\n' "$name" "$name"
