use anyhow::Result;
use std::path::Path;

pub fn create_test_files(root: &Path) -> Result<()> {
    std::fs::create_dir_all(root.join("subfolder"))?;
    std::fs::write(root.join("unique.txt"), "Уникальный контент")?;
    
    let content = "Контент для поиска дубликатов";
    std::fs::write(root.join("original.txt"), content)?;
    std::fs::write(root.join("copy_of_original.txt"), content)?;
    std::fs::write(root.join("subfolder").join("another_copy.txt"), content)?;
    std::fs::write(root.join("debug.log"), "мусор для фильтра")?;

    Ok(())
}

pub fn verify_test_files(root: &Path) -> Result<bool> {
    let expected = vec![
        root.join("unique.txt"),
        root.join("original.txt"),
        root.join("copy_of_original.txt"),
        root.join("subfolder").join("another_copy.txt"),
        root.join("debug.log"),
    ];
    for path in expected {
        if !path.exists() { return Ok(false); }
    }
    Ok(true)
}

