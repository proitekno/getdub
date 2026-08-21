<#
.SYNOPSIS
    Распаковывает .zspk пакет (одиночный или мульти-файл).
.DESCRIPTION
    Автоматически определяет формат пакета.
    Поддерживает опциональное сжатие и CRC32.
.PARAMETER InputFile
    Путь к .zspk файлу.
.PARAMETER BasePath
    Базовый путь для распаковки (по умолчанию текущая директория).
.EXAMPLE
    .\uzspk.ps1 -InputFile .\gdubv0-0-49f.zspk
.EXAMPLE
    .\uzspk.ps1 -InputFile .\fix.zspk -BasePath E:\src\win\rs\getdub
#>
param(
    [Parameter(Mandatory=$true)]
    [string]$InputFile,
    
    [string]$BasePath = $PSScriptRoot
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path $InputFile)) {
    Write-Host "Ошибка: Файл не найден: $InputFile" -ForegroundColor Red
    exit 1
}

function Get-CRC32 {
    param([byte[]]$Bytes)
    $crc = 0xFFFFFFFF
    foreach ($byte in $Bytes) {
        $crc = $crc -bxor $byte
        for ($i = 0; $i -lt 8; $i++) {
            if ($crc -band 1) { $crc = ($crc -shr 1) -bxor 0xEDB88320 }
            else { $crc = $crc -shr 1 }
        }
    }
    return "{0:X8}" -f ($crc -bxor 0xFFFFFFFF)
}

function Decode-Data {
    param([byte[]]$Bytes, [switch]$DoCompress)
    if ($DoCompress) {
        $ms = New-Object System.IO.MemoryStream(,$Bytes)
        $ds = New-Object System.IO.Compression.DeflateStream($ms, [System.IO.Compression.CompressionMode]::Decompress)
        $out = New-Object System.IO.MemoryStream
        $ds.CopyTo($out)
        $ds.Close()
        $ms.Close()
        $result = $out.ToArray()
        $out.Close()
        return $result
    }
    return $Bytes
}

$content = Get-Content -Path $InputFile -Raw -Encoding UTF8

# Извлекаем метаданные
$doCompress = $false
$doCrc32 = $false
if ($content -match '(?m)^#\s*COMPRESS:\s*(True|False)\s*$') { $doCompress = ($matches[1] -eq 'True') }
if ($content -match '(?m)^#\s*CRC32:\s*(True|False)\s*$') { $doCrc32 = ($matches[1] -eq 'True') }

Write-Host "Параметры пакета: Compress=$doCompress, Crc32=$doCrc32" -ForegroundColor Cyan

function Write-FileFromBase64 {
    param(
        [string]$FileName,
        [string]$Base64Data,
        [string]$ExpectedHash,
        [bool]$IsCompressed
    )
    
    $compressedBytes = [Convert]::FromBase64String($Base64Data.Trim())
    $bytes = Decode-Data $compressedBytes -DoCompress:$IsCompressed
    
    # Проверка CRC32
    if ($doCrc32 -and $ExpectedHash) {
        $actualHash = Get-CRC32 $bytes
        if ($actualHash -ne $ExpectedHash) {
            Write-Host "  ОШИБКА CRC32 для $FileName: ожидалось $ExpectedHash, получено $actualHash" -ForegroundColor Red
            return $false
        }
        Write-Host "  CRC32 OK: $actualHash" -ForegroundColor DarkGray
    }
    
    $fullPath = Join-Path $BasePath $FileName
    $outDir = Split-Path $fullPath -Parent
    if ($outDir -and -not (Test-Path $outDir)) {
        New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    }
    
    [System.IO.File]::WriteAllBytes($fullPath, $bytes)
    Write-Host "  [OK] $FileName ($($bytes.Length) байт)" -ForegroundColor Green
    return $true
}

$extracted = 0

if ($content -match 'BEGIN_MULTI') {
    # Мульти-пакет
    Write-Host "Распаковка мульти-пакета..." -ForegroundColor Cyan
    
    $fileBlocks = [regex]::Matches($content, '(?s)BEGIN_FILE:\s*(.+?)\r?\n((?:#\s*.+\r?\n)*)BEGIN_BASE64\r?\n(.+?)\r?\nEND_BASE64')
    
    foreach ($match in $fileBlocks) {
        $fileName = $match.Groups[1].Value.Trim()
        $metadata = $match.Groups[2].Value
        $base64Data = $match.Groups[3].Value
        
        $expectedHash = ""
        if ($metadata -match '#\s*HASH:\s*([A-Fa-f0-9]+)') {
            $expectedHash = $matches[1]
        }
        
        if (Write-FileFromBase64 $fileName $base64Data $expectedHash $doCompress) {
            $extracted++
        }
    }
}
else {
    # Одиночный файл
    $fileName = ""
    if ($content -match '(?m)^#\s*FILE:\s*(.+?)\s*$') {
        $fileName = $matches[1].Trim()
    }
    
    $expectedHash = ""
    if ($content -match '(?m)^#\s*HASH:\s*([A-Fa-f0-9]+)\s*$') {
        $expectedHash = $matches[1]
    }
    
    if ($content -match '(?s)BEGIN_BASE64\r?\n(.+?)\r?\nEND_BASE64') {
        $base64Data = $matches[1]
        if (Write-FileFromBase64 $fileName $base64Data $expectedHash $doCompress) {
            $extracted++
        }
    }
}

Write-Host "`nИзвлечено файлов: $extracted" -ForegroundColor Green