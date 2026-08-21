<#
.SYNOPSIS
    Распаковывает .zspk пакет (одиночный или мульти-файл).
.PARAMETER if
    Путь к .zspk файлу.
.PARAMETER o
    Базовый путь для распаковки (по умолчанию текущая директория).
.EXAMPLE
    .\uzspk.ps1 -if .\gdubv0-0-50f.zspk
#>
param(
    [Parameter(Mandatory=$true)]
    [string]$if,
    
    [string]$o = (Get-Location).Path
)

$ErrorActionPreference = 'Stop'

$if = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($if)
$o = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($o)

if (-not (Test-Path $if)) {
    Write-Host "ОШИБКА: Файл не найден: $if" -ForegroundColor Red
    exit 1
}

Write-Host "Входной файл: $if" -ForegroundColor Cyan
Write-Host "Базовый путь: $o" -ForegroundColor Cyan

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

$lines = Get-Content -Path $if -Encoding UTF8

$doCompress = $false
$doCrc32 = $false
foreach ($line in $lines) {
    if ($line -match '^#\s*COMPRESS:\s*(True|False)') { $doCompress = ($matches[1] -eq 'True') }
    if ($line -match '^#\s*CRC32:\s*(True|False)') { $doCrc32 = ($matches[1] -eq 'True') }
}

Write-Host "Параметры пакета: Compress=$doCompress, Crc32=$doCrc32" -ForegroundColor Cyan

function Write-FileFromBase64 {
    param(
        [string]$FileName,
        [string[]]$Base64Lines,
        [string]$ExpectedHash,
        [bool]$IsCompressed
    )
    
    try {
        $base64String = $Base64Lines -join ""
        $compressedBytes = [Convert]::FromBase64String($base64String)
        $bytes = Decode-Data $compressedBytes -DoCompress:$IsCompressed
        
        if ($doCrc32 -and $ExpectedHash) {
            $actualHash = Get-CRC32 $bytes
            if ($actualHash -ne $ExpectedHash) {
                Write-Host "  ОШИБКА CRC32 для $FileName : ожидалось $ExpectedHash, получено $actualHash" -ForegroundColor Red
                return $false
            }
            Write-Host "  CRC32 OK: $actualHash" -ForegroundColor DarkGray
        }
        
        $fullPath = Join-Path $o $FileName
        $outDir = Split-Path $fullPath -Parent
        if ($outDir -and -not (Test-Path $outDir)) {
            New-Item -ItemType Directory -Force -Path $outDir | Out-Null
        }
        
        [System.IO.File]::WriteAllBytes($fullPath, $bytes)
        Write-Host "  [OK] $FileName ($($bytes.Length) байт)" -ForegroundColor Green
        return $true
    } catch {
        Write-Host "  ОШИБКА для $FileName : $_" -ForegroundColor Red
        return $false
    }
}

$extracted = 0
$state = "HEADER"
$currentFileName = ""
$currentHash = ""
$base64Buffer = @()

foreach ($line in $lines) {
    $line = $line.Trim()
    
    switch ($state) {
        "HEADER" {
            if ($line -eq "BEGIN_MULTI") {
                $state = "MULTI"
            }
            elseif ($line -eq "BEGIN_BASE64") {
                $state = "SINGLE_BASE64"
                $base64Buffer = @()
            }
            elseif ($line -match '^#\s*FILE:\s*(.+)$') {
                $currentFileName = $matches[1].Trim()
            }
            elseif ($line -match '^#\s*HASH:\s*([A-Fa-f0-9]+)') {
                $currentHash = $matches[1]
            }
        }
        "MULTI" {
            if ($line -match '^BEGIN_FILE:\s*(.+)$') {
                $currentFileName = $matches[1].Trim()
                $currentHash = ""
                $state = "FILE_META"
            }
            elseif ($line -eq "END_MULTI") {
                $state = "DONE"
            }
        }
        "FILE_META" {
            if ($line -match '^#\s*HASH:\s*([A-Fa-f0-9]+)') {
                $currentHash = $matches[1]
            }
            elseif ($line -eq "BEGIN_BASE64") {
                $base64Buffer = @()
                $state = "FILE_BASE64"
            }
        }
        "FILE_BASE64" {
            if ($line -eq "END_BASE64") {
                if (Write-FileFromBase64 $currentFileName $base64Buffer $currentHash $doCompress) {
                    $extracted++
                }
                $state = "MULTI"
            } else {
                $base64Buffer += $line
            }
        }
        "SINGLE_BASE64" {
            if ($line -eq "END_BASE64") {
                if (Write-FileFromBase64 $currentFileName $base64Buffer $currentHash $doCompress) {
                    $extracted++
                }
                $state = "DONE"
            } else {
                $base64Buffer += $line
            }
        }
    }
}

Write-Host "`nИзвлечено файлов: $extracted" -ForegroundColor Green