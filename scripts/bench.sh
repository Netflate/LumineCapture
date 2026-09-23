#!/usr/bin/env bash
# AI wrriten Speed comparison: LumineCapture vs Spectacle vs Flameshot.
#
#   ./scripts/bench.sh
#
# The same job for all three: launch, capture the whole screen, write a PNG, exit. No editor,
# no options beyond the output directory
# Flameshot is measured twice: with its daemon already running, and as a cold start (it is
# killed before every run). A Flameshot that was running before is started again at the end.
#
# Needs hyperfine and the three tools in PATH. RUNS, WARMUP and OUT can be overridden:
#   RUNS=50 ./scripts/bench.sh
set -euo pipefail

RUNS=${RUNS:-30}
WARMUP=${WARMUP:-3}
OUT=${OUT:-bench-results.md}

# no dot in the directory name: Flameshot would take it for a file extension
DIR=$(mktemp -d -t lumine-bench-XXXXXX)

flameshot_was_running=0
if pgrep -x flameshot >/dev/null; then
    flameshot_was_running=1
fi

cleanup() {
    pkill -x flameshot || true
    rm -rf "$DIR"
    if [ "$flameshot_was_running" = 1 ]; then
        (flameshot >/dev/null 2>&1 &)
    fi
}
trap cleanup EXIT

if [ "$flameshot_was_running" = 0 ]; then
    (flameshot >/dev/null 2>&1 &)
    sleep 3
fi

# sh -c gives every Spectacle run its own file name ($$ is the pid), so nothing is overwritten;
# the daemon case goes before the cold one, because the cold one kills the daemon
hyperfine -N --warmup "$WARMUP" --runs "$RUNS" --export-markdown "$OUT" \
    --prepare true -n LumineCapture "lumine-capture -f -o $DIR" \
    --prepare true -n Spectacle "sh -c 'spectacle -b -n -f -o $DIR/spectacle-\$\$.png'" \
    --prepare true -n "Flameshot (daemon running)" "flameshot full -p $DIR" \
    --prepare 'sh -c "pkill -x flameshot || true"' -n "Flameshot (cold start)" "flameshot full -p $DIR"

echo "$(find "$DIR" -name '*.png' | wc -l) screenshots taken and thrown away, table written to $OUT"
