SHELL := /usr/bin/env bash
.SHELLFLAGS := --norc --noprofile -euo pipefail -c
.DEFAULT_GOAL := help

CARGO_BUILD_JOBS ?= 2
export CARGO_BUILD_JOBS

COMPOSE ?= docker compose

.PHONY: help dev-infra dev-infra-down migrate test-rust test-go test-ui test-web gen-openapi lint-all check

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

test-go: ## Run Go Server SDK (124 golden vectors) and Terraform provider tests with race detector
	go -C sdks/go-server test -race ./...
	go -C integrations/terraform-provider-skotos test -race ./...

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

check: lint-all test-rust test-go ## Run full lint, typecheck, test, and build verification
	pnpm typecheck
	pnpm test
	pnpm build
