# Contributing to Oxvelte

## For AI Agents

Read `program.md` — it contains everything you need. The setup phase will guide you through
cloning reference repos and establishing a baseline.

## For Humans

### Setup

```bash
# Clone and prepare
git clone https://github.com/user/oxvelte && cd oxvelte
bash prepare.sh
cargo build
cargo test
```

### Adding a lint rule

1. Create `src/linter/rules/<rule_name>.rs`
2. Implement the `Rule` trait
3. Register it in `src/linter/rules/mod.rs`
4. Reference test fixtures in `fixtures/linter/<rule-name>/` (read-only, do not modify)
5. Run `cargo test`

### Parser work

The parser test fixtures come from svelte's own test suite. After running `prepare.sh`,
they live in `fixtures/parser/`. Each test case has an `input.svelte`
and an expected `output.json`. Your parser output should match.

### Running tests

```bash
# All tests
cargo test

# Only parser tests
cargo test parser

# Only linter tests
cargo test linter
```

### Code style

- `cargo fmt` before committing
- `cargo clippy` should be clean
- No `unsafe` unless strictly necessary and documented
