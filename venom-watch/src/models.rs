use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub type_name: String,
    pub size: usize,
    pub offset: usize,
    pub is_array: bool,
    pub array_len: usize,
    pub line: usize,
    pub is_pointer: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StructLayout {
    pub name: String,
    pub fields: Vec<Field>,
    pub total_size: usize,
    pub file_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumMember {
    pub name: String,
    pub value: i64,
    pub line: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EnumLayout {
    pub name: String,
    pub members: Vec<EnumMember>,
    pub file_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ValidationResult {
    pub success: bool,
    pub server_size: usize,
    pub client_size: usize,
    pub issues: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum MemoryEventKind {
    Allocation,
    Free,
    PotentialMove, // Heuristic
    ExplicitMove,  // Annotation
    ConditionalFree,
    UseAfterFree,
    DoubleFree,
    BufferOverflow, // Reserved for future use
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MemoryEvent {
    pub kind: MemoryEventKind,
    pub variable: String,
    pub line: usize,
    pub context: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LeakReport {
    pub success: bool,
    pub findings: Vec<String>,
    pub events: Vec<MemoryEvent>,
    pub file_path: String,
}
