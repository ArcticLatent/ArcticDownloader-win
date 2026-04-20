param(
    [string]$Version,
    [string]$Repository = "ArcticLatent/Arctic-Helper",
    [string]$AssetName = "Arctic-ComfyUI-Helper.exe",
    [string]$OutputDir = "dist",
    [string]$NotesFile = "",
    [string]$EnvFile = ".env",
    [switch]$SkipSupabaseEnvCheck,
    [switch]$SkipClean
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Write-Utf8NoBom([string]$Path, [string]$Content) {
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, $Content, $utf8NoBom)
}

function Require-Command([string]$Name) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Required command '$Name' was not found in PATH."
    }
}

function Resolve-CargoExe {
    if (Get-Command cargo -ErrorAction SilentlyContinue) {
        return "cargo"
    }
    $fallback = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (Test-Path $fallback) {
        return $fallback
    }
    throw "Cargo not found. Install Rust or add cargo to PATH."
}

function Update-CargoVersion([string]$Path, [string]$NewVersion) {
    $raw = Get-Content $Path -Raw
    $pattern = '(?m)^version\s*=\s*".*?"\s*$'
    if (-not [regex]::IsMatch($raw, $pattern)) {
        throw "Could not update version in $Path"
    }
    $updated = [regex]::Replace($raw, $pattern, "version = `"$NewVersion`"", 1)
    Write-Utf8NoBom -Path $Path -Content $updated
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

if (-not $Version) {
    $Version = Read-Host "Release version (example: 0.1.1)"
}
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Version must be semantic version format: x.y.z"
}

$tag = "v$Version"
$releaseTitle = "Arctic ComfyUI Helper $Version"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $root
Import-DotEnv $EnvFile
if (-not $SkipSupabaseEnvCheck) {
    Require-SupabaseCatalogEnv
    Write-Host "Supabase catalog env loaded for release build."
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

$cargo = Resolve-CargoExe
Require-Command "gh"

Write-Host "Checking GitHub auth..."
& gh auth status | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "GitHub CLI is not authenticated. Run: gh auth login"
}

$tauriCargo = Join-Path $root "src-tauri\Cargo.toml"
$rootCargo = Join-Path $root "Cargo.toml"
$tauriConf = Join-Path $root "src-tauri\tauri.conf.json"

Write-Host "Updating versions to $Version ..."
Update-CargoVersion -Path $rootCargo -NewVersion $Version
Update-CargoVersion -Path $tauriCargo -NewVersion $Version
$conf = Get-Content $tauriConf -Raw | ConvertFrom-Json
$conf.version = $Version
$confJson = $conf | ConvertTo-Json -Depth 20
Write-Utf8NoBom -Path $tauriConf -Content $confJson

if (-not $SkipClean) {
    Write-Host "Running clean build..."
    & $cargo clean --manifest-path .\src-tauri\Cargo.toml
    if ($LASTEXITCODE -ne 0) {
        throw "cargo clean failed"
    }
}

Write-Host "Building release binary (tauri, no bundle)..."
& $cargo tauri build --no-bundle
if ($LASTEXITCODE -ne 0) {
    throw "cargo tauri build --no-bundle failed"
}

$binary = Join-Path $root "src-tauri\target\release\$AssetName"
if (-not (Test-Path $binary)) {
    throw "Release binary not found: $binary"
}

$dist = Join-Path $root $OutputDir
New-Item -ItemType Directory -Path $dist -Force | Out-Null

$assetPath = Join-Path $dist $AssetName
Copy-Item -Path $binary -Destination $assetPath -Force

$sha = (Get-FileHash -Path $assetPath -Algorithm SHA256).Hash.ToLowerInvariant()
$shaPath = Join-Path $dist "$AssetName.sha256"
"$sha *$AssetName" | Set-Content -Path $shaPath -Encoding ascii

$downloadUrl = "https://github.com/$Repository/releases/download/$tag/$AssetName"
$updateManifest = [ordered]@{
    version      = $Version
    download_url = $downloadUrl
    sha256       = $sha
    notes        = $notes
}
$updatePath = Join-Path $dist "update.json"
$updateJson = $updateManifest | ConvertTo-Json -Depth 10
Write-Utf8NoBom -Path $updatePath -Content $updateJson

$notesPath = Join-Path $dist "release-notes-$tag.md"
if ($resolvedNotesFile) {
    Copy-Item -Path $resolvedNotesFile -Destination $notesPath -Force
} else {
    Write-Utf8NoBom -Path $notesPath -Content $notes
}

Write-Host "Publishing GitHub release $tag to $Repository ..."
& gh release view $tag --repo $Repository | Out-Null
if ($LASTEXITCODE -eq 0) {
    & gh release edit $tag --repo $Repository --title $releaseTitle --notes-file $notesPath
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to edit existing release $tag"
    }
    & gh release upload $tag $assetPath $shaPath $updatePath --repo $Repository --clobber
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to upload release artifacts"
    }
} else {
    & gh release create $tag $assetPath $shaPath $updatePath --repo $Repository --title $releaseTitle --notes-file $notesPath
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to create release $tag"
    }
}

Write-Host ""
Write-Host "Release complete:"
Write-Host "  Repo:      $Repository"
Write-Host "  Tag:       $tag"
Write-Host "  Asset:     $assetPath"
Write-Host "  SHA256:    $sha"
Write-Host "  update.json: $updatePath"
