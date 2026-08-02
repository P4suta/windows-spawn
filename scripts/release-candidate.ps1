[CmdletBinding()]
param(
    [switch] $AllowDirty
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Get-Sha256Hex {
    param(
        [Parameter(Mandatory)]
        [string] $LiteralPath
    )

    $stream = [System.IO.File]::OpenRead($LiteralPath)
    try {
        $algorithm = [System.Security.Cryptography.SHA256]::Create()
        try {
            $bytes = $algorithm.ComputeHash($stream)
            return [System.BitConverter]::ToString($bytes).Replace("-", "")
        }
        finally {
            $algorithm.Dispose()
        }
    }
    finally {
        $stream.Dispose()
    }
}

$repositoryRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$outputPath = [System.IO.Path]::GetFullPath(
    (Join-Path $repositoryRoot "target\release-candidate")
)
$expectedOutputPath = [System.IO.Path]::GetFullPath(
    (Join-Path $repositoryRoot "target\release-candidate")
)
if ($outputPath -ne $expectedOutputPath) {
    throw "Refusing to clean unexpected output directory: $outputPath"
}

if (Test-Path -LiteralPath $outputPath) {
    Remove-Item -Recurse -Force -LiteralPath $outputPath
}
New-Item -ItemType Directory -Path $outputPath | Out-Null

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
    $crateName = "$packageName-$packageVersion.crate"
    $cargoCrate = Join-Path $repositoryRoot "target\package\$crateName"
    $candidateCrate = Join-Path $outputPath $crateName
    $packageArguments = @("package", "--locked")
    if ($AllowDirty) {
        Write-Warning "Generating a non-release candidate from a dirty worktree"
        $packageArguments += "--allow-dirty"
    }

    & cargo @packageArguments
    if ($LASTEXITCODE -ne 0) {
        throw "First cargo package run failed with exit code $LASTEXITCODE"
    }
    Copy-Item -LiteralPath $cargoCrate -Destination $candidateCrate
    $firstHash = (Get-Sha256Hex -LiteralPath $candidateCrate)

    & cargo @packageArguments
    if ($LASTEXITCODE -ne 0) {
        throw "Second cargo package run failed with exit code $LASTEXITCODE"
    }
    $secondHash = (Get-Sha256Hex -LiteralPath $cargoCrate)
    if ($firstHash -ne $secondHash) {
        throw "cargo package is not reproducible: $firstHash differs from $secondHash"
    }

    & (Join-Path $PSScriptRoot "check-packaged-reuse.ps1")
    if ($LASTEXITCODE -ne 0) {
        throw "Packaged REUSE verification failed"
    }

    & (Join-Path $PSScriptRoot "generate-sboms.ps1") -OutputDirectory $outputPath
    if ($LASTEXITCODE -ne 0) {
        throw "SBOM generation failed"
    }

    $artifacts = @(
        Get-ChildItem -LiteralPath $outputPath -File |
            Where-Object {
                $_.Extension -eq ".crate" -or
                $_.Name.EndsWith(".cdx.json") -or
                $_.Name.EndsWith(".reuse.spdx")
            } |
            Sort-Object -Property Name
    )
    if ($artifacts.Count -ne 3) {
        throw "Expected one crate and two SBOMs, found $($artifacts.Count)"
    }

    $checksumLines = @(
        foreach ($artifact in $artifacts) {
            $hash = (
                Get-Sha256Hex -LiteralPath $artifact.FullName
            ).ToLowerInvariant()
            "$hash  $($artifact.Name)"
        }
    )
    $checksumPath = Join-Path $outputPath "SHA256SUMS"
    $utf8WithoutBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllLines(
        $checksumPath,
        [string[]]$checksumLines,
        $utf8WithoutBom
    )

    Write-Host "Release candidate is reproducible and validated:"
    Get-ChildItem -LiteralPath $outputPath -File |
        Sort-Object -Property Name |
        ForEach-Object { Write-Host "  $($_.FullName)" }
}
finally {
    Pop-Location
}
