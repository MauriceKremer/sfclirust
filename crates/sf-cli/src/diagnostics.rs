//! Structured diagnostics output.
//!
//! Provides utilities for logging warnings, debug information, and progress
//! to stderr without interfering with stdout (which carries JSON/result data).
//!
//! This module is part of the foundational infrastructure (B10-03) and will be
//! used by commands in future backlog items. It is intentionally public but
//! currently unused.

#![allow(dead_code)]

use std::fmt;
use std::io;

/// Severity levels for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Debug,
    Info,
    Warning,
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Debug => write!(f, "debug"),
            Severity::Info => write!(f, "info"),
            Severity::Warning => write!(f, "warning"),
            Severity::Error => write!(f, "error"),
        }
    }
}

/// A diagnostic message with severity and optional context.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    severity: Severity,
    message: String,
    context: Option<String>,
}

impl Diagnostic {
    /// Create a new diagnostic message.
    pub fn new(severity: Severity, message: impl Into<String>) -> Self {
        Self {
            severity,
            message: message.into(),
            context: None,
        }
    }

    /// Create a debug diagnostic.
    pub fn debug(message: impl Into<String>) -> Self {
        Self::new(Severity::Debug, message)
    }

    /// Create an info diagnostic.
    pub fn info(message: impl Into<String>) -> Self {
        Self::new(Severity::Info, message)
    }

    /// Create a warning diagnostic.
    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, message)
    }

    /// Create an error diagnostic.
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(Severity::Error, message)
    }

    /// Add context information (e.g., suggestion, documentation link).
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Write this diagnostic to stderr.
    pub fn print_to_stderr(&self) -> io::Result<()> {
        if let Some(context) = &self.context {
            eprintln!("{}: {}\n  {}", self.severity, self.message, context);
        } else {
            eprintln!("{}: {}", self.severity, self.message);
        }
        Ok(())
    }

    /// Render this diagnostic as a human-readable string.
    pub fn render(&self) -> String {
        if let Some(context) = &self.context {
            format!("{}: {}\n  {}", self.severity, self.message, context)
        } else {
            format!("{}: {}", self.severity, self.message)
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}

/// Diagnostic collector for aggregating warnings and messages during command execution.
///
/// Use this to collect diagnostics during execution and emit them at the end,
/// after the main result has been written.
pub struct DiagnosticCollector {
    diagnostics: Vec<Diagnostic>,
}

impl DiagnosticCollector {
    /// Create a new diagnostic collector.
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    /// Add a diagnostic to the collection.
    pub fn add(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Add a warning.
    pub fn add_warning(&mut self, message: impl Into<String>) {
        self.add(Diagnostic::warning(message));
    }

    /// Add an info message.
    pub fn add_info(&mut self, message: impl Into<String>) {
        self.add(Diagnostic::info(message));
    }

    /// Add an error message.
    pub fn add_error(&mut self, message: impl Into<String>) {
        self.add(Diagnostic::error(message));
    }

    /// Add a debug message.
    pub fn add_debug(&mut self, message: impl Into<String>) {
        self.add(Diagnostic::debug(message));
    }

    /// Get all collected diagnostics.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Get the count of diagnostics at a specific severity level.
    pub fn count_by_severity(&self, severity: Severity) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == severity)
            .count()
    }

    /// Get all warnings.
    pub fn warnings(&self) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect()
    }

    /// Print all collected diagnostics to stderr.
    pub fn print_all(&self) -> io::Result<()> {
        for diagnostic in &self.diagnostics {
            diagnostic.print_to_stderr()?;
        }
        Ok(())
    }

    /// Print diagnostics of a specific severity level or higher.
    pub fn print_severity_and_above(&self, severity: Severity) -> io::Result<()> {
        for diagnostic in &self.diagnostics {
            if diagnostic.severity >= severity {
                diagnostic.print_to_stderr()?;
            }
        }
        Ok(())
    }

    /// Check if there are any diagnostics at Error level or above.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}

impl Default for DiagnosticCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_creation() {
        let diag = Diagnostic::warning("test warning");
        assert_eq!(diag.severity, Severity::Warning);
        assert_eq!(diag.message, "test warning");
        assert_eq!(diag.context, None);
    }

    #[test]
    fn diagnostic_with_context() {
        let diag =
            Diagnostic::error("failed to connect").with_context("Check your network connection");
        assert_eq!(diag.severity, Severity::Error);
        assert_eq!(
            diag.context,
            Some("Check your network connection".to_string())
        );
    }

    #[test]
    fn diagnostic_render() {
        let diag = Diagnostic::info("test message");
        let rendered = diag.render();
        assert!(rendered.contains("info"));
        assert!(rendered.contains("test message"));
    }

    #[test]
    fn collector_add_and_count() {
        let mut collector = DiagnosticCollector::new();
        collector.add_warning("warning 1");
        collector.add_warning("warning 2");
        collector.add_info("info 1");

        assert_eq!(collector.count_by_severity(Severity::Warning), 2);
        assert_eq!(collector.count_by_severity(Severity::Info), 1);
    }

    #[test]
    fn collector_filter_by_severity() {
        let mut collector = DiagnosticCollector::new();
        collector.add_info("info");
        collector.add_warning("warning");
        collector.add_error("error");

        let warnings = collector.warnings();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].message, "warning");
    }

    #[test]
    fn collector_has_errors() {
        let mut collector = DiagnosticCollector::new();
        collector.add_warning("warning");
        assert!(!collector.has_errors());

        collector.add_error("error");
        assert!(collector.has_errors());
    }

    #[test]
    fn severity_ordering() {
        assert!(Severity::Debug < Severity::Info);
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
    }

    #[test]
    fn diagnostic_impl_display() {
        let diag = Diagnostic::warning("test");
        let s = diag.to_string();
        assert!(s.contains("warning"));
        assert!(s.contains("test"));
    }
}
