<#
.SYNOPSIS
    Упаковывает один или несколько файлов в .zspk пакет.
.PARAMETER InputFiles
    Массив путей к файлам для упаковки.
.PARAMETER OutputFile
    Путь к выходному .zspk файлу.
.PARAMETER Compress
    Если указан, применяет Deflate-сжатие.
.PARAMETER Crc32
    Если указан, вычисляет и добавляет CRC32.
.EXAMPLE
    .\zspk.ps1 -InputFiles @('src\idxer.rs') -OutputFile .\fix.zspk
.EXAMPLE
    .\zspk.ps1 -InputFiles (Get-ChildItem -Recurse -Include *.rs,*.toml,*.ps1,*.md) -OutputFile .\gdubv0-0-49f.zspk
#>
param(
    [Parameter(Mandatory=$true)]
    [string[]]$InputFiles,
    
    [Parameter(Mandatory=$true)]
    [string]$OutputFile,
    
    [switch]$Compress,
    [switch]$Crc32
)

$ErrorActionPreference = 'Stop'

# Преобразуем OutputFile в абсолютный путь
$OutputFile = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputFile)
Write-Host "Выходной файл: $OutputFile" -ForegroundColor Cyan

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

function Encode-Data {
    param([byte[]]$Bytes, [switch]$DoCompress)
    if ($DoCompress) {
        $ms = New-Object System.IO.MemoryStream
        $ds = New-Object System.IO.Compression.DeflateStream($ms, [System.IO.Compression.CompressionLevel]::Optimal, $true)
        $ds.Write($Bytes, 0, $Bytes.Length)
        $ds.Close()
        $result = $ms.ToArray()
        $ms.Close()
        return $result
    }
    return $Bytes
}

$lines = @()
$lines += "# ZSPK v3.0"
$lines += "# COMPRESS: $($Compress.IsPresent)"
$lines += "# CRC32: $($Crc32.IsPresent)"
$lines += "# FILES: $($InputFiles.Count)"

if ($InputFiles.Count -eq 1) {
    # Одиночный файл
    $file = $InputFiles[0]
    $fileAbs = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($file)
    
    if (-not (Test-Path $fileAbs)) {
        Write-Host "ОШИБКА: Файл не найден: $fileAbs" -ForegroundColor Red
        exit 1
    }
    
    $bytes = [System.IO.File]::ReadAllBytes($fileAbs)
    $fileName = $fileAbs.Replace((Get-Location).Path + "\", "").Replace("\", "/")
    
    Write-Host "Упаковка файла: $fileName ($($bytes.Length) байт)" -ForegroundColor Cyan
    
    $lines += "# FILE: $fileName"
    $lines += "# SIZE: $($bytes.Length)"
    if ($Crc32) {
        $lines += "# HASH: $(Get-CRC32 $bytes)"
    }
    $lines += "BEGIN_BASE64"
    $encoded = Encode-Data $bytes -DoCompress:$Compress
    $lines += [Convert]::ToBase64String($encoded)
    $lines += "END_BASE64"
}
else {
    # Мульти-пакет
    Write-Host "Упаковка $($InputFiles.Count) файлов в мульти-пакет..." -ForegroundColor Cyan
    
    $lines += "BEGIN_MULTI"
    $packedCount = 0
    
    foreach ($file in $InputFiles) {
        $fileAbs = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($file)
        
        if (-not (Test-Path $fileAbs)) {
            Write-Host "  ПРЕДУПРЕЖДЕНИЕ: Файл не найден, пропускается: $fileAbs" -ForegroundColor Yellow
            continue
        }
        
        $bytes = [System.IO.File]::ReadAllBytes($fileAbs)
        $fileName = $fileAbs.Replace((Get-Location).Path + "\", "").Replace("\", "/")
        
        Write-Host "  [OK] $fileName ($($bytes.Length) байт)" -ForegroundColor Green
        
        $lines += "BEGIN_FILE: $fileName"
        $lines += "# SIZE: $($bytes.Length)"
        if ($Crc32) {
            $lines += "# HASH: $(Get-CRC32 $bytes)"
        }
        $lines += "BEGIN_BASE64"
        $encoded = Encode-Data $bytes -DoCompress:$Compress
        $lines += [Convert]::ToBase64String($encoded)
        $lines += "END_BASE64"
        $packedCount++
    }
    $lines += "END_MULTI"
    
    Write-Host "Упаковано файлов: $packedCount" -ForegroundColor Cyan
}

$content = $lines -join "`r`n"

try {
    [System.IO.File]::WriteAllText($OutputFile, $content, [System.Text.Encoding]::UTF8)
    
    if (Test-Path $OutputFile) {
        $fileInfo = Get-Item $OutputFile
        Write-Host "`nУСПЕХ: Файл создан: $OutputFile" -ForegroundColor Green
        Write-Host "  Размер: $($fileInfo.Length) байт" -ForegroundColor Cyan
    } else {
        Write-Host "`nОШИБКА: Файл не был создан по пути: $OutputFile" -ForegroundColor Red
        exit 1
    }
} catch {
    Write-Host "`nОШИБКА при записи файла: $_" -ForegroundColor Red
    Write-Host "Путь: $OutputFile" -ForegroundColor Red
    exit 1
}