<#
.SYNOPSIS
    Упаковывает один или несколько файлов в .zspk пакет.
.DESCRIPTION
    Поддерживает одиночные файлы и мульти-пакеты.
    Сжатие и CRC32 опциональны (по умолчанию выключены для максимальной совместимости).
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
    .\zspk.ps1 -InputFiles @('Cargo.toml','src\config.rs','src\main.rs') -OutputFile .\gdubv0-0-49f.zspk
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
    if (-not (Test-Path $file)) {
        Write-Host "Ошибка: Файл не найден: $file" -ForegroundColor Red
        exit 1
    }
    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path $file).Path)
    $fileName = (Resolve-Path $file).Path.Replace((Get-Location).Path + "\", "").Replace("\", "/")
    
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
    $lines += "BEGIN_MULTI"
    foreach ($file in $InputFiles) {
        if (-not (Test-Path $file)) {
            Write-Host "Предупреждение: Файл не найден, пропускается: $file" -ForegroundColor Yellow
            continue
        }
        $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path $file).Path)
        $fileName = (Resolve-Path $file).Path.Replace((Get-Location).Path + "\", "").Replace("\", "/")
        
        $lines += "BEGIN_FILE: $fileName"
        $lines += "# SIZE: $($bytes.Length)"
        if ($Crc32) {
            $lines += "# HASH: $(Get-CRC32 $bytes)"
        }
        $lines += "BEGIN_BASE64"
        $encoded = Encode-Data $bytes -DoCompress:$Compress
        $lines += [Convert]::ToBase64String($encoded)
        $lines += "END_BASE64"
    }
    $lines += "END_MULTI"
}

$content = $lines -join "`r`n"
[System.IO.File]::WriteAllText($OutputFile, $content, [System.Text.Encoding]::UTF8)

Write-Host "Упаковано файлов: $($InputFiles.Count) -> $OutputFile" -ForegroundColor Green