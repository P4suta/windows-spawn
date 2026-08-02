[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $Tag
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$semverTag = '^v(?<version>0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$'
if ($Tag -notmatch $semverTag) {
    throw "Release tag must be v-prefixed SemVer, for example v1.2.3"
}
$tagVersion = $Tag.Substring(1)

& git show-ref --verify --quiet "refs/tags/$Tag"
if ($LASTEXITCODE -ne 0) {
    throw "Tag does not exist in this checkout: $Tag"
}

$tagCommit = (& git rev-list -n 1 "refs/tags/$Tag").Trim()
$headCommit = (& git rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $tagCommit -ne $headCommit) {
    throw "Tag $Tag does not resolve to checked-out commit $headCommit"
}

$metadataJson = & cargo metadata --locked --no-deps --format-version 1
if ($LASTEXITCODE -ne 0) {
    throw "cargo metadata failed with exit code $LASTEXITCODE"
}
$metadata = $metadataJson | ConvertFrom-Json
$repositoryRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$manifestPath = [System.IO.Path]::GetFullPath(
    (Join-Path $repositoryRoot "Cargo.toml")
)
$package = @(
    $metadata.packages |
        Where-Object {
            [System.IO.Path]::GetFullPath($_.manifest_path) -eq $manifestPath
        }
)
if ($package.Count -ne 1 -or [string]$package[0].version -ne $tagVersion) {
    throw "Tag version $tagVersion does not match the root Cargo package version"
}

if (& git status --porcelain) {
    throw "Release checkout is not clean"
}

Write-Host "Verified $Tag at $headCommit for package version $tagVersion"
