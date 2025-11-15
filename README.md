# cargo-reduce-workspace-recipe

`cargo-reduce-workspace-recipe` post-processes `cargo-chef` recipes for workspaces with multiple interdependent members.

## Problem

When using `cargo-chef` on a workspace with multiple members, if **one member’s dependencies change**, the recipe still contains **all workspace members and their dependencies**. This causes unnecessary rebuilds of unrelated members.

See: [cargo-chef issue #314](https://github.com/LukeMathWalker/cargo-chef/issues/314)

## Solution

This crate filters a recipe **after a `cargo chef prepare` step**:

- Keeps only the workspace members that the main member depends on or transitively depends on
- Removes manifests and lockfile entries for unused workspace members.
- Preserves all external dependencies.

## Usage

```sh
cargo reduce-workspace-recipe --recipe-path-in recipe.json --recipe-path-out recipe-reduced.json
```
