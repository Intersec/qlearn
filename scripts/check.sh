#!/bin/sh
# The gate. One line per step, quiet until a step fails.
#
# A failing step prints its whole output, because a formatter failure carries
# no word you can grep for. `make check V=1` streams everything instead.
set -u

root=$(dirname "$0")/..
cd "$root" || exit 1

verbose=${V:-0}
cargo=${CARGO:-cargo}
npm=${NPM:-npm}
failed=0

step() {
    name=$1
    dir=$2
    shift 2
    if [ "$verbose" = 1 ]; then
        printf '=== %s\n' "$name"
        if (cd "$dir" && "$@"); then
            printf 'ok    %s\n' "$name"
        else
            printf 'FAIL  %s\n' "$name"
            failed=1
        fi
        return
    fi
    if out=$(cd "$dir" && "$@" 2>&1); then
        printf 'ok    %s\n' "$name"
    else
        printf 'FAIL  %s\n' "$name"
        printf '%s\n' "$out"
        failed=1
    fi
}

# QLEARN_BROWSER is the browser that the Playwright config takes first, so a
# machine with only that one still runs the browser tests. The path found here
# goes to Playwright, so the step runs the browser that decided not to skip it.
find_browser() {
    if [ -n "${QLEARN_BROWSER:-}" ] && [ -x "$QLEARN_BROWSER" ]; then
        echo "$QLEARN_BROWSER"
        return 0
    fi
    for name in google-chrome google-chrome-stable chromium chromium-browser; do
        if path=$(command -v "$name" 2>/dev/null); then
            echo "$path"
            return 0
        fi
    done
    return 1
}

if [ ! -d web/node_modules ]; then
    echo "web/node_modules is missing. Run: make setup"
    exit 1
fi

# The interface is built before the Rust steps: the tests and the release
# binary embed web/dist.
step "eslint"          web "$npm" run --silent lint
step "prettier"        web "$npm" run --silent format:check
step "vitest"          web "$npm" run --silent test:run
step "vite build"      web "$npm" run --silent build
step "cargo fmt"       .   "$cargo" fmt --all -- --check
step "cargo clippy"    .   "$cargo" clippy --workspace --all-targets -- -D warnings
step "cargo test"      .   "$cargo" test --workspace
step "cargo build"     .   "$cargo" build --release

# The browser tests need a browser. A machine without one still gets every
# other step, and says plainly that it skipped this one rather than passing
# quietly.
if browser=$(find_browser); then
    # --pass-with-no-tests goes away with the first spec in web/e2e/.
    step "playwright"      web env QLEARN_BROWSER="$browser" "$npm" exec -- playwright test --pass-with-no-tests
else
    printf 'skip  playwright (no QLEARN_BROWSER, and no chrome or chromium on the PATH)\n'
fi

exit $failed
