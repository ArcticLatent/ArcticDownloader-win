param(
    [string]$EnvFile = ".env",
    [switch]$SkipCatalogCheck,
    [switch]$Release
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $root

function Import-DotEnv([string]$Path) {
    if (-not (Test-Path $Path)) {
        $commaEnv = Join-Path $root ",env"
        if ($Path -eq ".env" -and (Test-Path $commaEnv)) {
            throw "Found ',env', but the expected file name is '.env'. Rename it, then run this script again."
        }
        throw "Missing env file: $Path"
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

function Require-Env([string]$Name) {
    $value = [Environment]::GetEnvironmentVariable($Name)
    if ([string]::IsNullOrWhiteSpace($value)) {
        throw "Missing required environment variable: $Name"
    }
    return $value
}

Import-DotEnv $EnvFile

$supabaseUrl = Require-Env "ARCTIC_SUPABASE_URL"
$supabaseKey = [Environment]::GetEnvironmentVariable("ARCTIC_SUPABASE_ANON_KEY")
if ([string]::IsNullOrWhiteSpace($supabaseKey)) {
    $supabaseKey = [Environment]::GetEnvironmentVariable("ARCTIC_SUPABASE_PUBLISHABLE_KEY")
}
if ([string]::IsNullOrWhiteSpace($supabaseKey)) {
    throw "Missing ARCTIC_SUPABASE_ANON_KEY or ARCTIC_SUPABASE_PUBLISHABLE_KEY"
}

if (-not $SkipCatalogCheck) {
    $catalogUrl = "$($supabaseUrl.TrimEnd('/'))/rest/v1/catalog_documents?select=catalog&catalog_key=eq.main&limit=1"
    Write-Host "Checking Supabase catalog..."
    $rows = Invoke-RestMethod -Uri $catalogUrl -Headers @{
        apikey = $supabaseKey
        Authorization = "Bearer $supabaseKey"
    }
    if (-not $rows -or $rows.Count -lt 1 -or -not $rows[0].catalog) {
        throw "Supabase responded, but catalog_documents.main was not returned."
    }
    Write-Host "Catalog check passed."
}

$manifest = Join-Path $root "src-tauri\Cargo.toml"
if (-not (Test-Path $manifest)) {
    throw "Missing Tauri manifest: $manifest"
}

if ($Release) {
    Write-Host "Launching release build with local env..."
    & cargo run --release --manifest-path $manifest
} else {
    Write-Host "Launching dev build with local env..."
    & cargo run --manifest-path $manifest
}
if ($LASTEXITCODE -ne 0) {
    throw "App exited with code $LASTEXITCODE"
}
