#Requires -RunAsAdministrator
param(
    [ValidatePattern('^[A-Za-z]$')][string]$DriveLetter,
    [ValidateSet('vhd', 'subst')][string]$Mode = 'vhd',
    [switch]$Force,
    [switch]$KillTested
)

$ErrorActionPreference = 'Stop'
$ProjectRoot  = $PSScriptRoot
$GetdubExe    = Join-Path $ProjectRoot 'target\release\getdub.exe'
$FdbDir       = Join-Path $ProjectRoot 'fdb'
$CsvReport    = Join-Path $ProjectRoot 'test_report.csv'
$LogFile      = Join-Path $ProjectRoot 'run_tests.log'
$VhdPath      = Join-Path $ProjectRoot 'test.vhd'

try {
    Start-Transcript -Path $LogFile -Force | Out-Null; $transcriptStarted = $true
} catch { Write-Warning "Не удалось запустить транскрипт: $_"; $transcriptStarted = $false }

try {
    function Write-Step  { param([string]$Msg) Write-Host "`n=== $Msg ===" -ForegroundColor Cyan }
    function Write-Ok    { param([string]$Msg) Write-Host "[OK] $Msg" -ForegroundColor Green }
    function Write-Fail  { param([string]$Msg) Write-Host "[FAIL] $Msg" -ForegroundColor Red }
    function Write-Info  { param([string]$Msg) Write-Host "[..] $Msg" -ForegroundColor Yellow }
    function Write-Skip  { param([string]$Msg) Write-Host "[SKIP] $Msg" -ForegroundColor DarkGray }

    function Invoke-Getdub {
        param([Parameter(Mandatory)][string[]]$CmdArgs, [int]$ExpectedExit = 0, [switch]$ExpectNonZero)
        Write-Info "getdub $($CmdArgs -join ' ')"
        & $GetdubExe $CmdArgs
        $code = $LASTEXITCODE
        if ($ExpectNonZero) {
            if ($code -eq 0) { throw "ожидался ненулевой код возврата, но getdub вернул 0" }
            Write-Info "exit code: $code (ожидался ненулевой - OK)"
        } elseif ($code -ne $ExpectedExit) {
            throw "getdub вернул код $code, ожидался $ExpectedExit"
        } else { Write-Info "exit code: $code" }
    }

    Write-Host "========================================" -ForegroundColor Magenta
    Write-Host " getdub test runner started" -ForegroundColor Magenta
    Write-Host " Время: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')" -ForegroundColor Magenta
    Write-Host "========================================" -ForegroundColor Magenta

    if ($KillTested) {
        Write-Step "Очистка тестовых данных"
        $cleaned = 0
        foreach ($item in @($FdbDir, $CsvReport, $VhdPath, (Join-Path $ProjectRoot 'testdrive'))) {
            if (Test-Path $item) { Remove-Item -Recurse -Force $item; $cleaned++ }
        }
        Write-Ok "Очистка завершена (удалено $cleaned объектов)"; exit 0
    }

    if ([string]::IsNullOrWhiteSpace($DriveLetter)) {
        Write-Fail "Укажите букву диска: .\run_tests.ps1 -DriveLetter X"; exit 1
    }

    $DriveLetter = $DriveLetter.ToUpper()
    $DriveRoot   = "${DriveLetter}:\"
    $DriveColon  = "${DriveLetter}:"

    if (-not (Test-Path $GetdubExe)) { Write-Fail "Бинарник не найден: $GetdubExe. Выполните: cargo build --release"; exit 1 }

    $DriveExists = Test-Path $DriveRoot
    $TestDriveDir = ""
    $expectUsn = $false
    $cleanupRealDrive = $false

    if ($DriveExists) {
        if (-not $Force) {
            Write-Fail "Диск $DriveRoot уже существует."
            Write-Info "Для тестирования существующего диска добавьте флаг -Force."
            Write-Info "Это защитит ваши данные от случайного форматирования."
            exit 1
        }
        Write-Step "Используется СУЩЕСТВУЮЩИЙ диск $DriveRoot (режим -Force)"
        Write-Info "ВНИМАНИЕ: Тестовые файлы будут созданы в $DriveRoot\getdub_test_data\"
        $TestDriveDir = "$DriveRoot\getdub_test_data"
        $cleanupRealDrive = $true
        
        New-Item -ItemType Directory -Force -Path $TestDriveDir | Out-Null
        New-Item -ItemType Directory -Force -Path "$TestDriveDir\subfolder" | Out-Null
        
        [System.IO.File]::WriteAllText("$TestDriveDir\unique.txt", "Уникальный контент", [System.Text.Encoding]::UTF8)
        $content = "Контент для поиска дубликатов"
        [System.IO.File]::WriteAllText("$TestDriveDir\original.txt", $content, [System.Text.Encoding]::UTF8)
        Copy-Item "$TestDriveDir\original.txt" "$TestDriveDir\copy_of_original.txt"
        Copy-Item "$TestDriveDir\original.txt" "$TestDriveDir\subfolder\another_copy.txt"
        [System.IO.File]::WriteAllText("$TestDriveDir\debug.log", "мусор", [System.Text.Encoding]::UTF8)
        
        $expectUsn = (fsutil usn queryjournal $DriveColon 2>$null) -match "USN Journal"
        if ($expectUsn) { Write-Ok "USN Journal активен на $DriveRoot" }
        else { Write-Skip "USN Journal не активен на $DriveRoot" }
    }
    else {
        if ($Mode -eq 'vhd') {
            Write-Step "Создание VHD (100 MB) и форматирование в NTFS"
            if (Test-Path $VhdPath) { Remove-Item -Force $VhdPath }
            $diskpartAttach = @"
create vdisk file="$VhdPath" maximum=100 type=expandable
select vdisk file="$VhdPath"
attach vdisk
create partition primary
format fs=ntfs quick
assign letter=$DriveLetter
"@
            $diskpartAttach | Out-File -FilePath "$ProjectRoot\diskpart_attach.txt" -Encoding ASCII
            $dpResult = & diskpart /s "$ProjectRoot\diskpart_attach.txt" 2>&1
            if ($LASTEXITCODE -ne 0) { Write-Fail "diskpart не смог создать/примонтировать VHD"; Write-Host $dpResult; exit 1 }
            Write-Ok "VHD создан и примонтирован как $DriveRoot"
            Start-Sleep -Seconds 5
            if (-not (Test-Path $DriveRoot)) { Write-Fail "Диск $DriveRoot не появился"; exit 1 }
            Write-Ok "Диск $DriveRoot доступен"

            Write-Step "Активация USN Journal на $DriveColon"
            $fsutilResult = & fsutil usn createjournal m=64 a=512 $DriveColon 2>&1
            if ($LASTEXITCODE -ne 0) { Write-Fail "fsutil usn createjournal не смог активировать USN Journal"; Write-Host $fsutilResult; exit 1 }
            Write-Ok "USN Journal активирован на $DriveColon"
            $expectUsn = $true

            Write-Step "Создание тестовой структуры на $DriveRoot"
            New-Item -ItemType Directory -Force -Path "$DriveRoot\subfolder" | Out-Null
            [System.IO.File]::WriteAllText("$DriveRoot\unique.txt", "Уникальный контент", [System.Text.Encoding]::UTF8)
            $content = "Контент для поиска дубликатов"
            [System.IO.File]::WriteAllText("$DriveRoot\original.txt", $content, [System.Text.Encoding]::UTF8)
            Copy-Item "$DriveRoot\original.txt" "$DriveRoot\copy_of_original.txt"
            Copy-Item "$DriveRoot\original.txt" "$DriveRoot\subfolder\another_copy.txt"
            [System.IO.File]::WriteAllText("$DriveRoot\debug.log", "мусор", [System.Text.Encoding]::UTF8)
            Write-Ok "Создано 5 файлов на $DriveRoot"
        }
        elseif ($Mode -eq 'subst') {
            Write-Step "Создание тестовой папки на физическом диске"
            $TestDriveDir = Join-Path $ProjectRoot 'testdrive'
            if (Test-Path $TestDriveDir) { Remove-Item -Recurse -Force $TestDriveDir }
            New-Item -ItemType Directory -Force -Path $TestDriveDir | Out-Null
            New-Item -ItemType Directory -Force -Path "$TestDriveDir\subfolder" | Out-Null
            [System.IO.File]::WriteAllText("$TestDriveDir\unique.txt", "Уникальный контент", [System.Text.Encoding]::UTF8)
            $content = "Контент для поиска дубликатов"
            [System.IO.File]::WriteAllText("$TestDriveDir\original.txt", $content, [System.Text.Encoding]::UTF8)
            Copy-Item "$TestDriveDir\original.txt" "$TestDriveDir\copy_of_original.txt"
            Copy-Item "$TestDriveDir\original.txt" "$TestDriveDir\subfolder\another_copy.txt"
            [System.IO.File]::WriteAllText("$TestDriveDir\debug.log", "мусор", [System.Text.Encoding]::UTF8)
            Write-Ok "Создано 5 файлов в $TestDriveDir"

            Write-Step "Подключение subst $DriveRoot -> $TestDriveDir"
            subst "${DriveLetter}:" $TestDriveDir
            if ($LASTEXITCODE -ne 0) { Write-Fail "subst не смог подключить диск"; exit 1 }
            Start-Sleep -Seconds 2
            if (-not (Test-Path $DriveRoot)) { Write-Fail "Диск $DriveRoot не появился"; subst "${DriveLetter}:" /d 2>$null; exit 1 }
            Write-Ok "Диск $DriveRoot подключён через subst"

            $usnCheck = & fsutil usn queryjournal $DriveColon 2>&1
            if ($LASTEXITCODE -eq 0) { Write-Ok "USN Journal активен (унаследован от физического тома)"; $expectUsn = $true }
            else { Write-Skip "USN Journal не активен на физическом томе"; $expectUsn = $false }
        }
    }

    $testsPassed = 0; $testsFailed = 0; $testsSkipped = 0

    function Run-Test {
        param([string]$Name, [scriptblock]$Body, [switch]$Skip)
        if ($Skip) { Write-Skip "ТЕСТ: $Name"; $script:testsSkipped++; return }
        Write-Step "ТЕСТ: $Name"
        try { & $Body; Write-Ok "Тест пройден: $Name"; $script:testsPassed++ }
        catch { Write-Fail "Тест провален: $Name — $_"; $script:testsFailed++ }
    }

    if (Test-Path $FdbDir) { Remove-Item -Recurse -Force $FdbDir }

    try {
        Run-Test "1. Базовое сканирование" { Invoke-Getdub -CmdArgs @('idx', 'drive', $DriveLetter) }
        
        $verifyArgs = @('verify', '--volume', $DriveRoot, '--expect-files-min', '5', '--expect-files-max', '5', '--integrity')
        if ($expectUsn) { $verifyArgs += '--expect-usn' }
        Run-Test "2. verify: том+файлы+USN" { Invoke-Getdub -CmdArgs $verifyArgs } -Skip:(-not $expectUsn -and $cleanupRealDrive)

        Run-Test "3. Сканирование с --hash" { Invoke-Getdub -CmdArgs @('idx', 'drive', $DriveLetter, '--hash') }
        
        Run-Test "4. verify: 100% хешей, 1 группа" {
            Invoke-Getdub -CmdArgs @('verify', '--volume', $DriveRoot, '--expect-files-min', '5', '--expect-files-max', '5', '--expect-hashed-pct', '100', '--expect-dup-groups', '1', '--expect-dup-members-min', '3')
        }

        Run-Test "5. verify-негатив: 99 групп (код 14)" {
            Invoke-Getdub -CmdArgs @('verify', '--volume', $DriveRoot, '--expect-dup-groups', '99') -ExpectedExit 14
        }

        Run-Test "6. verify-негатив: файлов > макс (код 12)" {
            Invoke-Getdub -CmdArgs @('verify', '--volume', $DriveRoot, '--expect-files-max', '2') -ExpectedExit 12
        }

        Run-Test "7. Soft-delete + verify 0 файлов" {
            Invoke-Getdub -CmdArgs @('fdb', 'erase', '--volume', $DriveRoot)
            Invoke-Getdub -CmdArgs @('verify', '--volume', $DriveRoot, '--expect-files-min', '0', '--expect-files-max', '0', '--expect-dup-groups', '0')
        }

        Run-Test "8. Скан с фильтром *.txt + verify" {
            Invoke-Getdub -CmdArgs @('idx', 'drive', $DriveLetter, '--include', '*.txt', '--hash')
            Invoke-Getdub -CmdArgs @('verify', '--volume', $DriveRoot, '--expect-files-min', '4', '--expect-files-max', '4', '--expect-dup-groups', '1', '--expect-dup-members-min', '3')
        }

        Run-Test "9. Инкремент (USN) + verify" {
            [System.IO.File]::WriteAllText("$TestDriveDir\new_file.txt", "новый файл", [System.Text.Encoding]::UTF8)
            Invoke-Getdub -CmdArgs @('idx', 'drive', $DriveLetter, '--incremental')
            $incArgs = @('verify', '--volume', $DriveRoot, '--integrity')
            if ($expectUsn) { $incArgs += '--expect-usn' }
            Invoke-Getdub -CmdArgs $incArgs
        }

        Run-Test "10. Экспорт в CSV" {
            Invoke-Getdub -CmdArgs @('fdb', 'export', '--out', $CsvReport)
            if (-not (Test-Path $CsvReport)) { throw "CSV не создан" }
            if ((Get-Content $CsvReport).Count -lt 2) { throw "CSV пуст" }
        }
    }
    finally {
        if ($cleanupRealDrive) {
            Write-Step "Очистка тестовых данных на реальном диске"
            if (Test-Path $TestDriveDir) { Remove-Item -Recurse -Force $TestDriveDir; Write-Ok "Папка $TestDriveDir удалена" }
        }
        elseif ($Mode -eq 'vhd' -and (Test-Path $VhdPath)) {
            Write-Step "Отмонтирование и удаление VHD"
            $diskpartDetach = "select vdisk file=`"$VhdPath`"`r`ndetach vdisk"
            $diskpartDetach | Out-File -FilePath "$ProjectRoot\diskpart_detach.txt" -Encoding ASCII
            & diskpart /s "$ProjectRoot\diskpart_detach.txt" 2>&1 | Out-Null
            if (Test-Path $VhdPath) { Remove-Item -Force $VhdPath; Write-Ok "VHD удалён: $VhdPath" }
            Remove-Item "$ProjectRoot\diskpart_attach.txt" -ErrorAction SilentlyContinue
            Remove-Item "$ProjectRoot\diskpart_detach.txt" -ErrorAction SilentlyContinue
        }
        elseif ($Mode -eq 'subst') {
            Write-Step "Отключение subst"
            subst "${DriveLetter}:" /d 2>$null
            if (Test-Path $DriveRoot) { Write-Fail "subst всё ещё активен" }
            else { Write-Ok "subst отключён" }
            if (Test-Path $TestDriveDir) { Remove-Item -Recurse -Force $TestDriveDir; Write-Ok "Тестовая папка удалена" }
        }
    }

    Write-Host "`n======================================" -ForegroundColor Magenta
    Write-Host " ИТОГ: пройдено $testsPassed, провалено $testsFailed, пропущено $testsSkipped " -ForegroundColor Magenta
    Write-Host "======================================" -ForegroundColor Magenta

    if ($testsFailed -gt 0) { exit 1 } else { exit 0 }

} finally {
    if ($transcriptStarted) { try { Stop-Transcript | Out-Null } catch {} }
}

