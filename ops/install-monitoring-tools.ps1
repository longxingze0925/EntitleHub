param(
    [switch]$Force
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$toolRoot = Join-Path $repoRoot ".tools"
$downloadRoot = Join-Path $toolRoot "downloads"
$binDir = Join-Path $toolRoot "bin"

New-Item -ItemType Directory -Force -Path $downloadRoot, $binDir | Out-Null

function Invoke-ToolDownload {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Uri,
        [Parameter(Mandatory = $true)]
        [string]$OutFile,
        [Parameter(Mandatory = $true)]
        [hashtable]$Headers
    )

    if ($null -ne (Get-Command curl.exe -ErrorAction SilentlyContinue)) {
        & curl.exe -L --fail --retry 3 --retry-delay 5 --output $OutFile $Uri
        if ($LASTEXITCODE -ne 0) {
            throw "curl failed downloading $Uri"
        }
        return
    }

    Invoke-WebRequest -Headers $Headers -Uri $Uri -OutFile $OutFile
}

function Install-GitHubReleaseTool {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Repo,
        [Parameter(Mandatory = $true)]
        [string]$AssetPattern,
        [Parameter(Mandatory = $true)]
        [string]$ExeName
    )

    $target = Join-Path $binDir $ExeName
    if ((Test-Path -LiteralPath $target) -and (-not $Force)) {
        Write-Host "$ExeName already installed at $target"
        return
    }

    $headers = @{
        "User-Agent" = "user-admin-platform-install-monitoring-tools"
    }
    $release = Invoke-RestMethod -Headers $headers -Uri "https://api.github.com/repos/$Repo/releases/latest"
    $asset = $release.assets |
        Where-Object { $_.name -match $AssetPattern } |
        Select-Object -First 1

    if ($null -eq $asset) {
        throw "No release asset matching $AssetPattern found for $Repo $($release.tag_name)"
    }

    $archive = Join-Path $downloadRoot $asset.name
    Write-Host "Downloading $Repo $($release.tag_name): $($asset.name)"
    Invoke-ToolDownload -Headers $headers -Uri $asset.browser_download_url -OutFile $archive

    $extractDir = Join-Path $downloadRoot ("extract-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force -Path $extractDir | Out-Null
    tar -xzf $archive -C $extractDir

    $exe = Get-ChildItem -LiteralPath $extractDir -Recurse -Filter $ExeName |
        Select-Object -First 1
    if ($null -eq $exe) {
        throw "$ExeName not found after extracting $archive"
    }

    Copy-Item -Force -LiteralPath $exe.FullName -Destination $target
    Write-Host "Installed $ExeName to $target"
}

Install-GitHubReleaseTool `
    -Repo "prometheus/prometheus" `
    -AssetPattern "^prometheus-.*windows-amd64\.tar\.gz$" `
    -ExeName "promtool.exe"

Install-GitHubReleaseTool `
    -Repo "prometheus/alertmanager" `
    -AssetPattern "^alertmanager-.*windows-amd64\.tar\.gz$" `
    -ExeName "amtool.exe"

$env:PATH = "$binDir;$env:PATH"

Write-Host ""
promtool --version
amtool --version
