use std::fs;
use std::path::PathBuf;
use tree_sitter::{Parser as TSParser, Query, QueryCursor, Node};
use streaming_iterator::StreamingIterator;
use crate::models::{MemoryEvent, MemoryEventKind};

pub fn check_overflows(path: PathBuf) -> Result<Vec<MemoryEvent>, String> {
    let code = fs::read_to_string(&path).map_err(|e| format!("Could not read file {}: {}", path.display(), e))?;
    let mut parser = TSParser::new();
    let language = tree_sitter_c::LANGUAGE;
    parser.set_language(&language.into()).expect("Error loading C grammar");

    let tree = parser.parse(&code, None).expect("Failed to parse code");
    let root_node = tree.root_node();

    let mut events = Vec::new();

    let func_query_str = r#"
        (function_definition
            declarator: (function_declarator
                declarator: (identifier) @func_name
            )
            body: (compound_statement) @body
        )
    "#;
    let func_query = Query::new(&language.into(), func_query_str).unwrap();
    let mut func_cursor = QueryCursor::new();
    let mut func_matches = func_cursor.matches(&func_query, root_node, code.as_bytes());

    while let Some(m) = func_matches.next() {
        let func_name = m.captures[0].node.utf8_text(code.as_bytes()).unwrap();
        let body_node = m.captures[1].node;

        let mut arrays = std::collections::HashMap::new();

        // 1. Find fixed-size arrays
        let decl_query_str = r#"
            (declaration
                declarator: (array_declarator
                    declarator: (identifier) @name
                    size: (number_literal) @size
                )
            )
        "#;
        let decl_query = Query::new(&language.into(), decl_query_str).unwrap();
        let mut decl_cursor = QueryCursor::new();
        let mut decl_matches = decl_cursor.matches(&decl_query, body_node, code.as_bytes());

        while let Some(dm) = decl_matches.next() {
            let name = dm.captures[0].node.utf8_text(code.as_bytes()).unwrap().to_string();
            let size_str = dm.captures[1].node.utf8_text(code.as_bytes()).unwrap();
            if let Ok(size) = size_str.parse::<usize>() {
                arrays.insert(name, size);
            }
        }

        // 2. Scan for if-guards and collect deductive constraints
        let if_query_str = r#"
            (if_statement
                condition: (parenthesized_expression
                    (binary_expression
                        left: (identifier) @var
                        operator: [
                            "<" @lt
                            "<=" @le
                            ">" @gt
                            ">=" @ge
                        ]
                        right: (number_literal) @val
                    )
                )
                consequence: (_) @then
                alternative: (else_clause (_))? @else
            )
        "#;
        let if_query = Query::new(&language.into(), if_query_str).unwrap();
        let mut if_cursor = QueryCursor::new();
        let mut if_matches = if_cursor.matches(&if_query, body_node, code.as_bytes());

        while let Some(im) = if_matches.next() {
            let var_name = im.captures[0].node.utf8_text(code.as_bytes()).unwrap().to_string();
            let op = im.captures[1].node.utf8_text(code.as_bytes()).unwrap().to_string();
            let val = im.captures[2].node.utf8_text(code.as_bytes()).unwrap().parse::<usize>().unwrap_or(0);
            
            let then_node = im.captures[3].node;
            let else_node = im.captures.get(4).map(|c| c.node);

            // Check THEN block with original constraint
            check_block_for_overflows(then_node, &var_name, &op, val, &arrays, func_name, code.as_bytes(), &mut events);
            
            // Check ELSE block with negated constraint
            if let Some(en) = else_node {
                let negated_op = match op.as_str() {
                    "<" => ">=",
                    "<=" => ">",
                    ">" => "<=",
                    ">=" => "<",
                    _ => continue,
                };
                check_block_for_overflows(en, &var_name, negated_op, val, &arrays, func_name, code.as_bytes(), &mut events);
            }
        }

        // 3. Scan for for-loops and collect deductive constraints (Off-By-One)

        let loop_query_str = r#"
            (for_statement
                condition: (_) @cond
                body: (_) @body
            )
        "#;
        let loop_query = Query::new(&language.into(), loop_query_str).unwrap();
        let mut loop_cursor = QueryCursor::new();
        let mut loop_matches = loop_cursor.matches(&loop_query, body_node, code.as_bytes());

        while let Some(lm) = loop_matches.next() {
            let cond_node = lm.captures[0].node;
            let loop_body = lm.captures[1].node;

            let mut cond_cursor = cond_node.walk();
            let mut binary_expr = None;
            
            // The condition of a for loop is often a parenthesized_expression or binary_expression
            if cond_node.kind() == "binary_expression" {
                binary_expr = Some(cond_node);
            } else {
                for child in cond_node.children(&mut cond_cursor) {
                    if child.kind() == "binary_expression" {
                        binary_expr = Some(child);
                        break;
                    }
                }
            }

            if let Some(be) = binary_expr {
                let mut be_cursor = be.walk();
                for child in be.children(&mut be_cursor) {
                }
                be_cursor = be.walk();
                let mut var_name = None;
                let mut op = None;
                let mut val = None;

                for child in be.children(&mut be_cursor) {
                    match child.kind() {
                        "identifier" => var_name = Some(child.utf8_text(code.as_bytes()).unwrap().to_string()),
                        "<" | "<=" | ">" | ">=" | "==" => op = Some(child.utf8_text(code.as_bytes()).unwrap().to_string()),
                        "number_literal" => val = child.utf8_text(code.as_bytes()).unwrap().parse::<usize>().ok(),
                        "declaration" => {
                            // Sometimes the decl is in the loop header
                        }
                        _ => {}
                    }
                }

                if let (Some(v), Some(o), Some(v_val)) = (var_name, op, val) {
                    check_block_for_overflows(loop_body, &v, &o, v_val, &arrays, func_name, code.as_bytes(), &mut events);
                }
            }
        }

        // 4. Simple literal overflows (non-branching)
        let access_query_str = r#"
            (subscript_expression
                argument: (identifier) @name
                index: (number_literal) @index
            )
        "#;
        let access_query = Query::new(&language.into(), access_query_str).unwrap();
        let mut access_cursor = QueryCursor::new();
        let mut access_matches = access_cursor.matches(&access_query, body_node, code.as_bytes());

        while let Some(am) = access_matches.next() {
            let name = am.captures[0].node.utf8_text(code.as_bytes()).unwrap().to_string();
            let index_str = am.captures[1].node.utf8_text(code.as_bytes()).unwrap();
            let line = am.captures[1].node.start_position().row + 1;

            if let Some(&size) = arrays.get(&name) {
                if let Ok(index) = index_str.parse::<usize>() {
                    if index >= size {
                        events.push(MemoryEvent {
                            kind: MemoryEventKind::BufferOverflow,
                            variable: name.clone(),
                            line,
                            context: format!("CRITICAL: Buffer Overflow in {}. Accessing {}[{}] but size is {}", func_name, name, index, size),
                        });
                    }
                }
            }
        }
    }

    Ok(events)
}

fn check_block_for_overflows(
    node: Node, 
    var_name: &str, 
    op: &str, 
    val: usize, 
    arrays: &std::collections::HashMap<String, usize>,
    func_name: &str,
    code: &[u8],
    events: &mut Vec<MemoryEvent>
) {
    let access_query_str = r#"
        (subscript_expression
            argument: (identifier) @arr_name
            index: (identifier) @idx_name
        )
    "#;
    let language = tree_sitter_c::LANGUAGE;
    let query = Query::new(&language.into(), access_query_str).unwrap();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, node, code);

    while let Some(m) = matches.next() {
        let subscript_node = m.captures[0].node.parent().unwrap();
        let mut sc = subscript_node.walk();
        for child in subscript_node.children(&mut sc) {
        }
        let arr_name = m.captures[0].node.utf8_text(code).unwrap().to_string();
        let idx_name = m.captures[1].node.utf8_text(code).unwrap();
        let line = m.captures[1].node.start_position().row + 1;

        if idx_name == var_name {
            if let Some(&arr_size) = arrays.get(&arr_name) {
                // Deduce if 'op val' guarantees an overflow
                // e.g. if we know idx >= 5 and arr_size is 5, then it's an overflow.
                let is_overflow = match op {
                    ">=" => val >= arr_size,
                    ">" => val >= arr_size - 1,
                    "==" => val >= arr_size,
                    "<=" => val >= arr_size,
                    _ => false, // We favor false negatives over false positives for now
                };

                if is_overflow {
                    events.push(MemoryEvent {
                        kind: MemoryEventKind::BufferOverflow,
                        variable: arr_name.clone(),
                        line,
                        context: format!("CRITICAL: Deductive Overflow in {}. Path constraint '{} {} {}' violates {} size {}", func_name, var_name, op, val, arr_name, arr_size),
                    });
                }
            }
        }
    }
}
