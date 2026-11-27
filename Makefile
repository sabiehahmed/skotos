SHELL := /usr/bin/env bash
.SHELLFLAGS := --norc --noprofile -euo pipefail -c
.DEFAULT_GOAL := help

CARGO_BUILD_JOBS ?= 2
export CARGO_BUILD_JOBS

COMPOSE ?= docker compose

.PHONY: help dev-infra dev-infra-down migrate test-rust test-ui test-web gen-openapi lint-all check

help: ## Show available Skotos development targets
	@awk 'BEGIN {FS = ":.*## "} /^[a-zA-Z0-9_-]+:.*## / {printf "  %-18s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

dev-infra: ## Start local Postgres, NATS JetStream, and MinIO and wait until healthy
	$(COMPOSE) up -d --wait --wait-timeout 90

dev-infra-down: ## Stop local dev infrastructure containers (preserving volumes)
	$(COMPOSE) down

migrate: ## Run embedded PostgreSQL migrations via control-api
	cargo run -p control-api --bin control-api -- --migrate-only

test-rust: ## Run all Rust workspace tests
	cargo test --workspace

test-ui: ## Run @skotos/ui test suite
	pnpm --filter @skotos/ui test

test-web: ## Run @skotos/web test suite
	pnpm --filter @skotos/web test

gen-openapi: ## Export OpenAPI 3.1 spec and regenerate @skotos/api-client
	cargo run -p control-api --bin export-openapi
	pnpm --filter @skotos/api-client gen
	pnpm --filter @skotos/api-client typecheck

lint-all: ## Run Rust fmt/clippy, ESLint boundaries, zero-Tailwind guard, and docs checks
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings
	pnpm lint
	pnpm lint:boundaries
	pnpm run lint:md
	pnpm run format:check

check: lint-all test-rust ## Run full offline lint, typecheck, test, and build verification
	pnpm typecheck
	pnpm test
	pnpm build
