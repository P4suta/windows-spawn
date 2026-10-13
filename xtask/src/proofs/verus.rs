//! The Verus layer: the crate's command-line encoder proved, for every argument, to write what the Lean model writes.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::tools::{unpack, VERUS};
use super::{fresh_directory, observe, succeeded};
use crate::gates::{lex, Token};
use crate::tasks::{fail, Result};

/// The copy Verus checks, and the source it copies.
const PROOF: &str = "proofs/verus/command_line.rs";
const SOURCE: &str = "src/domain/command_line_rules.rs";

/// The functions the copy holds, which must equal the source's once the Verus attributes and proof blocks are removed.
const FUNCTIONS: [&str; 4] = [
    "needs_quotes",
    "append_program",
    "append_argument",
    "put_backslashes",
];

/// What Verus reports for the copy: the four functions, `put` for `Vec<u16>`, the lemma, and the obligations Verus adds.
const VERIFIED: &str = "verification results:: 16 verified, 0 errors";

/// The negative control drops the doubling of the backslashes before the closing quote,
/// which `encode` requires, so the specification must refuse it.
const DOUBLING: &str = "        put_backslashes(line, backslashes);\n        line.put(QUOTE);";
const WITHOUT_DOUBLING: &str = "        line.put(QUOTE);";

/// Proves the copy, requires a defect in it to be refused, and requires the copy to be the crate's code.
pub(crate) fn verus(root: &Path) -> Result<()> {
    let verus = install(root)?;
    let proved = observe(&mut verifier(&verus, &root.join("proofs/verus")))?;
    let report = transcript(&proved);
    if !proved.status.success() || !report.contains(VERIFIED) {
        return fail(format!("Verus must report `{VERIFIED}`:\n{report}"));
    }

    let copy = read_lf(&root.join(PROOF))?;
    if copy.matches(DOUBLING).count() != 1 {
        return fail(format!(
            "{PROOF} must close a quoted argument exactly once with:\n{DOUBLING}"
        ));
    }
    let control = fresh_directory(&root.join("target/proofs"), "verus-control")?;
    fs::write(
        control.join("command_line.rs"),
        copy.replacen(DOUBLING, WITHOUT_DOUBLING, 1),
    )?;
    let refused = observe(&mut verifier(&verus, &control))?;
    let refusal = transcript(&refused);
    if refused.status.success() || !refusal.contains("postcondition not satisfied") {
        return fail(format!(
            "Verus accepted an encoder that leaves the closing backslashes single:\n{refusal}"
        ));
    }

    let source = read_lf(&root.join(SOURCE))?;
    same_functions(&source, &copy).map_err(crate::tasks::TaskError::from)?;
    println!(
        "Verus: {} functions of {SOURCE} proved for every argument, and the defect control refused",
        FUNCTIONS.len()
    );
    Ok(())
}

/// Reads `path` with LF line endings, which a Windows checkout with `core.autocrlf` replaces with CRLF.
fn read_lf(path: &Path) -> std::io::Result<String> {
    Ok(fs::read_to_string(path)?.replace("\r\n", "\n"))
}

/// Unpacks the pinned release for this host and installs the Rust toolchain it names.
fn install(root: &Path) -> Result<PathBuf> {
    let Some((_, _, archive)) = VERUS
        .iter()
        .find(|(os, arch, _)| *os == std::env::consts::OS && *arch == std::env::consts::ARCH)
    else {
        return fail(format!(
            "Verus publishes no release for {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
    };
    let unpacked = unpack(root, *archive)?;
    let folder = fs::read_dir(&unpacked)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| path.join("version.json").is_file())
        .ok_or_else(|| {
            crate::tasks::TaskError::from("the Verus release holds no version.json".to_owned())
        })?;
    let version: serde_json::Value =
        serde_json::from_slice(&fs::read(folder.join("version.json"))?)?;
    let toolchain = version
        .pointer("/verus/toolchain")
        .and_then(serde_json::Value::as_str)
        .and_then(|toolchain| toolchain.split_whitespace().next())
        .ok_or_else(|| {
            crate::tasks::TaskError::from("version.json names no toolchain".to_owned())
        })?;
    let mut rustup = Command::new("rustup");
    rustup.args(["toolchain", "install", toolchain, "--profile", "minimal"]);
    succeeded("rustup toolchain install", &observe(&mut rustup)?)?;
    Ok(folder.join("verus"))
}

/// Runs Verus on `command_line.rs` in `directory` with the toolchain the release names.
fn verifier(verus: &Path, directory: &Path) -> Command {
    let mut command = Command::new(verus);
    command
        .current_dir(directory)
        .arg("command_line.rs")
        .env_remove("RUSTUP_TOOLCHAIN");
    command
}

fn transcript(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Requires each function of [`FUNCTIONS`] to have the same tokens in `source` and in `copy`,
/// once the comments, the attributes, and the Verus blocks are removed.
fn same_functions(source: &str, copy: &str) -> std::result::Result<(), String> {
    let source = code(source);
    let copy = code(copy);
    for name in FUNCTIONS {
        let original =
            function(&source, name).ok_or_else(|| format!("{SOURCE} has no fn {name}"))?;
        let proved = function(&copy, name).ok_or_else(|| format!("{PROOF} has no fn {name}"))?;
        if original != proved {
            return Err(format!(
                "fn {name} differs between {SOURCE} and {PROOF}:\n  source: {}\n  proved: {}",
                render(original),
                render(proved)
            ));
        }
    }
    Ok(())
}

/// The tokens of `source` without comments, attributes, and the Verus blocks: `verus!`, which holds specifications and lemmas,
/// and `proof!` and `proof_decl!`, which hold proof steps inside the code.
fn code(source: &str) -> Vec<Token> {
    let tokens: Vec<Token> = lex(source)
        .into_iter()
        .map(|(_, token)| token)
        .filter(|token| !matches!(token, Token::Comment { .. }))
        .collect();
    let mut kept = Vec::new();
    let mut index = 0_usize;
    while let Some(token) = tokens.get(index) {
        let next = index.saturating_add(1);
        if *token == Token::Punct('#') {
            let open = if tokens.get(next) == Some(&Token::Punct('!')) {
                next.saturating_add(1)
            } else {
                next
            };
            if tokens.get(open) == Some(&Token::Punct('[')) {
                index = after_group(&tokens, open, '[', ']');
                continue;
            }
        }
        if matches!(token, Token::Ident(name) if name == "proof" || name == "proof_decl" || name == "verus")
            && tokens.get(next) == Some(&Token::Punct('!'))
        {
            index = after_group(&tokens, next.saturating_add(1), '{', '}');
            if tokens.get(index) == Some(&Token::Punct(';')) {
                index = index.saturating_add(1);
            }
            continue;
        }
        kept.push(token.clone());
        index = next;
    }
    kept
}

/// The index just past the group that opens at `open`, or the end when it never closes.
fn after_group(tokens: &[Token], open: usize, opening: char, closing: char) -> usize {
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if *token == Token::Punct(opening) {
            depth = depth.saturating_add(1);
        } else if *token == Token::Punct(closing) {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return index.saturating_add(1);
            }
        }
    }
    tokens.len()
}

/// The tokens of the first `fn name`, from its keyword through its body.
fn function<'a>(tokens: &'a [Token], name: &str) -> Option<&'a [Token]> {
    let start = tokens
        .iter()
        .zip(tokens.iter().skip(1))
        .position(|pair| {
            matches!(pair, (Token::Ident(keyword), Token::Ident(ident)) if keyword == "fn" && ident == name)
        })?;
    let body = tokens
        .iter()
        .skip(start)
        .position(|token| *token == Token::Punct('{'))?;
    let end = after_group(tokens, start.saturating_add(body), '{', '}');
    tokens.get(start..end)
}

fn render(tokens: &[Token]) -> String {
    let mut text = String::new();
    for token in tokens {
        let written = match token {
            Token::Ident(word) | Token::Literal(word) => write!(text, "{word} "),
            Token::Str(content) => write!(text, "{content:?} "),
            Token::Punct(character) => write!(text, "{character}"),
            Token::Comment { .. } => Ok(()),
        };
        if written.is_err() {
            break;
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE_TEXT: &str = "/// Docs.\n#[must_use]\npub(crate) fn twice(x: usize) -> usize {\n    // Plain.\n    x + x\n}\nfn other() {}\n";

    #[test]
    fn verus_annotations_vanish_and_code_must_match() {
        let copy = "#![feature(proc_macro_hygiene)]\nverus! { spec fn twice() {} }\n#[verus_spec(r => ensures r == 2 * x, seq![x][0] == x)]\npub(crate) fn twice(x: usize) -> usize {\n    proof! { assert(true); }\n    proof_decl! { let ghost y = x; }\n    #[verus_spec(invariant true)]\n    x + x\n}\n";
        let source = code(SOURCE_TEXT);
        let copied = code(copy);
        assert!(function(&source, "twice").is_some());
        assert_eq!(function(&source, "twice"), function(&copied, "twice"));
        assert!(function(&source, "missing").is_none());

        let altered = code(&copy.replace("x + x", "x * 2"));
        assert_ne!(function(&source, "twice"), function(&altered, "twice"));
        assert!(render(function(&source, "twice").unwrap()).contains("x +x"));
    }

    #[test]
    fn a_crlf_checkout_reads_as_lf() {
        let directory = fresh_directory(&std::env::temp_dir(), "verus-crlf").unwrap();
        let path = directory.join("command_line.rs");
        fs::write(&path, DOUBLING.replace('\n', "\r\n")).unwrap();
        assert_eq!(read_lf(&path).unwrap(), DOUBLING);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn the_copy_is_the_crate_code() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let source = read_lf(&root.join(SOURCE)).unwrap();
        let copy = read_lf(&root.join(PROOF)).unwrap();
        assert_eq!(same_functions(&source, &copy), Ok(()));
        assert_eq!(copy.matches(DOUBLING).count(), 1);
        let drifted = copy.replacen("backslashes = 0;", "backslashes = 1;", 1);
        assert!(same_functions(&source, &drifted).is_err());
    }
}
