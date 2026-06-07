#requires -Version 5.1
[CmdletBinding()]
param(
    [switch]$SkipFrontend,
    [switch]$SkipDocker
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot

$localToolBin = Join-Path $repoRoot ".tools\bin"
if (Test-Path -LiteralPath $localToolBin) {
    $env:PATH = "$localToolBin;$env:PATH"
}

function Invoke-Step {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [scriptblock]$Action
    )

    Write-Host "==> $Name"
    & $Action
    Write-Host ""
}

function Assert-Command {
    param([Parameter(Mandatory = $true)][string]$Name)

    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "$Name is required for strict release validation"
    }
}

function Invoke-NativeCommand {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$Arguments
    )

    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$FilePath $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

function Invoke-WithCargoEnvironment {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Action,
        [switch]$IncludeCargoDenyAdvisoryDb
    )

    $previousCargoHome = $env:CARGO_HOME
    $previousGitConfigCount = $env:GIT_CONFIG_COUNT
    $previousGitConfigKey0 = $env:GIT_CONFIG_KEY_0
    $previousGitConfigValue0 = $env:GIT_CONFIG_VALUE_0
    $previousGitConfigKey1 = $env:GIT_CONFIG_KEY_1
    $previousGitConfigValue1 = $env:GIT_CONFIG_VALUE_1
    $previousGitConfigKey2 = $env:GIT_CONFIG_KEY_2
    $previousGitConfigValue2 = $env:GIT_CONFIG_VALUE_2

    try {
        $env:CARGO_HOME = (Join-Path $repoRoot ".tools/cargo-home")
        New-Item -ItemType Directory -Force -Path $env:CARGO_HOME | Out-Null

        $advisoryDb = Get-ChildItem -LiteralPath (Join-Path $repoRoot ".tools\advisory-dbs") -Directory -Filter "advisory-db-*" -ErrorAction SilentlyContinue |
            Sort-Object LastWriteTime -Descending |
            Select-Object -First 1

        $env:GIT_CONFIG_COUNT = "1"
        $env:GIT_CONFIG_KEY_0 = "safe.directory"
        $env:GIT_CONFIG_VALUE_0 = ($repoRoot.Path -replace "\\", "/")

        $cargoAuditDb = Join-Path $repoRoot ".tools\advisory-db"
        if (Test-Path -LiteralPath $cargoAuditDb) {
            $env:GIT_CONFIG_COUNT = "2"
            $env:GIT_CONFIG_KEY_1 = "safe.directory"
            $env:GIT_CONFIG_VALUE_1 = ((Resolve-Path $cargoAuditDb).Path -replace "\\", "/")
        }

        if ($IncludeCargoDenyAdvisoryDb -and $advisoryDb) {
            $env:GIT_CONFIG_COUNT = "3"
            $env:GIT_CONFIG_KEY_2 = "safe.directory"
            $env:GIT_CONFIG_VALUE_2 = ($advisoryDb.FullName -replace "\\", "/")
        }

        & $Action
    } finally {
        $env:CARGO_HOME = $previousCargoHome
        $env:GIT_CONFIG_COUNT = $previousGitConfigCount
        $env:GIT_CONFIG_KEY_0 = $previousGitConfigKey0
        $env:GIT_CONFIG_VALUE_0 = $previousGitConfigValue0
        $env:GIT_CONFIG_KEY_1 = $previousGitConfigKey1
        $env:GIT_CONFIG_VALUE_1 = $previousGitConfigValue1
        $env:GIT_CONFIG_KEY_2 = $previousGitConfigKey2
        $env:GIT_CONFIG_VALUE_2 = $previousGitConfigValue2
    }
}

function Assert-CargoTreeExcludes {
    param(
        [Parameter(Mandatory = $true)][string]$ManifestPath,
        [Parameter(Mandatory = $true)][string]$CrateName
    )

    $cmdLine = 'cargo tree --manifest-path "' + $ManifestPath + '" --edges normal,build -i "' + $CrateName + '" 2>NUL'
    $output = & cmd /c $cmdLine
    $text = (($output | ForEach-Object { $_.ToString() }) -join [Environment]::NewLine)
    if ($LASTEXITCODE -ne 0) {
        Invoke-NativeCommand cargo @("tree", "--manifest-path", $ManifestPath, "--edges", "normal,build", "-i", $CrateName)
        throw "cargo tree failed for $CrateName in $ManifestPath`n$text"
    }

    if (-not [string]::IsNullOrWhiteSpace($text)) {
        throw "$CrateName is present in the normal/build dependency tree for $ManifestPath`n$text"
    }

    Write-Host "$CrateName is not present in $ManifestPath normal/build dependency tree."
}

Invoke-Step "Required release tools" {
    foreach ($tool in @("cargo", "cargo-audit", "cargo-deny", "npm", "docker", "promtool", "amtool")) {
        if (($tool -eq "docker" -and $SkipDocker) -or ($tool -eq "npm" -and $SkipFrontend)) {
            continue
        }

        Assert-Command $tool
    }
}

Invoke-Step "Baseline CI validation with strict external tools" {
    $args = @("-StrictExternalTools")
    if ($SkipFrontend) {
        $args += "-SkipFrontend"
    }
    if ($SkipDocker) {
        $args += "-SkipDocker"
    }

    & (Join-Path $repoRoot "ops\validate-ci.ps1") @args
}

Invoke-Step "Rust dependency policy including advisories" {
    Invoke-WithCargoEnvironment -IncludeCargoDenyAdvisoryDb -Action {
        Invoke-NativeCommand cargo @("deny", "--manifest-path", "backend/Cargo.toml", "--locked", "check", "--config", "deny.toml", "advisories", "bans", "licenses", "sources")
        Invoke-NativeCommand cargo @("deny", "--manifest-path", "client-sdk/Cargo.toml", "--locked", "check", "--config", "deny.toml", "advisories", "bans", "licenses", "sources")
    }
}

Invoke-Step "Rust runtime dependency exclusions" {
    Assert-CargoTreeExcludes -ManifestPath "backend/Cargo.toml" -CrateName "rsa"
    Assert-CargoTreeExcludes -ManifestPath "backend/Cargo.toml" -CrateName "sqlx-mysql"
    Assert-CargoTreeExcludes -ManifestPath "backend/Cargo.toml" -CrateName "sqlx-sqlite"
}

if (-not $SkipFrontend) {
    Invoke-Step "Admin dependency audit including dev tooling" {
        $npmCache = Join-Path $repoRoot ".tools\npm-cache"
        Invoke-NativeCommand npm @("-C", "admin", "--cache", $npmCache, "audit", "--audit-level=moderate")
        Invoke-NativeCommand npm @("-C", "admin", "--cache", $npmCache, "audit", "--omit=dev", "--audit-level=moderate")
    }
}

Write-Host "Strict release validation passed."
