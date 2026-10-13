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
        count: 1,
        reason: "sys is the only Win32 FFI boundary; it holds BorrowedPseudoConsole::borrow_raw and the test pseudoconsoles",
    },
    Exception {
        file: "src/sys.rs",
        lint: "clippy::as_conversions",
        count: 4,
        reason: "handle values and Win32 widths convert where Rust 1.75 has no From form",
    },
    Exception {
        file: "src/sys.rs",
        lint: "clippy::cast_possible_truncation",
        count: 1,
        reason: "dword_const is evaluated in constants, where an oversized value fails the build",
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

/// Library files that only tests compile; the library rules skip them as they skip test-only items.
const TEST_ONLY_FILES: [&str; 1] = ["src/failure_tests.rs"];

/// Why library code holds no shared or interior-mutable state; atomics, whose names start with `Atomic`, share it.
const SHARED_STATE: &str =
    "shares or mutates state outside the borrow checker; pass owned values and borrows";

/// Names library code doesn't use, each set with its reason (ADR 0016).
const LIBRARY_BANS: [(&[&str], &str); 4] = [
    (
        &[
            "Rc",
            "Arc",
            "Weak",
            "Cell",
            "RefCell",
            "OnceCell",
            "LazyCell",
            "UnsafeCell",
            "Mutex",
            "RwLock",
            "Condvar",
            "Barrier",
            "atomic",
        ],
        SHARED_STATE,
    ),
    (
        &[
            "static",
            "Once",
            "OnceLock",
            "LazyLock",
            "lazy_static",
            "thread_local",
        ],
        "keeps global state; pass the value to what needs it",
    ),
    (
        &["Any", "TypeId", "type_name", "type_name_of_val"],
        "inspects a type at run time; dispatch on an enum or a trait bound",
    ),
    (
        &["HashMap", "HashSet", "RandomState"],
        "iterates in a random order; use BTreeMap or BTreeSet",
    ),
];

/// A rule violation.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Finding {
    pub(crate) file: String,
    pub(crate) line: Option<usize>,
    pub(crate) rule: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(formatter, "{}:{line}: {}", self.file, self.rule),
            None => write!(formatter, "{}: {}", self.file, self.rule),
        }
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
                line: Some(allow.line),
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
                line: None,
                rule: format!(
                    "{} is registered {} time(s) ({}) but allowed {count} time(s)",
                    exception.lint, exception.count, exception.reason
                ),
            });
        }
    }
    findings
}

/// The offset of the token after the next one: `dyn` after `Box <`, the arguments after `name (`, and the bracket after `# !`.
const AFTER_NEXT: usize = 2;
/// Two adjacent tokens, such as `fn source` read backwards.
const PAIR: usize = 2;
/// The length of each comment delimiter: `//`, `/*`, and `*/`.
const DELIMITER: usize = 2;
/// The length of an escape: a backslash and the character it escapes.
const ESCAPE: usize = 2;
/// Where the closing quote of a one-character literal such as `'a'` sits after its opening quote.
const CLOSING_QUOTE: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommentKind {
    Doc,
    Plain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Token {
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
            line: Some(line),
            rule,
        });
    };
    check_comments(&tokens, &mut finding);
    let code: Vec<&(usize, Token)> = tokens
        .iter()
        .filter(|(_, token)| !matches!(token, Token::Comment { .. }))
        .collect();
    let guarded = test_only(&code);
    let library = file.starts_with("src/") && !TEST_ONLY_FILES.contains(&file);
    let mut allows = Vec::new();
    for (index, (line, token)) in code.iter().enumerate() {
        let next = |offset: usize| {
            code.get(index.saturating_add(offset))
                .map(|(_, following)| following)
        };
        match token {
            Token::Ident(name) => {
                if !guarded.get(index).copied().unwrap_or_default() {
                    if let Some(rule) = explicit_state_rule(&code, index, library) {
                        finding(*line, rule);
                    }
                }
                if is_time_name(name) {
                    finding(
                        *line,
                        format!("`{name}` uses time; wait for the event itself (ADR 0010)"),
                    );
                }
                if name == "Box"
                    && next(1) == Some(&Token::Punct('<'))
                    && next(AFTER_NEXT) == Some(&Token::Ident("dyn".to_owned()))
                {
                    finding(
                        *line,
                        "own a closed set with an enum or an open one with a generic, not Box<dyn>"
                            .to_owned(),
                    );
                }
                if name == "WaitForSingleObject" && next(1) == Some(&Token::Punct('(')) {
                    if let Some(argument) = second_argument(&code, index.saturating_add(AFTER_NEXT))
                    {
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
                    AFTER_NEXT
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

/// Returns the ADR 0016 rule that the identifier at `index` breaks.
///
/// A trait object is refused in any file, and the library bans only in library code.
fn explicit_state_rule(code: &[&(usize, Token)], index: usize, library: bool) -> Option<String> {
    let before = |offset: usize| {
        index
            .checked_sub(offset)
            .and_then(|at| code.get(at))
            .map(|(_, token)| token)
    };
    let Some(Token::Ident(name)) = before(0) else {
        return None;
    };
    if name == "dyn" {
        let boxed = before(AFTER_NEXT) == Some(&Token::Ident("Box".to_owned()))
            && before(1) == Some(&Token::Punct('<'));
        return (!boxed && !in_error_source(code, index)).then(|| {
            "a trait object hides which type runs; own a closed set with an enum or an open one with a generic (ADR 0016)"
                .to_owned()
        });
    }
    let lifetime = before(1) == Some(&Token::Punct('\''));
    if !library || lifetime {
        return None;
    }
    library_ban(name).map(|reason| format!("`{name}` {reason} (ADR 0016)"))
}

/// Returns why library code doesn't use `name`, if it doesn't.
fn library_ban(name: &str) -> Option<&'static str> {
    if name.starts_with("Atomic") {
        return Some(SHARED_STATE);
    }
    LIBRARY_BANS
        .iter()
        .find(|(names, _)| names.contains(&name))
        .map(|(_, reason)| *reason)
}

/// Returns true if the `dyn` at `index` names the `Error` trait in the return type of `source` in an `impl … Error for …` block:
/// the `Error::source` contract.
fn in_error_source(code: &[&(usize, Token)], index: usize) -> bool {
    let token_at = |at: usize| code.get(at).map(|(_, found)| found);
    let names_error = code
        .iter()
        .skip(index.saturating_add(1))
        .map(|(_, token)| token)
        .take_while(|token| matches!(token, Token::Ident(_) | Token::Punct(':')))
        .filter_map(|token| match token {
            Token::Ident(name) => Some(name.as_str()),
            Token::Literal(_) | Token::Str(_) | Token::Punct(_) | Token::Comment { .. } => None,
        })
        .last()
        == Some("Error");
    let Some(function) = (0..index)
        .rev()
        .take_while(|at| !matches!(token_at(*at), Some(Token::Punct('{' | '}' | ';'))))
        .find(|at| matches!(token_at(*at), Some(Token::Ident(keyword)) if keyword == "fn"))
    else {
        return false;
    };
    let name = function.saturating_add(1);
    names_error
        && matches!(token_at(name), Some(Token::Ident(source)) if source == "source")
        && return_arrow(code, name).is_some_and(|arrow| arrow < index)
        && in_error_impl(code, function)
}

/// Returns the index of the `>` of the `->` after the parameters of the function whose name is at `name`.
fn return_arrow(code: &[&(usize, Token)], name: usize) -> Option<usize> {
    let mut depth = 0_usize;
    let mut close = None;
    for (at, (_, token)) in code.iter().enumerate().skip(name) {
        if *token == Token::Punct('(') {
            depth = depth.saturating_add(1);
        } else if *token == Token::Punct(')') {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                close = Some(at);
                break;
            }
        }
    }
    let dash = close?.saturating_add(1);
    let arrow = dash.saturating_add(1);
    let punct = |at: usize, expected: char| {
        code.get(at)
            .is_some_and(|(_, token)| *token == Token::Punct(expected))
    };
    (punct(dash, '-') && punct(arrow, '>')).then_some(arrow)
}

/// Returns true if the item at `item` sits in the block of an `impl … Error for …`.
fn in_error_impl(code: &[&(usize, Token)], item: usize) -> bool {
    let mut depth = 0_usize;
    let mut opener = None;
    for (at, (_, token)) in code.iter().enumerate().take(item).rev() {
        if *token == Token::Punct('}') {
            depth = depth.saturating_add(1);
        } else if *token == Token::Punct('{') {
            if depth == 0 {
                opener = Some(at);
                break;
            }
            depth = depth.saturating_sub(1);
        }
    }
    let Some(opener) = opener else {
        return false;
    };
    let header: Vec<&Token> = code
        .iter()
        .take(opener)
        .rev()
        .map(|(_, token)| token)
        .take_while(|token| !matches!(token, Token::Punct('{' | '}' | ';')))
        .collect();
    header.contains(&&Token::Ident("impl".to_owned()))
        && header.windows(PAIR).any(|pair| {
            matches!(pair, [Token::Ident(keyword), Token::Ident(error)] if keyword == "for" && error == "Error")
        })
}

/// Marks each token of an item, statement, or field that a test-only `cfg` attribute guards.
fn test_only(code: &[&(usize, Token)]) -> Vec<bool> {
    let mut guarded = vec![false; code.len()];
    let mut index = 0_usize;
    while index < code.len() {
        let open = index.saturating_add(1);
        let attribute = matches!(code.get(index), Some((_, Token::Punct('#'))))
            && matches!(code.get(open), Some((_, Token::Punct('['))));
        if attribute && is_test_only_cfg(&bracketed(code, open)) {
            let end = item_end(code, open);
            for flag in guarded.iter_mut().take(end).skip(index) {
                *flag = true;
            }
            index = end;
        } else {
            index = open;
        }
    }
    guarded
}

/// Returns true if an attribute's tokens are `cfg(test)`, `cfg(kani)`, or `cfg(any(…))` over those two.
fn is_test_only_cfg(content: &[Token]) -> bool {
    let [Token::Ident(cfg), Token::Punct('('), predicate @ .., Token::Punct(')')] = content else {
        return false;
    };
    let members = match without_trailing_comma(predicate) {
        [Token::Ident(any), Token::Punct('('), members @ .., Token::Punct(')')] if any == "any" => {
            without_trailing_comma(members)
        }
        single => single,
    };
    cfg == "cfg"
        && members.split(|token| *token == Token::Punct(',')).all(
            |member| matches!(member, [Token::Ident(name)] if name == "test" || name == "kani"),
        )
}

/// Drops the one trailing comma Rust allows after the last element of a list.
fn without_trailing_comma(tokens: &[Token]) -> &[Token] {
    tokens.strip_suffix(&[Token::Punct(',')]).unwrap_or(tokens)
}

/// Returns true if `next`, the token after a closing brace, continues the statement the brace closed:
/// `else`, a method call, `?`, or the `;` or `,` that ends it.
fn continues(next: Option<&&(usize, Token)>) -> bool {
    match next.map(|(_, token)| token) {
        Some(Token::Ident(word)) => word == "else",
        Some(Token::Punct(punct)) => matches!(punct, '.' | '?' | ';' | ','),
        Some(Token::Literal(_) | Token::Str(_) | Token::Comment { .. }) | None => false,
    }
}

/// Returns true if what starts at `start`, past further attributes and a visibility,
/// is a `let` statement or a `static`, `const`, `use`, or `type` item, which only a `;` ends.
fn ends_at_semicolon(code: &[&(usize, Token)], start: usize) -> bool {
    let token_at = |at: usize| code.get(at).map(|(_, found)| found);
    let mut at = start;
    loop {
        match token_at(at) {
            Some(Token::Punct('#')) => at = group_end(code, at.saturating_add(1)),
            Some(Token::Ident(word)) if word == "pub" => {
                at = at.saturating_add(1);
                if token_at(at) == Some(&Token::Punct('(')) {
                    at = group_end(code, at);
                }
            }
            Some(Token::Ident(word)) if word == "const" => {
                return !matches!(
                    token_at(at.saturating_add(1)),
                    Some(Token::Ident(next)) if matches!(next.as_str(), "fn" | "unsafe" | "async" | "extern")
                );
            }
            Some(Token::Ident(word)) => {
                return matches!(word.as_str(), "let" | "static" | "use" | "type");
            }
            Some(Token::Literal(_) | Token::Str(_) | Token::Punct(_) | Token::Comment { .. })
            | None => {
                return false;
            }
        }
    }
}

/// Returns the index after the bracket that closes the one at `open`.
fn group_end(code: &[&(usize, Token)], open: usize) -> usize {
    let mut depth = 0_usize;
    for (index, (_, token)) in code.iter().enumerate().skip(open) {
        match token {
            Token::Punct('(' | '[' | '{') => depth = depth.saturating_add(1),
            Token::Punct(')' | ']' | '}') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return index.saturating_add(1);
                }
            }
            Token::Ident(_)
            | Token::Literal(_)
            | Token::Str(_)
            | Token::Punct(_)
            | Token::Comment { .. } => {}
        }
    }
    code.len()
}

/// Returns the index after what the attribute whose `[` is at `open` guards.
///
/// That ends with a `;` or `,` outside brackets, which alone ends a `let` statement and a `static`, `const`, `use`, or `type` item.
/// Another item ends with a `}` that closes its body when nothing continues the statement.
/// Either ends before the bracket that closes the enclosing block.
fn item_end(code: &[&(usize, Token)], open: usize) -> usize {
    let mut depth = 0_usize;
    let mut in_attribute = true;
    let mut semicolon = false;
    for (index, (_, token)) in code.iter().enumerate().skip(open) {
        match token {
            Token::Punct('(' | '[' | '{') => depth = depth.saturating_add(1),
            Token::Punct(close @ (')' | ']' | '}')) => {
                if depth == 0 {
                    return index;
                }
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if in_attribute {
                        in_attribute = false;
                        semicolon = ends_at_semicolon(code, index.saturating_add(1));
                    } else if *close == '}'
                        && !semicolon
                        && !continues(code.get(index.saturating_add(1)))
                    {
                        return index.saturating_add(1);
                    }
                }
            }
            Token::Punct(';' | ',') if depth == 0 && !in_attribute => {
                return index.saturating_add(1);
            }
            Token::Ident(_)
            | Token::Literal(_)
            | Token::Str(_)
            | Token::Punct(_)
            | Token::Comment { .. } => {}
        }
    }
    code.len()
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
pub(crate) fn lex(source: &str) -> Vec<(usize, Token)> {
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
            let body = text.get(DELIMITER..).unwrap_or_default().to_owned();
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
                    index = index.saturating_add(DELIMITER);
                } else if current == '*' && following == Some('/') {
                    depth = depth.saturating_sub(1);
                    text.push_str("*/");
                    index = index.saturating_add(DELIMITER);
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
            let closes_after_one =
                characters.get(index.saturating_add(CLOSING_QUOTE)) == Some(&'\'');
            if escaped || closes_after_one {
                let mut end = index.saturating_add(1);
                while let Some(&current) = characters.get(end) {
                    end = end.saturating_add(if current == '\\' { ESCAPE } else { 1 });
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
            index = index.saturating_add(ESCAPE);
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

    fn assert_clean(source: &str) {
        assert_eq!(rules(source), Vec::<String>::new(), "{source}");
    }

    #[test]
    fn only_doc_and_safety_comments_are_allowed() {
        assert_clean(concat!(
            "/// doc\n",
            "//! inner\n",
            "/** block doc */\n",
            "fn f() {}\n"
        ));
        assert_clean(concat!(
            "// SAFETY: first\n",
            "// second line\n",
            "unsafe {}\n"
        ));
        assert_eq!(rules(concat!("/", "/ plain\n", "fn f() {}\n")).len(), 1);
        assert_eq!(rules(concat!("fn f() {} /", "/ trailing\n")).len(), 1);
        assert_eq!(rules(concat!("/", "* block */\n")).len(), 1);
        assert_eq!(
            rules(concat!("// SAFETY: a\n", "\n", "/", "/ detached\n")).len(),
            1
        );
        assert_eq!(rules(concat!("//", "// four slashes\n")).len(), 1);
        assert_clean("fn f() { let s = \"// not a comment\"; }\n");
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
        assert_eq!(check_exceptions(&allows, &registered), Vec::new());
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
        assert_clean("fn f() { WaitForSingleObject(h, INFINITE); WaitForSingleObject(h, 0); }");
        assert_clean("use x::{WaitForSingleObject, INFINITE, WAIT_TIMEOUT};");
    }

    #[test]
    fn defaults_and_boxed_trait_objects_are_refused() {
        assert_eq!(rules(concat!("enum E { #[defa", "ult] A, B }")).len(), 1);
        assert_eq!(rules(concat!("struct S { f: Box<d", "yn Fn()> }")).len(), 1);
        assert_clean("struct S { f: Box<u8> }");
    }

    fn rules_in(file: &str, source: &str) -> Vec<String> {
        scan(file, source)
            .findings
            .into_iter()
            .map(|finding| finding.rule)
            .collect()
    }

    #[test]
    fn trait_objects_are_refused_outside_the_error_source_contract() {
        for source in [
            "fn f(x: &dyn Fn()) {}",
            "fn cause(&self) -> Option<&dyn Error> { None }",
            "impl Error for E { fn source(&self) -> Option<&(dyn Display + 'static)> { None } }",
            "impl Error for E { fn source(&self) {} fn g(x: &dyn Error) {} }",
            "impl Error for E { fn source(&self, x: &dyn Error) {} }",
            "fn source(&self) -> Option<&(dyn Error + 'static)> { None }",
            "impl Display for E { fn source(&self) -> Option<&(dyn Error + 'static)> { None } }",
            "m!(dyn T);",
            "struct S { f: Box<dyn Fn()> }",
        ] {
            assert_eq!(rules_in("tests/example.rs", source).len(), 1, "{source}");
        }
        for source in [
            "impl Error for E { fn source(&self) -> Option<&(dyn Error + 'static)> { None } }",
            "impl std::error::Error for E { fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { None } }",
            "impl<T> Error for W<T> { fn f() {} fn source(&self) -> Option<&(dyn Error + 'static)> { None } }",
            "#[cfg(test)] fn f(x: &dyn Fn()) {}",
        ] {
            assert_eq!(
                rules_in("tests/example.rs", source),
                Vec::<String>::new(),
                "{source}"
            );
        }
    }

    #[test]
    fn library_code_keeps_no_shared_global_reflected_or_hashed_state() {
        for source in [
            "use std::rc::Rc;",
            "fn f() { let x = Arc::new(1); }",
            "struct S { c: RefCell<u8> }",
            "fn f() { let x = AtomicBool::new(false); }",
            "fn f() { let m = Mutex::new(0); }",
            "static X: u8 = 0;",
            "thread_local! {}",
            "fn f() { let l = OnceLock::new(); }",
            "fn f() -> TypeId { g() }",
            "fn f() { let m = HashMap::new(); }",
            "#[cfg(not(test))] static X: u8 = 0;",
            "#[cfg(test)] COUNT.fetch_add(1); fn f() { let s = HashSet::new(); }",
            "#[cfg(test)] static X: u8 = 0; fn f() { let m = Mutex::new(0); }",
            "#[cfg(test)] let x = S { a: 1 }.with(Cell::new(1)); fn f() { let m = HashMap::new(); }",
            "#[cfg(test)] fn g() {} fn f() { let m = HashMap::new(); }",
            "#[cfg(test)] let x = { 1 }; fn f() { let m = HashMap::new(); }",
            "#[cfg(test)] const fn g() -> u8 { 1 } fn f() { let m = HashMap::new(); }",
            "#[cfg(test)] pub(crate) const unsafe fn g() {} fn f() { let m = HashMap::new(); }",
        ] {
            assert_eq!(rules_in("src/example.rs", source).len(), 1, "{source}");
            assert_eq!(rules_in("tests/example.rs", source), Vec::<String>::new());
            assert_eq!(
                rules_in("src/failure_tests.rs", source),
                Vec::<String>::new()
            );
        }
        for source in [
            "fn f(x: &'static str) -> Box<u8> { g() }",
            "const X: &'static str = \"\";",
            "#[cfg(test)] mod tests { use std::sync::Arc; }",
            "#[cfg(kani)] fn f() { let c = Cell::new(1); }",
            "#[cfg(any(test, kani))] static X: u8 = 0;",
            "struct S { #[cfg(test)] c: Cell<u8>, v: u8 }",
            "struct S { v: u8, #[cfg(test)] c: Cell<u8> }",
            "fn f() { #[cfg(test)] COUNT.fetch_add(1, atomic::Ordering::Relaxed); }",
            "fn f() { #[cfg(test)] let c = if a { Cell::new(1) } else { Cell::new(2) }; }",
            "fn f() { #[cfg(test)] let s = S { a: 1 }.with(RefCell::new(1))?; }",
            "match x { #[cfg(test)] A => { Cell::new(1) }, B => 2 }",
            "fn f() { #[cfg(test)] let value = { 1 } + Arc::strong_count(&arc); }",
            "#[cfg(test)] pub(crate) static S: u8 = { 1 } + Cell::new(1).get();",
            "#[cfg(test)] #[doc(hidden)] const N: usize = { 1 } + HashMap::<u8, u8>::new().len();",
            "#[cfg(test)] use std::{cell::Cell, sync::Arc};",
            "#[cfg(test)] type Shared = { Arc<u8> };",
        ] {
            assert_eq!(
                rules_in("src/example.rs", source),
                Vec::<String>::new(),
                "{source}"
            );
        }
    }

    #[test]
    fn only_cfg_test_and_kani_guard_test_code() {
        let content = |source: &str| {
            lex(source)
                .into_iter()
                .map(|(_, token)| token)
                .collect::<Vec<_>>()
        };
        for guard in [
            "cfg(test)",
            "cfg(kani)",
            "cfg(any(test, kani))",
            "cfg(any(test))",
            "cfg(any(test,))",
            "cfg(any(test, kani,))",
            "cfg(test,)",
        ] {
            assert!(is_test_only_cfg(&content(guard)), "{guard}");
        }
        for guard in [
            "cfg(not(test))",
            "cfg(any(test, windows))",
            "cfg(all(test, kani))",
            "cfg()",
            "cfg(any())",
            "cfg(any(test,,kani))",
            "cfg(any(,))",
            "cfg_attr(test, allow(x))",
            "test",
        ] {
            assert!(!is_test_only_cfg(&content(guard)), "{guard}");
        }
    }

    #[test]
    fn literals_do_not_confuse_the_lexer() {
        assert_clean("fn f<'a>(x: &'a str) -> char { let _ = r#\"// \"#; '\\'' }");
        assert_clean("fn f() { let c = '\"'; let s = b\"x\"; }");
    }
}
