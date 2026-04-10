BIN     := todo
INSTALL := $(HOME)/.local/bin
TARGET  := aarch64-apple-darwin

.PHONY: build release install uninstall run check fmt lint clean sync-gh sync-jira reindex

## Default: debug build
build:
	cargo build

## Optimised release binary
release:
	cargo build --release

## Install release binary to $(INSTALL)
install: release
	mkdir -p $(INSTALL)
	cp target/release/$(BIN) $(INSTALL)/$(BIN)
	@echo "Installed to $(INSTALL)/$(BIN)"

## Remove installed binary
uninstall:
	rm -f $(INSTALL)/$(BIN)
	@echo "Removed $(INSTALL)/$(BIN)"

## Run in debug mode
run:
	cargo run

## Check compilation without producing a binary (fast)
check:
	cargo check

## Format code
fmt:
	cargo fmt

## Lint with Clippy
lint:
	cargo clippy -- -D warnings

## Remove build artefacts
clean:
	cargo clean

## Download model and pre-compile graph cache (debug build to avoid OOM on large models)
setup:
	cargo run -- --model sentence-transformers/all-MiniLM-L6-v2
	cargo run -- --compile-model

## Pre-compile ONNX → NNEF cache so the release binary loads without OOM
compile-model:
	cargo run -- --compile-model

## Re-embed all items with the current model
reindex:
	cargo run --release -- --reindex

## Sync GitHub PRs
sync-gh:
	cargo run --release -- --sync-gh

## Sync Jira issues
sync-jira:
	cargo run --release -- --sync-jira

## Full sync (GitHub + Jira + reindex)
sync:
	cargo run --release -- --sync

## Show this help
help:
	@grep -E '^## ' Makefile | sed 's/## //'
