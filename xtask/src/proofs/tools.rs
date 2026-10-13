//! The verifiers the gates download: each one pinned to a release asset and its SHA-256.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use super::{observe, succeeded};
use crate::tasks::{fail, Result};

/// A release asset a gate runs, pinned by its digest.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tool {
    pub(crate) file: &'static str,
    pub(crate) url: &'static str,
    pub(crate) sha256: &'static str,
}

/// TLC, the explicit-state model checker of the TLA+ tools.
pub(crate) const TLA2TOOLS: Tool = Tool {
    file: "tla2tools-1.7.4.jar",
    url: "https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar",
    sha256: "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88",
};

/// The Alloy Analyzer, whose `exec` command runs every command of a model through a SAT solver.
pub(crate) const ALLOY: Tool = Tool {
    file: "org.alloytools.alloy.dist-6.2.0.jar",
    url: "https://github.com/AlloyTools/org.alloytools.alloy/releases/download/v6.2.0/org.alloytools.alloy.dist.jar",
    sha256: "6b8c1cb5bc93bedfc7c61435c4e1ab6e688a242dc702a394628d9a9801edb78d",
};

/// The Verus release for each host it supports, as (OS, architecture, archive).
pub(crate) const VERUS: [(&str, &str, Tool); 4] = [
    (
        "macos",
        "aarch64",
        Tool {
            file: "verus-0.2026.10.04.426d8b0-arm64-macos.zip",
            url: "https://github.com/verus-lang/verus/releases/download/release/0.2026.10.04.426d8b0/verus-0.2026.10.04.426d8b0-arm64-macos.zip",
            sha256: "83cafc50284caea07fa8762df04b00cc0af441f25cdf1631d6c30987c65af446",
        },
    ),
    (
        "macos",
        "x86_64",
        Tool {
            file: "verus-0.2026.10.04.426d8b0-x86-macos.zip",
            url: "https://github.com/verus-lang/verus/releases/download/release/0.2026.10.04.426d8b0/verus-0.2026.10.04.426d8b0-x86-macos.zip",
            sha256: "9b00357ee298a3b052038abc85c0be3129ab2821762fe8be5eb8dcbe37349f9c",
        },
    ),
    (
        "linux",
        "x86_64",
        Tool {
            file: "verus-0.2026.10.04.426d8b0-x86-linux.zip",
            url: "https://github.com/verus-lang/verus/releases/download/release/0.2026.10.04.426d8b0/verus-0.2026.10.04.426d8b0-x86-linux.zip",
            sha256: "624018662868563a7bec0dd07be5e4dd0c3d61124b4b55105765884bf2d4d1a0",
        },
    ),
    (
        "windows",
        "x86_64",
        Tool {
            file: "verus-0.2026.10.04.426d8b0-x86-win.zip",
            url: "https://github.com/verus-lang/verus/releases/download/release/0.2026.10.04.426d8b0/verus-0.2026.10.04.426d8b0-x86-win.zip",
            sha256: "b36968a0f333036e7e1770bb73fdd407a35c8fd163124c1d309972574150da61",
        },
    ),
];

/// Returns the directory that `archive`, a pinned zip file, unpacks into under `target/proofs/tools`.
pub(crate) fn unpack(root: &Path, archive: Tool) -> Result<PathBuf> {
    let zip = fetch(root, archive)?;
    let directory = zip.with_extension("");
    let marker = directory.join(".unpacked");
    if marker.is_file() {
        return Ok(directory);
    }
    if directory.exists() {
        fs::remove_dir_all(&directory)?;
    }
    let mut command = Command::new("unzip");
    command
        .args(["-q", "-o"])
        .arg(&zip)
        .arg("-d")
        .arg(&directory);
    succeeded("unzip", &observe(&mut command)?)?;
    fs::write(&marker, archive.sha256)?;
    Ok(directory)
}

/// Returns the path of `tool` under `target/proofs/tools`, downloading it first when it is missing or altered.
pub(crate) fn fetch(root: &Path, tool: Tool) -> Result<PathBuf> {
    let directory = root.join("target/proofs/tools");
    fs::create_dir_all(&directory)?;
    let path = directory.join(tool.file);
    if path.is_file() && digest(&path)? == tool.sha256 {
        return Ok(path);
    }
    let partial = directory.join(format!("{}.partial", tool.file));
    let mut command = Command::new("curl");
    command
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--output",
        ])
        .arg(&partial)
        .arg(tool.url);
    let downloaded = observe(&mut command)?;
    succeeded("the download", &downloaded)?;
    let actual = digest(&partial)?;
    if actual != tool.sha256 {
        fs::remove_file(&partial)?;
        return fail(format!(
            "{} has SHA-256 {actual}, not the pinned {}",
            tool.url, tool.sha256
        ));
    }
    fs::rename(&partial, &path)?;
    Ok(path)
}

fn digest(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    Ok(crate::tasks::lower_hex(&Sha256::digest(&bytes)))
}
