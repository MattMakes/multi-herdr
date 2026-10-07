#!/usr/bin/env bash
# Live check: Linux. Runs `cargo test --workspace` and Godot headless in an
# aarch64 Linux container under colima. Result file: docs/live-checks/linux.md.
#
# Steps:
#   colima       start colima (stopped again at exit if this script started it)
#   cargo-test   cargo test --workspace in the official rust image; the source
#                is mounted read-only, the target dir and HOME are container-local
#   godot-sha    download Godot 4.7.2 linux.arm64, check SHA-512 against the
#                release's SHA512-SUMS.txt
#   godot-version   godot --headless --version
#   godot-xdg    import + 1 headless script with XDG_* set to scratch paths;
#                every file Godot writes must be under a scratch path
#
# Env: HORCH_LINUX_IMAGE (default rust:1-bookworm),
#      HORCH_LINUX_STEPS (default "cargo-test godot"; pick a subset),
#      HORCH_LINUX_SRC (default "head": a `git archive HEAD` snapshot, so other
#      workers' uncommitted edits in the shared checkout do not break the build;
#      "worktree" mounts the checkout as it is; any other value is a source
#      directory to mount).
# Writes only under .worktrees/_scratch/live-linux/ (the logs, the snapshot).
# Builds no image; every container is --rm.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
SCRATCH="$ROOT/.worktrees/_scratch/live-linux"
IMAGE=${HORCH_LINUX_IMAGE:-rust:1-bookworm}
STEPS=${HORCH_LINUX_STEPS:-cargo-test godot}
SRC_MODE=${HORCH_LINUX_SRC:-head}
GODOT_VERSION=4.7.2
mkdir -p "$SCRATCH"

failed=0
pass() { echo "PASS $1${2:+: $2}"; }
fail() { echo "FAIL $1: $2"; failed=1; }
skip() { echo "SKIP $1: $2"; }
want() { [[ " $STEPS " == *" $1 "* ]]; }

for tool in colima docker; do
    if ! command -v "$tool" >/dev/null; then
        skip colima "$tool is not on PATH"
        exit 0
    fi
done

started=0
cleanup() {
    if [ "${SRC_MODE:-}" = head ]; then rm -rf "$SCRATCH/src"; fi
    if [ "$started" = 1 ]; then
        colima stop >/dev/null 2>&1 && echo "colima stopped" || echo "colima stop failed"
    fi
}
trap cleanup EXIT

if colima status >/dev/null 2>&1; then
    pass colima "already running ($(colima version | head -1))"
else
    if colima start >"$SCRATCH/colima.log" 2>&1; then
        started=1
        pass colima "started ($(colima version | head -1))"
    else
        fail colima "colima start failed; see $SCRATCH/colima.log"
        exit 1
    fi
fi

docker pull --quiet --platform linux/arm64 "$IMAGE" >/dev/null
echo "image $IMAGE $(docker image inspect "$IMAGE" --format '{{.Architecture}} {{.Size}}') bytes"

SRC=$ROOT
if [ "$SRC_MODE" = head ]; then
    SRC="$SCRATCH/src"
    rm -rf "$SRC" && mkdir -p "$SRC"
    git -C "$ROOT" archive HEAD | tar -x -C "$SRC"
    echo "source HEAD $(git -C "$ROOT" rev-parse --short HEAD) snapshot"
elif [ "$SRC_MODE" = worktree ]; then
    echo "source working tree $ROOT"
else
    SRC=$(cd "$SRC_MODE" && pwd)
    echo "source directory $SRC"
fi
# The snapshot (about 16 MB) is removed at exit by cleanup.

# One container per step: --rm, source read-only, everything else container-local.
run() {
    docker run --rm --platform linux/arm64 \
        -v "$SRC:/src:ro" -w /src \
        -e HOME=/work/home -e CARGO_TARGET_DIR=/work/target \
        -e HORCH_REQUIRE_GIT=1 -e HORCH_REQUIRE_SQLITE=1 \
        "$IMAGE" bash -euo pipefail -c "$1"
}

if want cargo-test; then
    log="$SCRATCH/cargo-test.log"
    rc=0
    # As a normal user, not root: root ignores file modes, and some tests
    # make a directory read-only to force a failure. bubblewrap and socat are
    # what Claude Code's sandbox needs on Linux; a test checks the host for them.
    # /bin/sh stays dash, the Debian and Ubuntu default.
    run '
        apt-get update -qq >/dev/null
        apt-get install -y -qq sqlite3 bubblewrap socat >/dev/null
        useradd -m -u 1000 dev
        mkdir -p /work/home && chown -R dev: /work
        echo "uname: $(uname -srm)"; rustc --version; git --version; echo "sqlite3 $(sqlite3 --version)"
        echo "/bin/sh -> $(readlink -f /bin/sh)"
        runuser -u dev -- env PATH="$PATH" RUSTUP_HOME="$RUSTUP_HOME" HOME=/work/home CARGO_HOME=/work/cargo CARGO_TARGET_DIR="$CARGO_TARGET_DIR" HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 bash -euo pipefail -c "
            git config --global --add safe.directory \"*\"
            git config --global user.name live-linux
            git config --global user.email live-linux@example.invalid
            echo user: \$(id -un)
            cargo test --workspace --locked --no-fail-fast 2>&1
        "
    ' >"$log" 2>&1 || rc=$?
    counts=$(awk '/^test result:/ { p += $4; f += $6; i += $8 } END { printf "passed=%d failed=%d ignored=%d", p, f, i }' "$log")
    if [ "$rc" = 0 ]; then
        pass cargo-test "$counts"
    else
        fail cargo-test "exit $rc, $counts; see $log"
        grep -E '^test .* FAILED$|^    [a-z_:]+$' "$log" | sort -u | head -40 || true
    fi
fi

if want godot; then
    log="$SCRATCH/godot.log"
    rc=0
    run '
        S=/work/scratch; mkdir -p $S/dl $S/home $S/xdg/data $S/xdg/config $S/xdg/cache $S/proj
        cd $S/dl
        Z=Godot_v'"$GODOT_VERSION"'-stable_linux.arm64.zip
        U=https://github.com/godotengine/godot/releases/download/'"$GODOT_VERSION"'-stable
        curl -fsSL -o $Z $U/$Z
        curl -fsSL -o SHA512-SUMS.txt $U/SHA512-SUMS.txt
        if grep " $Z\$" SHA512-SUMS.txt | sha512sum -c - ; then echo "STEP godot-sha ok"; else echo "STEP godot-sha bad"; exit 1; fi
        unzip -q $Z && G=$S/dl/${Z%.zip} && chmod +x $G
        export HOME=$S/home XDG_DATA_HOME=$S/xdg/data XDG_CONFIG_HOME=$S/xdg/config XDG_CACHE_HOME=$S/xdg/cache
        echo "STEP godot-version $($G --headless --version)"
        cat > $S/proj/project.godot <<EOF
config_version=5

[application]
config/name="LiveLinux"
config/features=PackedStringArray("4.7")
EOF
        cat > $S/proj/probe.gd <<EOF
extends SceneTree
func _init():
    var f = FileAccess.open("user://probe.txt", FileAccess.WRITE)
    f.store_string("ok")
    f.close()
    print("user dir: ", OS.get_user_data_dir())
    quit(0)
EOF
        touch $S/marker; sleep 1
        $G --headless --path $S/proj --import 2>&1 | tail -3
        $G --headless --path $S/proj --script res://probe.gd 2>&1 | tail -3
        echo "--- written under XDG dirs:"
        find $S/xdg $S/home -newer $S/marker -type f | sort
        echo "--- written elsewhere:"
        find / -xdev -newer $S/marker -type f \
            -not -path "$S/*" -not -path "/proc/*" -not -path "/sys/*" -not -path "/dev/*" | sort > $S/elsewhere
        cat $S/elsewhere
        echo "--- end"
        test -f $S/xdg/data/godot/app_userdata/LiveLinux/probe.txt || { echo "STEP godot-xdg bad: no user:// file under XDG_DATA_HOME"; exit 1; }
        test -n "$(find $S/xdg/config -type f)" || { echo "STEP godot-xdg bad: nothing under XDG_CONFIG_HOME"; exit 1; }
        test -z "$(find $S/home -type f)" || { echo "STEP godot-xdg bad: files under HOME"; exit 1; }
        test ! -s $S/elsewhere || { echo "STEP godot-xdg bad: files outside the scratch dirs"; exit 1; }
        echo "STEP godot-xdg ok"
    ' >"$log" 2>&1 || rc=$?
    grep -q '^STEP godot-sha ok' "$log" && pass godot-sha "SHA-512 matches SHA512-SUMS.txt" \
        || fail godot-sha "see $log"
    v=$(sed -n 's/^STEP godot-version //p' "$log")
    [ -n "$v" ] && pass godot-version "$v" || fail godot-version "see $log"
    if grep -q '^STEP godot-xdg ok' "$log"; then
        pass godot-xdg "writes only under XDG_* (see $log)"
    else
        fail godot-xdg "$(sed -n 's/^STEP godot-xdg bad: //p' "$log" | head -1) (exit $rc; see $log)"
    fi
fi

exit "$failed"
