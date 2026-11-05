.PHONY: test test-core test-app test-gpu lint rust project build run color-fixtures benchmark verify-clean commands
CONFIGURATION ?= Debug
DERIVED_DATA ?= build/DerivedData
BUILD_JOBS ?= 2
XCODE_ARGS = -jobs $(BUILD_JOBS) -project apps/macos/Strata.xcodeproj -scheme Strata -configuration $(CONFIGURATION) -destination 'platform=macOS,arch=arm64' -derivedDataPath $(DERIVED_DATA)

rust:
	bash scripts/build-rust.sh $(CONFIGURATION)

project: rust
	cd apps/macos && xcodegen generate

build: project
	xcodebuild $(XCODE_ARGS) build

run: build
	open $(DERIVED_DATA)/Build/Products/$(CONFIGURATION)/Strata.app $(if $(filter 1,$(FRAME_STATS)),--args --frame-stats,)

test: test-app

test-app: project test-core
	xcodebuild $(XCODE_ARGS) test

test-core:
	cargo test --workspace --locked

test-gpu:
	STRATA_REQUIRE_GPU=1 cargo test -p strata-render -p strata-ffi --locked -- --test-threads=1

lint:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets --locked -- -D warnings
	bash scripts/check-portability.sh
	bash scripts/lint-swift.sh
	python3 scripts/feature-coverage.py --check

# Refuses existing fixture outputs; choose a new directory for a separate run.
FIXTURE_OUTPUT ?= build/color-fixtures-v1
color-fixtures:
	cargo run --locked -p strata-io --example make_color_fixtures -- $(FIXTURE_OUTPUT)

benchmark:
	bash scripts/run-canvas-benchmark.sh

verify-clean:
	bash scripts/verify-clean-build.sh

commands:
	cargo run --locked -p strata-devtool -- examples
	bash scripts/check-command-docs.sh
