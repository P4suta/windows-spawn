use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use syn::visit::{self, Visit};
use syn::{Attribute, Expr, FnArg, Item, Meta, Signature, Type};

pub(crate) fn check(root: &Path) -> Result<(), String> {
    let source_root = root.join("src");
    let mut paths = Vec::new();
    collect_rust_files(&source_root, &mut paths)?;
    paths.sort();
    let mut report = String::new();
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
    if report.is_empty() {
        Ok(())
    } else {
        Err(report)
    }
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

    fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
        if let Some(segment) = path.path.segments.last() {
            let name = segment.ident.to_string();
            let raw_representation_allowed = self.raw_boundary || self.handle_value_boundary;
            if !raw_representation_allowed
                && matches!(name.as_str(), "isize" | "RawHandle" | "HANDLE")
            {
                self.reject("raw handle representation outside the raw adapter is forbidden");
            }
            if matches!(
                name.as_str(),
                "ManuallyDrop"
                    | "Mutex"
                    | "RwLock"
                    | "AtomicBool"
                    | "AtomicI8"
                    | "AtomicI16"
                    | "AtomicI32"
                    | "AtomicI64"
                    | "AtomicIsize"
                    | "AtomicPtr"
                    | "AtomicU8"
                    | "AtomicU16"
                    | "AtomicU32"
                    | "AtomicU64"
                    | "AtomicUsize"
                    | "UnsafeCell"
            ) {
                self.reject("unregistered ownership or synchronization primitive is forbidden");
            }
        }
        visit::visit_type_path(self, path);
    }
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
        ] {
            assert!(!violations(source).is_empty(), "{source}");
        }
    }

    #[test]
    fn ignores_test_only_modules() {
        let source = "#[cfg(test)] mod tests { fn assertion() { panic!() } }";
        assert!(violations(source).is_empty());
    }
}
