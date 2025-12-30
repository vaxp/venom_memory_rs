use clap::Parser;
use colored::Colorize;
use std::path::PathBuf;
use venom_watch::{analyze_file, analyze_enum, run_safety_analysis, StructLayout, EnumLayout, ValidationResult, MemoryEventKind};
use serde_json;
use std::io;
use ratatui::{
    backend::CrosstermBackend,
    widgets::{Block, Borders, List, ListItem},
    layout::{Layout, Constraint, Direction},
    style::{Style, Color, Modifier},
    Terminal,
};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

#[derive(Parser, Debug)]
#[command(name = "venom-watch")]
#[command(about = "🕵️ VenomMemory Structure Validator", long_about = None)]
struct Cli {
    /// Path to the server header file (C)
    #[arg(short, long)]
    server: Option<PathBuf>,

    /// Path to the client source file (C or Rust)
    #[arg(short, long)]
    client: Option<PathBuf>,

    /// Name of the struct to validate
    #[arg(short = 'n', long)]
    struct_name: Option<String>,

    /// Name of the enum to validate
    #[arg(short = 'e', long)]
    enum_name: Option<String>,

    /// Check for memory leaks in a C file
    #[arg(long)]
    check_leaks: Option<PathBuf>,

    /// Output results in JSON format
    #[arg(short, long)]
    json: bool,

    /// Launch interactive TUI for memory lifecycle visualization
    #[arg(long)]
    tui: bool,
}

fn main() {
    let args = Cli::parse();
    let mut overall_success = true;

    if !args.json {
        println!("{}", "🕵️ Venom Watch: Advanced Memory Analysis...".cyan().bold());
    }

    // 1. Structure/Enum Validation
    if let (Some(server_path), Some(client_path)) = (&args.server, &args.client) {
        if let Some(struct_name) = &args.struct_name {
            match analyze_file(server_path, struct_name) {
                Ok(server_layout) => {
                    match analyze_file(client_path, struct_name) {
                        Ok(client_layout) => {
                            if !compare_layouts(&server_layout, &client_layout, args.json) {
                                overall_success = false;
                            }
                        }
                        Err(e) => {
                            if !args.json { println!("{} {}", "Error:".red(), e); }
                            overall_success = false;
                        }
                    }
                }
                Err(e) => {
                    if !args.json { println!("{} {}", "Error:".red(), e); }
                    overall_success = false;
                }
            }
        } else if let Some(enum_name) = &args.enum_name {
            match analyze_enum(server_path, enum_name) {
                Ok(server_layout) => {
                    match analyze_enum(client_path, enum_name) {
                        Ok(client_layout) => {
                            if !compare_enums(&server_layout, &client_layout, args.json) {
                                overall_success = false;
                            }
                        }
                        Err(e) => {
                            if !args.json { println!("{} {}", "Error:".red(), e); }
                            overall_success = false;
                        }
                    }
                }
                Err(e) => {
                    if !args.json { println!("{} {}", "Error:".red(), e); }
                    overall_success = false;
                }
            }
        }
    }

    // 2. Leak Detection
    if let Some(leak_path) = &args.check_leaks {
        match run_safety_analysis(leak_path) {
            Ok(report) => {
                if args.tui {
                    if let Err(e) = run_tui(&report) {
                        eprintln!("TUI Error: {}", e);
                    }
                } else if args.json {
                    // JSON mode handles its own output for leaks too if needed, 
                    // but usually we want a combined JSON.
                    // For now, let's keep it simple: if leaks are requested, print leak report.
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                } else {
                    println!("\n{}", "🔍 Memory Leak Report:".bold());
                    println!("{}", "--------------------------------------------------".dimmed());
                    if report.success {
                        println!("{}", "✅ No obvious leaks detected in local scopes.".green());
                    } else {
                        for finding in &report.findings {
                            println!("❌ {}", finding.red());
                        }
                    }
                }
                if !report.success { overall_success = false; }
            }
            Err(e) => {
                if !args.json { println!("{} {}", "Error:".red(), e); }
                overall_success = false;
            }
        }
    }

    if !overall_success {
        std::process::exit(1);
    }
}

fn run_tui(report: &venom_watch::LeakReport) -> Result<(), io::Error> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let code_content = std::fs::read_to_string(&report.file_path).unwrap_or_default();
    let lines: Vec<&str> = code_content.lines().collect();
    
    let mut scroll = 0;

    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(70), Constraint::Percentage(30)].as_ref())
                .split(f.size());

            // 1. Code View
            let mut code_items = Vec::new();
            for (i, line) in lines.iter().enumerate().skip(scroll) {
                let line_num = i + 1;
                let mut style = Style::default();
                let mut prefix = format!("{:>3} | ", line_num);
                
                // Overlay markers
                for event in &report.events {
                    if event.line == line_num {
                        match event.kind {
                            MemoryEventKind::Allocation => {
                                prefix = format!("🟢 {:>1} | ", "A");
                                style = style.fg(Color::Green);
                            }
                            MemoryEventKind::Free => {
                                prefix = format!("🔴 {:>1} | ", "F");
                                style = style.fg(Color::Red);
                            }
                            MemoryEventKind::PotentialMove => {
                                prefix = format!("🟡 {:>1} | ", "M");
                                style = style.fg(Color::Yellow);
                            }
                            MemoryEventKind::ExplicitMove => {
                                prefix = format!("🔵 {:>1} | ", "E");
                                style = style.fg(Color::Blue);
                            }
                            MemoryEventKind::ConditionalFree => {
                                prefix = format!("🟧 {:>1} | ", "C");
                                style = style.fg(Color::Rgb(255, 165, 0));
                            }
                            MemoryEventKind::UseAfterFree => {
                                prefix = format!("💀 {:>1} | ", "U");
                                style = style.fg(Color::Magenta);
                            }
                            MemoryEventKind::DoubleFree => {
                                prefix = format!("🚫 {:>1} | ", "D");
                                style = style.fg(Color::LightRed);
                            }
                            MemoryEventKind::BufferOverflow => {
                                prefix = format!("⚠️  {:>1} | ", "O");
                                style = style.fg(Color::LightRed).add_modifier(Modifier::BOLD);
                            }
                        }
                    }
                }

                code_items.push(ListItem::new(format!("{}{}", prefix, line)).style(style));
            }
            let code_list = List::new(code_items)
                .block(Block::default().borders(Borders::ALL).title(format!(" Source: {} ", report.file_path)));
            f.render_widget(code_list, chunks[0]);

            // 2. Status Panel
            let mut status_text = vec![
                ListItem::new(" LEYEND:").style(Style::default().add_modifier(Modifier::BOLD)),
                ListItem::new(" 🟢 A: Allocation").style(Style::default().fg(Color::Green)),
                ListItem::new(" 🔴 F: Free").style(Style::default().fg(Color::Red)),
                ListItem::new(" 🟡 M: Potential Move").style(Style::default().fg(Color::Yellow)),
                ListItem::new(" 🔵 E: Explicit Move").style(Style::default().fg(Color::Blue)),
                ListItem::new(" 🟧 C: Conditional Free").style(Style::default().fg(Color::Rgb(255, 165, 0))),
                ListItem::new(" 💀 U: Use-After-Free").style(Style::default().fg(Color::Magenta)),
                ListItem::new(" 🚫 D: Double Free").style(Style::default().fg(Color::LightRed)),
                ListItem::new(" ⚠️  O: Buffer Overflow").style(Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)),
                ListItem::new(""),
                ListItem::new(" FINDINGS:").style(Style::default().add_modifier(Modifier::BOLD)),
            ];

            if report.findings.is_empty() {
                status_text.push(ListItem::new(" ✅ No leaks!").style(Style::default().fg(Color::Green)));
            } else {
                for finding in &report.findings {
                    status_text.push(ListItem::new(format!(" ❌ {}", finding)).style(Style::default().fg(Color::Red)));
                }
            }

            status_text.push(ListItem::new(""));
            status_text.push(ListItem::new(" (Press 'q' to exit, arrows to scroll)"));

            let status_list = List::new(status_text)
                .block(Block::default().borders(Borders::ALL).title(" Memory Lifecycle "));
            f.render_widget(status_list, chunks[1]);
        })?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Up => {
                        if scroll > 0 { scroll -= 1; }
                    }
                    KeyCode::Down => {
                        if scroll < lines.len() - 1 { scroll += 1; }
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

fn compare_layouts(server: &StructLayout, client: &StructLayout, json_mode: bool) -> bool {
    let mut all_match = true;
    let mut issues = Vec::new();

    if server.total_size != client.total_size {
        all_match = false;
        issues.push(format!("Size mismatch: Server={} bytes, Client={} bytes", server.total_size, client.total_size));
    }

    if !json_mode {
        println!("\n{} {}", "Validating Structure:".bold(), server.name.blue());
        println!("{}: {} bytes", "Server Struct".green(), server.total_size);
        println!("{}: {} bytes", "Client Struct".yellow(), client.total_size);
        println!("--------------------------------------------------");
        if all_match { println!("{}", "✅ Total sizes match.".green()); }
        else { println!("{}", "⚠️  SIZE MISMATCH IDENTIFIED!".red().bold()); }
        println!("\n{:<20} | {:<16} | {:<16} | {:<30}", "Field", "Server (Line)", "Client (Line)", "Status");
        println!("{}", "-".repeat(90));
    }

    let mut s_idx = 0;
    let mut c_idx = 0;
    let mut s_current_offset = 0;
    let mut c_current_offset = 0;

    loop {
        let s_field = server.fields.get(s_idx);
        let c_field = client.fields.get(c_idx);

        if s_field.is_none() && c_field.is_none() {
            // Check for trailing padding (struct total size vs last field)
            if s_current_offset < server.total_size || c_current_offset < client.total_size {
                let s_pad = server.total_size - s_current_offset;
                let c_pad = client.total_size - c_current_offset;
                if s_pad > 0 || c_pad > 0 {
                    if !json_mode {
                        println!("{:<20} | {:<16} | {:<16} | {}", 
                            "[TRAILING PAD]".cyan().dimmed(),
                            if s_pad > 0 { format!("{} bytes", s_pad).cyan() } else { "N/A".into() },
                            if c_pad > 0 { format!("{} bytes", c_pad).cyan() } else { "N/A".into() },
                            if s_pad == c_pad { "✅ OK".green() } else { "⚠️  Mismatch".yellow() }
                        );
                    } else {
                        if s_pad != c_pad {
                            issues.push(format!("Trailing padding mismatch: Server={} bytes, Client={} bytes", s_pad, c_pad));
                            all_match = false;
                        } else if s_pad > 0 {
                            issues.push(format!("Info: Trailing padding detected ({} bytes)", s_pad));
                        }
                    }
                }
            }
            break;
        }

        // Check for internal padding in server
        if let Some(s) = s_field {
            if s.offset > s_current_offset {
                let pad = s.offset - s_current_offset;
                if !json_mode {
                    println!("{:<20} | {:<16} | {:<16} | {}", 
                        "[PADDING]".cyan().dimmed(),
                        format!("{} bytes", pad).cyan(),
                        "",
                        "INTERNAL".dimmed()
                    );
                } else {
                    issues.push(format!("Info: Internal padding in server before {} ({} bytes)", s.name, pad));
                }
                s_current_offset = s.offset;
            }
        }

        // Check for internal padding in client
        if let Some(c) = c_field {
            if c.offset > c_current_offset {
                let pad = c.offset - c_current_offset;
                if !json_mode {
                    println!("{:<20} | {:<16} | {:<16} | {}", 
                        "[PADDING]".cyan().dimmed(),
                        "",
                        format!("{} bytes", pad).cyan(),
                        "INTERNAL".dimmed()
                    );
                } else {
                    issues.push(format!("Info: Internal padding in client before {} ({} bytes)", c.name, pad));
                }
                c_current_offset = c.offset;
            }
        }

        match (s_field, c_field) {
            (Some(s), Some(c)) => {
                let mut status_issues = Vec::new();
                let status = if s.offset != c.offset {
                    all_match = false;
                    status_issues.push("Offset Mismatch".to_string());
                    "❌ Offset Mismatch".red()
                } else if s.size != c.size {
                    all_match = false;
                    status_issues.push("Size Mismatch".to_string());
                    "❌ Size Mismatch".red()
                } else if s.name != c.name {
                    status_issues.push("Name Diff".to_string());
                     "⚠️ Name Diff".yellow()
                } else {
                     "✅ OK".green()
                };

                if s.is_pointer || c.is_pointer {
                    status_issues.push("🚨 POINTER DANGER!".to_string());
                }

                if !json_mode {
                    let mut status_str = status.to_string();
                    if s.is_pointer || c.is_pointer {
                        status_str = format!("{} | {}", status_str, "🚨 POINTER DANGER!".on_red().white().bold());
                    }
                    let s_info = format!("@{: <4} (L{})", s.offset, s.line);
                    let c_info = format!("@{: <4} (L{})", c.offset, c.line);
                    println!("{:<20} | {:<16} | {:<16} | {}", s.name.chars().take(20).collect::<String>(), s_info, c_info, status_str);
                }
                
                if !status_issues.is_empty() {
                    issues.push(format!("Field {}: {}", s.name, status_issues.join(", ")));
                }

                s_current_offset = s.offset + s.size;
                c_current_offset = c.offset + c.size;
                s_idx += 1;
                c_idx += 1;
            },
            (Some(s), None) => {
                 all_match = false;
                 issues.push(format!("Field {} missing in client", s.name));
                 if !json_mode {
                     let s_info = format!("@{: <4} (L{})", s.offset, s.line);
                     println!("{:<20} | {:<16} | {:<16} | {}", s.name, s_info, "MISSING", "❌ Missing in Client".red());
                 }
                 s_current_offset = s.offset + s.size;
                 s_idx += 1;
            },
            (None, Some(c)) => {
                 all_match = false;
                 issues.push(format!("Field {} extra in client", c.name));
                 if !json_mode {
                     let c_info = format!("@{: <4} (L{})", c.offset, c.line);
                     println!("{:<20} | {:<16} | {:<16} | {}", c.name, "MISSING", c_info, "❌ Extra in Client".red());
                 }
                 c_current_offset = c.offset + c.size;
                 c_idx += 1;
            },
            _ => unreachable!(),
        }
    }

    if json_mode {
        let result = ValidationResult {
            success: all_match,
            server_size: server.total_size,
            client_size: client.total_size,
            issues,
        };
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
    }

    all_match
}

fn compare_enums(server: &EnumLayout, client: &EnumLayout, json_mode: bool) -> bool {
    let mut all_match = true;
    let mut issues = Vec::new();

    if !json_mode {
        println!("\n{}: {} members", "Server Enum".green(), server.members.len());
        println!("{}: {} members", "Client Enum".yellow(), client.members.len());
        println!("--------------------------------------------------");
        println!("{:<25} | {:<15} | {:<15} | {:<20}", "Member", "Server (Val)", "Client (Val)", "Status");
        println!("{}", "-".repeat(80));
    }

    use std::collections::HashMap;
    let client_members: HashMap<String, &venom_watch::EnumMember> = client.members.iter().map(|m| (m.name.clone(), m)).collect();

    for s in &server.members {
        let c = client_members.get(&s.name);
        match c {
            Some(c_member) => {
                let matches = s.value == c_member.value;
                if !matches {
                    all_match = false;
                    issues.push(format!("Enum member {} mismatch: Server={}, Client={}", s.name, s.value, c_member.value));
                }
                if !json_mode {
                    let status = if matches { "✅ OK".green() } else { format!("❌ Mismatch (@L{})", c_member.line).red() };
                    println!("{:<25} | {:<15} | {:<15} | {}", s.name, format!("{} (L{})", s.value, s.line), format!("{} (L{})", c_member.value, c_member.line), status);
                }
            }
            None => {
                all_match = false;
                issues.push(format!("Enum member {} missing in client", s.name));
                if !json_mode {
                    println!("{:<25} | {:<15} | {:<15} | {}", s.name, s.value, "MISSING", "❌ Missing in Client".red());
                }
            }
        }
    }

    if json_mode {
        let result = ValidationResult {
            success: all_match,
            server_size: 0,
            client_size: 0,
            issues,
        };
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
    } else {
        if all_match { println!("\n{}", "✅ Enums are fully consistent!".green().bold()); }
        else { println!("\n{}", "⚠️  ENUM INCONSISTENCY DETECTED!".red().bold()); }
    }
    all_match
}
