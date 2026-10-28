#!/usr/bin/env bash
# A Codex worker commits only through `horch ledger commit`, and the
# nested-repository attack on raw git rules fails (docs/live-checks/codex-git.md).
# Runs `codex sandbox` and `codex execpolicy check` only: no model session.
set -u

repo=$(cd "$(dirname "$0")/../.." && pwd)
horch=${HORCH_BIN:-$repo/target/debug/horch}
rules=$repo/crates/horch-core/tests/fixtures/harness/codex_rules_worker.rules
failed=0

pass() { printf 'PASS %s\n' "$1"; }
fail() { printf 'FAIL %s: %s\n' "$1" "$2"; failed=1; }
skip() { printf 'SKIP %s: %s\n' "$1" "$2"; }

command -v codex >/dev/null || { skip codex "codex is not on PATH"; exit 0; }
[ -x "$horch" ] || { skip horch "no horch at '$horch' (cargo build -p horch, or set HORCH_BIN)"; exit 0; }

# Under /private/tmp: the fleet_write profile keeps /tmp read-only, so the
# main repository is outside every writable root, as in a real fleet.
s=$(mktemp -d /private/tmp/codex-git.XXXXXX)
trap 'rm -rf "$s"' EXIT
# A worker pane sets HORCH_PROJECT_DIR (and more): without this, horch would
# make the run branch and the worktree in that project, not in $s/main.
for v in $(env | sed -n 's/^\(HORCH_[A-Z_]*\)=.*/\1/p'); do unset "$v"; done
export HORCH_LEDGER_DB=$s/ledger.sqlite3 HORCH_STATE_DIR=$s/state HORCH_NO_WAKE=1 HORCH_NO_WATCHDOG=1
mkdir -p "$s/state"
for v in $(env | sed -n 's/^\(GIT_[A-Z_]*\)=.*/\1/p'); do unset "$v"; done
export GIT_AUTHOR_NAME=probe GIT_AUTHOR_EMAIL=probe@invalid GIT_COMMITTER_NAME=probe GIT_COMMITTER_EMAIL=probe@invalid

git init -q -b main "$s/main" && git -C "$s/main" commit -q --allow-empty -m init
# No remote: a push names this local bare repository by path, never origin.
git init -q --bare "$s/remote.git"
mkdir "$s/main/tickets" && printf '# 01: Probe\n' >"$s/main/tickets/01-probe.md"
cd "$s/main" || exit 1
"$horch" ledger run new codexgit >/dev/null
"$horch" ledger add --file tickets/01-probe.md --primary codex-sol --check true >/dev/null
w=$("$horch" ledger worker new --persona codex-sol)
export HORCH_WORKER_ID=$w
"$horch" ledger claim >/dev/null
wt=$("$horch" ledger worktree new probe | sed -n 's/^path: //p')
case $wt in "$s"/*) ;; *) fail setup "the worktree '$wt' is not under $s; stopping"; exit 1 ;; esac

# The pairs fleet_write_profile_args (crates/horch-core/src/harness/codex.rs)
# passes. sandboxed_in runs from <dir>; sandboxed runs from the worktree.
sandboxed_in() {
  local dir=$1 p=permissions.fleet_write
  shift
  codex sandbox -P fleet_write -C "$dir" \
    -c "$p.extends=\":workspace\"" \
    -c "$p.filesystem={\":project_roots\"={\".\"=\"write\",\".agents\"=\"write\"},\":slash_tmp\"=\"read\",\":tmpdir\"=\"read\"}" \
    -c "$p.workspace_roots={\"$s/state\"=true}" \
    -c "$p.network.enabled=true" -- "$@"
}
sandboxed() { sandboxed_in "$wt" "$@"; }
# can <label> <dir> <path>: report whether a pane in <dir> can write <path>.
can() {
  local label=$1 dir=$2 path=$3 want=$4 got=no
  sandboxed_in "$dir" sh -c "mkdir -p \"\$(dirname '$path')\" 2>/dev/null; echo >> '$path'" 2>/dev/null && got=yes
  [ "$got" = "$want" ] && pass "grant $label: write=$got" || fail "grant $label" "write=$got, want $want"
}
decision() {
  codex execpolicy check -r "$rules" -- "$@" |
    python3 -c 'import json,sys; print(json.load(sys.stdin).get("decision","no-match"))'
}

# 1. The shipped rules allow horch ledger and no raw git command.
for c in "git add a.txt" "git commit -m x" "git -c core.hooksPath=/dev/null commit -m x" "git push origin HEAD"; do
  # shellcheck disable=SC2086
  d=$(decision $c)
  [ "$d" = no-match ] && pass "execpolicy '$c' -> $d" || fail "execpolicy '$c'" "$d"
done
d=$(decision horch ledger commit -m x)
[ "$d" = allow ] && pass "execpolicy 'horch ledger commit -m x' -> allow" || fail "execpolicy horch ledger commit" "$d"

# 2. The fleet_write grants (decision 01c). A pane in the shared checkout
# could write .git/hooks and .git/config before the grants were removed.
main=$s/main
gd=$main/.git/worktrees/$(basename "$wt")
can "worktree pane: main .git/hooks/pre-commit" "$wt" "$main/.git/hooks/pre-commit" no
can "worktree pane: main .git/config" "$wt" "$main/.git/config" no
can "worktree pane: main .git/worktrees/<name>/x" "$wt" "$gd/x" no
can "worktree pane: main .codex/x" "$wt" "$main/.codex/x" no
can "worktree pane: worktree .codex/x" "$wt" "$wt/.codex/x" no
can "worktree pane: worktree .aws/x" "$wt" "$wt/.aws/x" no
can "worktree pane: worktree .agents/x" "$wt" "$wt/.agents/x" yes
can "worktree pane: worktree .git file" "$wt" "$wt/.git" no
can "shared-checkout pane: .git/hooks/pre-commit" "$main" "$main/.git/hooks/pre-commit" no
can "shared-checkout pane: .git/config" "$main" "$main/.git/config" no
can "shared-checkout pane: .codex/x" "$main" "$main/.codex/x" no
rm -rf "$wt/.agents"

# 3. Raw git stays in the sandbox, which refuses the git metadata.
echo a >"$wt/a.txt"
if sandboxed git add a.txt 2>"$s/err"; then fail "sandboxed git add" "exit 0"; else pass "sandboxed git add refused: $(head -1 "$s/err")"; fi
if sandboxed git push -q "$s/remote.git" HEAD 2>"$s/err"; then fail "sandboxed git push" "exit 0"; else pass "sandboxed git push refused: $(grep -m1 rejected "$s/err")"; fi

# 4. The nested-repository attack: a clean filter in a nested repo's config.
mkdir "$wt/nested" && git -C "$wt/nested" init -q
git -C "$wt/nested" config filter.x.clean "touch $s/ESCAPED; cat"
echo '* filter=x' >"$wt/nested/.gitattributes"
echo f >"$wt/nested/f.txt"
(cd "$wt/nested" && git add f.txt)
[ -e "$s/ESCAPED" ] && pass "attack is real: plain git add ran the filter" || fail attack "the filter did not run outside the sandbox"
# Unstage without git: `git rm --cached` would run the filter again.
rm -f "$wt/nested/.git/index" "$s/ESCAPED"
sandboxed sh -c 'cd nested && git add f.txt' 2>/dev/null
[ -e "$s/ESCAPED" ] && fail "sandboxed nested git add" "the filter wrote outside the sandbox" || pass "sandboxed nested git add: the filter cannot write outside"
if out=$(cd "$wt" && "$horch" ledger commit -m x -- nested/f.txt 2>&1); then fail "horch ledger commit nested" "exit 0"; else
  case $out in *"nested repository"*) pass "horch ledger commit refuses a nested path" ;; *) fail "horch ledger commit nested" "$out" ;; esac
fi
rm -rf "$wt/nested"

# 5. The trusted command commits in the recorded worktree.
before=$(git -C "$wt" rev-parse HEAD)
if id=$(cd "$wt" && "$horch" ledger commit -m probe -- a.txt 2>"$s/err"); then
  [ "$id" = "$(git -C "$wt" rev-parse HEAD)" ] && [ "$id" != "$before" ] &&
    pass "horch ledger commit -> ${id:0:12} on $(git -C "$wt" branch --show-current)" ||
    fail "horch ledger commit" "HEAD did not move to $id"
else
  fail "horch ledger commit" "$(cat "$s/err")"
fi

exit $failed
