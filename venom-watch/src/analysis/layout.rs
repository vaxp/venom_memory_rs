use std::fs;
use std::path::PathBuf;
use tree_sitter::{Parser as TSParser, Query, QueryCursor};
use streaming_iterator::StreamingIterator;
use crate::models::{Field, StructLayout, EnumMember, EnumLayout};

pub fn analyze_file(path: &PathBuf, struct_name: &str) -> Result<StructLayout, String> {
    let code = fs::read_to_string(path).map_err(|e| format!("Could not read file {}: {}", path.display(), e))?;
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    
    let mut parser = TSParser::new();
    let is_rust = ext == "rs";
    let language = if is_rust {
        tree_sitter_rust::LANGUAGE
    } else {
        tree_sitter_c::LANGUAGE
    };
    
    parser.set_language(&language.into()).expect("Error loading grammar");

    let tree = parser.parse(&code, None).expect("Failed to parse code");
    let root_node = tree.root_node();

    if is_rust {
        return analyze_rust_struct(struct_name, &code, root_node, path.to_string_lossy().to_string());
    }

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
                  let actual_name = if inner_decl.kind() == "pointer_declarator" {
                      inner_decl.child_by_field_name("declarator").unwrap().utf8_text(code.as_bytes()).unwrap()
                  } else {
                      inner_decl.utf8_text(code.as_bytes()).unwrap()
                  };
                  (actual_name, true, len)
             } else if decl_node.kind() == "pointer_declarator" {
                  (decl_node.child_by_field_name("declarator").unwrap().utf8_text(code.as_bytes()).unwrap(), false, 1)
             } else {
                  (decl_node.utf8_text(code.as_bytes()).unwrap(), false, 1)
             };

             let is_pointer = type_text.contains('*') || decl_node.kind() == "pointer_declarator";
             let size = if is_pointer { 8 } else { get_type_size(type_text, code, root_node) } * array_len;
             let align = if is_pointer { 8 } else { get_type_alignment(type_text, code, root_node) };
             
             let padding = (align - (current_offset % align)) % align;
             current_offset += padding;

             let start_pos = decl_node.start_position();
             let line = start_pos.row + 1;

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
    
    let max_align = fields.iter().map(|f| {
        let is_ptr = f.type_name.contains('*') || f.is_pointer;
        if is_ptr { 8 } else { get_type_alignment(&f.type_name, code, root_node) }
    }).max().unwrap_or(1);
    let padding = (max_align - (current_offset % max_align)) % max_align;
    current_offset += padding;

    Ok(StructLayout {
        name: struct_name.to_string(),
        fields,
        total_size: current_offset,
        file_path,
    })
}

fn analyze_rust_struct(struct_name: &str, code: &str, root_node: tree_sitter::Node, file_path: String) -> Result<StructLayout, String> {
    let language = tree_sitter_rust::LANGUAGE;
    let query_str = format!(
        r#"
        (struct_item
            name: (type_identifier) @name
            body: (field_declaration_list) @fields
        ) @item
        "#
    );
    
    let query = Query::new(&language.into(), &query_str).expect("Invalid query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root_node, code.as_bytes());

    while let Some(m) = matches.next() {
        let name_node = m.captures[1].node;
        let r_struct_name = name_node.utf8_text(code.as_bytes()).unwrap();

        if r_struct_name == struct_name {
            let fields_node = m.captures[2].node;
            return parse_rust_fields(fields_node, struct_name, code, root_node, file_path);
        }
    }
    
    Err(format!("Rust struct '{}' not found", struct_name))
}

fn parse_rust_fields(fields_list_node: tree_sitter::Node, struct_name: &str, code: &str, root_node: tree_sitter::Node, file_path: String) -> Result<StructLayout, String> {
    let mut fields = Vec::new();
    let mut current_offset = 0;
    
    let mut cursor = fields_list_node.walk();
    for child in fields_list_node.children(&mut cursor) {
        if child.kind() == "field_declaration" {
             let type_node = child.child_by_field_name("type").ok_or("No type")?;
             let name_node = child.child_by_field_name("name").ok_or("No name")?;
             
             let name = name_node.utf8_text(code.as_bytes()).unwrap();
             let type_text = type_node.utf8_text(code.as_bytes()).unwrap();
             
             let (size, align, is_array, array_len) = get_rust_type_info(type_text, code, root_node);
             
             let padding = (align - (current_offset % align)) % align;
             current_offset += padding;

             fields.push(Field {
                 name: name.to_string(),
                 type_name: type_text.to_string(),
                 size,
                 offset: current_offset,
                 is_array,
                 array_len,
                 line: name_node.start_position().row + 1,
                 is_pointer: type_text.contains('*') || type_text.starts_with('&'),
             });

             current_offset += size;
        }
    }
    
    let max_align = fields.iter().map(|f| {
        let (_, align, _, _) = get_rust_type_info(&f.type_name, code, root_node);
        align
    }).max().unwrap_or(1);
    let padding = (max_align - (current_offset % max_align)) % max_align;
    current_offset += padding;

    Ok(StructLayout {
        name: struct_name.to_string(),
        fields,
        total_size: current_offset,
        file_path,
    })
}

fn get_rust_type_info(t: &str, code: &str, root_node: tree_sitter::Node) -> (usize, usize, bool, usize) {
    let t = t.trim();
    if t.starts_with('[') && t.contains(';') {
        let inner = &t[1..t.len()-1];
        let parts: Vec<&str> = inner.split(';').collect();
        if parts.len() == 2 {
            let inner_type = parts[0].trim();
            let size_str = parts[1].trim();
            let len = size_str.parse::<usize>().unwrap_or(1);
            let (inner_size, inner_align, _, _) = get_rust_type_info(inner_type, code, root_node);
            return (inner_size * len, inner_align, true, len);
        }
    }

    let (size, align) = match t {
        "u8" | "i8" | "bool" => (1, 1),
        "u16" | "i16" => (2, 2),
        "u32" | "i32" | "f32" => (4, 4),
        "u64" | "i64" | "f64" | "usize" | "isize" => (8, 8),
        _ if t.starts_with('&') || t.contains('*') => (8, 8),
        _ => (4, 4), // Fallback
    };
    
    (size, align, false, 1)
}

fn parse_array_size(s: &str) -> usize {
    if let Ok(n) = s.parse::<usize>() {
        return n;
    }
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
        "long" | "int64_t" | "uint64_t" | "double" | "size_t" | "guint64" | "uintptr_t" => 8,
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

fn get_type_alignment(t: &str, code: &str, root_node: tree_sitter::Node) -> usize {
    match t {
        "char" | "int8_t" | "uint8_t" => 1,
        "short" | "int16_t" | "uint16_t" => 2,
        "int" | "int32_t" | "uint32_t" | "float" | "gint" | "guint32" | "gboolean" => 4,
        "long" | "int64_t" | "uint64_t" | "double" | "size_t" | "guint64" | "uintptr_t" => 8,
        _ => {
            if let Ok(layout) = find_and_parse_struct(t, code, root_node) {
                layout.fields.iter().map(|f| {
                    let is_ptr = f.type_name.contains('*') || f.is_pointer;
                    if is_ptr { 8 } else { get_type_alignment(&f.type_name, code, root_node) }
                }).max().unwrap_or(1)
            } else {
                1
            }
        }
    }
}

pub fn analyze_enum(path: &PathBuf, enum_name: &str) -> Result<EnumLayout, String> {
    let code = fs::read_to_string(path).map_err(|e| format!("Could not read file {}: {}", path.display(), e))?;
    let mut parser = TSParser::new();
    let language = tree_sitter_c::LANGUAGE;
    parser.set_language(&language.into()).expect("Error loading C grammar");

    let tree = parser.parse(&code, None).expect("Failed to parse code");
    let root_node = tree.root_node();

    let mut members = Vec::new();
    let mut cursor = root_node.walk();
    
    let mut found = false;
    for node in root_node.children(&mut cursor) {
        if node.kind() == "enum_specifier" {
            if let Some(name_node) = node.child_by_field_name("name") {
                if name_node.utf8_text(code.as_bytes()).unwrap() == enum_name {
                    found = true;
                    let body = node.child_by_field_name("body").ok_or("Enum has no body")?;
                    let mut body_cursor = body.walk();
                    let mut current_val = 0;
                    for member in body.children(&mut body_cursor) {
                        if member.kind() == "enumerator" {
                            let name = member.child_by_field_name("name").unwrap().utf8_text(code.as_bytes()).unwrap();
                            if let Some(val_node) = member.child_by_field_name("value") {
                                let val_text = val_node.utf8_text(code.as_bytes()).unwrap();
                                current_val = val_text.parse::<i64>().unwrap_or(0);
                            }
                            members.push(EnumMember {
                                name: name.to_string(),
                                value: current_val,
                                line: member.start_position().row + 1,
                            });
                            current_val += 1;
                        }
                    }
                }
            }
        }
    }

    if !found {
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
                let list_node = m.captures[0].node;
                let mut current_value = 0;
                let mut list_cursor = list_node.walk();
                for child in list_node.children(&mut list_cursor) {
                    if child.kind() == "enumerator" {
                        let name_node = child.child_by_field_name("name").unwrap();
                        let member_name = name_node.utf8_text(code.as_bytes()).unwrap();
                        
                        if let Some(value_node) = child.child_by_field_name("value") {
                            let value_text = value_node.utf8_text(code.as_bytes()).unwrap();
                            let val = value_text.parse::<i64>().unwrap_or(current_value);
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
                found = true;
                break;
            }
        }
    }

    if !found {
        return Err(format!("Enum '{}' not found in {}", enum_name, path.display()));
    }

    Ok(EnumLayout {
        name: enum_name.to_string(),
        members,
        file_path: path.to_string_lossy().to_string(),
    })
}
