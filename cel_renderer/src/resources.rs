use std::path::{Path, PathBuf};

pub fn get_resource_path(filename: &str) -> PathBuf {
    let filename = Path::new(filename);

    // Check for macOS .app bundle Resources dir: ../Resources/
    if let Ok(exe_path) = std::env::current_exe() {
        #[cfg(target_os = "macos")]
        {
            if let Some(resources_dir) = exe_path.parent().map(|p| p.join("../Resources/resources")) {
                let candidate = resources_dir.join(filename);
                if candidate.exists() {
                    return candidate;
                }
            }
        }

        // For all OSes: check ./resources/ next to the executable
        if let Some(resources_dir) = exe_path.parent().map(|p| p.join("resources")) {
            let candidate = resources_dir.join(filename);
            if candidate.exists() {
                return candidate;
            }
        }
    }

    // Fallback to dev path: relative to CWD
    Path::new("resources").join(filename)
}
