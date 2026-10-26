// SPDX-FileCopyrightText: 2026 Yasunobu Sakashita
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Evaluates the actual workflow contracts and their release-source bindings.

use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};
use yaml_rust2::{Yaml, YamlLoader};

#[derive(Debug)]
enum CheckError {
    Io(std::io::Error),
    Yaml(yaml_rust2::ScanError),
    Json(serde_json::Error),
    Contract(String),
}

impl fmt::Display for CheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Yaml(error) => error.fmt(formatter),
            Self::Json(error) => error.fmt(formatter),
            Self::Contract(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for CheckError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Yaml(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Contract(_) => None,
        }
    }
}

impl From<std::io::Error> for CheckError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<yaml_rust2::ScanError> for CheckError {
    fn from(error: yaml_rust2::ScanError) -> Self {
        Self::Yaml(error)
    }
}

impl From<serde_json::Error> for CheckError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<&str> for CheckError {
    fn from(message: &str) -> Self {
        Self::Contract(message.into())
    }
}

impl From<String> for CheckError {
    fn from(message: String) -> Self {
        Self::Contract(message)
    }
}

type Result<T = ()> = std::result::Result<T, CheckError>;

const CI: &str = ".github/workflows/ci.yml";
const RELEASE: &str = ".github/workflows/release.yml";
const LAYERS: [&str; 7] = ["kani", "lean", "verus", "tla", "quint", "alloy", "prolog"];
const JOBS: [&str; 7] = [
    "quality",
    "test",
    "i686-smoke",
    "arm64",
    "verifier",
    "api",
    "proofs",
];
const RELEASE_REF: &str = "${{ needs.resolve.outputs.revision }}";
const REVISION: &str = "0123456789abcdef0123456789abcdef01234567";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn yaml(path: &str) -> Result<Yaml> {
    parse(&std::fs::read_to_string(root().join(path))?)
}

fn parse(source: &str) -> Result<Yaml> {
    let mut documents = YamlLoader::load_from_str(source)?.into_iter();
    let document = documents.next().ok_or("missing YAML document")?;
    require(
        documents.next().is_none(),
        "one workflow document is required",
    )?;
    Ok(document)
}

fn require(condition: bool, message: &str) -> Result {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn field<'a>(node: &'a Yaml, key: &str) -> Result<&'a Yaml> {
    node.as_hash()
        .and_then(|mapping| mapping.get(&Yaml::String(key.into())))
        .ok_or_else(|| format!("missing YAML field {key}").into())
}

fn text(node: &Yaml) -> Result<&str> {
    node.as_str().ok_or_else(|| "expected YAML string".into())
}

fn array(node: &Yaml) -> Result<&[Yaml]> {
    node.as_vec()
        .map(Vec::as_slice)
        .ok_or_else(|| "expected YAML sequence".into())
}

fn job<'a>(workflow: &'a Yaml, name: &str) -> Result<&'a Yaml> {
    field(field(workflow, "jobs")?, name)
}

fn steps(job: &Yaml) -> Result<&[Yaml]> {
    array(field(job, "steps")?)
}

fn step<'a>(job: &'a Yaml, id: &str) -> Result<&'a Yaml> {
    steps(job)?
        .iter()
        .find(|step| {
            field(step, "id")
                .and_then(text)
                .is_ok_and(|actual| actual == id)
        })
        .ok_or_else(|| format!("missing production step {id}").into())
}

fn contract_step(job: &Yaml) -> Result<&Yaml> {
    let step = steps(job)?
        .iter()
        .find(|step| {
            field(step, "env")
                .and_then(|env| field(env, "CONTRACT"))
                .is_ok()
        })
        .ok_or("missing production contract")?;
    require(
        text(field(step, "run")?)?.contains("\"$CONTRACT\""),
        "the actual shell must execute the contract",
    )?;
    Ok(step)
}

fn contract(job: &Yaml) -> Result<&str> {
    text(field(field(contract_step(job)?, "env")?, "CONTRACT")?)
}

fn names(node: &Yaml) -> Result<Vec<&str>> {
    if let Some(name) = node.as_str() {
        Ok(vec![name])
    } else {
        array(node)?.iter().map(text).collect()
    }
}

fn needs(job: &Yaml) -> Result<Vec<&str>> {
    names(field(job, "needs")?)
}

fn summary_input(release: bool) -> Value {
    let mut results = serde_json::Map::new();
    for name in if release {
        JOBS.as_slice()
    } else {
        &["quality", "windows-fast"]
    } {
        let _previous = results.insert((*name).into(), json!({"result":"success"}));
    }
    if release {
        let _previous = results.insert(
            "quality".into(),
            json!({"result":"success", "outputs": {
                "revision": REVISION, "proofs": json!(LAYERS).to_string()
            }}),
        );
    }
    Value::Object(results)
}

fn summary(needs: &Value, release: bool) -> Result<Output> {
    summary_revision(needs, release, REVISION)
}

fn summary_revision(needs: &Value, release: bool, revision: &str) -> Result<Output> {
    let workflow = yaml(if release { RELEASE } else { CI })?;
    let required = job(&workflow, if release { "qualify" } else { "required" })?;
    let mut child = Command::new("jq")
        .args(["-e", "--arg", "revision", revision, contract(required)?])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("missing jq stdin")?
        .write_all(&serde_json::to_vec(needs)?)?;
    Ok(child.wait_with_output()?)
}

#[test]
fn ordinary_ci_edit_does_not_select_heavy_windows_jobs() -> Result {
    let workflow = yaml(CI)?;
    let mut selected = 0_usize;
    for (name, variants) in [
        ("test", 2),
        ("i686-smoke", 1),
        ("arm64", 1),
        ("api", 1),
        ("package", 1),
    ] {
        if job(&workflow, name).is_ok() {
            selected = selected.checked_add(variants).ok_or("job count overflow")?;
        }
    }
    assert_eq!(
        selected, 0,
        "an ordinary CI edit retains {selected} heavy Windows variants in the PR workflow"
    );
    assert!(
        !root().join(".github/filters/ci.yml").exists(),
        "ordinary selection must not retain path filters"
    );
    let jobs = field(&workflow, "jobs")?
        .as_hash()
        .ok_or("jobs must be a mapping")?;
    let mut job_names = jobs.keys().map(text).collect::<Result<Vec<_>>>()?;
    job_names.sort_unstable();
    assert_eq!(job_names, ["quality", "required", "windows-fast"]);
    let events = field(&workflow, "on")?
        .as_hash()
        .ok_or("events must be a mapping")?;
    let mut event_names = events.keys().map(text).collect::<Result<Vec<_>>>()?;
    event_names.sort_unstable();
    assert_eq!(event_names, ["merge_group", "pull_request"]);
    for name in ["quality", "windows-fast"] {
        assert!(
            field(job(&workflow, name)?, "if").is_err(),
            "{name} remains mandatory"
        );
    }
    Ok(())
}

#[test]
fn actual_summary_programs_cover_every_terminal_result() -> Result {
    for (release, count) in [(false, 16), (true, 16384)] {
        let workflow = yaml(if release { RELEASE } else { CI })?;
        let production = contract(job(
            &workflow,
            if release { "qualify" } else { "required" },
        )?)?;
        let inventory = if release {
            json!(JOBS)
        } else {
            json!(["quality", "windows-fast"])
        };
        let program = format!(
            r#"
            def production: {production};
            {inventory} as $names |
            def cases:
              range(0; {count}) as $encoded |
              [range(0; ($names | length)) | ["success", "failure", "cancelled", "skipped"]
                [(($encoded / pow(4; .) | floor) % 4)]] as $results |
              (reduce range(0; ($names | length)) as $i ({{}}; .[$names[$i]] = {{result: $results[$i]}})) |
              (if {release} then .quality.outputs = {{revision: $revision, proofs: (["kani","lean","verus","tla","quint","alloy","prolog"] | tojson)}} else . end) as $input |
              if ($input | production) == ($results | all(. == "success"))
              then 1 else error("production summary accepted an incorrect terminal result") end;
            reduce cases as $case (0; . + $case) == {count}
        "#
        );
        let output = Command::new("jq")
            .args(["-en", "--arg", "revision", REVISION, &program])
            .output()?;
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout)?,
            json!(true)
        );
    }
    Ok(())
}

#[test]
fn summaries_reject_missing_unknown_unfinished_and_malformed_results() -> Result {
    for release in [false, true] {
        let healthy = summary_input(release);
        assert!(summary(&healthy, release)?.status.success());
        for name in if release {
            JOBS.as_slice()
        } else {
            &["quality", "windows-fast"]
        } {
            let mut missing = healthy.clone();
            let _removed = missing
                .as_object_mut()
                .ok_or("needs must be an object")?
                .remove(*name);
            assert!(
                !summary(&missing, release)?.status.success(),
                "missing {name}"
            );
            for result in [
                json!(null),
                json!("queued"),
                json!("in_progress"),
                json!("unknown"),
                json!(true),
            ] {
                let mut invalid = healthy.clone();
                *invalid
                    .pointer_mut(&format!("/{name}/result"))
                    .ok_or("result field")? = result;
                assert!(
                    !summary(&invalid, release)?.status.success(),
                    "invalid {name}: {invalid}"
                );
            }
        }
        let mut unknown = healthy.clone();
        let _previous = unknown
            .as_object_mut()
            .ok_or("needs must be an object")?
            .insert("unexpected".into(), json!({"result":"success"}));
        assert!(!summary(&unknown, release)?.status.success());
        for value in [json!(null), json!([]), json!("invalid"), json!({})] {
            assert!(!summary(&value, release)?.status.success());
        }
    }
    Ok(())
}

fn bash() -> Result<PathBuf> {
    if cfg!(windows) {
        Ok(
            PathBuf::from(std::env::var_os("ProgramFiles").ok_or("missing ProgramFiles")?)
                .join("Git/bin/bash.exe"),
        )
    } else {
        Ok(PathBuf::from("bash"))
    }
}

fn release_selection() -> Result<Yaml> {
    Ok(step(job(&yaml(RELEASE)?, "quality")?, "selection")?.clone())
}

fn select_release(revision: &str, event: &str, reference: &str) -> Result<Output> {
    let selection = release_selection()?;
    Ok(Command::new("jq")
        .args([
            "-n", "-c", "-e", "--arg", "revision", revision, "--arg", "event", event, "--arg",
            "ref", reference,
        ])
        .arg(text(field(field(&selection, "env")?, "CONTRACT")?)?)
        .output()?)
}

fn selection_shell(
    revision: Option<&str>,
    event: &str,
    reference: &str,
) -> Result<(Output, String)> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let directory = std::env::temp_dir().join(format!(
        "windows-spawn-ci-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory)?;
    let path = directory.join("outputs");
    let selection = release_selection()?;
    let mut command = Command::new(bash()?);
    command
        .args(["-c", text(field(&selection, "run")?)?])
        .env_remove("RELEASE_REVISION")
        .env("EVENT_NAME", event)
        .env("EVENT_REF", reference)
        .env("GITHUB_OUTPUT", &path)
        .env(
            "CONTRACT",
            text(field(field(&selection, "env")?, "CONTRACT")?)?,
        );
    if let Some(revision) = revision {
        command.env("RELEASE_REVISION", revision);
    }
    let output = command.output();
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => {
            std::fs::remove_file(&path)?;
            Ok(contents)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error),
    };
    std::fs::remove_dir(directory)?;
    Ok((output?, contents?))
}

#[test]
fn release_selector_accepts_only_complete_revision_and_version_tag_event_shape() -> Result {
    let output = select_release(REVISION, "push", "refs/tags/v1.2.3")?;
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout)?,
        json!({"revision":REVISION,"proofs":LAYERS})
    );
    let selection = release_selection()?;
    let env = field(&selection, "env")?;
    assert_eq!(text(field(env, "RELEASE_REVISION")?)?, RELEASE_REF);
    assert_eq!(text(field(env, "EVENT_NAME")?)?, "${{ github.event_name }}");
    assert_eq!(text(field(env, "EVENT_REF")?)?, "${{ github.ref }}");
    for revision in [
        String::new(),
        "a".repeat(39),
        "a".repeat(41),
        "A".repeat(40),
        "main".into(),
        format!("{REVISION}\n"),
    ] {
        assert!(
            !select_release(&revision, "push", "refs/tags/v1.2.3")?
                .status
                .success(),
            "invalid revision {revision:?}"
        );
    }
    for (event, reference) in [
        ("pull_request", "refs/tags/v1.2.3"),
        ("merge_group", "refs/tags/v1.2.3"),
        ("workflow_call", "refs/tags/v1.2.3"),
        ("workflow_dispatch", "refs/tags/v1.2.3"),
        ("push", "refs/heads/main"),
        ("push", "refs/tags/v1.2"),
        ("push", "refs/tags/v1.2.3-rc.1"),
        ("push", "refs/tags/v1.2.3\n"),
    ] {
        assert!(
            !select_release(REVISION, event, reference)?.status.success(),
            "invalid release event {event}/{reference:?}"
        );
    }
    Ok(())
}

#[test]
fn actual_release_selector_shell_fails_closed_and_renders_exact_outputs() -> Result {
    let (valid_output, valid_contents) =
        selection_shell(Some(REVISION), "push", "refs/tags/v1.2.3")?;
    assert!(valid_output.status.success(), "{valid_output:?}");
    let expected = format!("proofs={}\nrevision={REVISION}\n", json!(LAYERS));
    assert_eq!(
        valid_contents.lines().collect::<Vec<_>>(),
        expected.lines().collect::<Vec<_>>()
    );
    for revision in [None, Some(""), Some("main")] {
        let (output, contents) = selection_shell(revision, "push", "refs/tags/v1.2.3")?;
        assert!(
            !output.status.success(),
            "invalid revision {revision:?}: {output:?}"
        );
        assert!(
            contents.is_empty(),
            "invalid revision emitted outputs: {contents}"
        );
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("command not found"),
            "the actual jq engine must run"
        );
    }
    Ok(())
}

#[test]
fn qualification_rejects_missing_mismatched_or_malformed_release_identity() -> Result {
    let healthy = summary_input(true);
    for revision in [
        String::new(),
        "main".into(),
        "a".repeat(39),
        "A".repeat(40),
        format!("{REVISION}\n"),
    ] {
        assert!(
            !summary_revision(&healthy, true, &revision)?
                .status
                .success(),
            "invalid revision argument {revision:?}"
        );
        let mut matching = healthy.clone();
        *matching
            .pointer_mut("/quality/outputs/revision")
            .ok_or("revision fixture")? = json!(revision);
        assert!(
            !summary_revision(&matching, true, &revision)?
                .status
                .success(),
            "invalid matching revision {revision:?}"
        );
    }
    for (path, value) in [
        ("/quality/outputs/revision", json!(null)),
        ("/quality/outputs/revision", json!("a".repeat(40))),
        ("/quality/outputs/proofs", json!(null)),
        ("/quality/outputs/proofs", json!("invalid")),
        ("/quality/outputs/proofs", json!("{}")),
        ("/quality/outputs/proofs", json!("[]")),
        ("/quality/outputs/proofs", json!("[\"kani\",\"kani\"]")),
        ("/quality/outputs/proofs", json!("[\"unknown\"]")),
    ] {
        let mut invalid = healthy.clone();
        *invalid
            .pointer_mut(path)
            .ok_or("missing identity fixture field")? = value;
        assert!(
            !summary(&invalid, true)?.status.success(),
            "invalid identity {invalid}"
        );
    }
    Ok(())
}

#[test]
fn actual_summary_shells_reject_skipped_or_missing_mandatory_work() -> Result {
    for release in [false, true] {
        let workflow = yaml(if release { RELEASE } else { CI })?;
        let required = job(&workflow, if release { "qualify" } else { "required" })?;
        let step = contract_step(required)?;
        for results in [summary_input(release), json!({})] {
            let output = Command::new(bash()?)
                .args(["-c", text(field(step, "run")?)?])
                .env("RESULTS", results.to_string())
                .env("RELEASE_REVISION", REVISION)
                .env("CONTRACT", contract(required)?)
                .output()?;
            assert_eq!(
                output.status.success(),
                results == summary_input(release),
                "{output:?}"
            );
            assert!(
                !String::from_utf8_lossy(&output.stderr).contains("command not found"),
                "the actual jq engine must run"
            );
        }
    }
    Ok(())
}

fn checkout(job: &Yaml) -> Result<&Yaml> {
    steps(job)?
        .iter()
        .find(|step| {
            field(step, "uses")
                .and_then(text)
                .is_ok_and(|uses| uses.starts_with("actions/checkout@"))
        })
        .ok_or_else(|| "missing checkout".into())
}

fn validate_revision(job: &Yaml, reference: &str) -> Result {
    require(
        text(field(field(checkout(job)?, "with")?, "ref")?)? == reference,
        "checkout must use the resolved revision",
    )?;
    require(
        steps(job)?.iter().any(|step| {
            field(step, "run").and_then(text).is_ok_and(|script| {
                script.contains("git rev-parse HEAD")
                    && script.contains("EXPECTED_REVISION")
                    && script.contains("[0-9a-f]{40}")
            }) && field(step, "env")
                .and_then(|env| field(env, "EXPECTED_REVISION"))
                .and_then(text)
                .is_ok_and(|value| value == reference)
        }),
        "checkout must verify actual HEAD against the resolved revision",
    )
}

#[test]
fn actual_release_head_guard_rejects_empty_mismatched_and_newline_revisions() -> Result {
    let release = yaml(RELEASE)?;
    let quality = job(&release, "quality")?;
    let guard = steps(quality)?
        .iter()
        .find(|step| {
            field(step, "env")
                .and_then(|env| field(env, "EXPECTED_REVISION"))
                .is_ok()
        })
        .ok_or("missing actual revision guard")?;
    let script = text(field(guard, "run")?)?;
    let actual = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root())
        .output()?;
    assert!(actual.status.success(), "{actual:?}");
    let head = String::from_utf8_lossy(&actual.stdout).trim().to_owned();
    let check = |revision: &str| -> Result<Output> {
        Ok(Command::new(bash()?)
            .args(["-c", script])
            .env("EXPECTED_REVISION", revision)
            .current_dir(root())
            .output()?)
    };
    assert!(
        check(&head)?.status.success(),
        "the actual checked-out HEAD must pass"
    );
    for invalid in [
        String::new(),
        "a".repeat(40),
        head.to_ascii_uppercase(),
        format!("{head}\n"),
    ] {
        let output = check(&invalid)?;
        assert!(
            !output.status.success(),
            "invalid expected HEAD {invalid:?}"
        );
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("command not found"),
            "the actual git engine must run"
        );
    }
    Ok(())
}

fn validate_release(release: &Yaml) -> Result {
    let events = field(release, "on")?.as_hash().ok_or("release events")?;
    require(
        events.len() == 1 && field(field(release, "on")?, "push").is_ok(),
        "release qualification is tag-push only",
    )?;
    require(
        names(field(field(field(release, "on")?, "push")?, "tags")?)? == ["v[0-9]+.[0-9]+.[0-9]+"],
        "release qualification uses version tags",
    )?;
    let resolve = job(release, "resolve")?;
    require(
        text(field(field(resolve, "outputs")?, "revision")?)?
            == "${{ steps.resolve.outputs.revision }}",
        "resolver publishes the checked revision",
    )?;
    let resolver = text(field(step(resolve, "resolve")?, "run")?)?;
    require(
        resolver.contains("git rev-parse HEAD")
            && resolver.contains("[0-9a-f]{40}")
            && resolver.contains("GITHUB_OUTPUT"),
        "resolver emits a complete commit ID",
    )?;
    let scripts = steps(resolve)?
        .iter()
        .filter_map(|step| field(step, "run").and_then(text).ok())
        .collect::<Vec<_>>()
        .join("\n");
    require(
        scripts.contains(".object.type")
            && scripts.contains(".verification.verified")
            && scripts.contains("cargo xtask verify-release-tag"),
        "verified annotated-tag, main, and version guards precede qualification",
    )?;
    for name in JOBS {
        let job = job(release, name)?;
        require(
            needs(job)?.contains(&"resolve"),
            "qualification depends on the checked resolver",
        )?;
        validate_revision(job, RELEASE_REF)?;
    }
    let repository_checks = job(release, "quality")?;
    for output in ["revision", "proofs"] {
        require(
            text(field(field(repository_checks, "outputs")?, output)?)?
                == format!("${{{{ steps.selection.outputs.{output} }}}}"),
            "quality publishes the actual selector outputs",
        )?;
    }
    require(
        text(field(
            field(field(job(release, "proofs")?, "strategy")?, "matrix")?,
            "layer",
        )?)? == "${{ fromJSON(needs.quality.outputs.proofs) }}",
        "release matrix must execute the selected exact proof inventory",
    )?;
    let qualify = job(release, "qualify")?;
    let qualified_revision = "${{ needs.quality.outputs.revision }}";
    validate_revision(qualify, qualified_revision)?;
    require(
        text(field(
            field(contract_step(qualify)?, "env")?,
            "RELEASE_REVISION",
        )?)? == qualified_revision,
        "summary checks the selected release revision",
    )?;
    let mut actual = needs(qualify)?;
    actual.sort_unstable();
    let mut expected = JOBS;
    expected.sort_unstable();
    require(
        actual == expected,
        "qualification summary requires the exact seven jobs",
    )?;
    require(
        text(field(qualify, "if")?)? == "always()",
        "qualification summary must report failures",
    )?;
    let candidate = job(release, "candidate")?;
    let mut dependencies = needs(candidate)?;
    dependencies.sort_unstable();
    require(
        dependencies == ["qualify", "resolve"],
        "candidate requires resolver and qualification",
    )?;
    validate_revision(candidate, RELEASE_REF)?;
    require(
        needs(job(release, "publish")?)?.contains(&"candidate"),
        "publication depends on candidate",
    )?;
    require(
        needs(job(release, "github-release")?)?.contains(&"publish"),
        "GitHub release follows crate publication",
    )
}

fn replace_yaml(node: &mut Yaml, path: &[&str], replacement: Yaml) -> Result {
    let (first, rest) = path.split_first().ok_or("empty YAML mutation path")?;
    let value = node
        .as_mut_hash()
        .and_then(|mapping| mapping.get_mut(&Yaml::String((*first).into())))
        .ok_or("missing YAML mutation field")?;
    if rest.is_empty() {
        *value = replacement;
        Ok(())
    } else {
        replace_yaml(value, rest, replacement)
    }
}

#[test]
fn release_inventory_and_candidate_chain_reject_broken_bindings() -> Result {
    let release = yaml(RELEASE)?;
    validate_release(&release)?;
    for (path, replacement) in [
        (
            vec!["jobs", "quality", "needs"],
            Yaml::String("candidate".into()),
        ),
        (
            vec!["jobs", "candidate", "needs"],
            Yaml::Array(vec![Yaml::String("resolve".into())]),
        ),
        (
            vec!["jobs", "publish", "needs"],
            Yaml::String("resolve".into()),
        ),
        (
            vec!["jobs", "github-release", "needs"],
            Yaml::String("candidate".into()),
        ),
        (
            vec!["jobs", "resolve", "outputs", "revision"],
            Yaml::String("${{ github.sha }}".into()),
        ),
        (
            vec!["jobs", "proofs", "strategy", "matrix", "layer"],
            Yaml::Array(vec![
                Yaml::String("kani".into()),
                Yaml::String("kani".into()),
            ]),
        ),
    ] {
        let mut broken = release.clone();
        replace_yaml(&mut broken, &path, replacement)?;
        assert!(
            validate_release(&broken).is_err(),
            "accepted broken release path {path:?}"
        );
    }
    let mut broken = release;
    let jobs = broken
        .as_mut_hash()
        .and_then(|mapping| mapping.get_mut(&Yaml::String("jobs".into())))
        .and_then(Yaml::as_mut_hash)
        .ok_or("jobs")?;
    let candidate = jobs
        .get_mut(&Yaml::String("candidate".into()))
        .ok_or("candidate")?;
    let steps = candidate
        .as_mut_hash()
        .and_then(|mapping| mapping.get_mut(&Yaml::String("steps".into())))
        .and_then(Yaml::as_mut_vec)
        .ok_or("candidate steps")?;
    let checkout = steps
        .iter_mut()
        .find(|step| {
            field(step, "uses")
                .and_then(text)
                .is_ok_and(|value| value.starts_with("actions/checkout@"))
        })
        .ok_or("candidate checkout")?;
    replace_yaml(checkout, &["with", "ref"], Yaml::String("main".into()))?;
    assert!(
        validate_release(&broken).is_err(),
        "candidate must reject a moving checkout ref"
    );
    Ok(())
}

#[test]
fn mandatory_summary_inventory_matches_the_actual_needs() -> Result {
    let workflow = yaml(CI)?;
    let required = job(&workflow, "required")?;
    let mut actual = needs(required)?;
    actual.sort_unstable();
    assert_eq!(actual, ["quality", "windows-fast"]);
    assert_eq!(text(field(required, "if")?)?, "always()");
    let windows = job(&workflow, "windows-fast")?;
    assert!(steps(windows)?.iter().any(|step| field(step, "run")
        .and_then(text)
        .is_ok_and(|script| script.contains("just windows-smoke"))));
    Ok(())
}

#[test]
fn yaml_adapter_preserves_on_and_rejects_duplicate_or_multiple_documents() -> Result {
    let workflow = parse("on:\n  pull_request:\njobs: {}\n")?;
    assert!(field(field(&workflow, "on")?, "pull_request").is_ok());
    assert!(parse("on: {}\non: {}\n").is_err());
    assert!(parse("jobs:\n  quality: {}\n  quality: {}\n").is_err());
    assert!(parse("---\njobs: {}\n---\njobs: {}\n").is_err());
    assert!(parse("jobs: [broken\n").is_err());
    Ok(())
}
