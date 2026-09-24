//! Repository laws Clippy cannot express, checked over the tokens of every Rust file (ADR 0013).

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A lint exception the repository accepts, with the number of attributes that claim it.
struct Exception {
    file: &'static str,
    lint: &'static str,
    count: usize,
    reason: &'static str,
}

/// Every `#[allow]` in the repository; an attribute outside this table, or a count that differs, fails the gate.
const EXCEPTIONS: &[Exception] = &[
    Exception {
        file: "src/lib.rs",
        lint: "unsafe_code",
        count: 2,
        reason: "sys is the only Win32 FFI boundary; failure_tests creates pseudoconsoles for tests",
    },
    Exception {
        file: "src/handles.rs",
        lint: "unsafe_code",
        count: 1,
        reason: "AsPseudoConsole is an unsafe trait whose implementors vouch for a live HPCON",
    },
    Exception {
        file: "src/options.rs",
        lint: "unsafe_code",
        count: 1,
        reason: "tests implement AsPseudoConsole",
    },
    Exception {
        file: "src/plan.rs",
        lint: "unsafe_code",
        count: 1,
        reason: "tests implement AsPseudoConsole",
    },
    Exception {
        file: "src/sys.rs",
        lint: "clippy::as_conversions",
        count: 2,
        reason: "handle values convert between pointers and integers, which has no From form before Rust 1.84",
    },
    Exception {
        file: "src/child.rs",
        lint: "clippy::expect_used",
        count: 2,
        reason: "a SuspendedChild owns its process until resume consumes it",
    },
    Exception {
        file: "src/transaction.rs",
        lint: "clippy::expect_used",
        count: 1,
        reason: "an uncommitted transaction owns its process until commit consumes it",
    },
    Exception {
        file: "src/transaction.rs",
        lint: "clippy::too_many_lines",
        count: 1,
        reason: "one function acquires every creation resource, so rollback has one owner (ADR 0007)",
    },
    Exception {
        file: "src/mitigation.rs",
        lint: "clippy::too_many_lines",
        count: 1,
        reason: "the encoding test lists every SDK 22621 field",
    },
    Exception {
        file: "tests/support/mod.rs",
        lint: "clippy::disallowed_methods",
        count: 1,
        reason: "a probe reports by exit code",
    },
    Exception {
        file: "tests/windows_spawn.rs",
        lint: "missing_docs",
        count: 1,
        reason: "the test crate is empty off Windows",
    },
    Exception {
        file: "tests/windows_spawn.rs",
        lint: "clippy::as_conversions",
        count: 1,
        reason: "tests call Win32 directly with handle values",
    },
    Exception {
        file: "tests/windows_spawn.rs",
        lint: "clippy::expect_used",
        count: 1,
        reason: "tests assert by panicking",
    },
    Exception {
        file: "tests/windows_spawn.rs",
        lint: "clippy::unwrap_in_result",
        count: 1,
        reason: "tests assert by panicking",
    },
    Exception {
        file: "tests/argv_roundtrip.rs",
        lint: "clippy::as_conversions",
        count: 1,
        reason: "tests call Win32 directly with handle values",
    },
    Exception {
        file: "tests/argv_roundtrip.rs",
        lint: "clippy::expect_used",
        count: 1,
        reason: "tests assert by panicking",
    },
    Exception {
        file: "tests/argv_roundtrip.rs",
        lint: "clippy::unwrap_in_result",
        count: 1,
        reason: "tests assert by panicking",
    },
];

/// Identifiers that measure or wait on time (ADR 0010).
const TIME_NAMES: [&str; 6] = [
    "SystemTime",
    "Instant",
    "Duration",
    "UNIX_EPOCH",
    "sleep",
    "elapsed",
];

/// Identifier fragments that name a time bound, matched case-insensitively.
const TIME_FRAGMENTS: [&str; 2] = ["timeout", "deadline"];

/// Time-named identifiers that decide nothing by time.
///
/// `WAIT_TIMEOUT` is what a 0 ms `WaitForSingleObject` query returns for an unsignaled object.
const TIME_NAME_EXCEPTIONS: [&str; 1] = ["WAIT_TIMEOUT"];

/// Shell commands that wait for a duration, matched case-insensitively inside string literals.
///
/// Each is split so this table does not match itself.
const DELAY_COMMANDS: [&str; 3] = [
    concat!("ping", " -n"),
    concat!("start", "-sleep"),
    concat!("timeout", " /t"),
];

/// A rule violation.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Finding {
    pub(crate) file: String,
    pub(crate) line: usize,
    pub(crate) rule: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}: {}", self.file, self.line, self.rule)
    }
}

/// Checks every Rust file under the repository's source directories.
pub(crate) fn check(root: &Path) -> io::Result<Vec<Finding>> {
    let mut files = Vec::new();
    for directory in ["src", "tests", "examples", "xtask/src"] {
        collect_rust_files(&root.join(directory), &mut files)?;
    }
    files.sort();
    let mut findings = Vec::new();
    let mut allows = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(&path)?;
        let scan = scan(&relative, &source);
        findings.extend(scan.findings);
        allows.extend(scan.allows);
    }
    findings.extend(check_exceptions(&allows, EXCEPTIONS));
    Ok(findings)
}

fn collect_rust_files(directory: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_rust_files(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

/// An `#[allow]` lint and where it appears.
#[derive(Debug, PartialEq, Eq)]
struct Allow {
    file: String,
    line: usize,
    lint: String,
}

fn check_exceptions(allows: &[Allow], exceptions: &[Exception]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for allow in allows {
        let registered = exceptions
            .iter()
            .any(|exception| exception.file == allow.file && exception.lint == allow.lint);
        if !registered {
            findings.push(Finding {
                file: allow.file.clone(),
                line: allow.line,
                rule: format!(
                    "#[allow({})] is not registered in xtask/src/gates.rs",
                    allow.lint
                ),
            });
        }
    }
    for exception in exceptions {
        let count = allows
            .iter()
            .filter(|allow| allow.file == exception.file && allow.lint == exception.lint)
            .count();
        if count != exception.count {
            findings.push(Finding {
                file: exception.file.to_owned(),
                line: 0,
                rule: format!(
                    "{} is registered {} time(s) ({}) but allowed {count} time(s)",
                    exception.lint, exception.count, exception.reason
                ),
            });
        }
    }
    findings
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommentKind {
    Doc,
    Plain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Ident(String),
    Literal(String),
    Str(String),
    Punct(char),
    Comment {
        kind: CommentKind,
        text: String,
        alone: bool,
        block: bool,
    },
}

struct Scan {
    findings: Vec<Finding>,
    allows: Vec<Allow>,
}

fn scan(file: &str, source: &str) -> Scan {
    let tokens = lex(source);
    let mut findings = Vec::new();
    let mut finding = |line: usize, rule: String| {
        findings.push(Finding {
            file: file.to_owned(),
            line,
            rule,
        });
    };
    check_comments(&tokens, &mut finding);
    let code: Vec<&(usize, Token)> = tokens
        .iter()
        .filter(|(_, token)| !matches!(token, Token::Comment { .. }))
        .collect();
    let mut allows = Vec::new();
    for (index, (line, token)) in code.iter().enumerate() {
        let next = |offset: usize| {
            code.get(index.saturating_add(offset))
                .map(|(_, following)| following)
        };
        match token {
            Token::Ident(name) => {
                if is_time_name(name) {
                    finding(
                        *line,
                        format!("`{name}` uses time; wait for the event itself (ADR 0010)"),
                    );
                }
                if name == "Box"
                    && next(1) == Some(&Token::Punct('<'))
                    && next(2) == Some(&Token::Ident("dyn".to_owned()))
                {
                    finding(
                        *line,
                        "own a closed set with an enum or an open one with a generic, not Box<dyn>"
                            .to_owned(),
                    );
                }
                if name == "WaitForSingleObject" && next(1) == Some(&Token::Punct('(')) {
                    if let Some(argument) = second_argument(&code, index.saturating_add(2)) {
                        let bounded = !matches!(
                            argument.as_slice(),
                            [Token::Ident(bound)] if bound == "INFINITE"
                        ) && !matches!(argument.as_slice(), [Token::Literal(value)] if value == "0");
                        if bounded {
                            finding(
                                *line,
                                "WaitForSingleObject waits only INFINITE or 0 (ADR 0010)"
                                    .to_owned(),
                            );
                        }
                    }
                }
            }
            Token::Str(text) => {
                let lower = text.to_ascii_lowercase();
                if DELAY_COMMANDS.iter().any(|command| lower.contains(command)) {
                    finding(
                        *line,
                        "a string scripts a delay; wait for the event itself (ADR 0010)".to_owned(),
                    );
                }
            }
            Token::Punct('#') => {
                let start = if next(1) == Some(&Token::Punct('!')) {
                    2
                } else {
                    1
                };
                if next(start) == Some(&Token::Punct('[')) {
                    let content = bracketed(&code, index.saturating_add(start));
                    if matches!(content.as_slice(), [Token::Ident(name)] if name == "default") {
                        finding(
                            *line,
                            "#[default] lets declaration order pick a default; write impl Default"
                                .to_owned(),
                        );
                    }
                    for lint in allowed_lints(&content) {
                        allows.push(Allow {
                            file: file.to_owned(),
                            line: *line,
                            lint,
                        });
                    }
                }
            }
            Token::Literal(_) | Token::Punct(_) | Token::Comment { .. } => {}
        }
    }
    Scan { findings, allows }
}

fn is_time_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let named = TIME_NAMES.contains(&name)
        || TIME_FRAGMENTS
            .iter()
            .any(|fragment| lower.contains(fragment));
    named && !TIME_NAME_EXCEPTIONS.contains(&name)
}

/// Allows doc comments and runs of whole-line `//` comments that start with `SAFETY:`.
fn check_comments(tokens: &[(usize, Token)], finding: &mut impl FnMut(usize, String)) {
    let mut previous_line: Option<usize> = None;
    let mut in_safety = false;
    for (line, token) in tokens {
        let Token::Comment {
            kind,
            text,
            alone,
            block,
        } = token
        else {
            previous_line = None;
            continue;
        };
        if *kind == CommentKind::Doc {
            previous_line = None;
            continue;
        }
        let continues = *alone
            && !*block
            && previous_line.is_some_and(|previous| previous.saturating_add(1) == *line);
        if !continues {
            in_safety = *alone && !*block && text.trim_start().starts_with("SAFETY:");
        }
        if !in_safety {
            finding(
                *line,
                "only doc comments and `// SAFETY:` comments are written; put the reason in the commit message"
                    .to_owned(),
            );
        }
        previous_line = if *alone && !*block { Some(*line) } else { None };
    }
}

/// Returns the tokens between the `[` at `open` and its matching `]`.
fn bracketed(code: &[&(usize, Token)], open: usize) -> Vec<Token> {
    let mut depth = 0_usize;
    let mut content = Vec::new();
    for (_, token) in code.iter().skip(open) {
        if *token == Token::Punct('[') {
            depth = depth.saturating_add(1);
            if depth == 1 {
                continue;
            }
        } else if *token == Token::Punct(']') {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return content;
            }
        }
        content.push(token.clone());
    }
    content
}

/// Returns the second top-level argument of the call whose tokens start at `after_open`.
fn second_argument(code: &[&(usize, Token)], after_open: usize) -> Option<Vec<Token>> {
    let mut depth = 0_usize;
    let mut argument = 0_usize;
    let mut tokens = Vec::new();
    for (_, token) in code.iter().skip(after_open) {
        match token {
            Token::Punct('(' | '[' | '{') => depth = depth.saturating_add(1),
            Token::Punct(')' | ']' | '}') if depth == 0 => {
                return (argument == 1).then_some(tokens);
            }
            Token::Punct(')' | ']' | '}') => depth = depth.saturating_sub(1),
            Token::Punct(',') if depth == 0 => {
                if argument == 1 {
                    return Some(tokens);
                }
                argument = argument.saturating_add(1);
                continue;
            }
            Token::Ident(_)
            | Token::Literal(_)
            | Token::Str(_)
            | Token::Punct(_)
            | Token::Comment { .. } => {}
        }
        if argument == 1 {
            tokens.push(token.clone());
        }
    }
    None
}

/// Returns the lint paths named by `allow(...)` inside an attribute.
fn allowed_lints(content: &[Token]) -> Vec<String> {
    let mut lints = Vec::new();
    let mut index = 0_usize;
    while let Some(token) = content.get(index) {
        let opens = matches!(token, Token::Ident(name) if name == "allow")
            && content.get(index.saturating_add(1)) == Some(&Token::Punct('('));
        if opens {
            let mut path = String::new();
            let mut depth = 0_usize;
            for inner in content.iter().skip(index.saturating_add(1)) {
                match inner {
                    Token::Punct('(') => depth = depth.saturating_add(1),
                    Token::Punct(')') => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            break;
                        }
                    }
                    Token::Punct(',') if depth == 1 => lints.push(std::mem::take(&mut path)),
                    Token::Punct(':') => path.push(':'),
                    Token::Ident(name) => path.push_str(name),
                    Token::Literal(_) | Token::Str(_) | Token::Punct(_) | Token::Comment { .. } => {
                    }
                }
            }
            if !path.is_empty() {
                lints.push(path);
            }
        }
        index = index.saturating_add(1);
    }
    lints
}

/// Splits Rust source into the tokens the rules need, tracking lines.
fn lex(source: &str) -> Vec<(usize, Token)> {
    let characters: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0_usize;
    let mut line = 1_usize;
    let mut line_has_code = false;
    while let Some(&character) = characters.get(index) {
        let next = characters.get(index.saturating_add(1)).copied();
        if character == '\n' {
            line = line.saturating_add(1);
            line_has_code = false;
            index = index.saturating_add(1);
        } else if character.is_whitespace() {
            index = index.saturating_add(1);
        } else if character == '/' && next == Some('/') {
            let end = characters
                .iter()
                .skip(index)
                .position(|candidate| *candidate == '\n')
                .map_or(characters.len(), |offset| index.saturating_add(offset));
            let text: String = characters
                .get(index..end)
                .unwrap_or_default()
                .iter()
                .collect();
            let kind = if (text.starts_with("///") && !text.starts_with("////"))
                || text.starts_with("//!")
            {
                CommentKind::Doc
            } else {
                CommentKind::Plain
            };
            let body = text.get(2..).unwrap_or_default().to_owned();
            tokens.push((
                line,
                Token::Comment {
                    kind,
                    text: body,
                    alone: !line_has_code,
                    block: false,
                },
            ));
            index = end;
        } else if character == '/' && next == Some('*') {
            let start_line = line;
            let mut depth = 0_usize;
            let mut text = String::new();
            while let Some(&current) = characters.get(index) {
                let following = characters.get(index.saturating_add(1)).copied();
                if current == '/' && following == Some('*') {
                    depth = depth.saturating_add(1);
                    text.push_str("/*");
                    index = index.saturating_add(2);
                } else if current == '*' && following == Some('/') {
                    depth = depth.saturating_sub(1);
                    text.push_str("*/");
                    index = index.saturating_add(2);
                    if depth == 0 {
                        break;
                    }
                } else {
                    if current == '\n' {
                        line = line.saturating_add(1);
                    }
                    text.push(current);
                    index = index.saturating_add(1);
                }
            }
            let kind = if (text.starts_with("/**") && !text.starts_with("/***") && text != "/**/")
                || text.starts_with("/*!")
            {
                CommentKind::Doc
            } else {
                CommentKind::Plain
            };
            tokens.push((
                start_line,
                Token::Comment {
                    kind,
                    text,
                    alone: !line_has_code,
                    block: true,
                },
            ));
        } else if character == '"' {
            let (text, end, newlines) = quoted(&characters, index);
            tokens.push((line, Token::Str(text)));
            line = line.saturating_add(newlines);
            line_has_code = true;
            index = end;
        } else if character == '\'' {
            line_has_code = true;
            let escaped = next == Some('\\');
            let closes_after_one = characters.get(index.saturating_add(2)) == Some(&'\'');
            if escaped || closes_after_one {
                let mut end = index.saturating_add(1);
                while let Some(&current) = characters.get(end) {
                    end = end.saturating_add(if current == '\\' { 2 } else { 1 });
                    if current == '\'' {
                        break;
                    }
                }
                tokens.push((line, Token::Literal("'".to_owned())));
                index = end;
            } else {
                tokens.push((line, Token::Punct('\'')));
                index = index.saturating_add(1);
            }
        } else if character.is_alphabetic() || character == '_' {
            line_has_code = true;
            let end = characters
                .iter()
                .skip(index)
                .position(|candidate| !(candidate.is_alphanumeric() || *candidate == '_'))
                .map_or(characters.len(), |offset| index.saturating_add(offset));
            let word: String = characters
                .get(index..end)
                .unwrap_or_default()
                .iter()
                .collect();
            let hashes = characters
                .iter()
                .skip(end)
                .take_while(|candidate| **candidate == '#')
                .count();
            let quote = characters.get(end.saturating_add(hashes)) == Some(&'"');
            if matches!(word.as_str(), "r" | "br" | "cr") && quote {
                let (text, after, newlines) = raw_quoted(&characters, end, hashes);
                tokens.push((line, Token::Str(text)));
                line = line.saturating_add(newlines);
                index = after;
            } else if matches!(word.as_str(), "b" | "c") && characters.get(end) == Some(&'"') {
                let (text, after, newlines) = quoted(&characters, end);
                tokens.push((line, Token::Str(text)));
                line = line.saturating_add(newlines);
                index = after;
            } else if word == "r" && hashes == 1 {
                index = end.saturating_add(1);
            } else {
                tokens.push((line, Token::Ident(word)));
                index = end;
            }
        } else if character.is_ascii_digit() {
            line_has_code = true;
            let end = characters
                .iter()
                .skip(index)
                .position(|candidate| !(candidate.is_alphanumeric() || *candidate == '_'))
                .map_or(characters.len(), |offset| index.saturating_add(offset));
            let literal: String = characters
                .get(index..end)
                .unwrap_or_default()
                .iter()
                .collect();
            tokens.push((line, Token::Literal(literal)));
            index = end;
        } else {
            line_has_code = true;
            tokens.push((line, Token::Punct(character)));
            index = index.saturating_add(1);
        }
    }
    tokens
}

/// Reads a `"..."` literal starting at the quote; returns its text, the index after it, and its newlines.
fn quoted(characters: &[char], quote: usize) -> (String, usize, usize) {
    let mut text = String::new();
    let mut index = quote.saturating_add(1);
    let mut newlines = 0_usize;
    while let Some(&character) = characters.get(index) {
        if character == '\\' {
            if let Some(&escaped) = characters.get(index.saturating_add(1)) {
                text.push(escaped);
            }
            index = index.saturating_add(2);
            continue;
        }
        index = index.saturating_add(1);
        if character == '"' {
            break;
        }
        if character == '\n' {
            newlines = newlines.saturating_add(1);
        }
        text.push(character);
    }
    (text, index, newlines)
}

/// Reads a raw literal whose `#`s start at `hashes_start`.
fn raw_quoted(characters: &[char], hashes_start: usize, hashes: usize) -> (String, usize, usize) {
    let mut text = String::new();
    let mut index = hashes_start.saturating_add(hashes).saturating_add(1);
    let mut newlines = 0_usize;
    while let Some(&character) = characters.get(index) {
        if character == '"' {
            let closing = characters
                .iter()
                .skip(index.saturating_add(1))
                .take(hashes)
                .filter(|candidate| **candidate == '#')
                .count();
            if closing == hashes {
                return (
                    text,
                    index.saturating_add(1).saturating_add(hashes),
                    newlines,
                );
            }
        }
        if character == '\n' {
            newlines = newlines.saturating_add(1);
        }
        text.push(character);
        index = index.saturating_add(1);
    }
    (text, index, newlines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(source: &str) -> Vec<String> {
        scan("src/example.rs", source)
            .findings
            .into_iter()
            .map(|finding| finding.rule)
            .collect()
    }

    fn lints(source: &str) -> Vec<String> {
        scan("src/example.rs", source)
            .allows
            .into_iter()
            .map(|allow| allow.lint)
            .collect()
    }

    #[test]
    fn only_doc_and_safety_comments_are_allowed() {
        assert!(rules(concat!(
            "/// doc\n",
            "//! inner\n",
            "/** block doc */\n",
            "fn f() {}\n"
        ))
        .is_empty());
        assert!(rules(concat!(
            "// SAFETY: first\n",
            "// second line\n",
            "unsafe {}\n"
        ))
        .is_empty());
        assert_eq!(rules(concat!("/", "/ plain\n", "fn f() {}\n")).len(), 1);
        assert_eq!(rules(concat!("fn f() {} /", "/ trailing\n")).len(), 1);
        assert_eq!(rules(concat!("/", "* block */\n")).len(), 1);
        assert_eq!(
            rules(concat!("// SAFETY: a\n", "\n", "/", "/ detached\n")).len(),
            1
        );
        assert_eq!(rules(concat!("//", "// four slashes\n")).len(), 1);
        assert!(rules("fn f() { let s = \"// not a comment\"; }\n").is_empty());
    }

    #[test]
    fn allow_attributes_are_collected_and_checked_against_the_registry() {
        assert_eq!(
            lints("#[allow(clippy::expect_used, unsafe_code)]\n#![cfg_attr(test, allow(dead_code))]\n"),
            ["clippy::expect_used", "unsafe_code", "dead_code"]
        );
        let allows = vec![Allow {
            file: "src/a.rs".to_owned(),
            line: 3,
            lint: "unsafe_code".to_owned(),
        }];
        let registered = [Exception {
            file: "src/a.rs",
            lint: "unsafe_code",
            count: 1,
            reason: "test",
        }];
        assert!(check_exceptions(&allows, &registered).is_empty());
        assert_eq!(check_exceptions(&allows, &[]).len(), 1);
        let twice = [Exception {
            file: "src/a.rs",
            lint: "unsafe_code",
            count: 2,
            reason: "test",
        }];
        assert_eq!(check_exceptions(&allows, &twice).len(), 1);
        assert_eq!(check_exceptions(&[], &registered).len(), 1);
    }

    #[test]
    fn time_is_refused() {
        for source in [
            concat!("fn f() { std::thread::sl", "eep(d); }"),
            concat!("fn f() { let t = Inst", "ant::now(); }"),
            concat!("fn f(r: R) { r.recv_time", "out(x); }"),
            concat!("fn f(d: Dura", "tion) {}"),
            concat!("fn f() { let x = \"ping", " -n 5 127.0.0.1\"; }"),
            concat!("fn f() { WaitForSingleObject(h, 5", "000); }"),
        ] {
            assert_eq!(rules(source).len(), 1, "{source}");
        }
        assert!(
            rules("fn f() { WaitForSingleObject(h, INFINITE); WaitForSingleObject(h, 0); }")
                .is_empty()
        );
        assert!(rules("use x::{WaitForSingleObject, INFINITE, WAIT_TIMEOUT};").is_empty());
    }

    #[test]
    fn defaults_and_boxed_trait_objects_are_refused() {
        assert_eq!(rules(concat!("enum E { #[defa", "ult] A, B }")).len(), 1);
        assert_eq!(rules(concat!("struct S { f: Box<d", "yn Fn()> }")).len(), 1);
        assert!(rules("struct S { f: Box<u8> }").is_empty());
    }

    #[test]
    fn literals_do_not_confuse_the_lexer() {
        assert!(rules("fn f<'a>(x: &'a str) -> char { let _ = r#\"// \"#; '\\'' }").is_empty());
        assert!(rules("fn f() { let c = '\"'; let s = b\"x\"; }").is_empty());
    }
}
