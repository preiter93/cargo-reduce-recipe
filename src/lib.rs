//! `cargo-reduce-recipe` reduces `cargo-chef` recipes for multi-member workspaces by removing dependencies that are unrelated to the targeted member. This results in improved Docker caching.
//!
//! # Problem
//!
//! Consider a workspace like this:
//! ```sh
//! ├── Cargo.toml
//! ├── bar
//! └── foo
//! ```
//! `bar` and `foo` are completely independent.
//!
//! However, when using [cargo-chef](https://github.com/LukeMathWalker/cargo-chef), adding a new dependency to `foo` will still force `bar` to be rebuilt even if you run:
//! ```sh
//! cargo chef prepare --recipe-path recipe-bar.json --bin bar
//! ```
//!
//! The issue is that cargo-chef’s generated recipe still includes all workspace members manifests and lockfiles even those that are unrelated to the filtered member.
//!
//! As a result a change in `foo`’s dependencies invalidates the Docker cache for `bar`.
//!
//! `cargo-reduce-recipe` fixes that. It post-processes the generated recipe and removes all dependency and lockfile entries that are not actually required by the selected workspace member (directly or transitively). The result is a minimized recipe ensuring that unrelated workspace changes no longer trigger unnecessary rebuilds.
//!
//! In a real-life unpublished workspace, using cargo-reduce-recipe cut Docker build times for unrelated members for me from 82s to 23s, a ~72% reduction.
//!
//! # Installation
//!
//! ```sh
//! cargo install --git https://github.com/preiter93/cargo-reduce-recipe --tag v0.1.0
//! ```
//!
//! # Usage
//!
//! To build dependency recipes for only a specific workspace member, follow this:
//!
//! 1. Prepare a recipe for a single member
//! ```sh
//! cargo chef prepare --recipe-path recipe-bar.json --bin bar
//! ```
//!
//! 2. Reduce the recipe
//! ```sh
//! cargo-reduce-recipe \
//!     --recipe-path-in recipe-bar.json \
//!     --recipe-path-out recipe-bar-reduced.json \
//!     --bin bar
//! ```
//!
//! 3. Cook the reduced recipe
//! ```sh
//! cargo chef cook --release --recipe-path recipe-bar-reduced.json --bin bar
//! ```
//!
//! # Docker
//!
//! `cargo-reduce-recipe` can be used together with `cargo-chef` in a Dockerfile:
//! ```Dockerfile
//! ARG SERVICE_NAME
//!
//! FROM rust:1.88-bookworm AS chef
//! WORKDIR /services
//!
//! # Install cargo-chef and cargo-reduce-recipe
//! RUN cargo install cargo-chef --locked --version 0.1.73 \
//!     && cargo install --git https://github.com/preiter93/cargo-reduce-recipe --tag v0.1.0
//!
//! # Prepare the workspace recipe
//! FROM chef as planner
//! ARG SERVICE_NAME
//! ENV SERVICE_NAME=${SERVICE_NAME}
//! COPY . .
//! RUN cargo chef prepare --recipe-path recipe.json --bin ${SERVICE_NAME} \
//!     && cargo-reduce-recipe --recipe-path-in recipe.json --recipe-path-out recipe-reduced.json --bin ${SERVICE_NAME}
//!
//! # Build the dependencies
//! FROM chef as builder
//! ARG SERVICE_NAME
//! ENV SERVICE_NAME=${SERVICE_NAME}
//! COPY --from=planner /services/recipe-reduced.json recipe-reduced.json
//! RUN cargo chef cook --release --recipe-path recipe-reduced.json --bin ${SERVICE_NAME}
//!
//! # Build the binary
//! COPY . .
//! RUN cargo build --release --bin ${SERVICE_NAME}
//!
//! # Run the service
//! FROM debian:bookworm-slim AS runtime
//! ARG SERVICE_NAME
//! COPY --from=builder /services/target/release/${SERVICE_NAME} /usr/local/bin/main
//! ENTRYPOINT ["/usr/local/bin/main"]
//! ```
use anyhow::{Context, Result};
use chef::{Manifest, Recipe};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};
use toml_edit::{Array, Document, Item};

/// Loads a recipe, reduces it with [`reduce_recipe`] and
/// saves the reduces recipe to a file.
///
/// # Errors
/// - Could not load the file
/// - Could not get root manifest
/// - Could not find root workspace members
/// - Could not find all workspace members
/// - Could not build dependencies
/// - Could not filter manifest
/// - Could not filter lockfile
/// - Could not save the file
pub fn reduce_recipe_file<P: AsRef<Path>>(
    input_path: &P,
    output_path: &P,
    target_member: &str,
) -> Result<()> {
    let recipe = load_recipe(input_path)?;

    let reduced = reduce_recipe(&recipe, target_member)?;

    let out = serde_json::to_string(&reduced).context("failed to serialize reduced recipe")?;
    save_recipe(&out, output_path)
}

/// Reduce a workspace recipe and return it as a JSON string
///
/// - Finds the root workspace members that the recipe should be reduced to
/// - Calculates dependencies and transitive dependencies of the root members
/// - Filters manifest and lockfile
///
/// # Errors
/// - Could not get root manifest
/// - Could not find root workspace members
/// - Could not find all workspace members
/// - Could not build dependencies
/// - Could not filter manifest
/// - Could not filter lockfile
pub fn reduce_recipe(recipe: &Recipe, target_member: &str) -> Result<Recipe> {
    let root_manifest = get_root_manifest(recipe)?;

    let all_members = get_all_workspace_members(recipe);

    let all_ws_deps = get_all_workspace_deps(root_manifest)?;

    let (members_graph, ws_deps_graph) = build_dependencies(recipe, &all_members, &all_ws_deps);

    let keep_members = compute_transitive_deps(target_member, &members_graph);

    let keep_ws_deps = compute_transitive_deps(target_member, &ws_deps_graph);

    let mut reduced = recipe.clone();
    filter_root_members(&mut reduced, target_member)?;

    filter_manifests(&mut reduced, &keep_members);

    filter_lockfile(&mut reduced, &all_members, &keep_members)?;

    filter_lockfile(&mut reduced, &all_ws_deps, &keep_ws_deps)?;

    Ok(reduced)
}

/// Find root manifest
fn get_root_manifest(recipe: &Recipe) -> Result<&Manifest> {
    recipe
        .skeleton
        .manifests
        .iter()
        .find(|m| m.relative_path.to_str() == Some("Cargo.toml"))
        .context("no root Cargo.toml found")
}

/// Extract the root workspace dependencies.
fn get_all_workspace_deps(root: &Manifest) -> Result<HashSet<String>> {
    let doc: Document<String> = root
        .contents
        .parse()
        .context("root Cargo.toml is not valid toml")?;

    let dependencies = doc["workspace"]["dependencies"]
        .as_table()
        .context("[workspace].dependencies must be a table")?;

    Ok(dependencies
        .iter()
        .map(|(name, _)| name.to_string())
        .collect())
}

// Extract all workspace members
fn get_all_workspace_members(recipe: &Recipe) -> HashSet<String> {
    let manifests = &recipe.skeleton.manifests;
    manifests.iter().filter_map(extract_crate_name).collect()
}

/// Build workspace dependency map
fn build_dependencies(
    recipe: &Recipe,
    all_ws_members: &HashSet<String>,
    all_ws_dependencies: &HashSet<String>,
) -> (
    HashMap<String, HashSet<String>>,
    HashMap<String, HashSet<String>>,
) {
    let mut members = HashMap::new();
    let mut dependencies = HashMap::new();

    for manifest in &recipe.skeleton.manifests {
        if let Some(name) = extract_crate_name(manifest) {
            let mut ws_members = HashSet::new();
            let mut ws_dependencies = HashSet::new();
            let doc: Document<String> = match manifest.contents.parse() {
                Ok(d) => d,
                Err(_) => continue,
            };
            for key in ["dependencies", "dev-dependencies"] {
                if let Some(table) = doc.get(key).and_then(|v| v.as_table()) {
                    for (dep_name, _) in table {
                        if all_ws_members.contains(dep_name) {
                            ws_members.insert(dep_name.to_string());
                        }
                        if all_ws_dependencies.contains(dep_name) {
                            ws_dependencies.insert(dep_name.to_string());
                        }
                    }
                }
            }
            members.insert(name.clone(), ws_members);
            dependencies.insert(name, ws_dependencies);
        }
    }

    (members, dependencies)
}

/// Compute all transitive dependencies of the given target member.
fn compute_transitive_deps(
    target: &str,
    deps: &HashMap<String, HashSet<String>>,
) -> HashSet<String> {
    let mut keep = HashSet::new();
    let mut stack = vec![target.to_string()];

    while let Some(member) = stack.pop() {
        if keep.insert(member.clone())
            && let Some(children) = deps.get(&member)
        {
            stack.extend(children.iter().cloned());
        }
    }

    keep
}
/// Filters the root manifest workspace members to keep ony the target member
fn filter_root_members(recipe: &mut Recipe, target: &str) -> Result<()> {
    let root = recipe
        .skeleton
        .manifests
        .iter_mut()
        .find(|m| m.relative_path.to_str() == Some("Cargo.toml"))
        .context("no root Cargo.toml found")?;

    let doc: Document<String> = root
        .contents
        .parse()
        .context("root Cargo.toml is not valid toml")?;
    let mut doc = doc.into_mut();

    let mut arr = Array::new();
    arr.push(target);
    doc["workspace"]["members"] = arr.into();

    root.contents = doc.to_string();

    Ok(())
}

/// Filter manifests to keep only the relevant workspace members
///
/// Keep if:
/// - It's the root Cargo.toml (no package name)
/// - Its crate name is in the keep set
fn filter_manifests(recipe: &mut Recipe, keep_members: &HashSet<String>) {
    recipe
        .skeleton
        .manifests
        .retain(|m| extract_crate_name(m).is_none_or(|name| keep_members.contains(&name)));
}

/// Filter lockfile to keep only relevant dependencies
fn filter_lockfile(
    recipe: &mut Recipe,
    all_members: &HashSet<String>,
    keep_members: &HashSet<String>,
) -> Result<()> {
    if let Some(lock_txt) = &recipe.skeleton.lock_file {
        let doc: Document<String> = lock_txt.parse()?;
        let mut doc = doc.into_mut();

        if let Some(Item::ArrayOfTables(array)) = doc.get_mut("package") {
            array.retain(|pkg| {
                pkg.get("name")
                    .and_then(|v| v.as_str())
                    .is_none_or(|name| !all_members.contains(name) || keep_members.contains(name))
            });
        }

        recipe.skeleton.lock_file = Some(doc.to_string());
    }

    Ok(())
}

/// Extract the crate name from a manifest
fn extract_crate_name(manifest: &Manifest) -> Option<String> {
    let doc: Document<String> = manifest.contents.parse().ok()?;
    doc.get("package")?
        .get("name")?
        .as_str()
        .map(ToOwned::to_owned)
}

/// Load recipe
fn load_recipe<P: AsRef<Path>>(path: P) -> Result<Recipe> {
    let input = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.as_ref().display()))?;
    let recipe: Recipe = serde_json::from_str(&input).context("Failed to parse recipe.json")?;
    Ok(recipe)
}

/// Save the reduced recipe
fn save_recipe<P: AsRef<Path>>(json: &str, path: P) -> Result<()> {
    fs::write(&path, json)
        .with_context(|| format!("Failed to write {}", path.as_ref().display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reduce_recipe_without_member_dependency() -> Result<()> {
        let given_path = "test-data/recipes/recipe-bar.json";
        let want_path = "test-data/recipes/recipe-bar-reduced.json";

        let recipe = load_recipe(given_path)?;
        let reduced = reduce_recipe(&recipe, "bar")?;

        let want_reduced = load_recipe(want_path)?;

        assert_eq!(
            reduced, want_reduced,
            "reduced recipe does not match expected output"
        );
        Ok(())
    }

    #[test]
    fn test_reduce_recipe_with_member_dependency() -> Result<()> {
        let given_path = "test-data/recipes/recipe-foo.json";
        let want_path = "test-data/recipes/recipe-foo-reduced.json";

        let recipe = load_recipe(given_path)?;
        let reduced = reduce_recipe(&recipe, "foo")?;

        let want_reduced = load_recipe(want_path)?;

        assert_eq!(
            reduced, want_reduced,
            "Reduced recipe does not match expected output"
        );
        Ok(())
    }
}
