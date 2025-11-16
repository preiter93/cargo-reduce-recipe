# cargo-reduce-recipe

<div align="center">

[![Continuous Integration](https://github.com/preiter93/cargo-reduce-recipe/actions/workflows/ci.yml/badge.svg)](https://github.com/preiter93/cargo-reduce-recipe/actions/workflows/ci.yml)

</div>

`cargo-reduce-recipe` reduces `cargo-chef` recipes for multi-member workspaces by removing dependencies that are unrelated to the targeted member. This results in improved Docker caching.

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
cargo chef prepare --recipe-path recipe-bar.json --bin bar 
```

The issue is that cargo-chef’s generated recipe still includes all workspace members manifests and lockfiles even those that are unrelated to the filtered member.

As a result a change in `foo`’s dependencies invalidates the Docker cache for `bar`.

`cargo-reduce-recipe` fixes that. It post-processes the generated recipe and removes all dependency and lockfile entries that are not actually required by the selected workspace member (directly or transitively). The result is a minimized recipe ensuring that unrelated workspace changes no longer trigger unnecessary rebuilds.

In a real-life unpublished workspace, using cargo-reduce-recipe cut Docker build times for unrelated members for me from 80s to 20s, a ~75% reduction.

## Installation

```sh
cargo install --git https://github.com/preiter93/cargo-reduce-recipe --tag v0.1.0
```

## Usage

To build dependency recipes for only a specific workspace member, follow this:

1. Prepare a recipe for a single member
```sh
cargo chef prepare --recipe-path recipe-bar.json --bin bar 
```

2. Reduce the recipe
```sh
cargo-reduce-recipe \
    --recipe-path-in recipe-bar.json \
    --recipe-path-out recipe-bar-reduced.json \
    --bin bar
```

3. Cook the reduced recipe
```sh
cargo chef cook --release --recipe-path recipe-bar-reduced.json --bin bar
```

License: MIT

## Docker

`cargo-reduce-recipe` can be used together with `cargo-chef` in a Dockerfile:
```Dockerfile
ARG SERVICE_NAME

FROM rust:1.88-bookworm AS chef
WORKDIR /services

# Install cargo-chef and cargo-reduce-recipe
RUN cargo install cargo-chef --locked --version 0.1.73 \
    && cargo install --git https://github.com/preiter93/cargo-reduce-recipe --tag v0.1.0

# Prepare and reduce the recipe 
FROM chef as planner
ARG SERVICE_NAME
ENV SERVICE_NAME=${SERVICE_NAME}
COPY . .
RUN cargo chef prepare --recipe-path recipe.json --bin ${SERVICE_NAME} \
    && cargo-reduce-recipe --recipe-path-in recipe.json --recipe-path-out recipe-reduced.json --bin ${SERVICE_NAME}

# Build the dependencies
FROM chef as builder
ARG SERVICE_NAME
ENV SERVICE_NAME=${SERVICE_NAME}
COPY --from=planner /services/recipe-reduced.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --bin ${SERVICE_NAME}

# Build the binary
COPY . .
RUN cargo build --release --bin ${SERVICE_NAME}

# Run the service
FROM debian:bookworm-slim AS runtime
ARG SERVICE_NAME
COPY --from=builder /services/target/release/${SERVICE_NAME} /usr/local/bin/main
ENTRYPOINT ["/usr/local/bin/main"]
```

## Disclaimer

This functionality should probably be part of cargo-chef itself, see https://github.com/LukeMathWalker/cargo-chef/issues/323.
