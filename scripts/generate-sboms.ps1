[CmdletBinding()]
param(
    [string] $OutputDirectory = "target\release-candidate"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repositoryRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
if ([System.IO.Path]::IsPathRooted($OutputDirectory)) {
    $outputPath = [System.IO.Path]::GetFullPath($OutputDirectory)
}
else {
    $outputPath = [System.IO.Path]::GetFullPath(
        (Join-Path $repositoryRoot $OutputDirectory)
    )
}

New-Item -ItemType Directory -Force -Path $outputPath | Out-Null

Push-Location -LiteralPath $repositoryRoot
try {
    $metadataJson = & cargo metadata --locked --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) {
        throw "cargo metadata failed with exit code $LASTEXITCODE"
    }
    $metadata = $metadataJson | ConvertFrom-Json
    $manifestPath = [System.IO.Path]::GetFullPath(
        (Join-Path $repositoryRoot "Cargo.toml")
    )
    $package = @(
        $metadata.packages |
            Where-Object {
                [System.IO.Path]::GetFullPath($_.manifest_path) -eq $manifestPath
            }
    )
    if ($package.Count -ne 1) {
        throw "Could not identify the root Cargo package"
    }

    $packageName = [string]$package[0].name
    $packageVersion = [string]$package[0].version
    $cycloneDxBaseName = "$packageName-$packageVersion.cdx"
    $cycloneDxName = "$cycloneDxBaseName.json"
    $generatedCycloneDx = Join-Path $repositoryRoot $cycloneDxName
    $cycloneDxPath = Join-Path $outputPath $cycloneDxName
    $reuseSpdxPath = Join-Path $outputPath (
        "$packageName-$packageVersion.reuse.spdx"
    )

    if (Test-Path -LiteralPath $generatedCycloneDx) {
        Remove-Item -Force -LiteralPath $generatedCycloneDx
    }

    & cargo cyclonedx --format json --spec-version 1.5 --target all --all-features --no-build-deps --override-filename $cycloneDxBaseName
    if ($LASTEXITCODE -ne 0) {
        throw "cargo cyclonedx failed with exit code $LASTEXITCODE"
    }
    if (-not (Test-Path -LiteralPath $generatedCycloneDx -PathType Leaf)) {
        throw "cargo cyclonedx did not create $generatedCycloneDx"
    }
    Move-Item -Force -LiteralPath $generatedCycloneDx -Destination $cycloneDxPath

    & python -m reuse spdx -o $reuseSpdxPath
    if ($LASTEXITCODE -ne 0) {
        throw "REUSE SPDX generation failed with exit code $LASTEXITCODE"
    }

    $bom = Get-Content -LiteralPath $cycloneDxPath -Raw | ConvertFrom-Json
    if (
        $bom.bomFormat -ne "CycloneDX" -or
        [string]$bom.specVersion -ne "1.5"
    ) {
        throw "CycloneDX SBOM is not JSON conforming to specification 1.5"
    }
    if (
        [string]$bom.metadata.component.name -ne $packageName -or
        [string]$bom.metadata.component.version -ne $packageVersion
    ) {
        throw "CycloneDX SBOM has incorrect root package metadata"
    }

    $components = @($bom.metadata.component) + @($bom.components)
    foreach ($component in $components) {
        if (@($component.licenses).Count -eq 0) {
            throw "CycloneDX component lacks license data: $($component.name)"
        }
    }
    if (@($bom.components).Count -eq 0 -or @($bom.dependencies).Count -eq 0) {
        throw "CycloneDX SBOM does not contain dependency components and relationships"
    }

    $reuseSpdx = Get-Content -LiteralPath $reuseSpdxPath -Raw
    if (
        $reuseSpdx -notmatch '(?m)^SPDXVersion: SPDX-2\.1\r?$' -or
        $reuseSpdx -notmatch "(?m)^DocumentName: $([regex]::Escape($packageName))\r?$" -or
        $reuseSpdx -notmatch '(?m)^LicenseInfoInFile: '
    ) {
        throw "REUSE SPDX SBOM is missing format, package, or license information"
    }

    Write-Host "Generated and validated:"
    Write-Host "  $cycloneDxPath"
    Write-Host "  $reuseSpdxPath"
}
finally {
    if (
        $null -ne (Get-Variable generatedCycloneDx -ErrorAction SilentlyContinue) -and
        (Test-Path -LiteralPath $generatedCycloneDx)
    ) {
        Remove-Item -Force -LiteralPath $generatedCycloneDx
    }
    Pop-Location
}
