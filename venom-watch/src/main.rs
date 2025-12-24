use clap::Parser;
use colored::*;
use std::fs;
use std::path::PathBuf;
use tree_sitter::{Parser as TSParser, Query, QueryCursor};
use streaming_iterator::StreamingIterator;

#[derive(Parser, Debug)]
#[command(name = "venom-watch")]
#[command(about = "🕵️ VenomMemory Structure Validator", long_about = None)]
struct Cli {
    /// Path to the server-side file (e.g., C header)
    #[arg(short, long)]
    server: PathBuf,

    /// Path to the client-side file (e.g., C source)
    #[arg(short, long)]
    client: PathBuf,

    /// Name of the struct to validate
    #[arg(short = 'n', long)]
    struct_name: Option<String>,

    /// Name of the enum to validate
    #[arg(short = 'e', long)]
    enum_name: Option<String>,
}

#[derive(Debug, Clone)]
struct Field {
    name: String,
    type_name: String,
    size: usize,
    offset: usize,
    #[allow(dead_code)]
    is_array: bool,
    #[allow(dead_code)]
    array_len: usize,
    line: usize,
    is_pointer: bool,
}

#[derive(Debug)]
struct StructLayout {
    #[allow(dead_code)]
    name: String,
    fields: Vec<Field>,
    total_size: usize,
    #[allow(dead_code)]
    file_path: String,
}

#[derive(Debug, Clone)]
struct EnumMember {
    name: String,
    value: i64,
    line: usize,
}

#[derive(Debug)]
struct EnumLayout {
    #[allow(dead_code)]
    name: String,
    members: Vec<EnumMember>,
    #[allow(dead_code)]
    file_path: String,
}

fn main() {
    let args = Cli::parse();

    println!("{}", "🕵️ Venom Watch: Validating IPC Structures...".cyan().bold());

    if let Some(ref s_name) = args.struct_name {
        let server_layout = analyze_file(&args.server, s_name, "Server").unwrap_or_else(|e| {
            eprintln!("{} {}", "❌ Server struct analysis failed:".red(), e);
            std::process::exit(1);
        });

        let client_layout = analyze_file(&args.client, s_name, "Client").unwrap_or_else(|e| {
            eprintln!("{} {}", "❌ Client struct analysis failed:".red(), e);
            std::process::exit(1);
        });

        compare_layouts(&server_layout, &client_layout);
    }

    if let Some(ref e_name) = args.enum_name {
        let server_enum = analyze_enum(&args.server, e_name).unwrap_or_else(|e| {
            eprintln!("{} {}", "❌ Server enum analysis failed:".red(), e);
            std::process::exit(1);
        });

        let client_enum = analyze_enum(&args.client, e_name).unwrap_or_else(|e| {
            eprintln!("{} {}", "❌ Client enum analysis failed:".red(), e);
            std::process::exit(1);
        });

        compare_enums(&server_enum, &client_enum);
    }

    if args.struct_name.is_none() && args.enum_name.is_none() {
        eprintln!("{}", "❌ Error: Please specify either --struct-name or --enum-name".red());
        std::process::exit(1);
    }
}

fn analyze_file(path: &PathBuf, struct_name: &str, _role: &str) -> Result<StructLayout, String> {
    let code = fs::read_to_string(path).map_err(|e| format!("Could not read file {}: {}", path.display(), e))?;
    
    let mut parser = TSParser::new();
    let language = tree_sitter_c::LANGUAGE;
    parser.set_language(&language.into()).expect("Error loading C grammar");

    let tree = parser.parse(&code, None).expect("Failed to parse code");
    let root_node = tree.root_node();

    // Query to find the struct definition
    let query_str = format!(
        r#"
        (struct_specifier
            name: (type_identifier) @struct_name
            body: (field_declaration_list) @fields
        )
        "#
    );
    
    let query = Query::new(&language.into(), &query_str).expect("Invalid query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root_node, code.as_bytes());

    while let Some(m) = matches.next() {
        let name_node = m.captures[0].node;
        let struct_def_name = name_node.utf8_text(code.as_bytes()).unwrap();

        if struct_def_name == struct_name {
            let fields_node = m.captures[1].node;
            return parse_fields(fields_node, struct_name, &code, root_node, path.to_string_lossy().to_string());
        }
    }
    
    // Check for typedef struct
    let typedef_query_str = format!(
        r#"
        (type_definition
            type: (struct_specifier
                body: (field_declaration_list) @fields
            )
            declarator: (type_identifier) @typedef_name
        )
        "#
    );
    let td_query = Query::new(&language.into(), &typedef_query_str).expect("Invalid typedef query");
    let mut td_cursor = QueryCursor::new();
    let mut td_matches = td_cursor.matches(&td_query, root_node, code.as_bytes());

    while let Some(m) = td_matches.next() {
        let name_node = m.captures[1].node;
        let type_name = name_node.utf8_text(code.as_bytes()).unwrap();

        if type_name == struct_name {
             let fields_node = m.captures[0].node;
             return parse_fields(fields_node, struct_name, &code, root_node, path.to_string_lossy().to_string());
        }
    }

    Err(format!("Struct '{}' not found in {}", struct_name, path.display()))
}

fn parse_fields(fields_list_node: tree_sitter::Node, struct_name: &str, code: &str, root_node: tree_sitter::Node, file_path: String) -> Result<StructLayout, String> {
    let mut fields = Vec::new();
    let mut current_offset = 0;
    
    let mut cursor = fields_list_node.walk();
    for child in fields_list_node.children(&mut cursor) {
        if child.kind() == "field_declaration" {
             let type_node = child.child_by_field_name("type").ok_or("No type")?;
             let decl_node = child.child_by_field_name("declarator").ok_or("No declarator")?;
             
             let type_text = type_node.utf8_text(code.as_bytes()).unwrap();
             
             let (name, is_array, array_len) = if decl_node.kind() == "array_declarator" {
                 let inner_decl = decl_node.child_by_field_name("declarator").unwrap();
                 let size_node = decl_node.child_by_field_name("size").unwrap();
                 let size_str = size_node.utf8_text(code.as_bytes()).unwrap();
                 let len = parse_array_size(size_str);
                 (inner_decl.utf8_text(code.as_bytes()).unwrap(), true, len)
             } else {
                 (decl_node.utf8_text(code.as_bytes()).unwrap(), false, 1)
             };

             let size = get_type_size(type_text, code, root_node) * array_len;
             let align = get_type_alignment(type_text);
             
             let padding = (align - (current_offset % align)) % align;
             current_offset += padding;

             // Extract line number
             let start_pos = decl_node.start_position();
             let line = start_pos.row + 1;

             // Detect pointers
             let is_pointer = type_text.contains('*') || decl_node.kind() == "pointer_declarator";

             fields.push(Field {
                 name: name.to_string(),
                 type_name: type_text.to_string(),
                 size,
                 offset: current_offset,
                 is_array,
                 array_len,
                 line,
                 is_pointer,
             });

             current_offset += size;
        }
    }
    
    let max_align = fields.iter().map(|f| get_type_alignment(&f.type_name)).max().unwrap_or(1);
    let padding = (max_align - (current_offset % max_align)) % max_align;
    current_offset += padding;

    Ok(StructLayout {
        name: struct_name.to_string(),
        fields,
        total_size: current_offset,
        file_path,
    })
}

fn parse_array_size(s: &str) -> usize {
    if let Ok(n) = s.parse::<usize>() {
        return n;
    }
    // Heuristic for macros
    match s {
        "MAX_DEVICE_NAME" => 128,
        "MAX_DEVICES" => 16,
        "MAX_APP_STREAMS" => 32,
        _ => 1,
    }
}

fn get_type_size(t: &str, code: &str, root_node: tree_sitter::Node) -> usize {
    match t {
        "char" | "int8_t" | "uint8_t" => 1,
        "short" | "int16_t" | "uint16_t" => 2,
        "int" | "int32_t" | "uint32_t" | "float" | "gint" | "guint32" | "gboolean" => 4,
        "long" | "int64_t" | "uint64_t" | "double" | "size_t" | "guint64" => 8,
        _ => {
            if t.ends_with('*') { 
                8 
            } else {
                if let Ok(layout) = find_and_parse_struct(t, code, root_node) {
                    layout.total_size
                } else {
                    4 
                }
            }
        }
    }
}

fn find_and_parse_struct(struct_name: &str, code: &str, root_node: tree_sitter::Node) -> Result<StructLayout, String> {
    let query_str = format!(
        r#"
        (struct_specifier
            name: (type_identifier) @struct_name
            body: (field_declaration_list) @fields
        )
        "#
    );
    let language = tree_sitter_c::LANGUAGE;
    let query = Query::new(&language.into(), &query_str).unwrap();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root_node, code.as_bytes());

    while let Some(m) = matches.next() {
        let name_node = m.captures[0].node;
        let name = name_node.utf8_text(code.as_bytes()).unwrap();
        if name == struct_name {
            return parse_fields(m.captures[1].node, struct_name, code, root_node, "nested".to_string());
        }
    }

    let typedef_query_str = format!(
        r#"
        (type_definition
            type: (struct_specifier
                body: (field_declaration_list) @fields
            )
            declarator: (type_identifier) @typedef_name
        )
        "#
    );
    let td_query = Query::new(&language.into(), &typedef_query_str).unwrap();
    let mut td_cursor = QueryCursor::new();
    let mut td_matches = td_cursor.matches(&td_query, root_node, code.as_bytes());

    while let Some(m) = td_matches.next() {
        let name_node = m.captures[1].node;
        let name = name_node.utf8_text(code.as_bytes()).unwrap();
        if name == struct_name {
             return parse_fields(m.captures[0].node, struct_name, code, root_node, "nested".to_string());
        }
    }

    Err("Struct not found".to_string())
}

fn get_type_alignment(t: &str) -> usize {
    match t {
        "char" | "int8_t" | "uint8_t" => 1,
        "short" | "int16_t" | "uint16_t" => 2,
        "int" | "int32_t" | "uint32_t" | "float" | "gint" | "guint32" | "gboolean" => 4,
        "long" | "int64_t" | "uint64_t" | "double" | "size_t" | "guint64" => 8,
        _ => 1,
    }
}

fn compare_layouts(server: &StructLayout, client: &StructLayout) {
    println!("\n{}: {} bytes", "Server Struct".green(), server.total_size);
    println!("{}: {} bytes", "Client Struct".yellow(), client.total_size);
    println!("--------------------------------------------------");

    if server.total_size != client.total_size {
        println!("{}", "⚠️  SIZE MISMATCH IDENTIFIED!".red().bold());
        println!("Expected: {} bytes", server.total_size);
        println!("Found:    {} bytes", client.total_size);
    } else {
        println!("{}", "✅ Total sizes match.".green());
    }
    
    println!("\n{:<20} | {:<16} | {:<16} | {:<30}", "Field", "Server (Line)", "Client (Line)", "Status");
    println!("{}", "-".repeat(90));

    let max_fields = std::cmp::max(server.fields.len(), client.fields.len());
    
    for i in 0..max_fields {
        let server_field = server.fields.get(i);
        let client_field = client.fields.get(i);

        match (server_field, client_field) {
            (Some(s), Some(c)) => {
                let status = if s.offset != c.offset {
                    "❌ Offset Mismatch".red()
                } else if s.size != c.size {
                    "❌ Size Mismatch".red()
                } else if s.name != c.name {
                     "⚠️ Name Diff".yellow()
                } else {
                     "✅ OK".green()
                };

                // Add pointer warning if detected
                let mut status_str = status.to_string();
                if s.is_pointer || c.is_pointer {
                    status_str = format!("{} | {}", status_str, "🚨 POINTER DANGER!".on_red().white().bold());
                }
                
                let s_info = format!("@{: <4} (L{})", s.offset, s.line);
                let c_info = format!("@{: <4} (L{})", c.offset, c.line);
                
                println!("{:<20} | {:<16} | {:<16} | {}", 
                    s.name.chars().take(20).collect::<String>(), 
                    s_info, 
                    c_info, 
                    status_str
                );
            },
            (Some(s), None) => {
                 let mut status_str = "❌ Missing in Client".red().to_string();
                 if s.is_pointer {
                     status_str = format!("{} | {}", status_str, "🚨 POINTER DANGER!".on_red().white().bold());
                 }
                 let s_info = format!("@{: <4} (L{})", s.offset, s.line);
                 println!("{:<20} | {:<16} | {:<16} | {}", s.name, s_info, "MISSING", status_str);
            },
            (None, Some(c)) => {
                 let mut status_str = "❌ Extra in Client".red().to_string();
                 if c.is_pointer {
                     status_str = format!("{} | {}", status_str, "🚨 POINTER DANGER!".on_red().white().bold());
                 }
                 let c_info = format!("@{: <4} (L{})", c.offset, c.line);
                 println!("{:<20} | {:<16} | {:<16} | {}", c.name, "MISSING", c_info, status_str);
            },
            (None, None) => break,
        }
    }
}

// -----------------------------------------------------------------------------
// Enum Validation Logic
// -----------------------------------------------------------------------------

fn analyze_enum(path: &PathBuf, enum_name: &str) -> Result<EnumLayout, String> {
    let code = fs::read_to_string(path).map_err(|e| format!("Could not read file {}: {}", path.display(), e))?;
    
    let mut parser = TSParser::new();
    let language = tree_sitter_c::LANGUAGE;
    parser.set_language(&language.into()).expect("Error loading C grammar");

    let tree = parser.parse(&code, None).expect("Failed to parse code");
    let root_node = tree.root_node();

    // Query for named enums: enum Name { ... }
    let query_str = r#"
        (enum_specifier
            name: (type_identifier) @name
            body: (enumerator_list) @list
        )
    "#;
    let query = Query::new(&language.into(), query_str).unwrap();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root_node, code.as_bytes());

    while let Some(m) = matches.next() {
        let name = m.captures[0].node.utf8_text(code.as_bytes()).unwrap();
        if name == enum_name {
            return parse_enum_list(m.captures[1].node, enum_name, &code, path.to_string_lossy().to_string());
        }
    }

    // Query for typedef enums: typedef enum { ... } Name;
    let td_query_str = r#"
        (type_definition
            type: (enum_specifier
                body: (enumerator_list) @list
            )
            declarator: (type_identifier) @name
        )
    "#;
    let td_query = Query::new(&language.into(), td_query_str).unwrap();
    let mut td_cursor = QueryCursor::new();
    let mut td_matches = td_cursor.matches(&td_query, root_node, code.as_bytes());

    while let Some(m) = td_matches.next() {
        let name = m.captures[1].node.utf8_text(code.as_bytes()).unwrap();
        if name == enum_name {
            return parse_enum_list(m.captures[0].node, enum_name, &code, path.to_string_lossy().to_string());
        }
    }

    Err(format!("Enum '{}' not found in {}", enum_name, path.display()))
}

fn parse_enum_list(list_node: tree_sitter::Node, name: &str, code: &str, path: String) -> Result<EnumLayout, String> {
    let mut members = Vec::new();
    let mut current_value = 0;

    let mut cursor = list_node.walk();
    for child in list_node.children(&mut cursor) {
        if child.kind() == "enumerator" {
            let name_node = child.child_by_field_name("name").unwrap();
            let member_name = name_node.utf8_text(code.as_bytes()).unwrap();
            
            if let Some(value_node) = child.child_by_field_name("value") {
                let value_text = value_node.utf8_text(code.as_bytes()).unwrap();
                // Basic parsing for integers, hex, etc.
                let val = if value_text.starts_with("0x") {
                    i64::from_str_radix(&value_text[2..], 16).unwrap_or(current_value)
                } else {
                    value_text.parse::<i64>().unwrap_or(current_value)
                };
                current_value = val;
            }

            members.push(EnumMember {
                name: member_name.to_string(),
                value: current_value,
                line: name_node.start_position().row + 1,
            });

            current_value += 1;
        }
    }

    Ok(EnumLayout {
        name: name.to_string(),
        members,
        file_path: path,
    })
}

fn compare_enums(server: &EnumLayout, client: &EnumLayout) {
    println!("\n{}: {} members", "Server Enum".green(), server.members.len());
    println!("{}: {} members", "Client Enum".yellow(), client.members.len());
    println!("--------------------------------------------------");

    let mut all_match = true;
    
    println!("{:<25} | {:<15} | {:<15} | {:<20}", "Member", "Server (Val)", "Client (Val)", "Status");
    println!("{}", "-".repeat(80));

    // Create a map for client members for easy lookup
    use std::collections::HashMap;
    let client_members: HashMap<String, &EnumMember> = client.members.iter().map(|m| (m.name.clone(), m)).collect();

    for s in &server.members {
        let c = client_members.get(&s.name);

        match c {
            Some(c_member) => {
                let status = if s.value == c_member.value {
                    "✅ OK".green()
                } else {
                    all_match = false;
                    format!("❌ Mismatch (@L{})", c_member.line).red()
                };

                println!("{:<25} | {:<15} | {:<15} | {}", 
                    s.name, 
                    format!("{} (L{})", s.value, s.line),
                    format!("{} (L{})", c_member.value, c_member.line),
                    status
                );
            }
            None => {
                all_match = false;
                println!("{:<25} | {:<15} | {:<15} | {}", s.name, s.value, "MISSING", "❌ Missing in Client".red());
            }
        }
    }

    // Check for extra members in client
    let server_names: Vec<String> = server.members.iter().map(|m| m.name.clone()).collect();
    for c in &client.members {
        if !server_names.contains(&c.name) {
            all_match = false;
            println!("{:<25} | {:<15} | {:<15} | {}", c.name, "MISSING", c.value, "❌ Extra in Client".red());
        }
    }

    if all_match {
        println!("\n{}", "✅ Enums are fully consistent!".green().bold());
    } else {
        println!("\n{}", "⚠️  ENUM INCONSISTENCY DETECTED!".red().bold());
    }
}
