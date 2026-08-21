.\zspk.ps1 -if @(
    'Cargo.toml','README.md',
    'src\config.rs','src\admin.rs','src\logger.rs','src\testing.rs',
    'src\verify.rs','src\fdb.rs','src\idxer.rs','src\main.rs',
    'src\fs\mod.rs','src\fs\media.rs','src\fs\ntfs.rs','src\fs\generic.rs',
    'run_tests.ps1','install-src.ps1','build-src.ps1','zspk.ps1','uzspk.ps1',
    'zspk-prj.ps1','zspk-prjcc.ps1'
) -comp -crc -o .\gdubv0-0-50fcc.zspk