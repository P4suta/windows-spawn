set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"]

default:
    @just --list

fmt:
    cargo xtask fmt

clippy:
    cargo xtask clippy

test:
    cargo xtask test

doc:
    cargo xtask doc

msrv:
    cargo xtask msrv

cross-targets:
    cargo xtask cross-targets

linux-empty:
    cargo xtask linux-empty

public-api:
    cargo xtask public-api

public-api-update:
    cargo xtask public-api --update

supply-chain:
    cargo xtask supply-chain

reuse:
    cargo xtask reuse

typos:
    cargo xtask typos

sbom:
    cargo xtask sbom

coverage:
    cargo xtask coverage

mutants:
    cargo xtask mutants --

mutants-ci shard:
    cargo xtask mutants --output . -- --in-place --shard {{ shard }}/4 --timeout 90 --build-timeout 180 --no-shuffle -vV

package-check:
    cargo xtask package-check

release-candidate:
    cargo xtask release-candidate

release-verify tag:
    cargo xtask verify-release-tag "{{ tag }}"

ci:
    cargo xtask ci
