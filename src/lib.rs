use anyhow::{Context, Result};
use chef::{Manifest, Recipe};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};
use toml_edit::{Document, Item};

/// Reduce a recipe and return it as a JSON string
pub fn reduce_workspace_recipe_string(recipe: &Recipe) -> Result<String> {
    // Find root Cargo.toml
    let root_manifest = get_root_manifest(recipe)?;

    // Extract the single workspace member
    let main_member = get_single_workspace_member(root_manifest)?;

    // Extract all workspace members
    let all_members = get_all_workspace_members(recipe);

    // Build workspace dependency map
    let dependencies = build_workspace_deps(&recipe, &all_members);

    // Compute transitive members
    let keep_members = compute_transitive_members(&main_member, &dependencies);

    // Filter manifests
    let mut reduced = recipe.clone();
    filter_manifests(&mut reduced, &keep_members);

    // Filter lockfile
    filter_lockfile_members(&mut reduced, &all_members, &keep_members)?;

    let out = serde_json::to_string(&reduced).context("failed to serialize reduced recipe")?;
    Ok(out)
}

/// Reduce a recipe and save it to a file
pub fn reduce_workspace_recipe_file<P: AsRef<Path>>(input_path: &P, output_path: &P) -> Result<()> {
    // Load the recipe
    let recipe = load_recipe(input_path)?;

    // Reduce it and get the string
    let reduced = reduce_workspace_recipe_string(&recipe)?;

    // Save using save_recipe
    save_recipe(&reduced, output_path)
}

/// Find root Cargo.toml
fn get_root_manifest(recipe: &Recipe) -> Result<&Manifest> {
    recipe
        .skeleton
        .manifests
        .iter()
        .find(|m| m.relative_path.to_str() == Some("Cargo.toml"))
        .context("no root Cargo.toml found")
}

/// Extract the single main workspace member
fn get_single_workspace_member(root: &Manifest) -> Result<String> {
    let doc: Document<String> = root
        .contents
        .parse()
        .context("root Cargo.toml is not valid TOML")?;
    let members = doc["workspace"]["members"]
        .as_array()
        .context("[workspace].members must be an array")?;

    if members.len() != 1 {
        anyhow::bail!(
            "expected exactly one workspace member, found {}",
            members.len()
        );
    }

    let member_name = members
        .get(0)
        .and_then(|v| v.as_str())
        .context("workspace member must be a string")?;
    Ok(member_name.to_string())
}

/// Returns all workspace members
fn get_all_workspace_members(recipe: &Recipe) -> HashSet<String> {
    recipe
        .skeleton
        .manifests
        .iter()
        .filter_map(|m| extract_crate_name(m))
        .collect()
}

/// Build workspace dependency map
fn build_workspace_deps(
    recipe: &Recipe,
    all_ws_members: &HashSet<String>,
) -> HashMap<String, HashSet<String>> {
    let mut map = HashMap::new();

    for manifest in &recipe.skeleton.manifests {
        if let Some(name) = extract_crate_name(manifest) {
            let mut deps = HashSet::new();
            let doc: Document<String> = match manifest.contents.parse() {
                Ok(d) => d,
                Err(_) => continue,
            };
            if let Some(table) = doc.get("dependencies").and_then(|v| v.as_table()) {
                for (dep_name, _) in table.iter() {
                    if all_ws_members.contains(dep_name) {
                        deps.insert(dep_name.to_string());
                    }
                }
            }
            map.insert(name, deps);
        }
    }

    map
}

/// Compute transitive dependencies of workspace members
fn compute_transitive_members(
    start: &str,
    deps: &HashMap<String, HashSet<String>>,
) -> HashSet<String> {
    let mut keep = HashSet::new();
    let mut stack = vec![start.to_string()];

    while let Some(member) = stack.pop() {
        if keep.insert(member.clone()) {
            if let Some(ds) = deps.get(&member) {
                stack.extend(ds.iter().cloned());
            }
        }
    }

    keep
}

/// Filter manifests to keep only the workspace members we want
fn filter_manifests(recipe: &mut Recipe, keep_ws_members: &HashSet<String>) {
    recipe.skeleton.manifests.retain(|m| {
        // Keep if:
        // - It's the root Cargo.toml (no package name)
        // - OR its crate name is in the keep set
        extract_crate_name(m).map_or(true, |name| keep_ws_members.contains(&name))
    });
}

/// Filter lockfile to keep only dependencies we want
fn filter_lockfile_members(
    recipe: &mut Recipe,
    all_ws_members: &HashSet<String>,
    keep_ws_members: &HashSet<String>,
) -> Result<()> {
    if let Some(lock_txt) = &recipe.skeleton.lock_file {
        let doc: Document<String> = lock_txt.parse()?;
        let mut doc = doc.into_mut();

        if let Some(Item::ArrayOfTables(array)) = doc.get_mut("package") {
            array.retain(|pkg| {
                pkg.get("name")
                    .and_then(|v| v.as_str())
                    .map(|name| !all_ws_members.contains(name) || keep_ws_members.contains(name))
                    .unwrap_or(true)
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
        let given_path = "test-files/given-recipe-bar.json";
        let want_path = "test-files/want-recipe-bar.json";

        let recipe = load_recipe(given_path)?;
        let reduced = reduce_workspace_recipe_string(&recipe)?;

        let want_recipe = load_recipe(want_path)?;
        let want = serde_json::to_string(&want_recipe)?;

        assert_eq!(
            reduced, want,
            "reduced recipe does not match expected output"
        );
        Ok(())
    }

    #[test]
    fn test_reduce_recipe_with_member_dependency() -> Result<()> {
        let given_path = "test-files/given-recipe-baz.json";
        let want_path = "test-files/want-recipe-baz.json";

        let recipe = load_recipe(given_path)?;
        let reduced = reduce_workspace_recipe_string(&recipe)?;

        let want_recipe = load_recipe(want_path)?;
        let want = serde_json::to_string(&want_recipe)?;

        assert_eq!(
            reduced, want,
            "Reduced recipe does not match expected output"
        );
        Ok(())
    }
}
