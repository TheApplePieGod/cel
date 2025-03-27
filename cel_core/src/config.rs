use std::path::PathBuf;

pub fn get_config_dir() -> PathBuf {
    // TODO: this should probably be done once at startup
    let mut dir = dirs::data_local_dir().expect("Unable to locate config dir");
    dir.push("cel");
    let _ = std::fs::create_dir_all(&dir);
    dir
}
