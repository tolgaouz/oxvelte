# oxvelte

A high-performance Svelte parser and linter written in Rust, powered by [oxc](https://oxc.rs/).

## What is this?

Oxvelte is a native Svelte toolchain that replaces eslint-plugin-svelte with a single fast
binary. It uses oxc's JavaScript/TypeScript parser and semantic analyzer for `<script>` blocks,
and implements a custom Svelte template parser for everything else.

- **Parser**: full Svelte 4 + Svelte 5 template language support
- **Linter**: all ~70 rules from eslint-plugin-svelte, ported to Rust
- **Formatter**: delegates JS/TS formatting to oxcfmt
- **Semantic analysis**: powered by oxc's semantic analyzer for scope/symbol resolution

## How it works

The repo is deliberately kept small and has a few key files:

- **`prepare.sh`** — one-time setup: clones svelte + eslint-plugin-svelte repos, copies test
  fixtures. Not modified by the agent.
- **`src/`** — all Rust source code lives here. **Modified by the agent.**
- **`program.md`** — agent instructions. **Modified by the human.**

This project follows the [autoresearch](https://github.com/karpathy/autoresearch) pattern:
you write high-level directions in `program.md`, point a coding agent at it, and let it
build and iterate autonomously.

## Quick start

**Requirements:** Rust 1.80+, Git.

```bash
# 1. Clone
git clone https://github.com/user/oxvelte && cd oxvelte

# 2. Fetch reference repos and test fixtures
bash prepare.sh

# 3. Build
cargo build

# 4. Run tests
cargo test

# 5. Run the linter on a file
cargo run -- lint path/to/Component.svelte
```

## Running the agent

Spin up Claude Code, Codex, or your preferred coding agent in this repo, then prompt:

```
Hi, have a look at program.md and let's kick off a new experiment! Let's do the setup first.
```

The `program.md` file is essentially a lightweight "skill" that gives the agent full context
on the project architecture, what to build, and how to evaluate progress.

## Project structure

```
program.md              — agent instructions (human edits this)
prepare.sh              — clone reference repos + copy test fixtures (do not modify)
verify_fixtures.sh      — checksums fixtures to prevent tampering (do not modify)
Cargo.toml              — single crate: one binary, all deps declared here

src/                    — THE ONLY DIRECTORY THE AGENT MODIFIES
  lib.rs                — module declarations + integration tests
  main.rs               — CLI entry point
  ast.rs                — Svelte AST type definitions
  parser/               — Svelte template parser
  linter/rules/         — lint rules (ported from eslint-plugin-svelte)

fixtures/               — read-only test data copied from vendor/ (do not modify)
vendor/                 — reference repos (git-ignored, created by prepare.sh)
  svelte/               — sveltejs/svelte (parser test fixtures)
  eslint-plugin-svelte/ — sveltejs/eslint-plugin-svelte (lint rule tests)
```

## Design choices

- **Reuse oxc**: don't rewrite JS/TS parsing. Oxc is the fastest Rust JS parser and its
  semantic analyzer gives us scopes, symbols, and type info for free.
- **Match existing tests**: the parser must pass all of svelte's own parser test fixtures.
  The linter must pass all of eslint-plugin-svelte's test cases. This ensures correctness.
- **Single binary**: no Node.js dependency. Ship as a native binary or WASM module.
- **Arena allocation**: AST nodes are allocated in oxc's bumpalo arena for cache-friendly
  traversal and instant deallocation.

## License

MIT
