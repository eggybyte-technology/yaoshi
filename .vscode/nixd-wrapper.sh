#!/usr/bin/env bash
set -euo pipefail
export PATH="/nix/var/nix/profiles/default/bin:$PATH"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec nix develop "$repo_root" --command nixd "$@"
