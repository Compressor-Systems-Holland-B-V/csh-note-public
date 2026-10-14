#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

WORKFLOW=.github/workflows/ci.yml
RUNNER_IMAGE=catthehacker/ubuntu:act-24.04
MANIFEST=src-tauri/Cargo.toml

usage() {
  cat <<'USAGE'
Runs the ci workflow (.github/workflows/ci.yml) on this machine.

Usage:
  scripts/test_ci.sh            run both jobs through act, in the ubuntu-24.04 runner image
  scripts/test_ci.sh --no-act   run the same steps directly here
  scripts/test_ci.sh --fix      apply rustfmt before checking, instead of only failing on it

The flags combine. act needs docker (https://nektosact.com/installation/).
--no-act skips the apt packages and runs pnpm install --frozen-lockfile, so
node_modules is rebuilt from the lockfile.
USAGE
}

use_act=true
fix=false
for arg in "$@"; do
  case "$arg" in
    --no-act) use_act=false ;;
    --fix) fix=true ;;
    -h | --help) usage; exit 0 ;;
    *) usage >&2; exit 1 ;;
  esac
done

if $fix; then
  echo "==> Formatting the rust backend"
  cargo fmt --manifest-path "$MANIFEST"
fi

if $use_act; then
  command -v act >/dev/null || {
    echo "act is not installed, see https://nektosact.com/installation/ or pass --no-act" >&2
    exit 1
  }
  exec act push -W "$WORKFLOW" -P ubuntu-24.04="$RUNNER_IMAGE"
fi

frontend() {
  echo "==> Installing the node packages"
  pnpm install --frozen-lockfile
  echo "==> Type checking the frontend"
  pnpm check
  echo "==> Running the frontend tests"
  pnpm test
  echo "==> Building the frontend"
  pnpm build
}

backend() {
  [ -d src-tauri/dist ] || pnpm build
  echo "==> Checking rust formatting"
  cargo fmt --manifest-path "$MANIFEST" --check
  echo "==> Linting the rust backend"
  cargo clippy --manifest-path "$MANIFEST" -- -D warnings
  echo "==> Running the rust tests"
  cargo test --manifest-path "$MANIFEST"
}

set +e
(set -e; frontend); front=$?
(set -e; backend); back=$?
set -e

echo
[ "$front" -eq 0 ] && echo "==> frontend checks passed" || echo "==> frontend checks FAILED"
[ "$back" -eq 0 ] && echo "==> rust checks passed" || echo "==> rust checks FAILED"
[ "$front" -eq 0 ] && [ "$back" -eq 0 ]
