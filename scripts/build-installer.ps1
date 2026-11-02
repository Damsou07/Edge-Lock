$ErrorActionPreference = 'Stop'

$taskRoot = Split-Path -Parent $PSScriptRoot
$taskOriginalPath = $env:Path
Push-Location $taskRoot
try {
    $taskConfig = Get-Content -LiteralPath 'src-tauri/tauri.conf.json' -Raw | ConvertFrom-Json
    $taskVersion = [string]$taskConfig.version
    $taskProduct = [string]$taskConfig.productName
    if ($taskVersion -notmatch '^\d+\.\d+\.\d+$' -or -not $taskProduct) {
        throw 'Nom de produit ou version numérique majeure.mineure.correctif invalide dans src-tauri/tauri.conf.json.'
    }

    # Use the repository's portable Node when the current one is unsuitable.
    $taskNode = Get-Command node.exe -ErrorAction SilentlyContinue
    $taskNodeVersion = if ($taskNode) { & $taskNode.Source --version } else { '' }
    if ($taskNodeVersion -notmatch '^v26\.(\d+)\.(\d+)$' -or [int]$Matches[1] -lt 13) {
        $taskPortableNode = Join-Path $taskRoot '.tools/node-v26.13.0-win-x64/node.exe'
        if (-not (Test-Path -LiteralPath $taskPortableNode -PathType Leaf)) {
            throw 'Node.js >= 26.13.0 et < 27 est requis. Installez-le ou ajoutez-le au PATH.'
        }
        $env:Path = "$(Split-Path -Parent $taskPortableNode);$taskOriginalPath"
    }
    if (-not (Get-Command npm.cmd -ErrorAction SilentlyContinue)) {
        throw 'npm est introuvable dans le PATH.'
    }
    if (-not (Get-Command cargo.exe -ErrorAction SilentlyContinue)) {
        throw 'Cargo est introuvable. Installez Rust via rustup et les outils C++ MSVC.'
    }
    if (-not (Test-Path -LiteralPath 'node_modules/.bin/tauri.cmd' -PathType Leaf)) {
        throw 'Les dépendances du projet sont absentes. Exécutez npm ci à la racine du projet, puis relancez ce script.'
    }

    $taskTargetDir = if ($env:CARGO_TARGET_DIR) {
        if ([System.IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) {
            [System.IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
        } else {
            [System.IO.Path]::GetFullPath((Join-Path $taskRoot $env:CARGO_TARGET_DIR))
        }
    } else {
        Join-Path $taskRoot 'target'
    }
    $taskInstaller = Join-Path $taskTargetDir "release/bundle/nsis/${taskProduct}_${taskVersion}_x64-setup.exe"
    $taskExecutable = Join-Path $taskTargetDir 'release/edge-lock.exe'
    $taskStart = [DateTime]::UtcNow

    Write-Host "Construction de $taskProduct $taskVersion depuis $taskRoot..."
    & npm.cmd run tauri -- build --bundles nsis --ci
    if ($LASTEXITCODE -ne 0) { throw "La construction Tauri a échoué (code $LASTEXITCODE)." }

    foreach ($taskFile in @($taskExecutable, $taskInstaller)) {
        if (-not (Test-Path -LiteralPath $taskFile -PathType Leaf)) {
            throw "Fichier attendu absent : $taskFile. Vérifiez la cible de compilation x64 et CARGO_TARGET_DIR."
        }
        $taskItem = Get-Item -LiteralPath $taskFile
        if ($taskItem.Length -eq 0) { throw "Fichier vide : $taskFile" }
        $taskActualVersion = $taskItem.VersionInfo.ProductVersion
        if ($taskActualVersion -notin @($taskVersion, "$taskVersion.0")) {
            throw "Version inattendue dans $taskFile : $taskActualVersion (attendue : $taskVersion)."
        }
    }
    if ((Get-Item -LiteralPath $taskInstaller).LastWriteTimeUtc -lt $taskStart.AddSeconds(-2)) {
        throw "L’installateur n’a pas été régénéré pendant cette construction : $taskInstaller"
    }

    $taskHash = (Get-FileHash -LiteralPath $taskInstaller -Algorithm SHA256).Hash
    Write-Host "Installateur prêt : $taskInstaller"
    Write-Host "Version : $taskVersion | SHA-256 : $taskHash"
} finally {
    $env:Path = $taskOriginalPath
    Pop-Location
}
