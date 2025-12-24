//! Templates module for VenomMemory project generation

pub mod c;
pub mod cpp;
pub mod rust;
pub mod flutter;
pub mod python;
pub mod go;
pub mod zig;
pub mod nim;

/// Project configuration passed to all template generators
pub struct ProjectConfig {
    pub name: String,
    pub channel: String,
    pub data_size: usize,
    pub cmd_slots: usize,
    pub max_clients: usize,
    pub output_dir: String,
}

/// Language enum for template selection
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Language {
    C,
    Cpp,
    Rust,
    Python,
    Go,
    Zig,
    Nim,
    Flutter,
}

/// Generate project based on language
pub fn generate(config: &ProjectConfig, lang: Language) {
    match lang {
        Language::C => c::generate(config),
        Language::Cpp => cpp::generate(config),
        Language::Rust => rust::generate(config),
        Language::Python => python::generate(config),
        Language::Go => go::generate(config),
        Language::Zig => zig::generate(config),
        Language::Nim => nim::generate(config),
        Language::Flutter => flutter::generate(config),
    }
}
