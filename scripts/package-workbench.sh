#!/bin/sh
# Build a local macOS workbench folder with a double-clickable Terminal launcher.
set -eu

if [ "$(uname -s)" != Darwin ]; then
  echo 'This launcher package is for macOS.' >&2
  exit 1
fi

repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
package_dir=${1:-"$repo_dir/dist/NB Storyboard"}
cd "$repo_dir"
cargo build --release -p storyboard-cli
mkdir -p "$package_dir"
staged_binary="$package_dir/.storyboard-staged-$$"
trap 'test ! -e "$staged_binary" || rm "$staged_binary"' EXIT HUP INT TERM
cp target/release/storyboard "$staged_binary"
chmod +x "$staged_binary"
mv -f "$staged_binary" "$package_dir/storyboard"
trap - EXIT HUP INT TERM
cat > "$package_dir/Start NB Storyboard.command" <<'LAUNCHER'
#!/bin/sh
set -eu
package_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
workspace=${NB_STORYBOARD_WORKSPACE:-"$HOME/Documents/NB Storyboard Projects"}
# Finder/Terminal launchers may not load the user's interactive shell PATH.
# Keep its existing order, then add the usual macOS locations for FFmpeg and Claude.
PATH=${PATH:-/usr/bin:/bin}
for tool_dir in /opt/homebrew/bin /usr/local/bin "$HOME/.local/bin"; do
  if [ -d "$tool_dir" ]; then
    PATH="$PATH:$tool_dir"
  fi
done
export PATH
mkdir -p "$workspace"
for tool in ffmpeg ffprobe; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "$tool is required for movie and audio import. Install it, then reopen NB Storyboard." >&2
    exit 1
  fi
done
echo "NB Storyboard workspace: $workspace"
echo 'Close this Terminal window or press Ctrl+C to stop the workbench.'
exec "$package_dir/storyboard" serve "$workspace" --port 0 "$@"
LAUNCHER
chmod +x "$package_dir/Start NB Storyboard.command"
(cd "$package_dir" && shasum -a 256 storyboard > SHA256.txt)
echo "Packaged NB Storyboard at $package_dir"
