use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use syn::visit::{self, Visit};
use syn::{
    Attribute, Expr, FnArg, Item, Meta, PathArguments, ReturnType, Signature, Type, UseTree,
};

pub(crate) fn check(root: &Path) -> Result<(), String> {
    let tracked_paths = tracked_rust_files(root)?;
    let mut report = String::new();
    for path in &tracked_paths {
        let source = fs::read_to_string(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let syntax = syn::parse_file(&source)
            .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
        let relative = path
            .strip_prefix(root)
            .map_err(|error| format!("failed to relativize {}: {error}", path.display()))?;
        for violation in inspect_structural(&syntax) {
            let _ = writeln!(report, "{}: {violation}", relative.display());
        }
    }

    let source_root = root.join("src");
    let mut paths = Vec::new();
    collect_rust_files(&source_root, &mut paths)?;
    paths.sort();
    for path in paths {
        let source = fs::read_to_string(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let syntax = syn::parse_file(&source)
            .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
        let relative = path
            .strip_prefix(root)
            .map_err(|error| format!("failed to relativize {}: {error}", path.display()))?;
        let raw_boundary = relative == Path::new("src").join("sys.rs");
        let pseudoconsole_boundary = relative == Path::new("src").join("handles.rs");
        let handle_value_boundary = relative == Path::new("src").join("resource.rs");
        let violations = inspect(
            &syntax,
            raw_boundary,
            pseudoconsole_boundary,
            handle_value_boundary,
        );
        for violation in violations {
            let _ = writeln!(report, "{}: {violation}", relative.display());
        }
    }
    for (path, dependency) in forbidden_manifest_dependencies(root)? {
        let _ = writeln!(
            report,
            "{}: dependency {dependency} is forbidden by the static-dispatch policy",
            path.display()
        );
    }
    if report.is_empty() {
        Ok(())
    } else {
        Err(report)
    }
}

fn tracked_rust_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths: Vec<PathBuf> = tracked_files(root)?
        .into_iter()
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("rs"))
        .collect();
    paths.sort();
    Ok(paths)
}

fn tracked_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "-z"])
        .output()
        .map_err(|error| format!("failed to enumerate tracked files: {error}"))?;
    if !output.status.success() {
        return Err(format!("git ls-files failed with status {}", output.status));
    }
    let mut paths = Vec::new();
    for encoded in output.stdout.split(|byte| *byte == 0) {
        if encoded.is_empty() {
            continue;
        }
        let relative = std::str::from_utf8(encoded)
            .map_err(|error| format!("tracked path is not UTF-8: {error}"))?;
        paths.push(root.join(relative));
    }
    paths.sort();
    Ok(paths)
}

fn forbidden_manifest_dependencies(root: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    let mut violations = Vec::new();
    for path in tracked_files(root)?
        .into_iter()
        .filter(|path| path.file_name().is_some_and(|name| name == "Cargo.toml"))
    {
        let source = fs::read_to_string(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let manifest: toml::Value = toml::from_str(&source)
            .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
        for dependency in forbidden_dependencies(&manifest) {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| format!("failed to relativize {}: {error}", path.display()))?;
            violations.push((relative.to_path_buf(), dependency));
        }
    }
    Ok(violations)
}

fn forbidden_dependencies(manifest: &toml::Value) -> BTreeSet<String> {
    let mut dependencies = BTreeSet::new();
    collect_forbidden_dependencies(manifest, &mut dependencies);
    dependencies
}

fn collect_forbidden_dependencies(value: &toml::Value, dependencies: &mut BTreeSet<String>) {
    let Some(table) = value.as_table() else {
        return;
    };
    for (name, child) in table {
        if matches!(
            name.as_str(),
            "dependencies" | "dev-dependencies" | "build-dependencies"
        ) {
            if let Some(entries) = child.as_table() {
                for (dependency, specification) in entries {
                    let package = specification
                        .as_table()
                        .and_then(|table| table.get("package"))
                        .and_then(toml::Value::as_str)
                        .unwrap_or(dependency);
                    if forbidden_dependency(package) {
                        dependencies.insert(package.to_owned());
                    }
                }
            }
        }
        collect_forbidden_dependencies(child, dependencies);
    }
}

fn forbidden_dependency(name: &str) -> bool {
    matches!(
        name,
        "anyhow"
            | "arc-swap"
            | "async-trait"
            | "color-eyre"
            | "dashmap"
            | "downcast-rs"
            | "erased-serde"
            | "eyre"
            | "inventory"
            | "lazy_static"
            | "linkme"
            | "once_cell"
            | "parking_lot"
            | "serde_traitobject"
            | "typetag"
    )
}

fn collect_rust_files(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("failed to read {}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("failed to enumerate {}: {error}", directory.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
        if file_type.is_dir() {
            collect_rust_files(&path, paths)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
            paths.push(path);
        }
    }
    Ok(())
}

fn inspect(
    file: &syn::File,
    raw_boundary: bool,
    pseudoconsole_boundary: bool,
    handle_value_boundary: bool,
) -> BTreeSet<&'static str> {
    let mut inspector = Inspector {
        raw_boundary,
        pseudoconsole_boundary,
        handle_value_boundary,
        violations: BTreeSet::new(),
    };
    inspector.check_attributes(&file.attrs, raw_boundary);
    inspector.visit_file(file);
    inspector.violations
}

fn inspect_structural(file: &syn::File) -> BTreeSet<&'static str> {
    let mut inspector = StructuralInspector {
        error_impl: false,
        error_source_method: false,
        error_source_return: false,
        violations: BTreeSet::new(),
    };
    inspector.visit_file(file);
    inspector.violations
}

struct StructuralInspector {
    error_impl: bool,
    error_source_method: bool,
    error_source_return: bool,
    violations: BTreeSet<&'static str>,
}

impl StructuralInspector {
    fn reject(&mut self, message: &'static str) {
        self.violations.insert(message);
    }
}

impl<'ast> Visit<'ast> for StructuralInspector {
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let previous = self.error_impl;
        self.error_impl = item
            .trait_
            .as_ref()
            .is_some_and(|(_, path, _)| is_standard_error_path(path));
        visit::visit_item_impl(self, item);
        self.error_impl = previous;
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        let previous = self.error_source_method;
        self.error_source_method = self.error_impl && item.sig.ident == "source";
        visit::visit_impl_item_fn(self, item);
        self.error_source_method = previous;
    }

    fn visit_return_type(&mut self, value: &'ast ReturnType) {
        let previous = self.error_source_return;
        self.error_source_return = self.error_source_method;
        visit::visit_return_type(self, value);
        self.error_source_return = previous;
    }

    fn visit_type_trait_object(&mut self, value: &'ast syn::TypeTraitObject) {
        if !self.error_source_return {
            self.reject("trait objects are forbidden outside std::error::Error::source");
        }
        visit::visit_type_trait_object(self, value);
    }

    fn visit_type_path(&mut self, value: &'ast syn::TypePath) {
        if let Some(segment) = value.path.segments.last() {
            let name = segment.ident.to_string();
            if matches!(name.as_str(), "Box" | "Arc" | "Rc")
                && arguments_contain_trait_object(&segment.arguments)
            {
                self.reject("owned trait objects are forbidden");
            }
            if matches!(name.as_str(), "Any" | "TypeId") {
                self.reject("runtime type reflection is forbidden");
            }
        }
        visit::visit_type_path(self, value);
    }

    fn visit_macro(&mut self, value: &'ast syn::Macro) {
        if token_stream_contains_ident(&value.tokens, "dyn") {
            self.reject("trait objects hidden in macro tokens are forbidden");
        }
        visit::visit_macro(self, value);
    }
}

fn is_standard_error_path(path: &syn::Path) -> bool {
    let segments: Vec<String> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    segments == ["std", "error", "Error"] || segments == ["core", "error", "Error"]
}

fn token_stream_contains_ident(stream: &proc_macro2::TokenStream, expected: &str) -> bool {
    stream.clone().into_iter().any(|token| match token {
        proc_macro2::TokenTree::Group(group) => {
            token_stream_contains_ident(&group.stream(), expected)
        }
        proc_macro2::TokenTree::Ident(identifier) => identifier == expected,
        proc_macro2::TokenTree::Literal(_) | proc_macro2::TokenTree::Punct(_) => false,
    })
}

fn arguments_contain_trait_object(arguments: &PathArguments) -> bool {
    let PathArguments::AngleBracketed(arguments) = arguments else {
        return false;
    };
    arguments.args.iter().any(|argument| {
        let syn::GenericArgument::Type(value) = argument else {
            return false;
        };
        let mut finder = TraitObjectFinder { found: false };
        finder.visit_type(value);
        finder.found
    })
}

struct TraitObjectFinder {
    found: bool,
}

impl<'ast> Visit<'ast> for TraitObjectFinder {
    fn visit_type_trait_object(&mut self, _value: &'ast syn::TypeTraitObject) {
        self.found = true;
    }
}

struct Inspector {
    raw_boundary: bool,
    pseudoconsole_boundary: bool,
    handle_value_boundary: bool,
    violations: BTreeSet<&'static str>,
}

impl Inspector {
    fn reject(&mut self, message: &'static str) {
        self.violations.insert(message);
    }

    fn check_attributes(&mut self, attributes: &[Attribute], permits_unsafe_allow: bool) {
        for attribute in attributes {
            if attribute
                .path()
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "async_trait")
            {
                self.reject("async trait erasure is forbidden");
            }
            if attribute.path().is_ident("allow") || attribute.path().is_ident("expect") {
                let permitted = permits_unsafe_allow
                    && attribute.path().is_ident("allow")
                    && attribute_tokens(attribute) == "unsafe_code";
                if !permitted {
                    self.reject("lint suppression is forbidden");
                }
            }
            if attribute.path().is_ident("cfg_attr") {
                let tokens = attribute_tokens(attribute);
                if tokens.contains("allow") || tokens.contains("expect") {
                    self.reject("conditional lint suppression is forbidden");
                }
            }
        }
    }

    fn check_signature(&mut self, signature: &Signature) {
        for input in &signature.inputs {
            if let FnArg::Typed(input) = input {
                if is_named_type(&input.ty, "bool") {
                    self.reject("semantic bool parameter is forbidden");
                }
            }
        }
        let allowed_unsafe =
            self.raw_boundary || (self.pseudoconsole_boundary && signature.ident == "from_raw");
        if signature.unsafety.is_some() && !allowed_unsafe {
            self.reject("unsafe function outside a declared boundary");
        }
    }

    fn check_item(&mut self, item: &Item) {
        let attributes = match item {
            Item::Const(value) => &value.attrs,
            Item::Enum(value) => &value.attrs,
            Item::ExternCrate(value) => &value.attrs,
            Item::Fn(value) => &value.attrs,
            Item::ForeignMod(value) => &value.attrs,
            Item::Impl(value) => &value.attrs,
            Item::Macro(value) => &value.attrs,
            Item::Mod(value) => &value.attrs,
            Item::Static(value) => &value.attrs,
            Item::Struct(value) => &value.attrs,
            Item::Trait(value) => &value.attrs,
            Item::TraitAlias(value) => &value.attrs,
            Item::Type(value) => &value.attrs,
            Item::Union(value) => &value.attrs,
            Item::Use(value) => &value.attrs,
            _ => return,
        };
        self.check_attributes(attributes, self.raw_boundary);
    }
}

impl<'ast> Visit<'ast> for Inspector {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_attributes(item).is_some_and(is_test_only) {
            return;
        }
        if !matches!(
            item,
            Item::Fn(_) | Item::Impl(_) | Item::Mod(_) | Item::Trait(_)
        ) {
            self.check_item(item);
        }
        visit::visit_item(self, item);
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if is_test_only(&item.attrs) {
            return;
        }
        let permits_unsafe_allow = self.raw_boundary || item.ident == "sys";
        self.check_attributes(&item.attrs, permits_unsafe_allow);
        visit::visit_item_mod(self, item);
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if is_test_only(&item.attrs) {
            return;
        }
        self.check_attributes(
            &item.attrs,
            self.raw_boundary || item.sig.unsafety.is_some(),
        );
        self.check_signature(&item.sig);
        visit::visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if is_test_only(&item.attrs) {
            return;
        }
        self.check_attributes(
            &item.attrs,
            self.raw_boundary || item.sig.unsafety.is_some(),
        );
        self.check_signature(&item.sig);
        visit::visit_impl_item_fn(self, item);
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        self.check_attributes(
            &item.attrs,
            self.raw_boundary || item.sig.unsafety.is_some(),
        );
        self.check_signature(&item.sig);
        visit::visit_trait_item_fn(self, item);
    }

    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        if is_test_only(&item.attrs) {
            return;
        }
        self.check_attributes(&item.attrs, self.raw_boundary || item.unsafety.is_some());
        if item.unsafety.is_some() && !self.raw_boundary {
            let allowed = self.pseudoconsole_boundary && item.ident == "AsPseudoConsole";
            if !allowed {
                self.reject("unsafe trait outside a declared boundary");
            }
        }
        visit::visit_item_trait(self, item);
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if is_test_only(&item.attrs) {
            return;
        }
        self.check_attributes(&item.attrs, self.raw_boundary || item.unsafety.is_some());
        if item.unsafety.is_some() && !self.raw_boundary {
            self.reject("unsafe impl outside a declared boundary");
        }
        visit::visit_item_impl(self, item);
    }

    fn visit_field(&mut self, field: &'ast syn::Field) {
        self.check_attributes(&field.attrs, self.raw_boundary);
        if is_named_type(&field.ty, "bool") {
            self.reject("semantic bool field is forbidden");
        }
        visit::visit_field(self, field);
    }

    fn visit_expr_method_call(&mut self, expression: &'ast syn::ExprMethodCall) {
        match expression.method.to_string().as_str() {
            "unwrap" => self.reject("unwrap is forbidden in production code"),
            "expect" => self.reject("expect is forbidden in production code"),
            _ => {}
        }
        visit::visit_expr_method_call(self, expression);
    }

    fn visit_expr_macro(&mut self, expression: &'ast syn::ExprMacro) {
        if let Some(name) = expression.mac.path.segments.last() {
            match name.ident.to_string().as_str() {
                "panic" | "todo" | "unimplemented" | "unreachable" => {
                    self.reject("panic path is forbidden in production code");
                }
                _ => {}
            }
        }
        visit::visit_expr_macro(self, expression);
    }

    fn visit_macro(&mut self, value: &'ast syn::Macro) {
        if let Some(name) = value.path.segments.last() {
            match name.ident.to_string().as_str() {
                "dbg" | "eprint" | "eprintln" | "lazy_static" | "print" | "println"
                | "thread_local" => {
                    self.reject("ad hoc diagnostics or global state macro is forbidden");
                }
                _ => {}
            }
        }
        visit::visit_macro(self, value);
    }

    fn visit_expr_call(&mut self, expression: &'ast syn::ExprCall) {
        if let Expr::Path(path) = expression.func.as_ref() {
            let segments: Vec<String> = path
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            if segments.ends_with(&["mem".to_owned(), "forget".to_owned()]) {
                self.reject("mem::forget is forbidden in production code");
            }
        }
        visit::visit_expr_call(self, expression);
    }

    fn visit_expr_index(&mut self, expression: &'ast syn::ExprIndex) {
        self.reject("unchecked indexing is forbidden in production code");
        visit::visit_expr_index(self, expression);
    }

    fn visit_expr_cast(&mut self, expression: &'ast syn::ExprCast) {
        if !self.raw_boundary {
            self.reject("unchecked cast outside the raw adapter is forbidden");
        }
        visit::visit_expr_cast(self, expression);
    }

    fn visit_expr_unsafe(&mut self, expression: &'ast syn::ExprUnsafe) {
        if !self.raw_boundary {
            self.reject("unsafe block outside the raw adapter is forbidden");
        }
        visit::visit_expr_unsafe(self, expression);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        if use_tree_contains_glob(&item.tree) {
            self.reject("wildcard import is forbidden in production code");
        }
        if use_tree_contains_forbidden_type(&item.tree) {
            self.reject(
                "runtime erasure nondeterminism or shared interior mutability is forbidden",
            );
        }
        visit::visit_item_use(self, item);
    }

    fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
        if let Some(segment) = path.path.segments.last() {
            let name = segment.ident.to_string();
            let raw_representation_allowed = self.raw_boundary || self.handle_value_boundary;
            if !raw_representation_allowed
                && matches!(name.as_str(), "isize" | "RawHandle" | "HANDLE")
            {
                self.reject("raw handle representation outside the raw adapter is forbidden");
            }
            if forbidden_runtime_type(&name) {
                self.reject(
                    "runtime erasure nondeterminism or shared interior mutability is forbidden",
                );
            }
        }
        visit::visit_type_path(self, path);
    }
}

fn use_tree_contains_glob(tree: &UseTree) -> bool {
    match tree {
        UseTree::Glob(_) => true,
        UseTree::Group(group) => group.items.iter().any(use_tree_contains_glob),
        UseTree::Name(_) | UseTree::Rename(_) => false,
        UseTree::Path(path) => use_tree_contains_glob(&path.tree),
    }
}

fn use_tree_contains_forbidden_type(tree: &UseTree) -> bool {
    match tree {
        UseTree::Glob(_) => false,
        UseTree::Group(group) => group.items.iter().any(use_tree_contains_forbidden_type),
        UseTree::Name(name) => forbidden_runtime_type(&name.ident.to_string()),
        UseTree::Path(path) => {
            forbidden_runtime_type(&path.ident.to_string())
                || use_tree_contains_forbidden_type(&path.tree)
        }
        UseTree::Rename(rename) => forbidden_runtime_type(&rename.ident.to_string()),
    }
}

fn forbidden_runtime_type(name: &str) -> bool {
    matches!(
        name,
        "Any"
            | "Arc"
            | "Barrier"
            | "Cell"
            | "Condvar"
            | "HashMap"
            | "HashSet"
            | "LazyCell"
            | "LazyLock"
            | "ManuallyDrop"
            | "MaybeUninit"
            | "Mutex"
            | "Once"
            | "OnceCell"
            | "OnceLock"
            | "Rc"
            | "RefCell"
            | "RwLock"
            | "TypeId"
            | "UnsafeCell"
            | "Weak"
    ) || name.starts_with("Atomic")
}

fn item_attributes(item: &Item) -> Option<&[Attribute]> {
    match item {
        Item::Const(value) => Some(&value.attrs),
        Item::Enum(value) => Some(&value.attrs),
        Item::ExternCrate(value) => Some(&value.attrs),
        Item::Fn(value) => Some(&value.attrs),
        Item::ForeignMod(value) => Some(&value.attrs),
        Item::Impl(value) => Some(&value.attrs),
        Item::Macro(value) => Some(&value.attrs),
        Item::Mod(value) => Some(&value.attrs),
        Item::Static(value) => Some(&value.attrs),
        Item::Struct(value) => Some(&value.attrs),
        Item::Trait(value) => Some(&value.attrs),
        Item::TraitAlias(value) => Some(&value.attrs),
        Item::Type(value) => Some(&value.attrs),
        Item::Union(value) => Some(&value.attrs),
        Item::Use(value) => Some(&value.attrs),
        _ => None,
    }
}

fn is_test_only(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("cfg") && attribute_tokens(attribute).replace(' ', "") == "test"
    })
}

fn attribute_tokens(attribute: &Attribute) -> String {
    match &attribute.meta {
        Meta::List(list) => list.tokens.to_string(),
        Meta::NameValue(_) | Meta::Path(_) => String::new(),
    }
}

fn is_named_type(value: &Type, expected: &str) -> bool {
    let Type::Path(path) = value else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn violations(source: &str) -> BTreeSet<&'static str> {
        inspect(&syn::parse_file(source).unwrap(), false, false, false)
    }

    fn structural_violations(source: &str) -> BTreeSet<&'static str> {
        inspect_structural(&syn::parse_file(source).unwrap())
    }

    #[test]
    fn accepts_closed_inputs_and_checked_access() {
        let source = "enum Policy { On, Off } fn apply(value: Policy, items: &[u8]) -> Option<&u8> { items.get(0) }";
        assert!(violations(source).is_empty());
    }

    #[test]
    fn rejects_forbidden_production_constructs() {
        for source in [
            "fn apply(value: bool) {}",
            "fn apply(value: Option<u8>) { value.unwrap(); }",
            "fn apply(values: &[u8]) -> u8 { values[0] }",
            "fn apply(value: u8) -> u64 { value as u64 }",
            "fn apply() { let _: std::sync::Mutex<u8>; }",
            "fn apply() { let _: std::rc::Rc<u8>; }",
            "fn apply() { let _: std::collections::HashMap<u8, u8>; }",
            "use std::sync::Arc as Shared; fn apply() { let _: Shared<u8>; }",
            "use crate::*;",
            "fn apply() { println!(\"diagnostic\"); }",
        ] {
            assert!(!violations(source).is_empty(), "{source}");
        }
    }

    #[test]
    fn permits_generics_and_the_standard_error_source_contract() {
        let source = "trait Work {} fn apply<Value: Work>(value: &Value) {} struct Failure; impl std::error::Error for Failure { fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { None } }";
        assert!(structural_violations(source).is_empty());
    }

    #[test]
    fn rejects_trait_objects_ownership_and_runtime_reflection_everywhere() {
        for source in [
            "trait Work {} fn apply(value: &dyn Work) {}",
            "trait Work {} fn apply(value: Box<dyn Work>) {}",
            "trait Work {} fn apply(value: std::sync::Arc<dyn Work>) {}",
            "fn apply(value: &std::any::Any) {}",
            "fn apply(value: std::any::TypeId) {}",
            "macro_rules! erased { () => { Box<dyn Work> } }",
            "trait Error {} trait Work {} struct Failure; impl Error for Failure { fn source(&self) -> Option<&dyn Work> { None } }",
            "trait Work {} struct Failure; impl std::error::Error for Failure { fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { let _: Option<Box<dyn Work>> = None; None } }",
        ] {
            assert!(!structural_violations(source).is_empty(), "{source}");
        }
    }

    #[test]
    fn rejects_erasure_and_global_state_dependencies() {
        let manifest: toml::Value = toml::from_str(
            "[dependencies]\nrenamed = { package = \"anyhow\", version = \"1\" }\n[target.'cfg(windows)'.dev-dependencies]\nparking_lot = \"1\"\n",
        )
        .unwrap();
        assert_eq!(
            forbidden_dependencies(&manifest),
            BTreeSet::from(["anyhow".to_owned(), "parking_lot".to_owned()])
        );
    }

    #[test]
    fn ignores_test_only_modules() {
        let source = "#[cfg(test)] mod tests { fn assertion() { panic!() } }";
        assert!(violations(source).is_empty());
    }
}
