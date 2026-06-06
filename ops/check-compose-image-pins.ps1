#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$EnvFile = ".env.compose.example",
    [string]$ProjectName = "user-admin",
    [switch]$RequireDigest
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$repoRoot = Split-Path -Parent $PSScriptRoot

function Resolve-RepoPath {
    param([string]$Path)

    if ([System.IO.Path]::IsPathRooted($Path)) {
        return $Path
    }

    return (Join-Path $repoRoot $Path)
}

function Invoke-DockerComposeConfigJson {
    param([string]$EnvPath)

    $previousComposeEnvFile = $env:COMPOSE_ENV_FILE
    try {
        $env:COMPOSE_ENV_FILE = $EnvPath
        $output = & docker compose -p $ProjectName --env-file $EnvPath config --format json
        if ($LASTEXITCODE -ne 0) {
            throw "docker compose config --format json failed."
        }

        return (($output | ForEach-Object { $_.ToString() }) -join [Environment]::NewLine)
    } finally {
        $env:COMPOSE_ENV_FILE = $previousComposeEnvFile
    }
}

function Test-ImageHasDigest {
    param([string]$Image)

    return $Image -match "@sha256:[0-9a-fA-F]{64}$"
}

function Test-ImageHasTag {
    param([string]$Image)

    $withoutDigest = ($Image -split "@")[0]
    $lastPathPart = ($withoutDigest -split "/")[-1]
    return $lastPathPart.Contains(":")
}

function Get-ImageTag {
    param([string]$Image)

    $withoutDigest = ($Image -split "@")[0]
    $lastPathPart = ($withoutDigest -split "/")[-1]
    if (-not $lastPathPart.Contains(":")) {
        return ""
    }

    return ($lastPathPart -split ":", 2)[1]
}

$envPath = Resolve-RepoPath $EnvFile
if (-not (Test-Path -LiteralPath $envPath)) {
    throw "Env file not found: $envPath"
}

$configJson = Invoke-DockerComposeConfigJson -EnvPath $envPath
$config = $configJson | ConvertFrom-Json

$results = @()
foreach ($service in $config.services.PSObject.Properties) {
    $name = $service.Name
    $imageProperty = $service.Value.PSObject.Properties["image"]
    $image = if ($imageProperty) { [string]$imageProperty.Value } else { "" }
    if ([string]::IsNullOrWhiteSpace($image)) {
        $results += [pscustomobject]@{
            Service = $name
            Image = "(built locally)"
            Status = "SKIP"
            Detail = "service uses build context"
        }
        continue
    }

    $hasTag = Test-ImageHasTag -Image $image
    $hasDigest = Test-ImageHasDigest -Image $image
    $tag = Get-ImageTag -Image $image
    $status = "PASS"
    $detail = "tag pinned"

    if (-not $hasTag -and -not $hasDigest) {
        $status = "FAIL"
        $detail = "image has no tag or digest"
    } elseif ($tag -eq "latest") {
        $status = "FAIL"
        $detail = "latest tag is not allowed"
    } elseif ($RequireDigest -and -not $hasDigest) {
        $status = "FAIL"
        $detail = "digest is required"
    } elseif ($hasDigest) {
        $detail = "digest pinned"
    }

    $results += [pscustomobject]@{
        Service = $name
        Image = $image
        Status = $status
        Detail = $detail
    }
}

$results | Sort-Object Service | Format-Table -AutoSize

$failed = @($results | Where-Object { $_.Status -eq "FAIL" })
if ($failed.Count -gt 0) {
    exit 1
}

Write-Host "Compose image pin check passed."
