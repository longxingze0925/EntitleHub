#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$EnvFile = ".env.compose",
    [string]$ProjectName = "user-admin",
    [string]$HostName = "localhost",
    [int]$TimeoutSeconds = 10,
    [string]$TenantName = "Local Smoke Tenant",
    [string]$TenantSlug = "local-smoke",
    [string]$OwnerEmail = "owner.local@example.com",
    [string]$OwnerName = "Local Owner",
    [string]$OwnerPassword = $env:INIT_OWNER_PASSWORD,
    [string]$CredentialFile = ".tools/init-owner-smoke.env",
    [switch]$RunMigrations,
    [switch]$NoCredentialFile
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

function Get-PortValue {
    param(
        [hashtable]$Values,
        [string]$Key,
        [int]$Default
    )

    $raw = Get-EnvValue -Values $Values -Key $Key -Default ([string]$Default)
    $port = 0
    if (-not [int]::TryParse($raw, [ref]$port) -or $port -lt 1 -or $port -gt 65535) {
        throw "$Key must be a valid TCP port."
    }

    return $port
}

function Invoke-NativeCapture {
    param(
        [string]$Command,
        [string[]]$Arguments
    )

    $previousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        $output = & $Command @Arguments 2>&1
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previousErrorActionPreference
    }

    $textOutput = @($output | ForEach-Object { $_.ToString() })
    if ($exitCode -ne 0) {
        throw ($textOutput -join [Environment]::NewLine)
    }

    return $textOutput
}

function Invoke-Compose {
    param([string[]]$Arguments)

    return Invoke-NativeCapture -Command "docker" -Arguments (@("compose", "-p", $ProjectName, "--env-file", $envPath) + $Arguments)
}

function Invoke-Psql {
    param(
        [string]$Database,
        [string]$Sql
    )

    $args = @(
        "exec",
        "-T",
        "-e",
        "PGPASSWORD=$postgresPassword",
        "postgres",
        "psql",
        "-v",
        "ON_ERROR_STOP=1",
        "-U",
        $postgresUser,
        "-d",
        $Database,
        "-At",
        "-c",
        $Sql
    )

    return Invoke-Compose -Arguments $args
}

function New-SmokePassword {
    $bytes = New-Object byte[] 24
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try {
        $rng.GetBytes($bytes)
    } finally {
        $rng.Dispose()
    }

    $token = [Convert]::ToBase64String($bytes).TrimEnd("=").Replace("+", "-").Replace("/", "_")
    return "Aa1!$token"
}

function Write-CredentialFile {
    param([string]$Path)

    $directory = Split-Path -Parent $Path
    if (-not [string]::IsNullOrWhiteSpace($directory)) {
        New-Item -ItemType Directory -Force -Path $directory | Out-Null
    }

    $lines = @(
        "# Local init-owner smoke credentials. Do not commit.",
        "INIT_TENANT_NAME=$TenantName",
        "INIT_TENANT_SLUG=$TenantSlug",
        "INIT_OWNER_EMAIL=$OwnerEmail",
        "INIT_OWNER_NAME=$OwnerName",
        "INIT_OWNER_PASSWORD=$OwnerPassword"
    )
    Set-Content -LiteralPath $Path -Value $lines -Encoding UTF8
}

function Invoke-Migrations {
    Write-Host "==> Run database migrations"
    Invoke-Compose -Arguments @("run", "--rm", "backend", "user-admin-backend", "migrate") | Out-Host
}

function Assert-ApiEnvelope {
    param(
        [object]$Envelope,
        [string]$Name
    )

    if ($Envelope.code -ne 0) {
        throw "$Name failed: code=$($Envelope.code) message=$($Envelope.message)"
    }

    if (-not $Envelope.data) {
        throw "$Name failed: missing data."
    }
}

$envPath = Resolve-RepoPath $EnvFile
if (-not (Test-Path -LiteralPath $envPath)) {
    throw "Env file not found: $envPath"
}

$credentialPath = Resolve-RepoPath $CredentialFile
if (-not $NoCredentialFile -and (Test-Path -LiteralPath $credentialPath) -and [string]::IsNullOrWhiteSpace($OwnerPassword)) {
    $credentialValues = Read-EnvFile -Path $credentialPath
    $TenantName = Get-EnvValue -Values $credentialValues -Key "INIT_TENANT_NAME" -Default $TenantName
    $TenantSlug = Get-EnvValue -Values $credentialValues -Key "INIT_TENANT_SLUG" -Default $TenantSlug
    $OwnerEmail = Get-EnvValue -Values $credentialValues -Key "INIT_OWNER_EMAIL" -Default $OwnerEmail
    $OwnerName = Get-EnvValue -Values $credentialValues -Key "INIT_OWNER_NAME" -Default $OwnerName
    $OwnerPassword = Get-EnvValue -Values $credentialValues -Key "INIT_OWNER_PASSWORD" -Default $OwnerPassword
}

$envValues = Read-EnvFile -Path $envPath
$backendPort = Get-PortValue -Values $envValues -Key "BACKEND_HOST_PORT" -Default 8080
$postgresDb = Get-EnvValue -Values $envValues -Key "POSTGRES_DB" -Default "user_admin"
$postgresUser = Get-EnvValue -Values $envValues -Key "POSTGRES_USER" -Default "app_user"
$postgresPassword = Get-EnvValue -Values $envValues -Key "POSTGRES_PASSWORD" -Default ""

if ([string]::IsNullOrWhiteSpace($postgresPassword)) {
    throw "POSTGRES_PASSWORD is required in $envPath."
}

if ($RunMigrations) {
    Invoke-Migrations
}

Write-Host "==> Check existing tenants"
try {
    $activeTenantCount = [int](((Invoke-Psql -Database $postgresDb -Sql "select count(*) from tenants where deleted_at is null;") -join "").Trim())
} catch {
    throw "Cannot query tenants. Run this script with -RunMigrations or apply migrations before init-owner smoke. $($_.Exception.Message)"
}

$generatedPassword = $false
if ($activeTenantCount -eq 0) {
    if ([string]::IsNullOrWhiteSpace($OwnerPassword)) {
        $OwnerPassword = New-SmokePassword
        $generatedPassword = $true
    }

    Write-Host "==> Run init-owner"
    Invoke-Compose -Arguments @(
        "run",
        "--rm",
        "-e",
        "INIT_TENANT_NAME=$TenantName",
        "-e",
        "INIT_TENANT_SLUG=$TenantSlug",
        "-e",
        "INIT_OWNER_EMAIL=$OwnerEmail",
        "-e",
        "INIT_OWNER_NAME=$OwnerName",
        "-e",
        "INIT_OWNER_PASSWORD=$OwnerPassword",
        "backend",
        "user-admin-backend",
        "init-owner"
    ) | Out-Host

    if ($generatedPassword -and -not $NoCredentialFile) {
        Write-CredentialFile -Path $credentialPath
        Write-Host "Generated local credentials: $credentialPath"
    }
} else {
    Write-Host "Active tenants already exist; skip init-owner."
    if ([string]::IsNullOrWhiteSpace($OwnerPassword)) {
        throw "Owner password is required for login smoke when the database is already initialized. Pass -OwnerPassword or keep $credentialPath."
    }
}

Write-Host "==> Verify admin login"
$loginUrl = "http://${HostName}:$backendPort/api/auth/login"
$loginPayload = @{
    email = $OwnerEmail
    password = $OwnerPassword
} | ConvertTo-Json -Compress

$webSession = $null
$loginResponse = Invoke-RestMethod -Uri $loginUrl -Method Post -Body $loginPayload -ContentType "application/json" -TimeoutSec $TimeoutSeconds -SessionVariable webSession
Assert-ApiEnvelope -Envelope $loginResponse -Name "admin login"

$loginData = $loginResponse.data
$expectedEmail = $OwnerEmail.ToLowerInvariant()
if ($loginData.user.email -ne $expectedEmail) {
    throw "admin login returned unexpected email: $($loginData.user.email)"
}

if (@($loginData.roles) -notcontains "owner") {
    throw "admin login did not return owner role."
}

if (@($loginData.permissions).Count -lt 1) {
    throw "admin login returned no permissions."
}

Write-Host "==> Verify /api/auth/me with login session"
$meUrl = "http://${HostName}:$backendPort/api/auth/me"
$meResponse = Invoke-RestMethod -Uri $meUrl -Method Get -TimeoutSec $TimeoutSeconds -WebSession $webSession
Assert-ApiEnvelope -Envelope $meResponse -Name "admin me"

if ($meResponse.data.user.email -ne $expectedEmail) {
    throw "admin me returned unexpected email: $($meResponse.data.user.email)"
}

if (@($meResponse.data.roles) -notcontains "owner") {
    throw "admin me did not return owner role."
}

Write-Host "Init-owner smoke passed."
