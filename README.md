# cargo-reduce-workspace-recipe

<div align="center">

[![Continuous Integration](https://github.com/preiter93/reduce-workspace-recipe/actions/workflows/ci.yml/badge.svg)](https://github.com/preiter93/reduce-workspace-recipe/actions/workflows/ci.yml)

</div>

`cargo-reduce-workspace-recipe` reduces `cargo-chef` recipes for multi-member workspaces by removing dependencies that are unrelated to the targeted member. This results in improved Docker caching.

## Problem

Consider a workspace like this:
```sh
├── Cargo.toml
├── bar
└── foo
```
`bar` and `foo` are completely independent.

However, when using [cargo-chef](https://github.com/LukeMathWalker/cargo-chef), adding a new dependency to `foo` will still force `bar` to be rebuilt even if you run:
```sh
cargo chef prepare --bin bar --recipe-path recipe-bar.json
```

The issue is that cargo-chef’s generated recipe still includes all workspace members manifests and lockfiles even those that are unrelated to the filtered member.

As a result a change in `foo`’s dependencies invalidates the Docker cache for `bar`.

`cargo-reduce-workspce-recipe` fixes that. It post-processes the generated recipe and removes all dependency and lockfile entries that are not actually required by the selected workspace member (directly or transitively). The result is a minimized recipe ensuring that unrelated workspace changes no longer trigger unnecessary rebuilds.

## Installation

```sh
cargo install --git https://github.com/preiter93/reduce-workspace-recipe --tag v0.1.0
```

## Usage

To build dependency recipes for only a specific workspace member, follow this:

1. Prepare a recipe for a single member
```sh
cargo chef prepare --bin bar --recipe-path recipe-bar.json
```

2. Reduce the recipe
```sh
cargo-reduce-workspace-recipe \
    --recipe-path-in recipe-bar.json \
    --recipe-path-out recipe-bar-reduced.json
```

3. Cook the reduced recipe
```sh
cargo chef cook --release --recipe-path recipe-bar-reduced.json --bin bar
```

License: MIT

## Docker

`cargo-reduce-workspace-recipe` can be used together with `cargo-chef` in a Dockerfile:
```Dockerfile
ARG SERVICE_NAME

FROM rust:1.88-bookworm AS chef
WORKDIR /services

# Install cargo-chef and cargo-reduce-workspace-recipe
RUN cargo install cargo-chef --locked --version 0.1.73 \
    && cargo install --git https://github.com/preiter93/reduce-workspace-recipe --tag v0.1.0

# Prepare the workspace recipe 
FROM chef as planner
ARG SERVICE_NAME
ENV SERVICE_NAME=${SERVICE_NAME}
COPY . .
RUN cargo chef prepare --bin ${SERVICE_NAME} --recipe-path recipe.json

# Reduce the workspace recipe
RUN cargo-reduce-workspace-recipe --recipe-path-in recipe.json --recipe-path-out recipe-reduced.json

# Build the dependencies
FROM chef as builder
ARG SERVICE_NAME
ENV SERVICE_NAME=${SERVICE_NAME}
COPY --from=planner /services/recipe-reduced.json recipe-reduced.json
RUN cargo chef cook --release --recipe-path recipe-reduced.json --bin ${SERVICE_NAME}

# Build the binary
COPY . .
RUN cargo build --release --bin ${SERVICE_NAME}

# Run the service
FROM debian:bookworm-slim AS runtime
ARG SERVICE_NAME
COPY --from=builder /services/target/release/${SERVICE_NAME} /usr/local/bin/main
ENTRYPOINT ["/usr/local/bin/main"]
```
