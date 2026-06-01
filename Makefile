# mcpscrk - build helpers.
#
# `make` builds an optimized binary and drops the freshest copy into ./bin,
# so bin/mcpscrk is always the latest build.

BIN := bin/mcpscrk
PORT ?= 8787

.PHONY: all build run dev clean fmt

all: build

# Optimized build, copied into ./bin.
build:
	cargo build --release
	@mkdir -p bin
	@cp target/release/mcpscrk $(BIN)
	@echo "-> $(BIN) updated"

# Build (release) then serve on $(PORT). Override: make run PORT=9000
run: build
	$(BIN) --port $(PORT)

# Fast unoptimized run for development.
dev:
	cargo run -- --port $(PORT)

fmt:
	cargo fmt

clean:
	cargo clean
	@rm -f $(BIN)
