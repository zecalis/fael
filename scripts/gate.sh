#!/bin/sh
# The pre-push gate (`git ship` runs it): the five validations from AGENTS.md "Git".
# One ✓/✗ line per step, each judged by its own exit code; only a failing step
# prints its output (last 40 lines). Exits 1 if any step failed.
# Bare `cargo clippy` exits 0 on warnings, so a local pass without
# RUSTFLAGS="-D warnings" proves nothing — CI lint fails on them.
cd "$(dirname "$0")/.." || exit 1
WIN=x86_64-pc-windows-msvc
bad=0
out=$(mktemp)
trap 'rm -f "$out"' EXIT

step() { # step <label> <cmd...>
  label=$1; shift
  if "$@" >"$out" 2>&1; then
    echo "✓ $label"
  else
    echo "✗ $label"; tail -n 40 "$out"; bad=1
  fi
}

clippy() { RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets --locked "$@"; }
tests() {
  # ponytail: CI uses nextest; plain cargo test when it is not installed
  if cargo nextest --version >/dev/null 2>&1; then
    cargo nextest run --workspace --locked --no-fail-fast
  else
    cargo test --workspace --locked
  fi
}

step "cargo fmt" cargo fmt --all --check
step "clippy" clippy
if rustup target list --installed 2>/dev/null | grep -qx "$WIN"; then
  step "clippy ($WIN)" clippy --target "$WIN" --no-default-features
else
  echo "✗ clippy ($WIN): target missing — run: rustup target add $WIN"; bad=1
fi
step "file-size" scripts/file-size.sh
step "tests" tests
exit $bad
