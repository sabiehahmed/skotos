SHELL := /usr/bin/env bash
.SHELLFLAGS := --norc --noprofile -euo pipefail -c
.DEFAULT_GOAL := help

CARGO_BUILD_JOBS ?= 2
export CARGO_BUILD_JOBS

COMPOSE ?= docker compose

.PHONY: help dev-up dev-up-analytics dev-down dev-status dev-logs seed-demo smoke dev-infra dev-infra-down migrate test-rust test-go test-python test-java test-ui test-web gen-openapi lint-all check

help: ## Show available Skotos development targets
	@awk 'BEGIN {FS = ":.*## "} /^[a-zA-Z0-9_-]+:.*## / {printf "  %-18s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

dev-up: ## Start full local stack (infra + all 4 Rust services + @skotos/web), seed demo data, and run smoke test
	./scripts/dev-stack.sh up --seed --smoke

dev-up-analytics: ## Start full local stack with ClickHouse analytics profile, seed demo data, and run smoke test
	./scripts/dev-stack.sh up --analytics --seed --smoke

dev-down: ## Stop all local Skotos app services (add --infra or --clean via scripts/dev-stack.sh)
	./scripts/dev-stack.sh down

dev-status: ## Show health status of Docker infra and all local Skotos services
	./scripts/dev-stack.sh status

dev-logs: ## Tail logs from all local Skotos services
	./scripts/dev-stack.sh logs

seed-demo: ## Idempotently seed demo flags, segments, experiment, guarded rollout, migration & telemetry
	./scripts/seed-demo.sh

smoke: ## Run live end-to-end smoke check across control-api, delivery-edge, event-ingest, relay-proxy & web
	./scripts/smoke-test.sh

dev-infra: ## Start local Postgres, NATS JetStream, and MinIO and wait until healthy
	$(COMPOSE) up -d --wait --wait-timeout 90

dev-infra-down: ## Stop local dev infrastructure containers (preserving volumes)
	$(COMPOSE) down

migrate: ## Run embedded PostgreSQL migrations via control-api
	cargo run -p control-api --bin control-api -- --migrate-only

test-rust: ## Run all Rust workspace tests
	cargo test --workspace

test-go: ## Run Go Server SDK (124 golden vectors) and Terraform provider tests with race detector
	go -C sdks/go-server test -race ./...
	go -C integrations/terraform-provider-skotos test -race ./...

test-python: ## Run Python Server SDK tests (124 golden vectors + OpenFeature + MigrationHelper)
	python3 -m unittest discover -s sdks/python-server/tests -v

test-java: ## Compile & run Java 21 Server SDK tests (124 golden vectors + OpenFeature)
	rm -rf sdks/java-server/build && mkdir -p sdks/java-server/build
	javac -Xlint:all -Werror -d sdks/java-server/build sdks/java-server/src/main/java/dev/skotos/sdk/*.java sdks/java-server/src/test/java/dev/skotos/sdk/GoldenSuiteTest.java
	java -cp sdks/java-server/build dev.skotos.sdk.GoldenSuiteTest
	rm -rf sdks/java-server/build

test-ui: ## Run @skotos/ui test suite
	pnpm --filter @skotos/ui test

test-web: ## Run @skotos/web test suite
	pnpm --filter @skotos/web test

gen-openapi: ## Export OpenAPI 3.1 spec and regenerate @skotos/api-client
	cargo run -p control-api --bin export-openapi
	pnpm --filter @skotos/api-client gen
	pnpm --filter @skotos/api-client typecheck

lint-all: ## Run Rust fmt/clippy, zero-DB guards, ESLint boundaries, zero-Tailwind guard, and docs checks
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings
	! cargo tree -p delivery-edge | grep -E "sqlx|skotos-storage"
	! cargo tree -p relay-proxy | grep -E "sqlx|skotos-storage"
	pnpm lint
	pnpm lint:boundaries
	pnpm run lint:md
	pnpm run format:check

check: lint-all test-rust test-go test-python test-java ## Run full lint, typecheck, test, and build verification
	pnpm typecheck
	pnpm test
	pnpm build
