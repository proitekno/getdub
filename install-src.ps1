param(
    [Parameter(Mandatory=$true)]
    [string]$PakFile
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path $PakFile)) {
    Write-Host "Ошибка: Файл пакета не найден: $PakFile" -ForegroundColor Red
    exit 1
}

Write-Host "Начинаю распаковку $PakFile..." -ForegroundColor Cyan

$content = Get-Content -Path $PakFile -Raw -Encoding UTF8
$blocks = $content -split '(?m)^---FILE_START:\s*(.+?)\s*---\r?\n'

$filesProcessed = 0
$blocksCount = ($blocks.Count - 1) / 2

for ($i = 1; $i -lt $blocks.Count; $i += 2) {
    $filePath = $blocks[$i].Trim()
    $fileContent = $blocks[$i+1]
    $fileContent = $fileContent -replace '(?m)^---FILE_END:\s*.+?\s*---\r?\n?$', ''

    $fullPath = Join-Path $PSScriptRoot $filePath
    $dir = Split-Path $fullPath -Parent

    if (-not (Test-Path $dir)) {
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
    }

    [System.IO.File]::WriteAllText($fullPath, $fileContent, [System.Text.Encoding]::UTF8)
    Write-Host "  [OK] $filePath" -ForegroundColor Green
    $filesProcessed++
}

Write-Host "`nУспешно распаковано файлов: $filesProcessed из $blocksCount" -ForegroundColor Cyan
Write-Host "Готово к сборке: cargo build --release" -ForegroundColor Yellow

