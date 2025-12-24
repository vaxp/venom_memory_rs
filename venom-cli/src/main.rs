//! Venom CLI - Interactive Code Generator for VenomMemory IPC Projects
//!
//! Clean modular structure:
//!   - main.rs: Interactive UI only
//!   - templates/: Code generation templates

mod templates;
mod library;

use clap::{Parser, Subcommand, ValueEnum};
use console::style;
use inquire::{Select, Text, Confirm};
use std::fs;
use std::path::Path;
use templates::{ProjectConfig, Language};

#[derive(Parser)]
#[command(name = "venom")]
#[command(about = "🐍 VenomMemory Code Generator", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new VenomMemory project
    Init {
        /// Project name
        name: String,
        
        /// Programming language
        #[arg(short, long, value_enum, default_value = "c")]
        lang: LangArg,
        
        /// Shared memory channel name
        #[arg(short, long)]
        channel: String,
        
        /// Data buffer size in KB
        #[arg(short, long, default_value = "16")]
        data_size: usize,
        
        /// Number of command slots
        #[arg(long, default_value = "32")]
        cmd_slots: usize,
        
        /// Maximum number of clients
        #[arg(long, default_value = "16")]
        max_clients: usize,
        
        /// Output directory
        #[arg(short, long)]
        output: Option<String>,
    },
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, ValueEnum)]
enum LangArg {
    C,
    Cpp,
    Rust,
    Python,
    Go,
    Zig,
    Nim,
    Flutter,
}

impl From<LangArg> for Language {
    fn from(l: LangArg) -> Self {
        match l {
            LangArg::C => Language::C,
            LangArg::Cpp => Language::Cpp,
            LangArg::Rust => Language::Rust,
            LangArg::Python => Language::Python,
            LangArg::Go => Language::Go,
            LangArg::Zig => Language::Zig,
            LangArg::Nim => Language::Nim,
            LangArg::Flutter => Language::Flutter,
        }
    }
}

fn main() {
    let cli = Cli::parse();
    
    match cli.command {
        Some(Commands::Init { name, lang, channel, data_size, cmd_slots, max_clients, output }) => {
            let config = ProjectConfig {
                name: name.clone(),
                channel,
                data_size: data_size * 1024,
                cmd_slots,
                max_clients,
                output_dir: output.unwrap_or(name),
            };
            generate_project(&config, lang.into());
        }
        None => {
            if let Some((config, lang)) = run_interactive_mode() {
                generate_project(&config, lang);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Interactive Mode
// ═══════════════════════════════════════════════════════════════════════════

fn run_interactive_mode() -> Option<(ProjectConfig, Language)> {
    print_header();
    
    // Project name
    let name = Text::new("📁 Project name:")
        .with_placeholder("my_daemon")
        .with_help_message("Name of your project directory")
        .prompt().ok()?;
    
    if name.is_empty() {
        println!("{}", style("❌ Project name cannot be empty").red());
        return None;
    }
    
    // Channel name
    let channel = Text::new("📡 Channel name:")
        .with_default(&name)
        .with_help_message("Shared memory channel identifier")
        .prompt().ok()?;
    
    // Language
    let lang_options = vec!["C", "C++", "Rust", "Python", "Go", "Zig", "Nim", "Flutter/Dart"];
    let lang_choice = Select::new("🔤 Language:", lang_options)
        .with_help_message("↑↓ to move, Enter to select")
        .prompt().ok()?;
    
    let lang = match lang_choice {
        "C++" => Language::Cpp,
        "Rust" => Language::Rust,
        "Python" => Language::Python,
        "Go" => Language::Go,
        "Zig" => Language::Zig,
        "Nim" => Language::Nim,
        "Flutter/Dart" => Language::Flutter,
        _ => Language::C,
    };
    
    // Data size
    let size_options = vec![
        "1 KB   - Small (configs)",
        "16 KB  - Medium (sensors)",
        "64 KB  - Large (images)",
        "256 KB - Very large (video)",
        "Custom...",
    ];
    let size_choice = Select::new("📊 Data buffer size:", size_options)
        .prompt().ok()?;
    
    let data_size = if size_choice.starts_with("Custom") {
        Text::new("   Size in KB:").with_default("16").prompt().ok()?
            .parse::<usize>().unwrap_or(16) * 1024
    } else if size_choice.starts_with("1 KB") { 1024 }
    else if size_choice.starts_with("16 KB") { 16 * 1024 }
    else if size_choice.starts_with("64 KB") { 64 * 1024 }
    else if size_choice.starts_with("256 KB") { 256 * 1024 }
    else { 16 * 1024 };
    
    // Command slots
    let cmd_slots = Select::new("📨 Command slots:", vec!["16", "32", "64", "128"])
        .with_starting_cursor(1).prompt().ok()?
        .parse::<usize>().unwrap_or(32);
    
    // Max clients
    let max_clients = Select::new("👥 Max clients:", vec!["4", "8", "16", "32"])
        .with_starting_cursor(2).prompt().ok()?
        .parse::<usize>().unwrap_or(16);
    
    // Output directory
    let output_dir = Text::new("📂 Output directory:")
        .with_default(&format!("./{}", name))
        .prompt().ok()?;
    
    // Summary
    println!();
    println!("{}", style("═══════════════════════════════════════════").cyan());
    println!("{}", style("📋 Configuration Summary").cyan().bold());
    println!("{}", style("═══════════════════════════════════════════").cyan());
    println!("   Project:     {}", style(&name).green());
    println!("   Channel:     {}", style(&channel).green());
    println!("   Language:    {}", style(format!("{:?}", lang)).green());
    println!("   Data size:   {}", style(format_size(data_size)).green());
    println!("   Cmd slots:   {}", style(cmd_slots).green());
    println!("   Max clients: {}", style(max_clients).green());
    println!("   Output:      {}", style(&output_dir).green());
    println!("{}", style("═══════════════════════════════════════════").cyan());
    println!();
    
    if !Confirm::new("✅ Generate project?").with_default(true).prompt().ok()? {
        println!("{}", style("❌ Cancelled").red());
        return None;
    }
    
    Some((ProjectConfig { name, channel, data_size, cmd_slots, max_clients, output_dir }, lang))
}

fn print_header() {
    println!();
    println!("{}", style("🐍 ═══════════════════════════════════════════════════════").magenta().bold());
    println!("{}", style("🐍   VenomMemory Project Generator v0.3.0").magenta().bold());
    println!("{}", style("🐍 ═══════════════════════════════════════════════════════").magenta().bold());
    println!();
}

fn format_size(bytes: usize) -> String {
    if bytes >= 1024 * 1024 { format!("{} MB", bytes / (1024 * 1024)) }
    else if bytes >= 1024 { format!("{} KB", bytes / 1024) }
    else { format!("{} bytes", bytes) }
}

// ═══════════════════════════════════════════════════════════════════════════
// Project Generation
// ═══════════════════════════════════════════════════════════════════════════

fn generate_project(config: &ProjectConfig, lang: Language) {
    println!();
    println!("{}", style("📁 Creating project structure...").cyan());
    
    templates::generate(config, lang);
    
    // Copy library to project
    library::copy_library_to(&config.output_dir);
    
    println!();
    println!("{}", style("✅ Project generated successfully!").green().bold());
    println!();
    println!("{}", style("📖 Next steps:").yellow());
    
    match lang {
        Language::C => {
            println!("   cd {}/daemon && make run", config.output_dir);
            println!("   cd {}/client && make run", config.output_dir);
        }
        Language::Cpp => {
            println!("   cd {}/daemon && make run", config.output_dir);
            println!("   cd {}/client && make run", config.output_dir);
        }
        Language::Rust => {
            println!("   cd {} && cargo run --bin daemon", config.output_dir);
            println!("   cd {} && cargo run --bin client", config.output_dir);
        }
        Language::Python => {
            println!("   cd {}/daemon && make run", config.output_dir);
            println!("   python3 {}/client.py", config.output_dir);
        }
        Language::Go => {
            println!("   cd {} && make run-daemon", config.output_dir);
            println!("   cd {} && make run-client", config.output_dir);
        }
        Language::Zig => {
            println!("   cd {} && zig build run-daemon", config.output_dir);
            println!("   cd {} && zig build run-client", config.output_dir);
        }
        Language::Nim => {
            println!("   cd {} && make run-daemon", config.output_dir);
            println!("   cd {} && make run-client", config.output_dir);
        }
        Language::Flutter => {
            let snake = config.name.replace("-", "_");
            println!("   cd {}/daemon && make run    # Terminal 1", config.output_dir);
            println!("   cd {} && dart compile exe bin/{}.dart -o client && ./client   # Terminal 2", config.output_dir, snake);
        }
    }
    println!();
}

// ═══════════════════════════════════════════════════════════════════════════
// File Utilities (used by templates)
// ═══════════════════════════════════════════════════════════════════════════

pub fn create_dir(path: &str) {
    fs::create_dir_all(path).expect(&format!("Failed to create: {}", path));
}

pub fn write_file(path: &str, content: &str) {
    let parent = Path::new(path).parent().unwrap();
    fs::create_dir_all(parent).ok();
    fs::write(path, content).expect(&format!("Failed to write: {}", path));
    println!("   {} {}", style("✓").green(), path);
}
