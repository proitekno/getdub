<#
.SYNOPSIS
    Упаковывает один или несколько файлов в .zspk пакет.
.PARAMETER if
    Массив путей к файлам для упаковки.
.PARAMETER o
    Путь к выходному .zspk файлу.
.PARAMETER comp
    Если указан, применяет Deflate-сжатие.
.PARAMETER crc
    Если указан, вычисляет и добавляет CRC32.
#>
param(
    [Parameter(Mandatory=$true)]
    [string[]]$if,
    
    [Parameter(Mandatory=$true)]
    [string]$o,
    
    [switch]$comp,
    [switch]$crc
)

$ErrorActionPreference = 'Stop'

$o = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($o)
Write-Host "Выходной файл: $o" -ForegroundColor Cyan

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
$lines += "# COMPRESS: $($comp.IsPresent)"
$lines += "# CRC32: $($crc.IsPresent)"
$lines += "# FILES: $($if.Count)"

if ($if.Count -eq 1) {
    $file = $if[0]
    $fileAbs = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($file)
    
    if (-not (Test-Path $fileAbs)) {
        Write-Host "ОШИБКА: Файл не найден: $fileAbs" -ForegroundColor Red
        exit 1
    }
    
    $bytes = [System.IO.File]::ReadAllBytes($fileAbs)
    $fileName = (Get-Item $fileAbs).Name
    
    Write-Host "Упаковка файла: $fileName ($($bytes.Length) байт)" -ForegroundColor Cyan
    
    $lines += "# FILE: $fileName"
    $lines += "# SIZE: $($bytes.Length)"
    if ($crc) {
        $lines += "# HASH: $(Get-CRC32 $bytes)"
    }
    $lines += "BEGIN_BASE64"
    $encoded = Encode-Data $bytes -DoCompress:$comp
    $lines += [Convert]::ToBase64String($encoded)
    $lines += "END_BASE64"
}
else {
    Write-Host "Упаковка $($if.Count) файлов в мульти-пакет..." -ForegroundColor Cyan
    
    $lines += "BEGIN_MULTI"
    $packedCount = 0
    
    foreach ($file in $if) {
        $fileAbs = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($file)
        
        if (-not (Test-Path $fileAbs)) {
            Write-Host "  ПРЕДУПРЕЖДЕНИЕ: Файл не найден, пропускается: $fileAbs" -ForegroundColor Yellow
            continue
        }
        
        $bytes = [System.IO.File]::ReadAllBytes($fileAbs)
        $fileName = (Get-Item $fileAbs).Name
        
        Write-Host "  [OK] $fileName ($($bytes.Length) байт)" -ForegroundColor Green
        
        $lines += "BEGIN_FILE: $fileName"
        $lines += "# SIZE: $($bytes.Length)"
        if ($crc) {
            $lines += "# HASH: $(Get-CRC32 $bytes)"
        }
        $lines += "BEGIN_BASE64"
        $encoded = Encode-Data $bytes -DoCompress:$comp
        $lines += [Convert]::ToBase64String($encoded)
        $lines += "END_BASE64"
        $packedCount++
    }
    $lines += "END_MULTI"
    
    Write-Host "Упаковано файлов: $packedCount" -ForegroundColor Cyan
}

$content = $lines -join "`r`n"

try {
    [System.IO.File]::WriteAllText($o, $content, [System.Text.Encoding]::UTF8)
    
    if (Test-Path $o) {
        $fileInfo = Get-Item $o
        Write-Host "`nУСПЕХ: Файл создан: $o" -ForegroundColor Green
        Write-Host "  Размер: $($fileInfo.Length) байт" -ForegroundColor Cyan
    } else {
        Write-Host "`nОШИБКА: Файл не был создан по пути: $o" -ForegroundColor Red
        exit 1
    }
} catch {
    Write-Host "`nОШИБКА при записи файла: $_" -ForegroundColor Red
    Write-Host "Путь: $o" -ForegroundColor Red
    exit 1
}