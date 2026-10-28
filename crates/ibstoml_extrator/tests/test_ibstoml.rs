use ibstoml_extrator::IbstomlMap;
use std::path::PathBuf;

#[test]
fn test_f103c8_example() {
    // 从子 crate 目录回溯到工作空间根目录下的 tests/examples/
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let file_path = manifest_dir.join("../../tests/examples/f103c8.ibs.toml");

    let map = IbstomlMap::from_file(file_path)
        .expect("Failed to load or parse f103c8.ibs.toml");
    
    // 全匹配
    let manufacturer = map.extract_text("Component.0.Manufacturer.manufacturer")
        .expect("Could not extract manufacturer");
    assert_eq!(manufacturer, "STMicroelectronics NV");

    // 匹配方式2
    let manufacturer = map.extract_text("Component.0.Manufacturer")
        .expect("Could not extract manufacturer");
    assert_eq!(manufacturer, "STMicroelectronics NV");
}
