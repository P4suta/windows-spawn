use crate::cli::{SimpleTask, Task};
use semver::Version;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};
use std::string::FromUtf8Error;
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

pub(crate) type Result<T> = std::result::Result<T, TaskError>;

#[derive(Debug)]
pub(crate) enum TaskError {
    Io(io::Error),
    Json(serde_json::Error),
    Utf8(FromUtf8Error),
    Semver(semver::Error),
    SystemTime(SystemTimeError),
    Message(String),
}

impl fmt::Display for TaskError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Json(error) => error.fmt(formatter),
            Self::Utf8(error) => error.fmt(formatter),
            Self::Semver(error) => error.fmt(formatter),
            Self::SystemTime(error) => error.fmt(formatter),
            Self::Message(message) => formatter.write_str(message),
        }
    }
}

impl Error for TaskError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Utf8(error) => Some(error),
            Self::Semver(error) => Some(error),
            Self::SystemTime(error) => Some(error),
            Self::Message(_) => None,
        }
    }
}

impl From<io::Error> for TaskError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for TaskError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<FromUtf8Error> for TaskError {
    fn from(error: FromUtf8Error) -> Self {
        Self::Utf8(error)
    }
}

impl From<semver::Error> for TaskError {
    fn from(error: semver::Error) -> Self {
        Self::Semver(error)
    }
}

impl From<SystemTimeError> for TaskError {
    fn from(error: SystemTimeError) -> Self {
        Self::SystemTime(error)
    }
}

impl From<String> for TaskError {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

const PACKAGE_NAME: &str = "windows-spawn";
const PUBLIC_API_TOOLCHAIN: &str = "nightly-2026-07-02";

#[derive(Clone, Debug, Eq, PartialEq)]
struct PackageInfo {
    name: String,
    version: Version,
}

pub(crate) fn execute(task: Task) -> Result<i32> {
    let root = repository_root()?;
    match task {
        Task::Simple(task) => run_simple(&root, task)?,
        Task::PublicApi { update } => public_api(&root, update)?,
        Task::PackageCheck { allow_dirty } => package_check(&root, allow_dirty)?,
        Task::Sbom { output } => {
            let output = output.unwrap_or_else(|| PathBuf::from("target/release-candidate"));
            generate_sboms(&root, &output, true)?;
        }
        Task::ReleaseCandidate {
            allow_dirty,
            github_output,
        } => release_candidate(&root, allow_dirty, github_output)?,
        Task::VerifyReleaseTag { tag } => verify_release_tag(&root, &tag)?,
        Task::Mutants { output, forwarded } => return run_mutants(&root, output, &forwarded),
        Task::DraftRelease { tag, github_output } => {
            draft_release(&root, &tag, github_output)?;
        }
        Task::CratesIoAuthMode { github_output } => crates_io_auth_mode(github_output)?,
        Task::Help => print_help(),
    }
    Ok(0)
}

fn print_help() {
    println!(
        "\
Repository tasks:
  cargo xtask fmt|clippy|test|doc|msrv|cross-targets|linux-empty
  cargo xtask supply-chain|reuse|typos|coverage|ci
  cargo xtask public-api [--update]
  cargo xtask package-check [--allow-dirty]
  cargo xtask sbom [--output DIR]
  cargo xtask release-candidate [--allow-dirty] [--github-output]
  cargo xtask verify-release-tag TAG
  cargo xtask mutants [--output DIR] -- [cargo-mutants arguments]
  cargo xtask draft-release TAG [--github-output]
  cargo xtask crates-io-auth-mode [--github-output]"
    );
}

fn run_simple(root: &Path, task: SimpleTask) -> Result<()> {
    match task {
        SimpleTask::Fmt => run_cargo(root, &["fmt", "--all", "--", "--check"]),
        SimpleTask::Clippy => run_cargo(
            root,
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--locked",
                "--",
                "-D",
                "warnings",
            ],
        ),
        SimpleTask::Test => {
            run_cargo(
                root,
                &[
                    "test",
                    "--workspace",
                    "--all-targets",
                    "--locked",
                    "--",
                    "--test-threads=1",
                ],
            )?;
            run_cargo(root, &["test", "--workspace", "--doc", "--locked"])
        }
        SimpleTask::Doc => {
            let mut command = cargo(root);
            command
                .args(["doc", "--workspace", "--no-deps", "--locked"])
                .env("RUSTDOCFLAGS", "-D warnings");
            run(&mut command)
        }
        SimpleTask::Msrv => run_cargo_with_toolchain(
            root,
            "1.75",
            &["check", "--workspace", "--all-targets", "--locked"],
        ),
        SimpleTask::CrossTargets => {
            for target in [
                "x86_64-pc-windows-msvc",
                "i686-pc-windows-msvc",
                "aarch64-pc-windows-msvc",
            ] {
                run_cargo(
                    root,
                    &[
                        "check",
                        "--package",
                        PACKAGE_NAME,
                        "--locked",
                        "--target",
                        target,
                    ],
                )?;
            }
            Ok(())
        }
        SimpleTask::LinuxEmpty => run_cargo(
            root,
            &[
                "check",
                "--package",
                PACKAGE_NAME,
                "--all-targets",
                "--locked",
                "--target",
                "x86_64-unknown-linux-gnu",
            ],
        ),
        SimpleTask::SupplyChain => {
            run_cargo(root, &["deny", "--all-features", "--locked", "check"])
        }
        SimpleTask::Reuse => run_program(root, "python", &["-m", "reuse", "lint"]),
        SimpleTask::Typos => run_program(root, "typos", &[]),
        SimpleTask::Coverage => coverage(root),
        SimpleTask::Ci => run_ci(root),
    }
}

fn run_ci(root: &Path) -> Result<()> {
    for check in [
        SimpleTask::Fmt,
        SimpleTask::Clippy,
        SimpleTask::Test,
        SimpleTask::Doc,
        SimpleTask::Msrv,
        SimpleTask::CrossTargets,
        SimpleTask::LinuxEmpty,
        SimpleTask::SupplyChain,
        SimpleTask::Reuse,
        SimpleTask::Typos,
    ] {
        run_simple(root, check)?;
    }
    public_api(root, false)?;
    package_check(root, false)
}

fn coverage(root: &Path) -> Result<()> {
    run_cargo(root, &["llvm-cov", "clean", "--workspace"])?;
    run_cargo(
        root,
        &[
            "llvm-cov",
            "--package",
            PACKAGE_NAME,
            "--all-targets",
            "--locked",
            "--",
            "--test-threads=1",
        ],
    )?;
    run_cargo(
        root,
        &[
            "llvm-cov",
            "report",
            "--fail-under-lines",
            "92",
            "--fail-under-regions",
            "92",
            "--fail-under-functions",
            "92",
        ],
    )
}

fn public_api(root: &Path, update: bool) -> Result<()> {
    let mut command = cargo_with_toolchain(root, PUBLIC_API_TOOLCHAIN);
    command.args(["public-api", "--package", PACKAGE_NAME, "--simplified"]);
    let actual = capture(&mut command)?;
    let snapshot = root.join("public-api/windows-spawn.txt");
    if update {
        fs::write(&snapshot, actual.as_bytes())?;
        println!("updated {}", snapshot.display());
        return Ok(());
    }

    let expected = fs::read_to_string(&snapshot)?;
    compare_snapshot(&expected, &actual).map_err(TaskError::from)
}

fn compare_snapshot(expected: &str, actual: &str) -> std::result::Result<(), String> {
    let expected: Vec<&str> = expected.lines().collect();
    let actual: Vec<&str> = actual.lines().collect();
    if expected == actual {
        return Ok(());
    }

    let first = expected
        .iter()
        .zip(&actual)
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| expected.len().min(actual.len()));
    Err(format!(
        "public API differs at line {}\nexpected: {}\n  actual: {}\nrun `cargo xtask public-api --update` for an intentional change",
        first + 1,
        expected.get(first).copied().unwrap_or("<end of file>"),
        actual.get(first).copied().unwrap_or("<end of file>")
    ))
}

fn package_check(root: &Path, allow_dirty: bool) -> Result<()> {
    cargo_package(root, allow_dirty)?;
    check_packaged_reuse(root)
}

fn cargo_package(root: &Path, allow_dirty: bool) -> Result<()> {
    let mut arguments = vec!["package", "--package", PACKAGE_NAME, "--locked"];
    if allow_dirty {
        arguments.push("--allow-dirty");
    }
    run_cargo(root, &arguments)
}

fn check_packaged_reuse(root: &Path) -> Result<()> {
    let package = root_package(root)?;
    let expanded = expanded_package_path(root, &package);
    if !expanded.is_dir() {
        return fail(format!(
            "expanded package not found: {}; run cargo package first",
            expanded.display()
        ));
    }
    run_program(&expanded, "python", &["-m", "reuse", "lint"])
}

fn prepare_sbom_package(root: &Path) -> Result<()> {
    run_cargo(
        root,
        &[
            "package",
            "--package",
            PACKAGE_NAME,
            "--locked",
            "--allow-dirty",
            "--no-verify",
        ],
    )
}

fn expanded_package_path(root: &Path, package: &PackageInfo) -> PathBuf {
    root.join("target/package")
        .join(format!("{}-{}", package.name, package.version))
}

#[derive(Debug)]
struct SbomArtifacts {
    cyclone_dx: PathBuf,
    reuse_spdx: PathBuf,
}

fn generate_sboms(
    root: &Path,
    requested_output: &Path,
    prepare_package: bool,
) -> Result<SbomArtifacts> {
    let output = resolve_directory(root, requested_output)?;
    let package = root_package(root)?;
    if prepare_package {
        prepare_sbom_package(root)?;
    }
    let package_root = expanded_package_path(root, &package);
    if !package_root.is_dir() {
        return fail(format!(
            "expanded package not found: {}; run cargo package first",
            package_root.display()
        ));
    }
    let base_name = format!("{}-{}.cdx", package.name, package.version);
    let cyclone_name = format!("{base_name}.json");
    let generated = package_root.join(&cyclone_name);
    let cyclone_dx = output.join(&cyclone_name);
    let reuse_spdx = output.join(format!("{}-{}.reuse.spdx", package.name, package.version));

    if generated.exists() && generated != cyclone_dx {
        fs::remove_file(&generated)?;
    }

    let result = (|| {
        let mut cyclone = cargo(root);
        cyclone
            .arg("cyclonedx")
            .arg("--manifest-path")
            .arg(package_root.join("Cargo.toml"))
            .args([
                "--format",
                "json",
                "--spec-version",
                "1.5",
                "--target",
                "all",
                "--all-features",
                "--no-build-deps",
                "--override-filename",
                &base_name,
            ]);
        run(&mut cyclone)?;
        if !generated.is_file() {
            return fail(format!(
                "cargo cyclonedx did not create {}",
                generated.display()
            ));
        }
        if generated != cyclone_dx {
            fs::copy(&generated, &cyclone_dx)?;
            fs::remove_file(&generated)?;
        }

        let mut reuse = Command::new("python");
        reuse
            .current_dir(root)
            .args(["-m", "reuse", "spdx", "-o"])
            .arg(&reuse_spdx);
        run(&mut reuse)?;

        validate_cyclonedx(&cyclone_dx, &package)?;
        validate_reuse_spdx(&reuse_spdx, &package)?;
        Ok(SbomArtifacts {
            cyclone_dx: cyclone_dx.clone(),
            reuse_spdx,
        })
    })();

    if generated.exists() && generated != cyclone_dx {
        fs::remove_file(&generated)?;
    }
    let artifacts = result?;
    println!("generated and validated:");
    println!("  {}", artifacts.cyclone_dx.display());
    println!("  {}", artifacts.reuse_spdx.display());
    Ok(artifacts)
}

fn validate_cyclonedx(path: &Path, package: &PackageInfo) -> Result<()> {
    let document: Value = serde_json::from_slice(&fs::read(path)?)?;
    if document.get("bomFormat").and_then(Value::as_str) != Some("CycloneDX")
        || document.get("specVersion").and_then(Value::as_str) != Some("1.5")
    {
        return fail("CycloneDX SBOM is not JSON conforming to specification 1.5");
    }
    let component = document
        .pointer("/metadata/component")
        .ok_or_else(|| invalid_data("CycloneDX SBOM has no root component"))?;
    if component.get("name").and_then(Value::as_str) != Some(package.name.as_str())
        || component.get("version").and_then(Value::as_str)
            != Some(package.version.to_string().as_str())
    {
        return fail("CycloneDX SBOM has incorrect root package metadata");
    }
    require_licenses(component)?;

    let components = document
        .get("components")
        .and_then(Value::as_array)
        .filter(|components| !components.is_empty())
        .ok_or_else(|| invalid_data("CycloneDX SBOM has no dependency components"))?;
    for dependency in components {
        require_licenses(dependency)?;
    }
    if document
        .get("dependencies")
        .and_then(Value::as_array)
        .map_or(true, Vec::is_empty)
    {
        return fail("CycloneDX SBOM has no dependency relationships");
    }
    Ok(())
}

fn require_licenses(component: &Value) -> Result<()> {
    if component
        .get("licenses")
        .and_then(Value::as_array)
        .map_or(true, Vec::is_empty)
    {
        let name = component
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");
        return fail(format!("CycloneDX component lacks license data: {name}"));
    }
    Ok(())
}

fn validate_reuse_spdx(path: &Path, package: &PackageInfo) -> Result<()> {
    let document = fs::read_to_string(path)?;
    validate_reuse_spdx_text(&document, &package.name).map_err(TaskError::from)
}

fn validate_reuse_spdx_text(document: &str, package_name: &str) -> std::result::Result<(), String> {
    let document_name = format!("DocumentName: {package_name}");
    let valid = document.lines().any(|line| line == "SPDXVersion: SPDX-2.1")
        && document.lines().any(|line| line == document_name)
        && document
            .lines()
            .any(|line| line.starts_with("LicenseInfoInFile: "));
    if valid {
        Ok(())
    } else {
        Err("REUSE SPDX SBOM is missing format, package, or license information".to_owned())
    }
}

fn release_candidate(root: &Path, allow_dirty: bool, github_output: bool) -> Result<()> {
    let output = root.join("target/release-candidate");
    validate_release_output(root, &output)?;
    if output.exists() {
        fs::remove_dir_all(&output)?;
    }
    fs::create_dir_all(&output)?;

    if allow_dirty {
        eprintln!("warning: generating a non-release candidate from a dirty worktree");
    }
    let package = root_package(root)?;
    let crate_name = format!("{}-{}.crate", package.name, package.version);
    let cargo_crate = root.join("target/package").join(&crate_name);
    let candidate_crate = output.join(&crate_name);

    cargo_package(root, allow_dirty)?;
    fs::copy(&cargo_crate, &candidate_crate)?;
    let first_hash = sha256(&candidate_crate)?;
    cargo_package(root, allow_dirty)?;
    let second_hash = sha256(&cargo_crate)?;
    if first_hash != second_hash {
        return fail(format!(
            "cargo package is not reproducible: {first_hash} differs from {second_hash}"
        ));
    }

    check_packaged_reuse(root)?;
    let sboms = generate_sboms(root, &output, false)?;
    let artifacts = release_artifacts(&output)?;
    if artifacts.len() != 3 {
        return fail(format!(
            "expected one crate and two SBOMs, found {}",
            artifacts.len()
        ));
    }
    write_checksums(&output.join("SHA256SUMS"), &artifacts)?;
    if github_output {
        write_github_output("sbom", &sboms.cyclone_dx)?;
    }

    println!("release candidate is reproducible and validated:");
    for artifact in all_files_sorted(&output)? {
        println!("  {}", artifact.display());
    }
    Ok(())
}

fn validate_release_output(root: &Path, output: &Path) -> Result<()> {
    let expected = normalize_path(&root.join("target/release-candidate"));
    if normalize_path(output) != expected {
        return fail(format!(
            "refusing to clean unexpected output directory: {}",
            output.display()
        ));
    }
    let canonical_root = root.canonicalize()?;
    if output.exists() {
        let metadata = fs::symlink_metadata(output)?;
        if metadata.file_type().is_symlink() || !output.canonicalize()?.starts_with(&canonical_root)
        {
            return fail(format!(
                "refusing to clean output outside the repository: {}",
                output.display()
            ));
        }
    } else {
        let parent = output
            .parent()
            .ok_or_else(|| invalid_data("release output has no parent"))?;
        fs::create_dir_all(parent)?;
        if !parent.canonicalize()?.starts_with(&canonical_root) {
            return fail(format!(
                "refusing to create output outside the repository: {}",
                output.display()
            ));
        }
    }
    Ok(())
}

fn release_artifacts(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut artifacts: Vec<PathBuf> = all_files_sorted(directory)?
        .into_iter()
        .filter(|path| {
            let name = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
            is_release_artifact_name(name)
        })
        .collect();
    artifacts.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    Ok(artifacts)
}

fn is_release_artifact_name(name: &str) -> bool {
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    match extension {
        "crate" => true,
        "json" => stem.rsplit_once('.').is_some_and(|(_, kind)| kind == "cdx"),
        "spdx" => stem
            .rsplit_once('.')
            .is_some_and(|(_, kind)| kind == "reuse"),
        _ => false,
    }
}

fn all_files_sorted(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            files.push(entry.path());
        }
    }
    files.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    Ok(files)
}

fn write_checksums(path: &Path, artifacts: &[PathBuf]) -> Result<()> {
    let mut checksum_file = File::create(path)?;
    for artifact in artifacts {
        let name = artifact
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| invalid_data("release artifact name is not Unicode"))?;
        writeln!(checksum_file, "{}  {name}", sha256(artifact)?)?;
    }
    Ok(())
}

fn sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn verify_release_tag(root: &Path, tag: &str) -> Result<()> {
    let version = parse_release_tag(tag).map_err(|error| invalid_input(&error))?;
    let reference = format!("refs/tags/{tag}");
    let mut exists = Command::new("git");
    exists
        .current_dir(root)
        .args(["show-ref", "--verify", "--quiet", &reference]);
    if !exists.status()?.success() {
        return fail(format!("tag does not exist in this checkout: {tag}"));
    }

    let tag_commit = capture_program(root, "git", &["rev-list", "-n", "1", &reference])?;
    let head_commit = capture_program(root, "git", &["rev-parse", "HEAD"])?;
    let tag_commit = tag_commit.trim();
    let head_commit = head_commit.trim();
    if tag_commit != head_commit {
        return fail(format!(
            "tag {tag} does not resolve to checked-out commit {head_commit}"
        ));
    }
    let package = root_package(root)?;
    if package.version != version {
        return fail(format!(
            "tag version {version} does not match package version {}",
            package.version
        ));
    }
    if !capture_program(root, "git", &["status", "--porcelain"])?.is_empty() {
        return fail("release checkout is not clean");
    }
    println!("verified {tag} at {head_commit} for package version {version}");
    Ok(())
}

fn parse_release_tag(tag: &str) -> std::result::Result<Version, String> {
    let version = tag
        .strip_prefix('v')
        .ok_or_else(|| "release tag must be v-prefixed SemVer, for example v1.2.3".to_owned())?;
    Version::parse(version)
        .map_err(|_| "release tag must be v-prefixed SemVer, for example v1.2.3".to_owned())
}

fn draft_release(root: &Path, tag: &str, github_output: bool) -> Result<()> {
    let output = root.join("target/release-candidate");
    let artifacts = all_files_sorted(&output)?;
    if artifacts.is_empty() {
        return fail("release candidate has no artifacts");
    }
    let mut command = Command::new("gh");
    command.current_dir(root).args(["release", "create", tag]);
    command.args(&artifacts);
    command.args([
        "--verify-tag",
        "--draft",
        "--generate-notes",
        "--title",
        tag,
    ]);
    let url = capture(&mut command)?;
    let url = url.trim();
    println!("{url}");
    if github_output {
        write_github_output_value("url", url)?;
    }
    Ok(())
}

fn crates_io_auth_mode(github_output: bool) -> Result<()> {
    let bootstrap = env::var_os("CRATES_IO_BOOTSTRAP_TOKEN").is_some_and(|value| !value.is_empty());
    println!("bootstrap={bootstrap}");
    if github_output {
        write_github_output_value("bootstrap", &bootstrap.to_string())?;
    }
    Ok(())
}

fn write_github_output(key: &str, value: &Path) -> Result<()> {
    write_github_output_value(key, &value.to_string_lossy())
}

fn write_github_output_value(key: &str, value: &str) -> Result<()> {
    let path =
        env::var_os("GITHUB_OUTPUT").ok_or_else(|| invalid_input("GITHUB_OUTPUT is not set"))?;
    append_github_output(Path::new(&path), key, value)
}

fn append_github_output(path: &Path, key: &str, value: &str) -> Result<()> {
    if key.contains(['\r', '\n']) || value.contains(['\r', '\n']) {
        return fail("GitHub output keys and values must be single-line");
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{key}={value}")?;
    Ok(())
}

#[cfg(windows)]
fn run_mutants(root: &Path, output: Option<PathBuf>, forwarded: &[String]) -> Result<i32> {
    use windows_spawn::{Command as SpawnCommand, DropPolicy, SpawnOptions};

    let output = mutation_output(root, output)?;
    println!("cargo-mutants output: {}", output.display());
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo.exe"));
    let mut command = SpawnCommand::new(cargo);
    command
        .args(["mutants", "--package", PACKAGE_NAME])
        .args(forwarded)
        .env("CARGO_MUTANTS_OUTPUT", &output)
        .current_dir(root);
    let status = command.status_with(SpawnOptions::new().drop_policy(DropPolicy::KillTree))?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(not(windows))]
fn run_mutants(_root: &Path, _output: Option<PathBuf>, _forwarded: &[String]) -> Result<i32> {
    fail("mutation containment requires Windows")
}

fn mutation_output(root: &Path, requested: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(output) = requested {
        return Ok(output);
    }
    if let Some(output) = env::var_os("CARGO_MUTANTS_OUTPUT").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(output));
    }
    let millis = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let output = root
        .join("target/mutants/runs")
        .join(format!("{millis}-{}", std::process::id()));
    fs::create_dir_all(&output)?;
    Ok(output)
}

fn resolve_directory(root: &Path, requested: &Path) -> Result<PathBuf> {
    let path = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        root.join(requested)
    };
    fs::create_dir_all(&path)?;
    Ok(normalize_path(&path))
}

fn capture_program(root: &Path, program: &str, arguments: &[&str]) -> Result<String> {
    let mut command = Command::new(program);
    command.current_dir(root).args(arguments);
    capture(&mut command)
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn root_package(root: &Path) -> Result<PackageInfo> {
    let mut command = cargo(root);
    command.args(["metadata", "--locked", "--no-deps", "--format-version", "1"]);
    let output = capture(&mut command)?;
    let metadata: Value = serde_json::from_str(&output)?;
    select_root_package(&metadata, root)
}

fn select_root_package(metadata: &Value, root: &Path) -> Result<PackageInfo> {
    let packages = metadata
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| TaskError::Message("cargo metadata has no packages array".to_owned()))?;
    let root_manifest = normalize_path(&root.join("Cargo.toml"));
    let mut matches = packages.iter().filter(|package| {
        package
            .get("manifest_path")
            .and_then(Value::as_str)
            .is_some_and(|path| normalize_path(Path::new(path)) == root_manifest)
    });
    let package = matches.next().ok_or_else(|| {
        TaskError::Message("could not identify the root Cargo package".to_owned())
    })?;
    if matches.next().is_some() {
        return Err(TaskError::Message(
            "cargo metadata contains duplicate root packages".to_owned(),
        ));
    }
    let name = package
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| TaskError::Message("root package has no name".to_owned()))?;
    let version = package
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| TaskError::Message("root package has no version".to_owned()))?;
    Ok(PackageInfo {
        name: name.to_owned(),
        version: Version::parse(version)?,
    })
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn repository_root() -> Result<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "xtask has no parent"))?;
    Ok(normalize_path(root))
}

fn cargo(root: &Path) -> Command {
    let executable = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let mut command = Command::new(executable);
    command.current_dir(root);
    command
}

fn cargo_with_toolchain(root: &Path, toolchain: &str) -> Command {
    let mut command = Command::new("rustup");
    command.current_dir(root).args(["run", toolchain, "cargo"]);
    command
}

fn run_cargo(root: &Path, arguments: &[&str]) -> Result<()> {
    let mut command = cargo(root);
    command.args(arguments);
    run(&mut command)
}

fn run_cargo_with_toolchain(root: &Path, toolchain: &str, arguments: &[&str]) -> Result<()> {
    let mut command = cargo_with_toolchain(root, toolchain);
    command.args(arguments);
    run(&mut command)
}

fn run_program(root: &Path, program: &str, arguments: &[&str]) -> Result<()> {
    let mut command = Command::new(program);
    command.current_dir(root).args(arguments);
    run(&mut command)
}

fn run(command: &mut Command) -> Result<()> {
    println!("+ {command:?}");
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        fail(format!("command failed with {status}: {command:?}"))
    }
}

fn capture(command: &mut Command) -> Result<String> {
    println!("+ {command:?}");
    let output = command.output()?;
    ensure_success(command, &output)?;
    String::from_utf8(output.stdout).map_err(TaskError::from)
}

fn ensure_success(command: &Command, output: &Output) -> Result<()> {
    if output.status.success() {
        return Ok(());
    }
    io::stderr().write_all(&output.stderr)?;
    io::stdout().write_all(&output.stdout)?;
    fail(format!(
        "command failed with {}: {command:?}",
        output.status
    ))
}

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(TaskError::Message(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn selects_only_the_root_manifest_package() {
        let root = repository_root().unwrap();
        let metadata = json!({
            "packages": [
                {
                    "name": "xtask",
                    "version": "0.0.0",
                    "manifest_path": root.join("xtask/Cargo.toml")
                },
                {
                    "name": PACKAGE_NAME,
                    "version": "0.1.0",
                    "manifest_path": root.join("Cargo.toml")
                }
            ]
        });
        assert_eq!(
            select_root_package(&metadata, &root).unwrap(),
            PackageInfo {
                name: PACKAGE_NAME.to_owned(),
                version: Version::new(0, 1, 0),
            }
        );
    }

    #[test]
    fn snapshot_comparison_reports_the_first_change() {
        assert!(compare_snapshot("one\ntwo\n", "one\ntwo\n").is_ok());
        let error = compare_snapshot("one\ntwo\n", "one\nthree\n").unwrap_err();
        assert!(error.contains("line 2"));
        assert!(error.contains("expected: two"));
    }

    #[test]
    fn validates_semver_release_tags() {
        assert_eq!(
            parse_release_tag("v1.2.3-rc.1+build.4").unwrap(),
            Version::parse("1.2.3-rc.1+build.4").unwrap()
        );
        assert!(parse_release_tag("1.2.3").is_err());
        assert!(parse_release_tag("v01.2.3").is_err());
        assert!(parse_release_tag("v1.2").is_err());
    }

    #[test]
    fn task_error_preserves_typed_sources_and_messages() {
        let io_error = TaskError::from(io::Error::new(io::ErrorKind::InvalidData, "io failure"));
        assert_eq!(io_error.to_string(), "io failure");
        assert!(io_error.source().is_some());

        let json_error = TaskError::from(serde_json::from_str::<Value>("{").unwrap_err());
        assert!(json_error.source().is_some());

        let utf8_error = TaskError::from(String::from_utf8(vec![0xff]).unwrap_err());
        assert!(utf8_error.source().is_some());

        let semver_error = TaskError::from(Version::parse("not-semver").unwrap_err());
        assert!(semver_error.source().is_some());

        let time_error = TaskError::from(
            UNIX_EPOCH
                .duration_since(UNIX_EPOCH + Duration::from_secs(1))
                .unwrap_err(),
        );
        assert!(time_error.source().is_some());

        let message = TaskError::from("plain failure".to_owned());
        assert_eq!(message.to_string(), "plain failure");
        assert!(message.source().is_none());
    }

    #[test]
    fn validates_sbom_formats_and_package_identity() {
        let package = PackageInfo {
            name: PACKAGE_NAME.to_owned(),
            version: Version::new(0, 1, 0),
        };
        let directory = temporary_directory("sbom");
        let cyclone = directory.join("test.cdx.json");
        let document = json!({
            "bomFormat": "CycloneDX",
            "specVersion": "1.5",
            "metadata": {"component": {
                "name": PACKAGE_NAME,
                "version": "0.1.0",
                "licenses": [{"license": {"id": "MIT"}}]
            }},
            "components": [{
                "name": "dependency",
                "licenses": [{"license": {"id": "MIT"}}]
            }],
            "dependencies": [{"ref": "root", "dependsOn": ["dependency"]}]
        });
        fs::write(&cyclone, serde_json::to_vec(&document).unwrap()).unwrap();
        validate_cyclonedx(&cyclone, &package).unwrap();
        assert!(validate_reuse_spdx_text(
            "SPDXVersion: SPDX-2.1\nDocumentName: windows-spawn\nLicenseInfoInFile: MIT\n",
            PACKAGE_NAME
        )
        .is_ok());
        assert!(validate_reuse_spdx_text("SPDXVersion: SPDX-2.1\n", PACKAGE_NAME).is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn hashes_and_sorts_release_artifacts() {
        let directory = temporary_directory("artifacts");
        fs::write(directory.join("z.reuse.spdx"), b"reuse").unwrap();
        fs::write(directory.join("a.crate"), b"abc").unwrap();
        fs::write(directory.join("m.cdx.json"), b"cyclone").unwrap();
        fs::write(directory.join("ignored.txt"), b"ignored").unwrap();
        let artifacts = release_artifacts(&directory).unwrap();
        let names: Vec<&OsStr> = artifacts
            .iter()
            .map(|path| path.file_name().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                OsStr::new("a.crate"),
                OsStr::new("m.cdx.json"),
                OsStr::new("z.reuse.spdx")
            ]
        );
        assert_eq!(
            sha256(&directory.join("a.crate")).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let sums = directory.join("SHA256SUMS");
        write_checksums(&sums, &artifacts).unwrap();
        assert!(fs::read_to_string(sums)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .ends_with("  a.crate"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn release_cleanup_guard_and_github_output_are_exact() {
        let root = repository_root().unwrap();
        assert!(validate_release_output(&root, &root.join("target/release-candidate")).is_ok());
        assert!(validate_release_output(&root, &root.join("target/other")).is_err());

        let directory = temporary_directory("github-output");
        let output = directory.join("output.txt");
        append_github_output(&output, "sbom", r"C:\candidate\bom.json").unwrap();
        append_github_output(&output, "bootstrap", "false").unwrap();
        assert_eq!(
            fs::read_to_string(&output).unwrap(),
            "sbom=C:\\candidate\\bom.json\nbootstrap=false\n"
        );
        assert!(append_github_output(&output, "bad", "two\nlines").is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    fn temporary_directory(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "windows-spawn-xtask-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
