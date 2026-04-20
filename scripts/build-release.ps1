param(
    [Parameter(Mandatory = $true)]
    [string]$Version,
    [string]$Repository = "ArcticLatent/Arctic-Helper",
    [string]$Tag = "",
    [string]$OutputDir = "dist",
    [string]$AssetName = "Arctic-ComfyUI-Helper.exe",
    [string]$NotesFile = "",
    [string]$EnvFile = ".env",
    [switch]$SkipSupabaseEnvCheck
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not $Tag) {
    $Tag = "v$Version"
}

if (-not $Repository) {
    throw "Repository is required (for download URL generation)."
}

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $root

function Import-DotEnv([string]$Path) {
    if (-not (Test-Path $Path)) {
        $commaEnv = Join-Path $root ",env"
        if ($Path -eq ".env" -and (Test-Path $commaEnv)) {
            throw "Found ',env', but the expected file name is '.env'. Rename it before building a release."
        }
        return
    }

    Get-Content $Path | ForEach-Object {
        $line = $_.Trim()
        if (-not $line -or $line.StartsWith("#")) {
            return
        }

        $match = [regex]::Match($line, '^\s*([^=]+?)\s*=\s*(.*)\s*$')
        if (-not $match.Success) {
            return
        }

        $name = $match.Groups[1].Value.Trim()
        $value = $match.Groups[2].Value.Trim()
        if (
            ($value.StartsWith('"') -and $value.EndsWith('"')) -or
            ($value.StartsWith("'") -and $value.EndsWith("'"))
        ) {
            $value = $value.Substring(1, $value.Length - 2)
        }

        if ($name) {
            Set-Item -Path "Env:$name" -Value $value
        }
    }
}

function Require-SupabaseCatalogEnv {
    $url = [Environment]::GetEnvironmentVariable("ARCTIC_SUPABASE_URL")
    $anonKey = [Environment]::GetEnvironmentVariable("ARCTIC_SUPABASE_ANON_KEY")
    $publishableKey = [Environment]::GetEnvironmentVariable("ARCTIC_SUPABASE_PUBLISHABLE_KEY")

    if ([string]::IsNullOrWhiteSpace($url)) {
        throw "Missing ARCTIC_SUPABASE_URL. Set it in .env or the current shell before release build."
    }
    if ([string]::IsNullOrWhiteSpace($anonKey) -and [string]::IsNullOrWhiteSpace($publishableKey)) {
        throw "Missing ARCTIC_SUPABASE_ANON_KEY or ARCTIC_SUPABASE_PUBLISHABLE_KEY. Set a public read key before release build."
    }
}

function Resolve-NotesFile([string]$RepoRoot, [string]$ReleaseVersion, [string]$ExplicitNotesFile) {
    if ($ExplicitNotesFile) {
        $resolved = Resolve-Path $ExplicitNotesFile -ErrorAction Stop
        return [string]$resolved
    }

    $defaultNotes = Join-Path $RepoRoot "CHANGELOG_$ReleaseVersion.md"
    if (Test-Path $defaultNotes) {
        return $defaultNotes
    }

    return $null
}

Import-DotEnv $EnvFile
if (-not $SkipSupabaseEnvCheck) {
    Require-SupabaseCatalogEnv
    Write-Host "Supabase catalog env loaded for release build."
}

$cargo = "cargo"
$tauriManifest = Join-Path $root "src-tauri\Cargo.toml"
$rootManifest = Join-Path $root "Cargo.toml"
if (-not (Test-Path $tauriManifest)) {
    throw "Missing Tauri manifest at $tauriManifest"
}
if (-not (Test-Path $rootManifest)) {
    throw "Missing root manifest at $rootManifest"
}

function Read-CargoVersion([string]$Path) {
    $raw = Get-Content $Path -Raw
    $m = [regex]::Match($raw, '(?m)^version\s*=\s*"([^"]+)"\s*$')
    if (-not $m.Success) {
        throw "Could not read version from $Path"
    }
    return $m.Groups[1].Value
}

$rootVersion = Read-CargoVersion $rootManifest
$tauriVersion = Read-CargoVersion $tauriManifest
if ($rootVersion -ne $Version -or $tauriVersion -ne $Version) {
    throw "Version mismatch. Expected $Version, found root=$rootVersion tauri=$tauriVersion. Update both Cargo.toml files first."
}

$resolvedNotesFile = Resolve-NotesFile -RepoRoot $root -ReleaseVersion $Version -ExplicitNotesFile $NotesFile
$notes = "Release v$Version"
if ($resolvedNotesFile) {
    Write-Host "Using changelog notes file: $resolvedNotesFile"
    $notes = (Get-Content $resolvedNotesFile -Raw).Trim()
    if ([string]::IsNullOrWhiteSpace($notes)) {
        $notes = "Release v$Version"
    }
}

Write-Host "Building release binary..."
& $cargo build --release --manifest-path $tauriManifest
if ($LASTEXITCODE -ne 0) {
    throw "cargo build --release --manifest-path src-tauri/Cargo.toml failed"
}

$binary = Join-Path $root "src-tauri\target\release\$AssetName"
if (-not (Test-Path $binary)) {
    throw "Expected binary not found at $binary"
}

$distDir = Join-Path $root $OutputDir
New-Item -ItemType Directory -Path $distDir -Force | Out-Null

$assetPath = Join-Path $distDir $AssetName
Copy-Item -Path $binary -Destination $assetPath -Force
if (-not (Test-Path $assetPath)) {
    throw "Release asset not found at $assetPath"
}

$sha = (Get-FileHash -Path $assetPath -Algorithm SHA256).Hash.ToLowerInvariant()
$downloadUrl = "https://github.com/$Repository/releases/download/$Tag/$AssetName"

$manifest = [ordered]@{
    version      = $Version
    download_url = $downloadUrl
    sha256       = $sha
    notes        = $notes
}

$manifestJson = $manifest | ConvertTo-Json -Depth 4
$manifestPath = Join-Path $root "update.json"
$manifestDistPath = Join-Path $root "$OutputDir\update.json"
$manifestJson | Set-Content -Path $manifestPath -Encoding utf8
$manifestJson | Set-Content -Path $manifestDistPath -Encoding utf8

if ($resolvedNotesFile) {
    $notesDistPath = Join-Path $distDir "release-notes-$Tag.md"
    Copy-Item -Path $resolvedNotesFile -Destination $notesDistPath -Force
}

Write-Host "Asset: $assetPath"
Write-Host "SHA256: $sha"
Write-Host "Manifest: $manifestPath"

if ($env:GITHUB_OUTPUT) {
    "asset_path=$assetPath" | Out-File -FilePath $env:GITHUB_OUTPUT -Encoding utf8 -Append
    "manifest_path=$manifestDistPath" | Out-File -FilePath $env:GITHUB_OUTPUT -Encoding utf8 -Append
    "sha256=$sha" | Out-File -FilePath $env:GITHUB_OUTPUT -Encoding utf8 -Append
    "version=$Version" | Out-File -FilePath $env:GITHUB_OUTPUT -Encoding utf8 -Append
}
