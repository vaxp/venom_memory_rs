//! Library embedding and distribution
//! Bundles libvenom_memory.so with the CLI

/// Embedded library binary
pub const LIBRARY_BINARY: &[u8] = include_bytes!("../resources/libvenom_memory.so");

/// Library filename
pub const LIBRARY_NAME: &str = "libvenom_memory.so";

/// Write the embedded library to the specified directory
pub fn copy_library_to(dir: &str) {
    let lib_dir = format!("{}/lib", dir);
    crate::create_dir(&lib_dir);
    
    let lib_path = format!("{}/{}", lib_dir, LIBRARY_NAME);
    std::fs::write(&lib_path, LIBRARY_BINARY)
        .expect(&format!("Failed to write library to: {}", lib_path));
    
    // Make it executable (chmod +x)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&lib_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&lib_path, perms).ok();
    }
    
    println!("   {} {}", console::style("✓").green(), lib_path);
}
