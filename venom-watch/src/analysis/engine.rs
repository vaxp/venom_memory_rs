use std::fs;
use std::path::PathBuf;
use tree_sitter::{Parser as TSParser, Query, QueryCursor};
use streaming_iterator::StreamingIterator;
use crate::models::{LeakReport, MemoryEvent, MemoryEventKind};

pub fn check_leaks(path: &PathBuf) -> Result<LeakReport, String> {
    let code = fs::read_to_string(path).map_err(|e| format!("Could not read file {}: {}", path.display(), e))?;
    let mut parser = TSParser::new();
    let language = tree_sitter_c::LANGUAGE;
    parser.set_language(&language.into()).expect("Error loading C grammar");

    let tree = parser.parse(&code, None).expect("Failed to parse code");
    let root_node = tree.root_node();

    let mut findings = Vec::new();
    let mut events = Vec::new();
    let owning_keywords = vec!["free", "destroy", "clean", "delete", "release", "drop", "close"];

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

        let mut allocations = std::collections::HashMap::new();
        let mut usages = std::collections::HashMap::new();
        let mut deaths = std::collections::HashMap::new();
        let mut usage_in_calls = std::collections::HashMap::new();
        let mut unconditional_frees = std::collections::HashSet::new();
        let mut conditional_frees = std::collections::HashMap::new();

        let comment_query_str = "(comment) @comment";
        let comment_query = Query::new(&language.into(), comment_query_str).unwrap();
        let mut comment_cursor = QueryCursor::new();
        let mut comment_matches = comment_cursor.matches(&comment_query, body_node, code.as_bytes());

        while let Some(cm) = comment_matches.next() {
            let comment_text = cm.captures[0].node.utf8_text(code.as_bytes()).unwrap();
            if comment_text.contains("@Venom:Owns") {
                if let Some(start) = comment_text.find('(') {
                    if let Some(end) = comment_text.find(')') {
                        let var_name = comment_text[start+1..end].trim().to_string();
                        let line = cm.captures[0].node.start_position().row + 1;
                        deaths.insert(var_name.clone(), (line, MemoryEventKind::ExplicitMove));
                        events.push(MemoryEvent {
                            kind: MemoryEventKind::ExplicitMove,
                            variable: var_name,
                            line,
                            context: format!("Ownership transferred via annotation in {}", func_name),
                        });
                    }
                }
            }
        }

        let usage_query_str = "(identifier) @usage";
        let usage_query = Query::new(&language.into(), usage_query_str).unwrap();
        let mut usage_cursor = QueryCursor::new();
        let mut usage_matches = usage_cursor.matches(&usage_query, body_node, code.as_bytes());
        while let Some(um) = usage_matches.next() {
            let var_name = um.captures[0].node.utf8_text(code.as_bytes()).unwrap().to_string();
            let line = um.captures[0].node.start_position().row + 1;
            usages.entry(var_name).or_insert_with(Vec::new).push(line);
        }

        let alloc_query_str = r#"
            (assignment_expression
                left: [
                    (identifier) @var
                    (pointer_declarator declarator: (identifier) @var)
                ]
                right: (call_expression
                    function: (identifier) @func
                    arguments: (argument_list)
                    (#match? @func "^(malloc|calloc|realloc)$")
                )
            )
            (init_declarator
                declarator: [
                    (identifier) @var
                    (pointer_declarator declarator: (identifier) @var)
                ]
                value: (call_expression
                    function: (identifier) @func
                    arguments: (argument_list)
                    (#match? @func "^(malloc|calloc|realloc)$")
                )
            )
        "#;
        let alloc_query = Query::new(&language.into(), alloc_query_str).unwrap();
        let mut alloc_cursor = QueryCursor::new();
        let mut alloc_matches = alloc_cursor.matches(&alloc_query, body_node, code.as_bytes());

        while let Some(am) = alloc_matches.next() {
            let var_name = am.captures[0].node.utf8_text(code.as_bytes()).unwrap().to_string();
            let line = am.captures[0].node.start_position().row + 1;
            allocations.insert(var_name.clone(), line);
            events.push(MemoryEvent {
                kind: MemoryEventKind::Allocation,
                variable: var_name,
                line,
                context: format!("Allocated in {}", func_name),
            });
        }

        let call_query_str = r#"
            (call_expression
                function: (identifier) @func
                arguments: (argument_list (identifier) @var)
            ) @call
        "#;
        let call_query = Query::new(&language.into(), call_query_str).unwrap();
        let mut call_cursor = QueryCursor::new();
        let mut call_matches = call_cursor.matches(&call_query, body_node, code.as_bytes());

        while let Some(cm) = call_matches.next() {
            let call_node = cm.captures[0].node;
            let func_called = cm.captures[1].node.utf8_text(code.as_bytes()).unwrap().to_string();
            let var_name = cm.captures[2].node.utf8_text(code.as_bytes()).unwrap().to_string();
            let line = call_node.start_position().row + 1;
            
            if func_called == "free" {
                if let Some((death_line, _)) = deaths.get(&var_name) {
                    findings.push(format!("CRITICAL: Double Free of '{}' in {} at line {} (previously freed at line {})", var_name, func_name, line, death_line));
                    events.push(MemoryEvent {
                        kind: MemoryEventKind::DoubleFree,
                        variable: var_name.clone(),
                        line,
                        context: format!("Variable '{}' freed again!", var_name),
                    });
                    continue;
                }

                let mut is_conditional = false;
                let mut parent = call_node.parent();
                while let Some(p) = parent {
                    if p.kind() == "if_statement" {
                        is_conditional = true;
                        break;
                    }
                    if p.kind() == "compound_statement" && p.parent().map(|pp| pp.kind() == "function_definition").unwrap_or(false) {
                        break;
                    }
                    parent = p.parent();
                }

                if is_conditional {
                    conditional_frees.entry(var_name.clone()).or_insert_with(Vec::new).push(line);
                    events.push(MemoryEvent {
                        kind: MemoryEventKind::ConditionalFree,
                        variable: var_name,
                        line,
                        context: format!("Freed inside branch in {}", func_name),
                    });
                } else {
                    unconditional_frees.insert(var_name.clone());
                    deaths.insert(var_name.clone(), (line, MemoryEventKind::Free));
                    events.push(MemoryEvent {
                        kind: MemoryEventKind::Free,
                        variable: var_name,
                        line,
                        context: format!("Unconditionally freed in {}", func_name),
                    });
                }
            } else {
                usage_in_calls.entry(var_name).or_insert_with(Vec::new).push((func_called, line));
            }
        }

        for (var, alloc_line) in allocations {
            if let Some(&(death_line, _)) = deaths.get(&var) {
                if let Some(usage_lines) = usages.get(&var) {
                    for &u_line in usage_lines {
                        if u_line > death_line {
                            findings.push(format!("CRITICAL: Use-After-Free of '{}' at line {} (freed/moved at line {})", var, u_line, death_line));
                            events.push(MemoryEvent {
                                kind: MemoryEventKind::UseAfterFree,
                                variable: var.clone(),
                                line: u_line,
                                context: format!("Accessed variable '{}' after it was freed/moved", var),
                            });
                        }
                    }
                }
            }

            if unconditional_frees.contains(&var) || deaths.contains_key(&var) {
                continue;
            }

            if let Some(free_lines) = conditional_frees.get(&var) {
                findings.push(format!("⚠️  Warning (70%): variable '{}' (line {}) is only freed conditionally at line(s) {}; potential leak in other paths", var, alloc_line, free_lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(", ")));
                continue;
            }

            if let Some(funcs_with_lines) = usage_in_calls.get(&var) {
                let mut matched_heuristics = Vec::new();
                for (f, l) in funcs_with_lines {
                    let f_low = f.to_lowercase();
                    if owning_keywords.iter().any(|kw| f_low.contains(kw)) {
                        matched_heuristics.push(f.clone());
                        events.push(MemoryEvent {
                            kind: MemoryEventKind::PotentialMove,
                            variable: var.clone(),
                            line: *l,
                            context: format!("Heuristic match: variable passed to {}", f),
                        });
                    }
                }

                if !matched_heuristics.is_empty() {
                    findings.push(format!("⚠️  Warning (50%): variable '{}' (line {}) might have transferred ownership to {}", var, alloc_line, matched_heuristics.join(", ")));
                } else {
                    let funcs_only: Vec<_> = funcs_with_lines.iter().map(|(f, _)| f.as_str()).collect();
                    findings.push(format!("Potential leak in {}: variable '{}' (line {}) is passed to {} but never freed; likely a borrow leak", func_name, var, alloc_line, funcs_only.join(", ")));
                }
            } else {
                findings.push(format!("Potential leak in {}: variable '{}' allocated at line {} is never freed in the same scope", func_name, var, alloc_line));
            }
        }
    }

    Ok(LeakReport {
        success: findings.is_empty(),
        findings,
        events,
        file_path: path.to_string_lossy().to_string(),
    })
}
