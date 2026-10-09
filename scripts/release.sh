#!/bin/sh
# Cut a release: bump fael/Cargo.toml, commit + tag on main, push.
# The tag runs .github/workflows/release.yml → GitHub Release + npm @zecalis/fael + Homebrew tap.
# usage: scripts/release.sh [patch|minor|major] [--no-local]   (default patch)
# --no-local leaves this machine's npm/brew fael alone, so auto-update has a release to catch
set -eu
cd "$(git rev-parse --show-toplevel)"

part=patch
no_local=
for a in "$@"; do
  case $a in
  patch | minor | major) part=$a ;;
  --no-local) no_local=1 ;;
  *) echo "usage: $0 [patch|minor|major] [--no-local]" >&2; exit 2 ;;
  esac
done

[ -z "$(git status --porcelain)" ] || { echo "release: working tree is dirty — commit or stash first" >&2; exit 1; }
# dependabot PRs don't block — they carry no work of ours
prs=$(gh pr list --state open --base main --json number,author --jq '[.[] | select(.author.login != "app/dependabot") | "#\(.number)"] | join(" ")')
[ -z "$prs" ] || { echo "release: open PR(s) into main: $prs — merge or close them first" >&2; exit 1; }
git checkout -q main
git pull -q --ff-only

# the bump commit itself skips CI (direct push), so the commit under it must already be green
# — a run still queued or going (a merge made seconds ago) is waited on, not refused
sha=$(git rev-parse HEAD)
ci_run=
for _ in 1 2 3 4 5 6 7 8 9 10 11 12; do
  ci_run=$(gh run list --workflow ci.yml --commit "$sha" -L1 --json databaseId --jq '.[0].databaseId // empty')
  [ -n "$ci_run" ] && break
  sleep 5
done
if [ -n "$ci_run" ]; then
  echo "release: waiting on CI run $ci_run for $(git rev-parse --short HEAD)"
  gh run watch "$ci_run" --exit-status >/dev/null || true
fi
ci=$(gh run list --workflow ci.yml --commit "$sha" -L1 --json status,conclusion --jq '.[] | "\(.status) \(.conclusion)"')
[ "$ci" = "completed success" ] || { echo "release: CI on main $(git rev-parse --short HEAD) is '${ci:-not run}' — fix it first" >&2; exit 1; }

old=$(sed -n 's/^version = "\(.*\)"$/\1/p' fael/Cargo.toml | head -n1)
IFS=. read -r major minor patch <<V
$old
V
case $part in
patch) patch=$((patch + 1)) ;;
minor) minor=$((minor + 1)); patch=0 ;;
major) major=$((major + 1)); minor=0; patch=0 ;;
esac
new=$major.$minor.$patch

git rev-parse -q --verify "refs/tags/v$new" >/dev/null && { echo "release: tag v$new already exists" >&2; exit 1; }

# first `version = ` line only — the [package] one
perl -0pi -e "s/^version = \"\Q$old\E\"/version = \"$new\"/m" fael/Cargo.toml
cargo metadata --format-version 1 >/dev/null # rewrites Cargo.lock for the new version

git commit -q -am "release v$new"
git tag -a "v$new" -m "release v$new"
git push -q origin main --follow-tags

echo "v$old -> v$new pushed."

# owner's machine: pull every fael worktree onto the new main (alias fael-sync); skipped where `repos` is absent
# a worktree that can't fast-forward is not a release failure — report it and keep going
if command -v repos >/dev/null 2>&1; then repos fael sync || echo "release: fael sync had failures (above) — continuing" >&2; fi

# wait for the tag's release.yml (GitHub Release + npm + brew), then move this machine's npm install onto it
run=
for _ in 1 2 3 4 5 6 7 8 9 10 11 12; do
  run=$(gh run list --workflow release.yml --branch "v$new" -L1 --json databaseId --jq '.[0].databaseId // empty')
  [ -n "$run" ] && break
  sleep 5
done
[ -n "$run" ] || { echo "release: no release.yml run for v$new after 60s — check Actions" >&2; exit 1; }
gh run watch "$run" --exit-status >/dev/null || { echo "release: release.yml run $run failed — gh run rerun $run --failed" >&2; exit 1; }
echo "release.yml $run green"
if [ -n "$no_local" ]; then
  echo "local fael left as is (--no-local)"
  exit 0
fi
if npm ls -g @zecalis/fael >/dev/null 2>&1; then
  # registry lag has no fixed length (usually seconds) — poll up to 5 min instead of guessing a sleep
  for _ in $(seq 30); do
    [ "$(npm view "@zecalis/fael@$new" version 2>/dev/null)" = "$new" ] && break
    sleep 10
  done
  npm i -g "@zecalis/fael@$new" >/dev/null && echo "local fael -> $new (npm)" \
    || echo "release: npm has no $new after 5 min — run: npm i -g @zecalis/fael@$new" >&2
fi
# release.yml's publish-homebrew-formula job already pushed the tap, so one `brew update` sees it
if command -v brew >/dev/null 2>&1 && brew list --versions fael >/dev/null 2>&1; then
  brew update -q >/dev/null && brew upgrade -q zecalis/tap/fael >/dev/null \
    && echo "local fael -> $(brew list --versions fael | cut -d' ' -f2) (brew)" \
    || echo "release: brew upgrade failed — run: brew update && brew upgrade zecalis/tap/fael" >&2
fi
