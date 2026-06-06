#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$EnvFile = ".env.compose",
    [string]$ProjectName = "user-admin",
    [string]$BackupDir = "backups/drills",
    [switch]$KeepDatabases
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

function Get-EnvValue {
    param(
        [hashtable]$Values,
        [string]$Key,
        [string]$Default
    )

    if ($Values.ContainsKey($Key) -and -not [string]::IsNullOrWhiteSpace($Values[$Key])) {
        return $Values[$Key]
    }

    return $Default
}

function Assert-SafeIdentifier {
    param([string]$Value)

    if ($Value -notmatch "^[a-z][a-z0-9_]{0,62}$") {
        throw "Unsafe SQL identifier: $Value"
    }
}

function Invoke-Compose {
    param(
        [string[]]$Arguments,
        [switch]$Capture
    )

    if ($Capture) {
        $output = & docker compose -p $ProjectName --env-file $envPath @Arguments 2>&1
        if ($LASTEXITCODE -ne 0) {
            throw ($output -join [Environment]::NewLine)
        }
        return $output
    }

    & docker compose -p $ProjectName --env-file $envPath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "docker compose $($Arguments -join ' ') failed."
    }
}

function Invoke-Docker {
    param([string[]]$Arguments)

    & docker @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "docker $($Arguments -join ' ') failed."
    }
}

function Invoke-Psql {
    param(
        [string]$Database,
        [string]$Sql,
        [switch]$TuplesOnly
    )

    $args = @("exec", "-T", "-e", "PGPASSWORD=$postgresPassword", "postgres", "psql", "-v", "ON_ERROR_STOP=1", "-U", $postgresUser, "-d", $Database)
    if ($TuplesOnly) {
        $args += "-At"
    }
    $args += @("-c", $Sql)

    return Invoke-Compose -Arguments $args -Capture
}

$envPath = Resolve-RepoPath $EnvFile
if (-not (Test-Path -LiteralPath $envPath)) {
    throw "Env file not found: $envPath"
}

$backupRoot = Resolve-RepoPath $BackupDir
New-Item -ItemType Directory -Force -Path $backupRoot | Out-Null

$envValues = Read-EnvFile -Path $envPath
$postgresUser = Get-EnvValue -Values $envValues -Key "POSTGRES_USER" -Default "app_user"
$postgresPassword = Get-EnvValue -Values $envValues -Key "POSTGRES_PASSWORD" -Default ""

if ([string]::IsNullOrWhiteSpace($postgresPassword)) {
    throw "POSTGRES_PASSWORD is required in $envPath."
}

$suffix = ([Guid]::NewGuid().ToString("N")).Substring(0, 12)
$sourceDb = "backup_drill_src_$suffix"
$restoreDb = "backup_drill_restore_$suffix"
Assert-SafeIdentifier -Value $sourceDb
Assert-SafeIdentifier -Value $restoreDb

$timestamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$containerDump = "/tmp/${sourceDb}.dump"
$localDump = Join-Path $backupRoot "${sourceDb}_${timestamp}.dump"
$localSha = "$localDump.sha256"
$createdSourceDb = $false
$createdRestoreDb = $false

try {
    Write-Host "==> Locate postgres container"
    $containerId = ((Invoke-Compose -Arguments @("ps", "-q", "postgres") -Capture) -join "").Trim()
    if ([string]::IsNullOrWhiteSpace($containerId)) {
        throw "postgres container is not running for project $ProjectName."
    }

    Write-Host "==> Create disposable databases"
    Invoke-Psql -Database "postgres" -Sql "CREATE DATABASE $sourceDb;" | Out-Null
    $createdSourceDb = $true
    Invoke-Psql -Database "postgres" -Sql "CREATE DATABASE $restoreDb;" | Out-Null
    $createdRestoreDb = $true

    Write-Host "==> Seed source database"
    Invoke-Psql -Database $sourceDb -Sql "CREATE TABLE backup_drill_items (id integer PRIMARY KEY, name text NOT NULL); INSERT INTO backup_drill_items (id, name) VALUES (1, 'alpha'), (2, 'beta'), (3, 'gamma');" | Out-Null
    $sourceFingerprint = ((Invoke-Psql -Database $sourceDb -TuplesOnly -Sql "SELECT count(*)::text || ':' || sum(id)::text || ':' || string_agg(name, ',' ORDER BY id) FROM backup_drill_items;") -join "").Trim()

    Write-Host "==> Dump source database"
    Invoke-Compose -Arguments @("exec", "-T", "-e", "PGPASSWORD=$postgresPassword", "postgres", "pg_dump", "-U", $postgresUser, "-d", $sourceDb, "--format=custom", "--no-owner", "--no-acl", "--file", $containerDump)
    Invoke-Docker -Arguments @("cp", "${containerId}:$containerDump", $localDump)
    $hash = Get-FileHash -Algorithm SHA256 -LiteralPath $localDump
    Set-Content -LiteralPath $localSha -Value "$($hash.Hash.ToLowerInvariant())  $(Split-Path -Leaf $localDump)" -Encoding UTF8

    Write-Host "==> Restore into disposable database"
    Invoke-Compose -Arguments @("exec", "-T", "-e", "PGPASSWORD=$postgresPassword", "postgres", "pg_restore", "-U", $postgresUser, "-d", $restoreDb, "--no-owner", "--no-acl", $containerDump)
    $restoreFingerprint = ((Invoke-Psql -Database $restoreDb -TuplesOnly -Sql "SELECT count(*)::text || ':' || sum(id)::text || ':' || string_agg(name, ',' ORDER BY id) FROM backup_drill_items;") -join "").Trim()

    if ($sourceFingerprint -ne $restoreFingerprint) {
        throw "restore verification failed: source=$sourceFingerprint restore=$restoreFingerprint"
    }

    Write-Host "Backup drill passed."
    Write-Host "Dump: $localDump"
    Write-Host "SHA256: $localSha"
} finally {
    Write-Host "==> Cleanup"
    try {
        Invoke-Compose -Arguments @("exec", "-T", "postgres", "rm", "-f", $containerDump)
    } catch {
        Write-Warning $_.Exception.Message
    }

    if (-not $KeepDatabases) {
        try {
            if ($createdRestoreDb) {
                Invoke-Psql -Database "postgres" -Sql "DROP DATABASE $restoreDb WITH (FORCE);" | Out-Null
            }
            if ($createdSourceDb) {
                Invoke-Psql -Database "postgres" -Sql "DROP DATABASE $sourceDb WITH (FORCE);" | Out-Null
            }
        } catch {
            Write-Warning $_.Exception.Message
        }
    }
}
