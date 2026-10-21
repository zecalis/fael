#!/bin/sh
# The pre-push gate (`git ship` runs it): the five validations from AGENTS.md "Git".
# One ✓/✗ line per step, each judged by its own exit code; only a failing step
# prints a short digest: the first 20 compiler diagnostics (one line each), the
# offending files for fmt, the last 60 lines otherwise. Exits 1 if any step failed.
# Bare `cargo clippy` exits 0 on warnings, so a local pass without
# `-- -D warnings` proves nothing — CI lint fails on them. Passed after `--`, not as
# RUSTFLAGS: a RUSTFLAGS change rebuilds every dep the next `cargo check` reuses.
# Runs at nice 15 so other work on the machine (e2e runs, other agents) goes first.
cd "$(dirname "$0")/.." || exit 1
renice -n 15 $$ >/dev/null 2>&1 || true
WIN=x86_64-pc-windows-msvc
bad=0
out=$(mktemp)
trap 'rm -f "$out"' EXIT

run() { # run <diag|files|tail> <label> <cmd...>
  show=$1; label=$2; shift 2
  if "$@" >"$out" 2>&1; then
    echo "✓ $label"
  else
    echo "✗ $label"; bad=1
    case $show in
      diag) grep -E '^[^ ]+:[0-9]+:[0-9]+: (error|warning)' "$out" | head -n 20 | grep . || tail -n 20 "$out" ;;
      files) cat "$out"; echo "fix: cargo fmt --all" ;;
      *) tail -n 60 "$out" ;;
    esac
  fi
}
step() { run tail "$@"; }

clippy() { cargo clippy --workspace --all-targets --locked --message-format=short "$@" -- -D warnings; }
tests() {
  # ponytail: CI uses nextest; plain cargo test when it is not installed
  if cargo nextest --version >/dev/null 2>&1; then
    cargo nextest run --workspace --locked --no-fail-fast --profile gate --hide-progress-bar --cargo-quiet
  else
    cargo test --workspace --locked
  fi
}

run files "cargo fmt" cargo fmt --all -- --check -l
run diag "clippy" clippy
if rustup target list --installed 2>/dev/null | grep -qx "$WIN"; then
  run diag "clippy ($WIN)" clippy --target "$WIN" --no-default-features
else
  echo "✗ clippy ($WIN): target missing — run: rustup target add $WIN"; bad=1
fi
step "file-size" scripts/file-size.sh
step "tests" tests
exit $bad
