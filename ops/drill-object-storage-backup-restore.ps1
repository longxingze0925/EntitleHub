#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$ProjectName = "user-admin",
    [string]$BackupDir = "backups/object-storage-drills",
    [string]$Image = "debian:bookworm-slim",
    [switch]$KeepRestoreVolume
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

function Assert-SafeDockerName {
    param([string]$Value)

    if ($Value -notmatch "^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$") {
        throw "Unsafe Docker name: $Value"
    }
}

function Invoke-Docker {
    param([string[]]$Arguments)

    $previousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        $output = & docker @Arguments 2>&1
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

function Get-VolumeManifest {
    param([string]$VolumeName)

    $manifest = Invoke-Docker -Arguments @(
        "run",
        "--rm",
        "-v",
        "${VolumeName}:/data:ro",
        $Image,
        "sh",
        "-c",
        "cd /data && find . -type f -exec sha256sum {} \; | sort"
    )

    return @($manifest | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
}

function Remove-DockerContainerQuietly {
    param([string]$Name)

    try {
        Invoke-Docker -Arguments @("rm", "-f", $Name) | Out-Null
    } catch {
        Write-Warning $_.Exception.Message
    }
}

function Remove-DockerVolumeQuietly {
    param([string]$Name)

    try {
        Invoke-Docker -Arguments @("volume", "rm", $Name) | Out-Null
    } catch {
        Write-Warning $_.Exception.Message
    }
}

Assert-SafeDockerName -Value $ProjectName

$backupRoot = Resolve-RepoPath $BackupDir
New-Item -ItemType Directory -Force -Path $backupRoot | Out-Null

$sourceVolume = "${ProjectName}_object-storage"
Assert-SafeDockerName -Value $sourceVolume
Invoke-Docker -Arguments @("volume", "inspect", $sourceVolume) | Out-Null

$suffix = ([Guid]::NewGuid().ToString("N")).Substring(0, 12)
$timestamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$archiveName = "object_storage_${timestamp}_${suffix}.tar.gz"
$localArchive = Join-Path $backupRoot $archiveName
$localSha = "$localArchive.sha256"
$backupContainer = "object-storage-backup-$suffix"
$restoreContainer = "object-storage-restore-$suffix"
$restoreVolume = "${ProjectName}_object-storage-restore-drill-$suffix"

Assert-SafeDockerName -Value $backupContainer
Assert-SafeDockerName -Value $restoreContainer
Assert-SafeDockerName -Value $restoreVolume

$restoreVolumeCreated = $false

try {
    Write-Host "==> Build source manifest"
    $sourceManifest = @(Get-VolumeManifest -VolumeName $sourceVolume)

    Write-Host "==> Archive object-storage volume"
    Invoke-Docker -Arguments @(
        "create",
        "--name",
        $backupContainer,
        "-v",
        "${sourceVolume}:/data:ro",
        $Image,
        "sh",
        "-c",
        "tar -czf /tmp/$archiveName -C /data ."
    ) | Out-Null
    Invoke-Docker -Arguments @("start", "-a", $backupContainer) | Out-Host
    Invoke-Docker -Arguments @("cp", "${backupContainer}:/tmp/$archiveName", $localArchive) | Out-Null

    $hash = Get-FileHash -Algorithm SHA256 -LiteralPath $localArchive
    Set-Content -LiteralPath $localSha -Value "$($hash.Hash.ToLowerInvariant())  $(Split-Path -Leaf $localArchive)" -Encoding UTF8

    Write-Host "==> Restore archive into disposable volume"
    Invoke-Docker -Arguments @("volume", "create", $restoreVolume) | Out-Null
    $restoreVolumeCreated = $true
    Invoke-Docker -Arguments @(
        "create",
        "--name",
        $restoreContainer,
        "-v",
        "${restoreVolume}:/restore",
        $Image,
        "sh",
        "-c",
        "tar -xzf /tmp/$archiveName -C /restore"
    ) | Out-Null
    Invoke-Docker -Arguments @("cp", $localArchive, "${restoreContainer}:/tmp/$archiveName") | Out-Null
    Invoke-Docker -Arguments @("start", "-a", $restoreContainer) | Out-Host

    Write-Host "==> Compare restored manifest"
    $restoreManifest = @(Get-VolumeManifest -VolumeName $restoreVolume)
    $sourceText = ($sourceManifest -join [Environment]::NewLine)
    $restoreText = ($restoreManifest -join [Environment]::NewLine)
    if ($sourceText -ne $restoreText) {
        throw "object-storage restore verification failed."
    }

    Write-Host "Object-storage backup drill passed."
    Write-Host "Files checked: $($sourceManifest.Count)"
    Write-Host "Archive: $localArchive"
    Write-Host "SHA256: $localSha"
} finally {
    Write-Host "==> Cleanup"
    Remove-DockerContainerQuietly -Name $backupContainer
    Remove-DockerContainerQuietly -Name $restoreContainer
    if ($restoreVolumeCreated -and -not $KeepRestoreVolume) {
        Remove-DockerVolumeQuietly -Name $restoreVolume
    }
}
