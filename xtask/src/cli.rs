use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SimpleTask {
    Fmt,
    Clippy,
    Gates,
    Test,
    Doc,
    Msrv,
    CrossTargets,
    LinuxEmpty,
    SupplyChain,
    Reuse,
    Typos,
    Coverage,
    Ci,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Task {
    Simple(SimpleTask),
    PublicApi {
        update: bool,
    },
    PackageCheck {
        allow_dirty: bool,
    },
    Sbom {
        output: Option<PathBuf>,
    },
    ReleaseCandidate {
        allow_dirty: bool,
        github_output: bool,
    },
    VerifyReleaseTag {
        tag: String,
    },
    Mutation {
        forwarded: Vec<String>,
    },
    DraftRelease {
        tag: String,
        github_output: bool,
    },
    Help,
}

pub(crate) fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Task, String> {
    let mut arguments = arguments.into_iter();
    let Some(command) = arguments.next() else {
        return Ok(Task::Help);
    };
    let rest: Vec<String> = arguments.collect();

    match command.as_str() {
        "fmt" => no_arguments(&rest, Task::Simple(SimpleTask::Fmt)),
        "clippy" => no_arguments(&rest, Task::Simple(SimpleTask::Clippy)),
        "gates" => no_arguments(&rest, Task::Simple(SimpleTask::Gates)),
        "test" => no_arguments(&rest, Task::Simple(SimpleTask::Test)),
        "doc" => no_arguments(&rest, Task::Simple(SimpleTask::Doc)),
        "msrv" => no_arguments(&rest, Task::Simple(SimpleTask::Msrv)),
        "cross-targets" => no_arguments(&rest, Task::Simple(SimpleTask::CrossTargets)),
        "linux-empty" => no_arguments(&rest, Task::Simple(SimpleTask::LinuxEmpty)),
        "supply-chain" => no_arguments(&rest, Task::Simple(SimpleTask::SupplyChain)),
        "reuse" => no_arguments(&rest, Task::Simple(SimpleTask::Reuse)),
        "typos" => no_arguments(&rest, Task::Simple(SimpleTask::Typos)),
        "coverage" => no_arguments(&rest, Task::Simple(SimpleTask::Coverage)),
        "ci" => no_arguments(&rest, Task::Simple(SimpleTask::Ci)),
        "public-api" => parse_public_api(&rest),
        "package-check" => parse_single_flag(&rest, "--allow-dirty")
            .map(|allow_dirty| Task::PackageCheck { allow_dirty }),
        "sbom" => parse_output(&rest).map(|output| Task::Sbom { output }),
        "release-candidate" => parse_release_candidate(&rest),
        "verify-release-tag" => parse_tag(&rest).map(|tag| Task::VerifyReleaseTag { tag }),
        "mutation" => parse_mutation(&rest),
        "draft-release" => parse_tag_and_github_output(&rest)
            .map(|(tag, github_output)| Task::DraftRelease { tag, github_output }),
        "help" | "-h" | "--help" => no_arguments(&rest, Task::Help),
        _ => Err(format!("unknown xtask command: {command}")),
    }
}

fn no_arguments(rest: &[String], task: Task) -> Result<Task, String> {
    match rest {
        [] => Ok(task),
        [first, ..] => Err(format!("unexpected argument: {first}")),
    }
}

fn parse_public_api(rest: &[String]) -> Result<Task, String> {
    match rest {
        [] => Ok(Task::PublicApi { update: false }),
        [flag] if flag == "--update" => Ok(Task::PublicApi { update: true }),
        [argument, ..] => Err(format!("unexpected public-api argument: {argument}")),
    }
}

fn parse_single_flag(rest: &[String], expected: &str) -> Result<bool, String> {
    match rest {
        [] => Ok(false),
        [flag] if flag == expected => Ok(true),
        [argument, ..] => Err(format!("unexpected argument: {argument}")),
    }
}

fn parse_output(rest: &[String]) -> Result<Option<PathBuf>, String> {
    match rest {
        [] => Ok(None),
        [flag, value] if flag == "--output" => Ok(Some(PathBuf::from(value))),
        [flag] if flag == "--output" => Err("--output requires a directory".to_owned()),
        [argument, ..] => Err(format!("unexpected sbom argument: {argument}")),
    }
}

fn parse_release_candidate(rest: &[String]) -> Result<Task, String> {
    let mut allow_dirty = false;
    let mut github_output = false;
    for argument in rest {
        match argument.as_str() {
            "--allow-dirty" if !allow_dirty => allow_dirty = true,
            "--github-output" if !github_output => github_output = true,
            _ => return Err(format!("unexpected release-candidate argument: {argument}")),
        }
    }
    Ok(Task::ReleaseCandidate {
        allow_dirty,
        github_output,
    })
}

fn parse_tag(rest: &[String]) -> Result<String, String> {
    match rest {
        [tag] => Ok(tag.clone()),
        [] => Err("a v-prefixed release tag is required".to_owned()),
        [_, argument, ..] => Err(format!("unexpected argument: {argument}")),
    }
}

fn parse_tag_and_github_output(rest: &[String]) -> Result<(String, bool), String> {
    match rest {
        [tag] => Ok((tag.clone(), false)),
        [tag, flag] if flag == "--github-output" => Ok((tag.clone(), true)),
        [] => Err("a release tag is required".to_owned()),
        [_, argument, ..] => Err(format!("unexpected draft-release argument: {argument}")),
    }
}

fn parse_mutation(rest: &[String]) -> Result<Task, String> {
    let forwarded = match rest {
        [] => &[][..],
        [delimiter, forwarded @ ..] if delimiter == "--" => forwarded,
        [argument, ..] => return Err(format!("unexpected mutation argument: {argument}")),
    };
    reject_mutation_scope(forwarded)?;
    Ok(Task::Mutation {
        forwarded: forwarded.to_vec(),
    })
}

/// Rejects arguments that would measure something other than `.rust-mutants.toml` describes.
fn reject_mutation_scope(arguments: &[String]) -> Result<(), String> {
    let prohibited = ["--package", "--root", "--config", "--no-config"];
    for argument in arguments {
        let name = argument.split('=').next().unwrap_or(argument);
        if prohibited.contains(&name) {
            return Err(format!(
                "the mutation scope is fixed by .rust-mutants.toml: {argument}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn parses_flags_and_forwarded_mutation_arguments() {
        assert_eq!(
            parse(strings(&["public-api", "--update"])).unwrap(),
            Task::PublicApi { update: true }
        );
        assert_eq!(
            parse(strings(&["mutation", "--", "--shard", "1/4", "--json"])).unwrap(),
            Task::Mutation {
                forwarded: strings(&["--shard", "1/4", "--json"]),
            }
        );
        assert_eq!(
            parse(strings(&["mutation"])).unwrap(),
            Task::Mutation {
                forwarded: Vec::new()
            }
        );
    }

    #[test]
    fn rejects_unknown_and_scope_changing_mutation_arguments() {
        assert!(parse(strings(&["no-such-command"])).is_err());
        assert!(parse(strings(&["mutation", "--", "--package", "xtask"])).is_err());
        assert!(parse(strings(&["mutation", "--", "--root=elsewhere"])).is_err());
        assert!(parse(strings(&["mutation", "--", "--no-config"])).is_err());
        assert!(parse(strings(&["mutation", "--", "--config", "other.toml"])).is_err());
        assert!(parse(strings(&["mutation", "--shard", "1/4"])).is_err());
    }

    #[test]
    fn parses_release_and_artifact_options() {
        assert_eq!(
            parse(strings(&[
                "release-candidate",
                "--github-output",
                "--allow-dirty"
            ]))
            .unwrap(),
            Task::ReleaseCandidate {
                allow_dirty: true,
                github_output: true,
            }
        );
        assert_eq!(
            parse(strings(&["sbom", "--output", "artifacts"])).unwrap(),
            Task::Sbom {
                output: Some(PathBuf::from("artifacts")),
            }
        );
        assert_eq!(
            parse(strings(&["verify-release-tag", "v1.2.3"])).unwrap(),
            Task::VerifyReleaseTag {
                tag: "v1.2.3".to_owned(),
            }
        );
        assert!(parse(strings(&[
            "release-candidate",
            "--allow-dirty",
            "--allow-dirty"
        ]))
        .is_err());
        assert!(parse(strings(&["sbom", "--output"])).is_err());
    }
}
