param(
    [Parameter(Mandatory = $true)][string]$InstallerPath,
    [string]$PreviousInstallerPath,
    [Parameter(Mandatory = $true)][string]$FixtureLibraryPath,
    [switch]$DisposableEnvironment
)
$ErrorActionPreference = 'Stop'
if (-not $DisposableEnvironment -and $env:USERNAME -ne 'WDAGUtilityAccount') {
    throw 'Exécuter exclusivement dans Windows Sandbox ou une VM jetable, avec -DisposableEnvironment.'
}
$taskInstaller = (Resolve-Path -LiteralPath $InstallerPath).Path
$taskFixture = (Resolve-Path -LiteralPath $FixtureLibraryPath).Path
if (-not (Test-Path -LiteralPath (Join-Path $taskFixture 'library.sqlite3'))) {
    throw "Fournir une copie jetable fermée d’une bibliothèque Edge Lock créée par les tests desktop."
}
$taskRoot = Join-Path $env:TEMP ('Edge-Lock-installer-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $taskRoot | Out-Null
$taskInstallDir = Join-Path $taskRoot 'app'
$taskLibrary = Join-Path $taskRoot 'library'
Copy-Item -LiteralPath $taskFixture -Destination $taskLibrary -Recurse
function Get-LibraryHashes([string]$Directory) {
    @(Get-ChildItem -LiteralPath $Directory -File -Recurse | Sort-Object FullName | ForEach-Object {
        $_.FullName.Substring($Directory.Length) + ':' + (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash
    })
}
$taskBefore = Get-LibraryHashes $taskLibrary
function Install-Package([string]$Package) {
    $taskProcess = Start-Process -FilePath $Package -ArgumentList @('/S', "/D=$taskInstallDir") -WindowStyle Hidden -Wait -PassThru
    if ($taskProcess.ExitCode -ne 0) { throw "Installation échouée : $($taskProcess.ExitCode)" }
    if (-not (Test-Path -LiteralPath (Join-Path $taskInstallDir 'edge-lock.exe'))) { throw 'Exécutable installé absent.' }
}
if ($PreviousInstallerPath) { Install-Package (Resolve-Path -LiteralPath $PreviousInstallerPath).Path }
Install-Package $taskInstaller
$taskVersion = (Get-Item -LiteralPath (Join-Path $taskInstallDir 'edge-lock.exe')).VersionInfo.ProductVersion
if ($taskVersion -notmatch '^0\.2\.0(?:\.0)?$') { throw "Version inattendue : $taskVersion" }
if (Compare-Object $taskBefore (Get-LibraryHashes $taskLibrary)) { throw "La bibliothèque a changé pendant l’installation." }
Install-Package $taskInstaller
if (Compare-Object $taskBefore (Get-LibraryHashes $taskLibrary)) { throw 'La bibliothèque a changé pendant la réinstallation.' }
[pscustomobject]@{ Installer=$taskInstaller; Version=$taskVersion; LibraryPreserved=$true; PreviousVersionTested=[bool]$PreviousInstallerPath; TestRoot=$taskRoot } | ConvertTo-Json
