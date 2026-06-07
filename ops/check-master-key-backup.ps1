#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$EnvFile = ".env.compose",
    [string]$BackupKeyFile = "",
    [string]$FingerprintDir = "backups/master-key-fingerprints",
    [switch]$RequireBackup
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot

function Resolve-RepoPath {
    param([string]$Path)

    if ([System.IO.Path]::IsPathRooted($Path)) {
        return $Path
    }

    return (Join-Path $repoRoot $Path)
}

function Read-EnvFile {
    param([string]$Path)

    $values = @{}
    foreach ($rawLine in Get-Content -LiteralPath $Path -Encoding UTF8) {
        $line = $rawLine.Trim()
        if ($line.Length -eq 0 -or $line.StartsWith("#")) {
            continue
        }

        $separator = $line.IndexOf("=")
        if ($separator -lt 1) {
            continue
        }

        $key = $line.Substring(0, $separator).Trim()
        $value = $line.Substring($separator + 1).Trim()
        if (($value.StartsWith('"') -and $value.EndsWith('"')) -or ($value.StartsWith("'") -and $value.EndsWith("'"))) {
            $value = $value.Substring(1, $value.Length - 2)
        }

        $values[$key] = $value
    }

    return $values
}

function Read-MasterKeyFile {
    param([string]$Path)

    $content = (Get-Content -LiteralPath $Path -Encoding UTF8) -join [Environment]::NewLine
    $trimmed = $content.Trim()
    if ($trimmed -match "(?m)^\s*MASTER_KEY\s*=\s*(.+?)\s*$") {
        $value = $Matches[1].Trim()
        if (($value.StartsWith('"') -and $value.EndsWith('"')) -or ($value.StartsWith("'") -and $value.EndsWith("'"))) {
            return $value.Substring(1, $value.Length - 2)
        }

        return $value
    }

    return $trimmed
}

function Get-MasterKeyBytes {
    param(
        [string]$Value,
        [string]$Label
    )

    if ([string]::IsNullOrWhiteSpace($Value)) {
        throw "$Label is empty."
    }

    try {
        $bytes = [Convert]::FromBase64String($Value.Trim())
    } catch {
        throw "$Label must be base64 encoded."
    }

    if ($bytes.Length -ne 32) {
        throw "$Label must decode to exactly 32 bytes."
    }

    return $bytes
}

function Get-Sha256Hex {
    param([byte[]]$Bytes)

    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hash = $sha.ComputeHash($Bytes)
    } finally {
        $sha.Dispose()
    }

    return (($hash | ForEach-Object { $_.ToString("x2") }) -join "")
}

$envPath = Resolve-RepoPath $EnvFile
if (-not (Test-Path -LiteralPath $envPath)) {
    throw "Env file not found: $envPath"
}

$envValues = Read-EnvFile -Path $envPath
if (-not $envValues.ContainsKey("MASTER_KEY")) {
    throw "MASTER_KEY is required in $envPath."
}

$masterKeyBytes = Get-MasterKeyBytes -Value $envValues["MASTER_KEY"] -Label "MASTER_KEY"
$fingerprint = Get-Sha256Hex -Bytes $masterKeyBytes

$fingerprintRoot = Resolve-RepoPath $FingerprintDir
New-Item -ItemType Directory -Force -Path $fingerprintRoot | Out-Null
$timestamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$fingerprintFile = Join-Path $fingerprintRoot "master_key_${timestamp}.sha256"
Set-Content -LiteralPath $fingerprintFile -Value "$fingerprint  MASTER_KEY" -Encoding UTF8

Write-Host "MASTER_KEY is valid base64 for 32 bytes."
Write-Host "Fingerprint: $fingerprint"
Write-Host "Fingerprint file: $fingerprintFile"

if (-not [string]::IsNullOrWhiteSpace($BackupKeyFile)) {
    $backupPath = Resolve-RepoPath $BackupKeyFile
    if (-not (Test-Path -LiteralPath $backupPath)) {
        throw "Backup key file not found: $backupPath"
    }

    $backupKeyBytes = Get-MasterKeyBytes -Value (Read-MasterKeyFile -Path $backupPath) -Label "Backup MASTER_KEY"
    $backupFingerprint = Get-Sha256Hex -Bytes $backupKeyBytes
    if ($backupFingerprint -ne $fingerprint) {
        throw "Backup MASTER_KEY fingerprint mismatch."
    }

    Write-Host "Backup MASTER_KEY matches current MASTER_KEY."
} else {
    if ($RequireBackup) {
        throw "Backup key file is required when -RequireBackup is set."
    }

    Write-Host "No backup key file was provided; only format and fingerprint were checked."
}
