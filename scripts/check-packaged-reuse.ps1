[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repositoryRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
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

    $expandedPackage = Join-Path $repositoryRoot (
        "target\package\{0}-{1}" -f $package[0].name, $package[0].version
    )
    if (-not (Test-Path -LiteralPath $expandedPackage -PathType Container)) {
        throw "Expanded package not found: $expandedPackage. Run cargo package first."
    }

    Push-Location -LiteralPath $expandedPackage
    try {
        & python -m reuse lint
        if ($LASTEXITCODE -ne 0) {
            throw "REUSE lint failed for the expanded package"
        }
    }
    finally {
        Pop-Location
    }
}
finally {
    Pop-Location
}
