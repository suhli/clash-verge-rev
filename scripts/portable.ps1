[CmdletBinding()]
param(
    [ValidateSet('x86_64-pc-windows-msvc', 'i686-pc-windows-msvc')]
    [string]$Target = 'x86_64-pc-windows-msvc',
    [switch]$CoreOnly
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows) { throw 'The Portable build requires Windows.' }

$repository = Split-Path -Parent $PSScriptRoot
$source = Join-Path $repository 'third-party/ProxyBridge'
if (-not (Test-Path -LiteralPath (Join-Path $source 'Windows/CMakeLists.txt'))) {
    throw 'Initialize third-party/ProxyBridge at the recorded commit before building (git submodule update --init --recursive).'
}

$work = Join-Path $repository 'target/portable'
$cache = Join-Path $work 'downloads'
$coreBuild = Join-Path $work "proxybridge-msvc-$Target"
$portable = Join-Path $work "Clash-Verge-$Target"
$sdk = Join-Path $cache 'WinDivert-2.2.2-A'
$archive = "$sdk.zip"
$expectedHash = '63CB41763BB4B20F600B6DE04E991A9C2BE73279E317D4D82F237B150C5F3F15'
$x64 = $Target.StartsWith('x86_64')
$architecture = if ($x64) { 'x64' } else { 'x86' }
$platform = if ($x64) { 'x64' } else { 'Win32' }
$divertRuntime = Join-Path $sdk $architecture

New-Item -ItemType Directory -Force -Path $cache | Out-Null
if (-not (Test-Path -LiteralPath $archive) -or (Get-FileHash -LiteralPath $archive).Hash -ne $expectedHash) {
    curl.exe --fail --location --retry 2 --max-time 240 --output $archive `
        'https://github.com/basil00/WinDivert/releases/download/v2.2.2/WinDivert-2.2.2-A.zip'
}
if ((Get-FileHash -LiteralPath $archive).Hash -ne $expectedHash) {
    throw 'The WinDivert archive does not match the pinned SHA-256.'
}
if (-not (Test-Path -LiteralPath "$sdk/include/windivert.h") -or
    -not (Test-Path -LiteralPath "$divertRuntime/WinDivert.lib")) {
    Expand-Archive -LiteralPath $archive -DestinationPath $cache -Force
}

cmake -S (Join-Path $source 'Windows') -B $coreBuild -G 'Visual Studio 17 2022' -A $platform "-DWINDIVERT_ROOT=$sdk"
cmake --build $coreBuild --config Release --target ProxyBridgeCore --parallel
if ($CoreOnly) {
    Write-Output (Join-Path $coreBuild 'Release/ProxyBridgeCore.dll')
    return
}

Push-Location -LiteralPath $repository
$previousCargoTarget = $env:CARGO_TARGET_DIR
$previousProxyBridgeUi = $env:VITE_PROXYBRIDGE_BUNDLED
try {
    $env:CARGO_TARGET_DIR = Join-Path $work 'cargo'
    $env:VITE_PROXYBRIDGE_BUNDLED = '1'
    pnpm run prebuild $Target --portable
    node node_modules/@tauri-apps/cli/tauri.js build --no-bundle --target $Target --features proxybridge-bundled '--' '--locked'

    $resources = Join-Path $portable 'resources'
    $proxyBridgeResources = Join-Path $resources 'proxybridge'
    $licenses = Join-Path $portable 'licenses'
    New-Item -ItemType Directory -Force -Path $resources, $proxyBridgeResources, $licenses | Out-Null
    Copy-Item -LiteralPath "$env:CARGO_TARGET_DIR/$Target/release/clash-verge.exe" -Destination $portable -Force
    foreach ($name in @('verge-mihomo', 'verge-mihomo-alpha')) {
        Copy-Item -LiteralPath "src-tauri/sidecar/$name-$Target.exe" -Destination "$portable/$name.exe" -Force
    }
    Get-ChildItem -LiteralPath 'src-tauri/resources' -File | Copy-Item -Destination $resources -Force
    Copy-Item -LiteralPath "$coreBuild/Release/ProxyBridgeCore.dll" -Destination $proxyBridgeResources -Force
    Copy-Item -LiteralPath "$divertRuntime/WinDivert.dll" -Destination $proxyBridgeResources -Force
    Get-ChildItem -LiteralPath $divertRuntime -Filter 'WinDivert*.sys' -File | Copy-Item -Destination $proxyBridgeResources -Force
    Copy-Item -LiteralPath 'LICENSE' -Destination "$licenses/Clash-Verge-Rev.txt" -Force
    Copy-Item -LiteralPath "$source/LICENSE" -Destination "$licenses/ProxyBridge.txt" -Force
    Copy-Item -LiteralPath "$sdk/LICENSE" -Destination "$licenses/WinDivert.txt" -Force

    Write-Output "Portable executable: $portable\clash-verge.exe"
} finally {
    $env:CARGO_TARGET_DIR = $previousCargoTarget
    $env:VITE_PROXYBRIDGE_BUNDLED = $previousProxyBridgeUi
    Pop-Location
}
