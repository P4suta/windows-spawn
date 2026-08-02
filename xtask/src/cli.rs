use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SimpleTask {
    Fmt,
    Clippy,
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
    Mutants {
        output: Option<PathBuf>,
        forwarded: Vec<String>,
    },
    DraftRelease {
        tag: String,
        github_output: bool,
    },
    CratesIoAuthMode {
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
        "mutants" => parse_mutants(&rest),
        "draft-release" => parse_tag_and_github_output(&rest)
            .map(|(tag, github_output)| Task::DraftRelease { tag, github_output }),
        "crates-io-auth-mode" => parse_single_flag(&rest, "--github-output")
            .map(|github_output| Task::CratesIoAuthMode { github_output }),
        "help" | "-h" | "--help" => no_arguments(&rest, Task::Help),
        _ => Err(format!("unknown xtask command: {command}")),
    }
}

fn no_arguments(rest: &[String], task: Task) -> Result<Task, String> {
    if rest.is_empty() {
        Ok(task)
    } else {
        Err(format!("unexpected argument: {}", rest[0]))
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

fn parse_mutants(rest: &[String]) -> Result<Task, String> {
    let delimiter = rest.iter().position(|argument| argument == "--");
    let (options, forwarded) = delimiter.map_or((rest, &[][..]), |index| {
        (&rest[..index], &rest[index + 1..])
    });
    let output = parse_output(options).map_err(|error| error.replace("sbom", "mutants"))?;
    reject_mutation_scope(forwarded)?;
    Ok(Task::Mutants {
        output,
        forwarded: forwarded.to_vec(),
    })
}

fn reject_mutation_scope(arguments: &[String]) -> Result<(), String> {
    let prohibited = ["--workspace", "--all", "--package", "-p", "--manifest-path"];
    for (index, argument) in arguments.iter().enumerate() {
        if prohibited.contains(&argument.as_str())
            || argument.starts_with("--workspace=")
            || argument.starts_with("--all=")
            || argument.starts_with("--package=")
            || argument.starts_with("--manifest-path=")
            || (argument.starts_with("-p") && argument.len() > 2)
        {
            return Err(format!(
                "mutation package selection is fixed to windows-spawn: {argument}"
            ));
        }
        if index > 0 && prohibited[..].contains(&arguments[index - 1].as_str()) {
            return Err(format!(
                "mutation package selection is fixed to windows-spawn: {argument}"
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
    fn parses_flags_and_forwarded_mutant_arguments() {
        assert_eq!(
            parse(strings(&["public-api", "--update"])).unwrap(),
            Task::PublicApi { update: true }
        );
        assert_eq!(
            parse(strings(&[
                "mutants", "--output", "out", "--", "--list", "-vV"
            ]))
            .unwrap(),
            Task::Mutants {
                output: Some(PathBuf::from("out")),
                forwarded: strings(&["--list", "-vV"]),
            }
        );
    }

    #[test]
    fn rejects_unknown_and_workspace_mutation_arguments() {
        assert!(parse(strings(&["no-such-command"])).is_err());
        assert!(parse(strings(&["mutants", "--", "--workspace"])).is_err());
        assert!(parse(strings(&["mutants", "--", "--workspace=true"])).is_err());
        assert!(parse(strings(&["mutants", "--", "-p", "other"])).is_err());
        assert!(parse(strings(&["mutants", "--list"])).is_err());
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
