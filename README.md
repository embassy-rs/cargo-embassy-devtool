# cargo-embassy-devtool

A specialized Cargo workspace tool for managing intra-repository Rust crate dependencies in the Embassy ecosystem.

## Features

- **Dependency Analysis**: List crate dependencies and dependents within the repository
- **Build Management**: Build specific crates or all crates in the workspace.
- **Semantic Version Checking**: Automatically detect required version bumps using semver analysis
- **Release Preparation**: Automate the process of preparing crate releases with proper version bumping and changelog updates

## Commands

### `list`
Display all crates and their direct dependencies in topological order.

### `dependencies <CRATE>`

Show all dependencies for a specific crate.

### `dependents <CRATE>`

Show all crates that depend on a specific crate.

### `build [CRATE]`

Build a specific crate or all crates if none specified.

### `semver-check <CRATE>`

Run semantic version analysis to determine the minimum required version bump for a crate.

### `prepare-release <CRATE>`

Prepare a crate and all its dependents for release by:
- Running semver checks to determine version bumps
- Updating version numbers in Cargo.toml files
- Updating changelogs
- Generating git commands for tagging and publishing

## Installation

```bash
cargo install --path .
```

## Usage

The tool must be run from within a git repository containing Embassy crates. It automatically discovers the repository root and scans for crates with `embassy-*` dependencies.

```bash
# List all crates
cargo embassy-devtool list

# Show dependencies for embassy-time
cargo embassy-devtool dependencies embassy-time

# Prepare embassy-boot for release
cargo embassy-devtool prepare-release embassy-boot
```

## Configuration

Crates can be configured through `Cargo.toml` metadata:

```toml
[package]

publish = false # If the crate should not be checked and published.

[package.metadata.embassy]
skip = true  # Skip this crate during discovery
build = [
    { features = ["std"], target = "x86_64-unknown-linux-gnu" },
    { features = ["defmt"] }
]
```

## License

Embassy is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
