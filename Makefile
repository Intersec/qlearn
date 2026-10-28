# qlearn. `make check` is the gate; everything else is a shortcut.
#
# The interface is always built before the binary: the binary embeds web/dist.

CARGO ?= cargo
NPM ?= npm
PREFIX ?= $(HOME)/.local
MUSL_TARGET := x86_64-unknown-linux-musl
# What the release file is called. No version in it: the release page and
# the package registry both carry that, and a name that never changes is
# what a permanent link can point at.
DIST_NAME := qlearn-linux-x86_64.gz
# The port that web/vite.config.ts proxies the API to.
DEV_PORT := 7420

.PHONY: all setup web build check test fmt lint install dist musl-target dev clean

all: build

## Install the interface dependencies. Needed once, and after a lockfile change.
setup:
	cd web && $(NPM) ci

## Build the interface into web/dist.
web:
	cd web && $(NPM) run build

## Build the release binary, interface included.
build: web
	$(CARGO) build --release

## The whole gate. Run this before every commit. `make check V=1` streams it.
check:
	@V=$(V) CARGO=$(CARGO) NPM=$(NPM) ./scripts/check.sh

## The tests alone.
test: web
	$(CARGO) test --workspace
	cd web && $(NPM) run test:run

## Correct the format of everything.
fmt:
	$(CARGO) fmt --all
	cd web && $(NPM) run format

## Lint the Rust code and the interface.
lint:
	$(CARGO) clippy --workspace --all-targets -- -D warnings
	cd web && $(NPM) run lint

## The binary into $(PREFIX)/bin.
install: build
	$(CARGO) install --locked --path crates/qlearn --root $(PREFIX) --force

## One static binary, compressed for a release. It adds the target if it is
## missing.
dist: web musl-target
	$(CARGO) build --release --target $(MUSL_TARGET)
	@rm -rf dist && mkdir dist
	@target=$$($(CARGO) metadata --format-version 1 --no-deps \
		| sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p'); \
		cp "$$target/$(MUSL_TARGET)/release/qlearn" dist/
	@gzip -9 -c dist/qlearn > dist/$(DIST_NAME)
	@ls -lh dist/

# A target belongs to rustup, not to asdf: `.tool-versions` pins a Rust
# version and knows nothing about targets. asdf installs Rust through rustup
# and points RUSTUP_HOME at its own directory, so this works under asdf too,
# and the target lands inside the asdf install.
#
# Only `dist` does this. `build` never touches the toolchain.
musl-target:
	@command -v rustup >/dev/null 2>&1 || { \
		echo "rustup is missing. It installs the $(MUSL_TARGET) target."; \
		exit 1; \
	}
	@rustup target list --installed | grep -qx '$(MUSL_TARGET)' || { \
		echo "adding the $(MUSL_TARGET) target"; \
		rustup target add $(MUSL_TARGET); \
	}

## The server and Vite together, with hot reload on the interface.
## Open the URL that Vite prints. Its proxy reads the token in
## .learn/server.json, so that URL needs none.
dev:
	@trap 'kill 0' EXIT INT TERM; \
	$(CARGO) run -- --port $(DEV_PORT) --no-open & \
	cd web && $(NPM) run dev; \
	wait

clean:
	rm -rf dist
	$(CARGO) clean
	rm -rf web/dist web/node_modules
