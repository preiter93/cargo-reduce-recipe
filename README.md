# cargo-reduce-workspace-recipe

`cargo-reduce-workspace-recipe` post-processes `cargo-chef` recipes for workspaces with multiple interdependent members.

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
