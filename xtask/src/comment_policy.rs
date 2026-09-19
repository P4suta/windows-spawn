use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const OCOMMENT_REVISION: &str = "897c441e50a800817bfc16746bfe5c289b955ce5";
const OCOMMENT_VERSION: &str = "ocomment 0.1.0";
const OCOMMENT_REPOSITORY: &str = "https://github.com/P4suta/OComment.git";
const OCOMMENT_TOOLCHAIN: &str = "1.98.0";

pub(crate) fn build_from_source(root: &Path) -> Result<(), String> {
    let tools = root.join("target").join("tools");
    let source = tools.join("ocomment-checkout");
    let target = tools.join("ocomment-source");
    std::fs::create_dir_all(&tools)
        .map_err(|error| format!("failed to create {}: {error}", tools.display()))?;
    if !source.exists() {
        run_status(
            Command::new("git")
                .current_dir(root)
                .args(["clone", "--filter=blob:none", "--no-checkout"])
                .arg(OCOMMENT_REPOSITORY)
                .arg(&source),
            "clone OComment",
        )?;
    }
    if !source.join(".git").is_dir() {
        return Err(format!(
            "OComment source directory is not a Git checkout: {}",
            source.display()
        ));
    }
    run_status(
        Command::new("git").current_dir(&source).args([
            "fetch",
            "--depth",
            "1",
            "origin",
            OCOMMENT_REVISION,
        ]),
        "fetch pinned OComment revision",
    )?;
    run_status(
        Command::new("git")
            .current_dir(&source)
            .args(["checkout", "--detach", OCOMMENT_REVISION]),
        "check out pinned OComment revision",
    )?;
    let revision = Command::new("git")
        .current_dir(&source)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|error| format!("failed to identify OComment revision: {error}"))?;
    if !revision.status.success() {
        return Err(format!(
            "git rev-parse failed with status {}",
            revision.status
        ));
    }
    let revision = String::from_utf8(revision.stdout)
        .map_err(|_| "OComment revision is not UTF-8".to_owned())?;
    if revision.trim() != OCOMMENT_REVISION {
        return Err(format!(
            "OComment checkout mismatch: expected {OCOMMENT_REVISION}, got {}",
            revision.trim()
        ));
    }
    run_status(
        Command::new("cargo")
            .current_dir(&source)
            .arg(format!("+{OCOMMENT_TOOLCHAIN}"))
            .args(["build", "--release", "--locked", "-p", "ocomment"])
            .arg("--manifest-path")
            .arg(source.join("rust").join("Cargo.toml"))
            .arg("--target-dir")
            .arg(&target),
        "build OComment from source",
    )?;
    std::fs::write(target.join("REVISION"), format!("{OCOMMENT_REVISION}\n"))
        .map_err(|error| format!("failed to record OComment revision: {error}"))?;
    Ok(())
}

fn run_status(command: &mut Command, operation: &str) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|error| format!("failed to {operation}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{operation} failed with status {status}"))
    }
}

#[derive(Debug, Eq, PartialEq)]
struct Violation {
    line: usize,
    reason: &'static str,
}

pub(crate) fn check_repository(root: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .output()
        .map_err(|error| format!("failed to enumerate tracked files: {error}"))?;
    if !output.status.success() {
        return Err(format!("git ls-files failed with status {}", output.status));
    }
    let mut sources = BTreeMap::new();
    for encoded in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let relative = String::from_utf8(encoded.to_vec())
            .map_err(|_| "tracked path is not valid UTF-8".to_owned())?;
        let path = root.join(&relative);
        let bytes =
            std::fs::read(&path).map_err(|error| format!("failed to read {relative}: {error}"))?;
        let source = String::from_utf8(bytes)
            .map_err(|_| format!("tracked text file is not UTF-8: {relative}"))?;
        scan(Path::new(&relative), &source)?;
        sources.insert(normalize_path(&relative), source);
    }

    let mut findings = BTreeSet::new();
    let ocomment_paths: Vec<&str> = sources
        .keys()
        .filter(|path| ocomment_supported(Path::new(path.as_str())))
        .map(String::as_str)
        .collect();
    for (path, violation) in scan_with_ocomment(root, &ocomment_paths, &sources)? {
        findings.insert((path, violation.line, violation.reason));
    }
    for (path, source) in &sources {
        if ocomment_supported(Path::new(path)) {
            continue;
        }
        for violation in scan(Path::new(path), source)? {
            findings.insert((path.clone(), violation.line, violation.reason));
        }
    }

    let mut report = String::new();
    for (path, line, reason) in findings {
        let _ = writeln!(report, "{path}:{line}: {reason}");
    }
    if report.is_empty() {
        Ok(())
    } else {
        Err(format!("comment policy violations:\n{report}"))
    }
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn ocomment_supported(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("rs" | "toml" | "yml" | "yaml" | "sh" | "bash" | "md" | "json")
    ) && path.file_name().and_then(|value| value.to_str()) != Some("Cargo.lock")
}

fn ocomment_program(root: &Path) -> Result<PathBuf, String> {
    if let Some(program) = env::var_os("OCOMMENT").filter(|value| !value.is_empty()) {
        let revision = env::var("OCOMMENT_REVISION")
            .map_err(|_| "OCOMMENT_REVISION is required with OCOMMENT".to_owned())?;
        if revision != OCOMMENT_REVISION {
            return Err(format!(
                "OCOMMENT_REVISION mismatch: expected {OCOMMENT_REVISION}, got {revision}"
            ));
        }
        return Ok(PathBuf::from(program));
    }
    let executable = if cfg!(windows) {
        "ocomment.exe"
    } else {
        "ocomment"
    };
    let source_build = root
        .join("target")
        .join("tools")
        .join("ocomment-source")
        .join("release")
        .join(executable);
    if source_build.is_file() {
        let marker = std::fs::read_to_string(
            root.join("target")
                .join("tools")
                .join("ocomment-source")
                .join("REVISION"),
        )
        .map_err(|error| format!("OComment source revision marker is missing: {error}"))?;
        if marker.trim() != OCOMMENT_REVISION {
            return Err(format!(
                "OComment source revision mismatch: expected {OCOMMENT_REVISION}, got {}",
                marker.trim()
            ));
        }
        Ok(source_build)
    } else {
        Err(format!(
            "OComment is required; run `cargo xtask build-ocomment` to build revision {OCOMMENT_REVISION} from source"
        ))
    }
}

fn scan_with_ocomment(
    root: &Path,
    paths: &[&str],
    sources: &BTreeMap<String, String>,
) -> Result<Vec<(String, Violation)>, String> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let program = ocomment_program(root)?;
    validate_ocomment_version(root, &program)?;
    let document = ocomment_document(root, &program, paths)?;
    analyze_ocomment_document(&document, paths, sources)
}

fn validate_ocomment_version(root: &Path, program: &Path) -> Result<(), String> {
    let version = Command::new(program)
        .current_dir(root)
        .arg("--version")
        .output()
        .map_err(|error| {
            format!("OComment is required; build revision {OCOMMENT_REVISION} from source: {error}")
        })?;
    if !version.status.success() {
        return Err(format!(
            "OComment --version failed with status {}",
            version.status
        ));
    }
    let actual_version = String::from_utf8(version.stdout)
        .map_err(|_| "OComment --version output is not UTF-8".to_owned())?;
    if actual_version.trim() != OCOMMENT_VERSION {
        return Err(format!(
            "OComment version mismatch: expected {OCOMMENT_VERSION} from {OCOMMENT_REVISION}, got {}",
            actual_version.trim()
        ));
    }
    Ok(())
}

fn ocomment_document(root: &Path, program: &Path, paths: &[&str]) -> Result<Value, String> {
    let mut command = Command::new(program);
    command.current_dir(root).args([
        "scan",
        "--format",
        "json",
        "--no-preview",
        "--color",
        "never",
        "--progress",
        "never",
        "--policy",
        "all",
    ]);
    command.args(paths);
    let output = command
        .output()
        .map_err(|error| format!("failed to execute OComment: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "OComment scan failed with status {}: {}",
            output.status,
            stderr.trim()
        ));
    }
    let document: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("invalid OComment JSON: {error}"))?;
    Ok(document)
}

fn analyze_ocomment_document(
    document: &Value,
    paths: &[&str],
    sources: &BTreeMap<String, String>,
) -> Result<Vec<(String, Violation)>, String> {
    if document.get("version").and_then(Value::as_u64) != Some(1) {
        return Err("unsupported OComment JSON protocol".to_owned());
    }
    let skipped = document
        .get("skipped")
        .and_then(Value::as_array)
        .ok_or_else(|| "OComment JSON has no skipped array".to_owned())?;
    if !skipped.is_empty() {
        return Err(format!("OComment skipped tracked inputs: {skipped:?}"));
    }
    let files = document
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| "OComment JSON has no files array".to_owned())?;
    let expected: BTreeSet<String> = paths.iter().map(|path| normalize_path(path)).collect();
    let mut seen = BTreeSet::new();
    let mut violations = Vec::new();
    for file in files {
        let path = file
            .get("path")
            .and_then(Value::as_str)
            .map(normalize_path)
            .ok_or_else(|| "OComment file has no path".to_owned())?;
        if !expected.contains(&path) {
            return Err(format!("OComment reported an unrequested path: {path}"));
        }
        if !seen.insert(path.clone()) {
            return Err(format!("OComment reported a path twice: {path}"));
        }
        let report = file
            .get("report")
            .ok_or_else(|| format!("OComment report is missing for {path}"))?;
        if report.get("valid").and_then(Value::as_bool) != Some(true) {
            return Err(format!("OComment could not parse {path}: {report}"));
        }
        let comments = report
            .get("comments")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("OComment comments are missing for {path}"))?;
        let source = sources
            .get(&path)
            .ok_or_else(|| format!("OComment returned an unknown path: {path}"))?;
        let mut safety_line = None;
        for comment in comments {
            let span = comment
                .get("span")
                .ok_or_else(|| format!("OComment span is missing for {path}"))?;
            let start = json_usize(span, "start", &path)?;
            let end = json_usize(span, "end", &path)?;
            let line = json_usize(comment, "line", &path)?;
            let kind = comment
                .get("kind")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("OComment kind is missing for {path}:{line}"))?;
            let allowed = comment_allowed(
                Path::new(&path),
                source,
                kind,
                start,
                end,
                line,
                &mut safety_line,
            )?;
            if !allowed {
                violations.push((
                    path.clone(),
                    Violation {
                        line,
                        reason: violation_reason(Path::new(&path), kind),
                    },
                ));
            }
        }
    }
    if seen != expected {
        let missing: Vec<_> = expected.difference(&seen).cloned().collect();
        return Err(format!(
            "OComment did not report tracked inputs: {missing:?}"
        ));
    }
    Ok(violations)
}

fn json_usize(value: &Value, field: &str, path: &str) -> Result<usize, String> {
    let raw = value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("OComment {field} is missing for {path}"))?;
    usize::try_from(raw).map_err(|_| format!("OComment {field} is too large for {path}"))
}

fn comment_allowed(
    path: &Path,
    source: &str,
    kind: &str,
    start: usize,
    end: usize,
    line: usize,
    safety_line: &mut Option<usize>,
) -> Result<bool, String> {
    let text = source
        .get(start..end)
        .ok_or_else(|| format!("OComment returned an invalid span for {}", path.display()))?;
    if path.extension().and_then(|value| value.to_str()) != Some("rs") {
        *safety_line = None;
        return Ok((kind == "shebang" && line == 1 && text.starts_with("#!"))
            || exact_tool_directive(path, line, text));
    }
    let line_start = source[..start].rfind('\n').map_or(0, |at| at + 1);
    let standalone = source[line_start..start].trim().is_empty();
    let allowed = match kind {
        "doc-line" | "doc-block" => {
            *safety_line = None;
            if text.starts_with("//!") || text.starts_with("/*!") {
                is_crate_root(path)
            } else {
                outer_doc_is_public(source, start, end)
            }
        }
        "line" => {
            let begins = standalone && text.starts_with("// SAFETY:");
            let continues = standalone
                && safety_line.is_some_and(|previous| previous + 1 == line)
                && text.starts_with("//");
            if begins || continues {
                *safety_line = Some(line);
                true
            } else {
                *safety_line = None;
                false
            }
        }
        _ => {
            *safety_line = None;
            false
        }
    };
    Ok(allowed)
}

fn violation_reason(path: &Path, kind: &str) -> &'static str {
    if path.extension().and_then(|value| value.to_str()) == Some("rs") {
        match kind {
            "doc-line" | "doc-block" => "Rustdoc is allowed only at crate roots and on public API",
            "block" => "ordinary Rust block comment is forbidden",
            _ => "ordinary Rust line comment is forbidden",
        }
    } else {
        "comment is forbidden in this file"
    }
}

fn scan(path: &Path, source: &str) -> Result<Vec<Violation>, String> {
    let extension = path.extension().and_then(|value| value.to_str());
    match extension {
        Some("rs") => Ok(scan_rust(path, source)),
        Some("toml") => Ok(scan_tool_toml(path, source)),
        Some("yml" | "yaml") => Ok(scan_yaml(source, 0)),
        Some("ps1") => Ok(scan_hash(source, HashDialect::PowerShell, 0)),
        Some("sh" | "bash") => Ok(scan_hash(source, HashDialect::Shell, 0)),
        Some("md") => Ok(scan_markdown(source)),
        Some("json") => Ok(scan_json(source, 0)),
        Some("lock") if path.file_name().and_then(|value| value.to_str()) == Some("Cargo.lock") => {
            Ok(scan_cargo_lock(source))
        }
        Some("lock") if path == Path::new("supply-chain/imports.lock") => {
            Ok(scan_tool_toml(path, source))
        }
        Some("txt") => Ok(Vec::new()),
        None => scan_extensionless(path, source),
        Some(other) => Err(format!(
            "comment policy has no scanner for {} files: {}",
            other,
            path.display()
        )),
    }
}

fn scan_tool_toml(path: &Path, source: &str) -> Vec<Violation> {
    let mut violations = scan_hash(source, HashDialect::Toml, 0);
    violations.retain(|violation| {
        let text = source
            .lines()
            .nth(violation.line.saturating_sub(1))
            .unwrap_or("");
        !exact_tool_directive(path, violation.line, text)
    });
    violations
}

fn exact_tool_directive(path: &Path, line: usize, text: &str) -> bool {
    if line != 2 {
        return false;
    }
    matches!(
        (path, text),
        (path, "# cargo-vet config file")
            if path == Path::new("supply-chain/config.toml")
    ) || matches!(
        (path, text),
        (path, "# cargo-vet audits file")
            if path == Path::new("supply-chain/audits.toml")
    ) || matches!(
        (path, text),
        (path, "# cargo-vet imports lock")
            if path == Path::new("supply-chain/imports.lock")
    )
}

fn scan_extensionless(path: &Path, source: &str) -> Result<Vec<Violation>, String> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if matches!(
        name,
        "justfile" | ".gitignore" | ".gitattributes" | "CODEOWNERS"
    ) {
        return Ok(scan_hash(source, HashDialect::Shell, 0));
    }
    if name.starts_with("LICENSE") {
        return Ok(Vec::new());
    }
    Err(format!(
        "comment policy has no scanner for extensionless file: {}",
        path.display()
    ))
}

fn scan_cargo_lock(source: &str) -> Vec<Violation> {
    let mut violations = scan_hash(source, HashDialect::Toml, 0);
    violations.retain(|violation| {
        let line = source.lines().nth(violation.line.saturating_sub(1));
        !matches!(
            (violation.line, line),
            (1, Some("# This file is automatically @generated by Cargo."))
                | (2, Some("# It is not intended for manual editing."))
        )
    });
    violations
}

fn scan_rust(path: &Path, source: &str) -> Vec<Violation> {
    let bytes = source.as_bytes();
    let mut violations = Vec::new();
    let mut index = 0;
    let mut safety_line = None;
    while index < bytes.len() {
        if let Some(end) = raw_string_end(bytes, index) {
            index = end;
            continue;
        }
        match bytes[index] {
            b'"' => {
                index = quoted_end(bytes, index, b'"');
            }
            b'\'' if char_literal_starts(bytes, index) => {
                index = quoted_end(bytes, index, b'\'');
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                let end = bytes[index..]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(bytes.len(), |offset| index + offset);
                let text = &source[index..end];
                let line = line_number(source, index);
                let line_start = source[..index].rfind('\n').map_or(0, |at| at + 1);
                let standalone = source[line_start..index].trim().is_empty();
                let outer_doc = text.starts_with("///") && !text.starts_with("////");
                let inner_doc = text.starts_with("//!");
                let safety = text.starts_with("// SAFETY:") && standalone;
                let continuation = standalone
                    && safety_line.is_some_and(|previous| previous + 1 == line)
                    && !outer_doc
                    && !inner_doc;
                if safety || continuation {
                    safety_line = Some(line);
                } else {
                    safety_line = None;
                    if inner_doc {
                        if !is_crate_root(path) {
                            violations.push(Violation {
                                line,
                                reason: "Rustdoc is allowed only at crate roots and on public API",
                            });
                        }
                    } else if outer_doc {
                        if !outer_doc_is_public(source, index, end) {
                            violations.push(Violation {
                                line,
                                reason: "Rustdoc on a private item is forbidden",
                            });
                        }
                    } else {
                        violations.push(Violation {
                            line,
                            reason: "ordinary Rust line comment is forbidden",
                        });
                    }
                }
                index = end;
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                let (end, terminated) = nested_block_end(bytes, index);
                let text = &source[index..end];
                let line = line_number(source, index);
                let outer_doc = text.starts_with("/**") && !text.starts_with("/***");
                let inner_doc = text.starts_with("/*!");
                if !terminated {
                    violations.push(Violation {
                        line,
                        reason: "unterminated Rust block comment",
                    });
                } else if inner_doc {
                    if !is_crate_root(path) {
                        violations.push(Violation {
                            line,
                            reason: "Rustdoc is allowed only at crate roots and on public API",
                        });
                    }
                } else if outer_doc {
                    if !outer_doc_is_public(source, index, end) {
                        violations.push(Violation {
                            line,
                            reason: "Rustdoc on a private item is forbidden",
                        });
                    }
                } else {
                    violations.push(Violation {
                        line,
                        reason: "ordinary Rust block comment is forbidden",
                    });
                }
                safety_line = None;
                index = end;
            }
            _ => {
                index += 1;
            }
        }
    }
    violations
}

fn raw_string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut cursor = start;
    if bytes.get(cursor) == Some(&b'b') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'r') {
        return None;
    }
    cursor += 1;
    let hashes_start = cursor;
    while bytes.get(cursor) == Some(&b'#') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'"') {
        return None;
    }
    let hashes = cursor - hashes_start;
    cursor += 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b'"'
            && bytes
                .get(cursor + 1..cursor + 1 + hashes)
                .is_some_and(|tail| tail.iter().all(|byte| *byte == b'#'))
        {
            return Some(cursor + 1 + hashes);
        }
        cursor += 1;
    }
    Some(bytes.len())
}

fn quoted_end(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut cursor = start + 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\\' {
            cursor = (cursor + 2).min(bytes.len());
        } else if bytes[cursor] == quote {
            return cursor + 1;
        } else {
            cursor += 1;
        }
    }
    bytes.len()
}

fn char_literal_starts(bytes: &[u8], start: usize) -> bool {
    let Some(next) = bytes.get(start + 1) else {
        return false;
    };
    if *next == b'\\' {
        bytes.get(start + 3) == Some(&b'\'')
    } else {
        bytes.get(start + 2) == Some(&b'\'')
    }
}

fn nested_block_end(bytes: &[u8], start: usize) -> (usize, bool) {
    let mut depth = 1_usize;
    let mut cursor = start + 2;
    while cursor < bytes.len() {
        if bytes.get(cursor..cursor + 2) == Some(b"/*") {
            depth += 1;
            cursor += 2;
        } else if bytes.get(cursor..cursor + 2) == Some(b"*/") {
            depth -= 1;
            cursor += 2;
            if depth == 0 {
                return (cursor, true);
            }
        } else {
            cursor += 1;
        }
    }
    (bytes.len(), false)
}

fn is_crate_root(path: &Path) -> bool {
    let name = path.file_name().and_then(|value| value.to_str());
    name == Some("lib.rs")
        || name == Some("main.rs")
        || path.starts_with("tests")
        || path.starts_with("examples")
}

fn outer_doc_is_public(source: &str, start: usize, end: usize) -> bool {
    if inside_public_enum_or_trait(source, start) {
        return true;
    }
    let mut cursor = end;
    loop {
        cursor = skip_whitespace(source, cursor);
        let tail = &source[cursor..];
        if tail.starts_with("///") {
            cursor = tail
                .find('\n')
                .map_or(source.len(), |offset| cursor + offset + 1);
            continue;
        }
        if tail.starts_with("#[") {
            cursor = skip_attribute(source, cursor);
            continue;
        }
        break;
    }
    let tail = &source[cursor..];
    tail.starts_with("pub ")
        || tail.starts_with("pub\n")
        || tail.starts_with("pub\t")
        || tail.starts_with("unsafe pub ")
        || tail.starts_with("pub unsafe ")
}

fn inside_public_enum_or_trait(source: &str, position: usize) -> bool {
    for marker in ["pub enum ", "pub trait ", "pub unsafe trait ", "pub union "] {
        let Some(start) = source[..position].rfind(marker) else {
            continue;
        };
        let Some(open_offset) = source[start..position].find('{') else {
            continue;
        };
        let open = start + open_offset;
        let mut depth = 0_i32;
        for byte in source.as_bytes()[open..position].iter().copied() {
            match byte {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
        }
        if depth > 0 {
            return true;
        }
    }
    false
}

fn skip_whitespace(source: &str, mut cursor: usize) -> usize {
    while source
        .as_bytes()
        .get(cursor)
        .is_some_and(u8::is_ascii_whitespace)
    {
        cursor += 1;
    }
    cursor
}

fn skip_attribute(source: &str, start: usize) -> usize {
    let bytes = source.as_bytes();
    let mut cursor = start;
    let mut depth = 0_i32;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return cursor + 1;
                }
            }
            b'"' => {
                cursor = quoted_end(bytes, cursor, b'"');
                continue;
            }
            _ => {}
        }
        cursor += 1;
    }
    bytes.len()
}

#[derive(Clone, Copy)]
enum HashDialect {
    Toml,
    PowerShell,
    Shell,
}

fn scan_hash(source: &str, dialect: HashDialect, line_offset: usize) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut multiline = None;
    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_offset + line_index + 1;
        let bytes = line.as_bytes();
        let mut index = 0;
        let mut quote = multiline;
        while index < bytes.len() {
            if let Some((delimiter, triple)) = quote {
                if triple && bytes.get(index..index + 3) == Some([delimiter; 3].as_slice()) {
                    quote = None;
                    index += 3;
                } else if !triple && bytes[index] == delimiter {
                    quote = None;
                    index += 1;
                } else if delimiter == b'"' && bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else {
                    index += 1;
                }
                continue;
            }
            match bytes[index] {
                b'\'' | b'"' => {
                    let delimiter = bytes[index];
                    let triple = matches!(dialect, HashDialect::Toml)
                        && bytes.get(index..index + 3) == Some([delimiter; 3].as_slice());
                    quote = Some((delimiter, triple));
                    index += if triple { 3 } else { 1 };
                }
                b'#' => {
                    let directive = match dialect {
                        HashDialect::PowerShell => line.trim_start().starts_with("#Requires "),
                        HashDialect::Shell => line_number == 1 && line.starts_with("#!"),
                        HashDialect::Toml => false,
                    };
                    if !directive {
                        violations.push(Violation {
                            line: line_number,
                            reason: "ordinary hash comment is forbidden",
                        });
                    }
                    break;
                }
                b'\\' if matches!(dialect, HashDialect::PowerShell) => {
                    index = (index + 2).min(bytes.len());
                }
                _ => index += 1,
            }
        }
        multiline = match dialect {
            HashDialect::Toml => quote.filter(|(_, triple)| *triple),
            HashDialect::PowerShell | HashDialect::Shell => None,
        };
    }
    violations
}

fn scan_yaml(source: &str, line_offset: usize) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut block_indent = None;
    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_offset + line_index + 1;
        let indent = line.bytes().take_while(|byte| *byte == b' ').count();
        if let Some(parent_indent) = block_indent {
            if line.trim().is_empty() || indent > parent_indent {
                continue;
            }
            block_indent = None;
        }
        let bytes = line.as_bytes();
        let mut quote = None;
        let mut index = 0;
        let mut scalar_marker = false;
        while index < bytes.len() {
            match quote {
                Some(b'"') if bytes[index] == b'\\' => {
                    index = (index + 2).min(bytes.len());
                }
                Some(delimiter) if bytes[index] == delimiter => {
                    quote = None;
                    index += 1;
                }
                Some(_) => index += 1,
                None => match bytes[index] {
                    b'\'' | b'"' => {
                        quote = Some(bytes[index]);
                        index += 1;
                    }
                    b'#' => {
                        violations.push(Violation {
                            line: line_number,
                            reason: "ordinary YAML comment is forbidden",
                        });
                        break;
                    }
                    b'|' | b'>' => {
                        let prefix = line[..index].trim_end();
                        if prefix.ends_with(':') || prefix.ends_with('-') {
                            scalar_marker = true;
                        }
                        index += 1;
                    }
                    _ => index += 1,
                },
            }
        }
        if scalar_marker {
            block_indent = Some(indent);
        }
    }
    violations
}

fn scan_json(source: &str, line_offset: usize) -> Vec<Violation> {
    let bytes = source.as_bytes();
    let mut violations = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'"' {
            index = quoted_end(bytes, index, b'"');
        } else if bytes.get(index..index + 2) == Some(b"//")
            || bytes.get(index..index + 2) == Some(b"/*")
        {
            violations.push(Violation {
                line: line_offset + line_number(source, index),
                reason: "JSON comments are forbidden",
            });
            index += 2;
        } else {
            index += 1;
        }
    }
    violations
}

fn scan_markdown(source: &str) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut fence: Option<(String, String, usize, String)> = None;
    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let trimmed = line.trim_start();
        if let Some((delimiter, language, start, content)) = &mut fence {
            if trimmed.starts_with(delimiter.as_str())
                && trimmed[delimiter.len()..].trim().is_empty()
            {
                let mut nested = match language.as_str() {
                    "rust" => scan_rust(Path::new("examples/fenced.rs"), content),
                    "toml" => scan_hash(content, HashDialect::Toml, 0),
                    "yaml" | "yml" => scan_yaml(content, 0),
                    "powershell" => scan_hash(content, HashDialect::PowerShell, 0),
                    "sh" | "bash" => scan_hash(content, HashDialect::Shell, 0),
                    "json" => scan_json(content, 0),
                    "console" | "text" => Vec::new(),
                    _ => vec![Violation {
                        line: 1,
                        reason: "Markdown fence language has no comment scanner",
                    }],
                };
                for violation in &mut nested {
                    violation.line += *start;
                }
                violations.extend(nested);
                fence = None;
            } else {
                content.push_str(line);
                content.push('\n');
            }
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let delimiter = if trimmed.starts_with("```") {
                "```"
            } else {
                "~~~"
            };
            let language = trimmed[delimiter.len()..].trim();
            fence = Some((
                delimiter.to_owned(),
                language.to_ascii_lowercase(),
                line_number,
                String::new(),
            ));
        }
        if line.contains("<!--") || line.contains("-->") {
            violations.push(Violation {
                line: line_number,
                reason: "HTML comments are forbidden",
            });
        }
    }
    if let Some((_, _, start, _)) = fence {
        violations.push(Violation {
            line: start,
            reason: "unterminated Markdown fence",
        });
    }
    violations
}

fn line_number(source: &str, position: usize) -> usize {
    source
        .get(..position)
        .map_or(1, |prefix| prefix.split('\n').count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_distinguishes_raw_strings_urls_nested_comments_docs_and_safety() {
        let accepted = r####"
#![allow(dead_code)]
//! crate docs
pub const URL: &str = "https://example.invalid/a//b";
pub const RAW: &str = r###"/* data */ // data"###;
/// Public docs.
pub struct Public;
fn raw_boundary() {
    // SAFETY: the test models one raw operation.
    unsafe { std::hint::unreachable_unchecked() };
}
"####;
        assert!(scan_rust(Path::new("src/lib.rs"), accepted).is_empty());
        let rejected = "fn private() {}\n/// private docs\nfn hidden() {}\n/* outer /* inner */ end */\n// note\n";
        let violations = scan_rust(Path::new("src/module.rs"), rejected);
        assert_eq!(violations.len(), 3);
    }

    #[test]
    fn yaml_block_scalars_and_quoted_hashes_are_data() {
        let accepted = "url: \"https://example.invalid/#part\"\nscript: |\n  # data\n  echo ok # data\nnext: value\n";
        assert!(scan_yaml(accepted, 0).is_empty());
        assert_eq!(scan_yaml("key: value # explanation\n", 0).len(), 1);
    }

    #[test]
    fn markdown_scans_fences_and_html_comments() {
        let accepted =
            "Text with https://example.invalid/a//b.\n```rust\nlet value = r#\"// data\"#;\n```\n";
        assert!(scan_markdown(accepted).is_empty());
        assert_eq!(scan_markdown("<!-- hidden -->\n").len(), 1);
        assert_eq!(scan_markdown("```rust\n// hidden\n```\n").len(), 1);
    }

    #[test]
    fn exact_directives_are_the_only_hash_exceptions() {
        assert!(scan_hash("#!/bin/sh\necho ok\n", HashDialect::Shell, 0).is_empty());
        assert!(scan_hash("#Requires -Version 7\n", HashDialect::PowerShell, 0).is_empty());
        assert_eq!(scan_hash("# explanation\n", HashDialect::Toml, 0).len(), 1);
    }

    #[test]
    fn generated_cargo_lock_headers_are_path_independent_and_exact() {
        let accepted =
            "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n";
        assert!(scan(Path::new("fuzz/Cargo.lock"), accepted)
            .expect("Cargo.lock has a scanner")
            .is_empty());
        assert_eq!(
            scan(Path::new("nested/Cargo.lock"), "# explanation\n")
                .expect("Cargo.lock has a scanner")
                .len(),
            1
        );
    }

    #[test]
    fn cargo_vet_headers_are_exact_tool_directives() {
        assert!(scan(
            Path::new("supply-chain/config.toml"),
            "\n# cargo-vet config file\n[cargo-vet]\nversion = \"0.10\"\n"
        )
        .expect("cargo-vet config has a scanner")
        .is_empty());
        assert_eq!(
            scan(
                Path::new("supply-chain/config.toml"),
                "\n# cargo-vet configuration\n"
            )
            .expect("cargo-vet config has a scanner")
            .len(),
            1
        );
        assert!(scan(
            Path::new("supply-chain/imports.lock"),
            "\n# cargo-vet imports lock\n"
        )
        .expect("cargo-vet imports lock has a scanner")
        .is_empty());
    }
}
