use std::collections::BTreeSet;
use std::path::{Component, Path};

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    version: u32,
    invariant: Vec<Invariant>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Invariant {
    id: String,
    target: String,
    enforcement: Vec<Enforcement>,
    artifacts: Vec<String>,
    trust_boundaries: Vec<TrustBoundary>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Enforcement {
    Type,
    Const,
    Lint,
    Kani,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum TrustBoundary {
    RustCompiler,
    StandardLibrary,
    WindowsApi,
    UnsafePseudoconsoleImplementor,
}

pub(crate) fn check(root: &Path) -> Result<(), String> {
    let path = root.join("quality").join("invariants.toml");
    let source = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let registry: Registry =
        toml::from_str(&source).map_err(|error| format!("invalid invariant registry: {error}"))?;
    validate(root, &registry)
}

fn validate(root: &Path, registry: &Registry) -> Result<(), String> {
    if registry.version != 1 {
        return Err(format!(
            "unsupported invariant registry version: {}",
            registry.version
        ));
    }
    if registry.invariant.is_empty() {
        return Err("invariant registry must not be empty".to_owned());
    }
    let mut ids = BTreeSet::new();
    for invariant in &registry.invariant {
        if !valid_id(&invariant.id) {
            return Err(format!("invalid invariant id: {}", invariant.id));
        }
        if !ids.insert(invariant.id.as_str()) {
            return Err(format!("duplicate invariant id: {}", invariant.id));
        }
        if invariant.target.trim().is_empty() {
            return Err(format!("{} has an empty target", invariant.id));
        }
        if invariant.enforcement.is_empty() {
            return Err(format!("{} has no static enforcement", invariant.id));
        }
        if invariant.artifacts.is_empty() {
            return Err(format!("{} has no evidence artifact", invariant.id));
        }
        if invariant.trust_boundaries.is_empty() {
            return Err(format!("{} has no trust boundary", invariant.id));
        }
        for artifact in &invariant.artifacts {
            validate_artifact(root, &invariant.id, artifact)?;
        }
    }
    Ok(())
}

fn valid_id(id: &str) -> bool {
    id.len() == 7 && id.starts_with("INV-") && id.as_bytes()[4..].iter().all(u8::is_ascii_digit)
}

fn validate_artifact(root: &Path, id: &str, artifact: &str) -> Result<(), String> {
    let path = Path::new(artifact);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        return Err(format!("{id} has an invalid artifact path: {artifact}"));
    }
    if !root.join(path).exists() {
        return Err(format!("{id} artifact does not exist: {artifact}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforcement_and_trust_boundaries_are_closed_sets() {
        let forbidden = r#"
version = 1
[[invariant]]
id = "INV-001"
target = "example"
enforcement = ["comment"]
artifacts = ["Cargo.toml"]
trust_boundaries = ["reviewer"]
"#;
        assert!(toml::from_str::<Registry>(forbidden).is_err());
    }

    #[test]
    fn ids_have_a_canonical_shape() {
        assert!(valid_id("INV-001"));
        assert!(!valid_id("INV-1"));
        assert!(!valid_id("inv-001"));
    }
}
