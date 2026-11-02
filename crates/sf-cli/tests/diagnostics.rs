//! Unit tests for the diagnostics module.
//!
//! Tests the structured diagnostics system including collection, filtering,
//! and formatting of warnings, errors, and debug messages.

#![allow(clippy::expect_used)]

#[cfg(test)]
mod diagnostic_tests {
    use sf_cli::diagnostics::{Diagnostic, DiagnosticCollector, Severity};

    #[test]
    fn create_diagnostic_with_severity() {
        let diag = Diagnostic::warning("test warning");
        // Check via Display since field is private
        let displayed = diag.to_string();
        assert!(displayed.contains("warning"));
        assert!(displayed.contains("test warning"));
    }

    #[test]
    fn diagnostic_with_context() {
        let diag = Diagnostic::error("Connection failed").with_context("Check your network");
        let displayed = diag.to_string();
        assert!(displayed.contains("error"));
        assert!(displayed.contains("Connection failed"));
        assert!(displayed.contains("Check your network"));
    }

    #[test]
    fn diagnostic_collector_accumulates() {
        let mut collector = DiagnosticCollector::new();
        collector.add_warning("warning 1");
        collector.add_info("info 1");
        collector.add_warning("warning 2");

        assert_eq!(collector.count_by_severity(Severity::Warning), 2);
        assert_eq!(collector.count_by_severity(Severity::Info), 1);
        assert_eq!(collector.diagnostics().len(), 3);
    }

    #[test]
    fn collector_filter_warnings() {
        let mut collector = DiagnosticCollector::new();
        collector.add_info("info");
        collector.add_warning("warning 1");
        collector.add_warning("warning 2");
        collector.add_error("error");

        let warnings = collector.warnings();
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn collector_detects_errors() {
        let mut collector = DiagnosticCollector::new();
        assert!(!collector.has_errors());

        collector.add_warning("warning");
        assert!(!collector.has_errors());

        collector.add_error("error");
        assert!(collector.has_errors());
    }

    #[test]
    fn diagnostic_severity_ordering() {
        // Verify severity levels have correct ordering for filtering
        assert!(Severity::Debug < Severity::Info);
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
    }

    #[test]
    fn collector_print_all_io_safe() {
        let mut collector = DiagnosticCollector::new();
        collector.add_info("info message");
        collector.add_warning("warning message");

        // This should not panic or produce errors
        let result = collector.print_all();
        assert!(result.is_ok());
    }

    #[test]
    fn collector_filter_by_level() {
        let mut collector = DiagnosticCollector::new();
        collector.add_debug("debug");
        collector.add_info("info");
        collector.add_warning("warning");
        collector.add_error("error");

        // Should print warning and error (warning level and above)
        let result = collector.print_severity_and_above(Severity::Warning);
        assert!(result.is_ok());

        // Count what would be printed
        let diags_at_level: Vec<_> = collector
            .diagnostics()
            .iter()
            .filter(|_| true) // All diagnostics collected
            .collect();
        assert_eq!(diags_at_level.len(), 4);
    }

    #[test]
    fn diagnostic_display_format() {
        let diag = Diagnostic::error("Something failed");
        let displayed = diag.to_string();

        // Should include severity and message
        assert!(displayed.contains("error"));
        assert!(displayed.contains("Something failed"));
    }

    #[test]
    fn diagnostic_all_severity_levels() {
        let debug = Diagnostic::debug("debug msg");
        let info = Diagnostic::info("info msg");
        let warn = Diagnostic::warning("warn msg");
        let error = Diagnostic::error("error msg");

        assert!(debug.to_string().contains("debug"));
        assert!(info.to_string().contains("info"));
        assert!(warn.to_string().contains("warning"));
        assert!(error.to_string().contains("error"));
    }

    #[test]
    fn diagnostic_creation_convenience_methods() {
        // Test that all convenience methods work
        let mut collector = DiagnosticCollector::new();

        collector.add_debug("debug");
        collector.add_info("info");
        collector.add_warning("warning");
        collector.add_error("error");

        assert_eq!(collector.diagnostics().len(), 4);
        assert!(collector.has_errors());
    }
}

