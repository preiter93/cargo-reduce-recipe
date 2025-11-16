use anyhow::{Context, Result};
use chef::{Manifest, Recipe};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};
use toml_edit::{Document, Item};

/// Loads a recipe, reduces it with [`reduce_workspace_recipe`] and
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
pub fn reduce_workspace_recipe_file<P: AsRef<Path>>(input_path: &P, output_path: &P) -> Result<()> {
    let recipe = load_recipe(input_path)?;

    let reduced = reduce_workspace_recipe(&recipe)?;

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
pub fn reduce_workspace_recipe(recipe: &Recipe) -> Result<Recipe> {
    let root_manifest = get_root_manifest(recipe)?;

    let root_members = get_root_workspace_members(root_manifest)?;

    let all_members = get_all_workspace_members(recipe);

    let dependencies = build_workspace_deps(recipe, &all_members);

    let keep_members = compute_transitive_members(&root_members, &dependencies);

    let mut reduced = recipe.clone();
    filter_manifests(&mut reduced, &keep_members);

    filter_lockfile_members(&mut reduced, &all_members, &keep_members)?;

    Ok(reduced)
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

/// Extract the root workspace members that the recipe will be reduce to
fn get_root_workspace_members(root: &Manifest) -> Result<HashSet<String>> {
    let doc: Document<String> = root
        .contents
        .parse()
        .context("root Cargo.toml is not valid toml")?;

    let members = doc["workspace"]["members"]
        .as_array()
        .context("[workspace].members must be an array")?;

    Ok(members
        .iter()
        .filter_map(|x| x.as_str().map(ToString::to_string))
        .collect())
}

// Extract all workspace members
fn get_all_workspace_members(recipe: &Recipe) -> HashSet<String> {
    let manifests = &recipe.skeleton.manifests;
    manifests.iter().filter_map(extract_crate_name).collect()
}

/// Build workspace dependency map
fn build_workspace_deps(
    recipe: &Recipe,
    all_members: &HashSet<String>,
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
                for (dep_name, _) in table {
                    if all_members.contains(dep_name) {
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
    root_members: &HashSet<String>,
    deps: &HashMap<String, HashSet<String>>,
) -> HashSet<String> {
    let mut keep = HashSet::new();
    let mut stack: Vec<&String> = root_members.iter().collect();

    while let Some(member) = stack.pop() {
        if keep.insert(member.clone())
            && let Some(ds) = deps.get(member)
        {
            stack.extend(ds.iter());
        }
    }

    keep
}

/// Filter manifests to keep only the workspace members we want
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

/// Filter lockfile to keep only dependencies we want
fn filter_lockfile_members(
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
    fn test_do_not_reduce_full_member_recipe() -> Result<()> {
        let given_path = "test-files/recipe.json";
        let want_path = "test-files/recipe.json";

        let recipe = load_recipe(given_path)?;
        let reduced = reduce_workspace_recipe(&recipe)?;

        let want_reduced = load_recipe(want_path)?;

        assert_eq!(
            reduced, want_reduced,
            "reduced recipe does not match expected output"
        );
        Ok(())
    }

    #[test]
    fn test_reduce_recipe_without_member_dependency() -> Result<()> {
        let given_path = "test-files/given-recipe-bar.json";
        let want_path = "test-files/want-recipe-bar.json";

        let recipe = load_recipe(given_path)?;
        let reduced = reduce_workspace_recipe(&recipe)?;

        let want_reduced = load_recipe(want_path)?;

        assert_eq!(
            reduced, want_reduced,
            "reduced recipe does not match expected output"
        );
        Ok(())
    }

    #[test]
    fn test_reduce_recipe_with_member_dependency() -> Result<()> {
        let given_path = "test-files/given-recipe-baz.json";
        let want_path = "test-files/want-recipe-baz.json";

        let recipe = load_recipe(given_path)?;
        let reduced = reduce_workspace_recipe(&recipe)?;

        let want_reduced = load_recipe(want_path)?;

        assert_eq!(
            reduced, want_reduced,
            "Reduced recipe does not match expected output"
        );
        Ok(())
    }
}
