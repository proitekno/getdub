<#
.SYNOPSIS
    Автоматизирует версионирование, сборку и упаковку исходников getdub.
.DESCRIPTION
    1. Инкрементирует версию в Cargo.toml (по умолчанию Patch).
    2. Выполняет cargo clean.
    3. Выполняет cargo build --release с логированием в build.log.
    4. Упаковывает исходники:
       - С флагом -Arc: создаёт getdub-vX-Y-Z.zip НЕЗАВИСИМО от результата компиляции.
       - Без флага -Arc: создаёт getdub-vX-Y-Z.srcpak только при успешной сборке.
    5. Логирует весь процесс в build-src.log.
.PARAMETER Build
		Выполняет процесс компиляции исходников
.PARAMETER Bump
    Уровень инкремента версии: Major, Minor или Patch (по умолчанию Patch).
.PARAMETER Arc
    Если указан, создаёт ZIP-архив с версионированным именем, игнорируя ошибки компиляции.
.PARAMETER SrcPak
    Если указан, создаёт SrcPak с версионированным именем.
.EXAMPLE
    .\build-src.ps1 -Build -Bump Patch
.EXAMPLE
    .\build-src.ps1 -Build -Bump Minor -Arc
#>
param(
    [ValidateSet('Major', 'Minor', 'Patch')]
    [string]$Bump = 'Patch',

    [switch]$Arc,
    [switch]$SrcPak,
    [switch]$Build
)

$ErrorActionPreference = 'Stop'
$ProjectRoot = $PSScriptRoot
$CargoToml   = Join-Path $ProjectRoot 'Cargo.toml'
$BuildLog    = Join-Path $ProjectRoot 'build.log'
$ScriptLog   = Join-Path $ProjectRoot 'build-src.log'

Start-Transcript -Path $ScriptLog -Force | Out-Null

try {
    Write-Host "=== Начало процесса ===" -ForegroundColor Cyan

    $filesToPack = @(
        "Cargo.toml",
        "src\config.rs",
        "src\admin.rs",
        "src\logger.rs",
        "src\testing.rs",
        "src\verify.rs",
        "src\fdb.rs",
        "src\idxer.rs",
        "src\main.rs",
        "src\fs\mod.rs",
        "src\fs\media.rs",
        "src\fs\ntfs.rs",
        "src\fs\generic.rs",
        "run_tests.ps1",
        "uzspk.ps1",
        "zspk.ps1",
        "zspk-prj.ps1",
        "zspk-prjcc.ps1",
        "install-src.ps1",
        "build-src.ps1"
    )

    if ($Build) {
		    Write-Host "--- Сборка getdub ---" -ForegroundColor Cyan

        $cargoContent = Get-Content $CargoToml -Raw
        if ($cargoContent -match 'version\s*=\s*"(\d+)\.(\d+)\.(\d+)"') {
            $major = [int]$matches[1]
            $minor = [int]$matches[2]
            $patch = [int]$matches[3]
        
            switch ($Bump) {
                'Major' { $major++; $minor = 0; $patch = 0}
                'Minor' { $minor++; $patch = 0}
                'Patch' { $patch++}
            }
        
            $newVersion = "$major.$minor.$patch"
            $newCargoContent = $cargoContent -replace 'version\s*=\s*"\d+\.\d+\.\d+"', "version = `"$newVersion`""
            Set-Content -Path $CargoToml -Value $newCargoContent -NoNewline -Encoding UTF8
            Write-Host "Версия обновлена: $major.$minor.$patch -> $newVersion" -ForegroundColor Green
        } else {
            throw "Не удалось найти строку версии в Cargo.toml"
        }
        
        Write-Host "Выполнение cargo clean..." -ForegroundColor Yellow
        cargo clean 2>&1 | Out-Null
        
        Write-Host "Выполнение cargo build --release..." -ForegroundColor Yellow
        cargo build --release 2>&1 | Tee-Object -FilePath $BuildLog
        $buildResult = $LASTEXITCODE
        
        if ($buildResult -ne 0) {
            Write-Host "СБОРКА ЗАВЕРШИЛАСЬ С ОШИБКОЙ (код $buildResult). См. build.log" -ForegroundColor Red
        } else {
            Write-Host "Сборка успешна!" -ForegroundColor Green
        }
    }
        
    if ($Arc) {
        $archiveName = "getdub-v$major-$minor-$patch.zip"
        $archivePath = Join-Path $ProjectRoot $archiveName
        $tempDir     = Join-Path $ProjectRoot "temp_pack_dir"

        Write-Host "(-Arc) Подготовка файлов для архивации..." -ForegroundColor Yellow

        if (Test-Path $tempDir) { Remove-Item -Recurse -Force $tempDir }
        New-Item -ItemType Directory -Force -Path $tempDir | Out-Null

        foreach ($file in $filesToPack) {
            $fullPath = Join-Path $ProjectRoot $file
            if (Test-Path $fullPath) {
                $destPath = Join-Path $tempDir $file
                $destDir  = Split-Path $destPath -Parent
                if (-not (Test-Path $destDir)) {
                    New-Item -ItemType Directory -Force -Path $destDir | Out-Null
                }
                Copy-Item -Path $fullPath -Destination $destPath -Force
            } else {
                Write-Warning "Файл не найден, пропускается: $file"
            }
        }

        Write-Host "Создание архива $archiveName..." -ForegroundColor Yellow
        Compress-Archive -Path "$tempDir\*" -DestinationPath $archivePath -Force
        Remove-Item -Recurse -Force $tempDir

        Write-Host "Архив успешно создан: $archivePath" -ForegroundColor Green

        if ($buildResult -ne 0) {
            Write-Host "ПРЕДУПРЕЖДЕНИЕ: Компиляция упала (код $buildResult), но архив с исходниками создан." -ForegroundColor Yellow
        }

        Write-Host "(-Arc) Процесс завершен ===" -ForegroundColor Cyan
    }


    if ($Build) {
        if ($buildResult -ne 0) {
            Write-Host "Прерывание из-за ошибки компиляции. SRCPAK не создан." -ForegroundColor Red
            exit $buildResult
        }
    }

    if ($SrcPak) {
        $pakFileName = "getdub-v$major-$minor-$patch.srcpak"
        $pakFilePath = Join-Path $ProjectRoot $pakFileName
        Write-Host "(-SrcPak) Упаковка исходников в текстовый пакет $pakFileName..." -ForegroundColor Yellow
        
        $pakContent = ""
        foreach ($file in $filesToPack) {
            $fullPath = Join-Path $ProjectRoot $file
            if (Test-Path $fullPath) {
                $content = Get-Content -Path $fullPath -Raw -Encoding UTF8
                $pakContent += "---FILE_START: $file---`r`n"
                $pakContent += $content
                $pakContent += "`r`n---FILE_END: $file---`r`n"
            } else {
                Write-Warning "Файл не найден, пропускается: $file"
            }
        }
        [System.IO.File]::WriteAllText($pakFilePath, $pakContent, [System.Text.Encoding]::UTF8)
        Write-Host "(-SrcPak) Текстовый пакет успешно создан: $pakFilePath" -ForegroundColor Green
    }
        
		Write-Host "=== Процесс успешно завершен ===" -ForegroundColor Cyan

} catch {
    Write-Host "ОШИБКА: $_" -ForegroundColor Red
    exit 1
} finally {
    $tempDir = Join-Path $ProjectRoot "temp_pack_dir"
    if (Test-Path $tempDir) {
        Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
    }
    Stop-Transcript
}
