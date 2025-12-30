pub mod models;
pub mod analysis;

pub use models::*;
pub use analysis::layout::{analyze_file, analyze_enum};
pub use analysis::engine::check_leaks;
pub use analysis::overflow::check_overflows;

use std::path::PathBuf;

pub fn run_safety_analysis(path: &PathBuf) -> Result<LeakReport, String> {
    let mut report = check_leaks(path)?;
    if let Ok(overflow_events) = check_overflows(path.clone()) {
        for event in overflow_events {
            report.findings.push(event.context.clone());
            report.events.push(event);
            report.success = false;
        }
    }
    Ok(report)
}
