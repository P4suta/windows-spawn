use std::collections::BTreeSet;
use std::path::{Component, Path};

use serde::Deserialize;
use syn::visit::{self, Visit};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    version: u32,
    invariant: Vec<Invariant>,
    #[serde(default)]
    mutation_exemption: Vec<MutationExemption>,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MutationExemption {
    matcher: String,
    source: String,
    expression: String,
    proof: String,
    invariant: String,
}

#[derive(Deserialize)]
struct MutantsConfig {
    #[serde(default)]
    exclude_re: Vec<String>,
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
    validate(root, &registry)?;
    validate_mutation_exemptions(root, &registry)
}

fn validate_mutation_exemptions(root: &Path, registry: &Registry) -> Result<(), String> {
    let config_path = root.join(".cargo").join("mutants.toml");
    let config_source = std::fs::read_to_string(&config_path)
        .map_err(|error| format!("failed to read {}: {error}", config_path.display()))?;
    let config: MutantsConfig = toml::from_str(&config_source)
        .map_err(|error| format!("invalid cargo-mutants config: {error}"))?;
    let configured: BTreeSet<&str> = config.exclude_re.iter().map(String::as_str).collect();
    let registered: BTreeSet<&str> = registry
        .mutation_exemption
        .iter()
        .map(|entry| entry.matcher.as_str())
        .collect();
    if configured != registered {
        return Err("cargo-mutants exclusions must exactly match the proof registry".to_owned());
    }
    let kani_invariants: BTreeSet<&str> = registry
        .invariant
        .iter()
        .filter(|entry| {
            entry
                .enforcement
                .iter()
                .any(|value| matches!(value, Enforcement::Kani))
        })
        .map(|entry| entry.id.as_str())
        .collect();
    let mut matchers = BTreeSet::new();
    for exemption in &registry.mutation_exemption {
        if !matchers.insert(exemption.matcher.as_str()) {
            return Err(format!(
                "duplicate mutation exemption: {}",
                exemption.matcher
            ));
        }
        if !exemption.matcher.starts_with('^') || !exemption.matcher.ends_with('$') {
            return Err(format!(
                "mutation exemption is not anchored: {}",
                exemption.matcher
            ));
        }
        if !kani_invariants.contains(exemption.invariant.as_str()) {
            return Err(format!(
                "mutation exemption does not name a Kani invariant: {}",
                exemption.invariant
            ));
        }
        validate_artifact(root, &exemption.invariant, &exemption.source)?;
        let source_path = root.join(&exemption.source);
        let source = std::fs::read_to_string(&source_path)
            .map_err(|error| format!("failed to read {}: {error}", source_path.display()))?;
        if !source.contains(&exemption.expression) {
            return Err(format!(
                "mutation exemption expression is absent from {}: {}",
                exemption.source, exemption.expression
            ));
        }
        let syntax = syn::parse_file(&source)
            .map_err(|error| format!("failed to parse {}: {error}", exemption.source))?;
        let mut proofs = KaniProofs::default();
        proofs.visit_file(&syntax);
        if !proofs.names.contains(exemption.proof.as_str()) {
            return Err(format!(
                "mutation exemption proof is not a Kani harness: {}",
                exemption.proof
            ));
        }
    }
    Ok(())
}

#[derive(Default)]
struct KaniProofs {
    names: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for KaniProofs {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let is_proof = item.attrs.iter().any(|attribute| {
            let mut segments = attribute.path().segments.iter();
            matches!(segments.next(), Some(segment) if segment.ident == "kani")
                && matches!(segments.next(), Some(segment) if segment.ident == "proof")
                && segments.next().is_none()
        });
        if is_proof {
            self.names.insert(item.sig.ident.to_string());
        }
        visit::visit_item_fn(self, item);
    }
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
