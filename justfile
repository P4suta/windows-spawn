set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"]

default:
    @just --list

fmt:
    cargo fmt --all -- --check

clippy:
    cargo clippy --all-targets --locked -- -D warnings

test:
    cargo test --all-targets --locked -- --test-threads=1
    cargo test --doc --locked

doc:
    $env:RUSTDOCFLAGS = '-D warnings'; cargo doc --no-deps --locked

msrv:
    cargo +1.75.0 check --all-targets --locked

cross-targets:
    cargo check --locked --target x86_64-pc-windows-msvc
    cargo check --locked --target i686-pc-windows-msvc
    cargo check --locked --target aarch64-pc-windows-msvc

linux-empty:
    cargo check --all-targets --locked --target x86_64-unknown-linux-gnu

public-api:
    $actual = @(cargo +nightly-2026-07-02 public-api --simplified); if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; $expected = @(Get-Content -LiteralPath 'public-api/windows-spawn.txt'); $difference = @(Compare-Object -ReferenceObject $expected -DifferenceObject $actual -SyncWindow 0); if ($difference.Count -ne 0) { $difference | Format-Table | Out-String | Write-Error; exit 1 }

public-api-update:
    cargo +nightly-2026-07-02 public-api --simplified | Set-Content -LiteralPath public-api/windows-spawn.txt -Encoding utf8

supply-chain:
    cargo deny --all-features --locked check

reuse:
    python -m reuse lint

sbom:
    & '.\scripts\generate-sboms.ps1'

coverage:
    cargo llvm-cov clean --workspace
    cargo llvm-cov --all-targets --locked -- --test-threads=1
    cargo llvm-cov report --fail-under-lines 92 --fail-under-regions 92 --fail-under-functions 92

mutants:
    & '.\scripts\run-mutants-contained.ps1'

mutants-ci shard:
    $env:CARGO_MUTANTS_OUTPUT = '.'; & '.\scripts\run-mutants-contained.ps1' --in-place --shard {{ shard }}/4 --timeout 90 --build-timeout 180 --no-shuffle -vV

package-check:
    cargo package --locked
    & '.\scripts\check-packaged-reuse.ps1'

release-candidate:
    & '.\scripts\release-candidate.ps1'

release-verify tag:
    & '.\scripts\verify-release-tag.ps1' -Tag '{{ tag }}'

ci: fmt clippy test doc msrv cross-targets linux-empty public-api supply-chain reuse package-check
