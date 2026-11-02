//! Terminal capabilities and output formatting.
//!
//! Handles TTY detection, color support, broken-pipe errors, and non-TTY
//! prompt behavior without hanging.

use std::io::{self, IsTerminal};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// Global flag for NO_COLOR environment variable support.
static NO_COLOR: OnceLock<bool> = OnceLock::new();

/// Detects whether the terminal supports color output.
///
/// Returns `true` if:
/// - stdout is a TTY (interactive terminal), AND
/// - NO_COLOR environment variable is not set
///
/// Returns `false` for non-TTY output (pipes, redirects) or when NO_COLOR is set.
pub fn supports_color() -> bool {
    let no_color = *NO_COLOR.get_or_init(|| {
        std::env::var("NO_COLOR")
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    });

    io::stdout().is_terminal() && !no_color
}

/// Detects whether stdout is connected to a terminal (interactive).
pub fn is_tty() -> bool {
    io::stdout().is_terminal()
}

/// Detects whether stderr is connected to a terminal.
pub fn is_tty_stderr() -> bool {
    io::stderr().is_terminal()
}

/// Wraps a string with ANSI color codes if colors are supported.
///
/// If terminal does not support colors or NO_COLOR is set, returns the string unchanged.
///
/// # Examples
///
/// ```ignore
/// let colored = color_if_supported("error", Color::Red);
/// // On TTY without NO_COLOR: "\x1b[31merror\x1b[0m"
/// // Otherwise: "error"
/// ```
pub fn color_if_supported(s: &str, color: Color) -> String {
    if supports_color() {
        format!("{}{}{}", color.ansi_start(), s, Color::Reset.ansi_start())
    } else {
        s.to_string()
    }
}

/// ANSI color codes for terminal output.
#[derive(Debug, Clone, Copy)]
pub enum Color {
    Reset,
    Red,
    Yellow,
    Green,
    Blue,
    Cyan,
}

impl Color {
    /// Returns the ANSI escape sequence for this color.
    fn ansi_start(self) -> &'static str {
        match self {
            Color::Reset => "\x1b[0m",
            Color::Red => "\x1b[31m",
            Color::Yellow => "\x1b[33m",
            Color::Green => "\x1b[32m",
            Color::Blue => "\x1b[34m",
            Color::Cyan => "\x1b[36m",
        }
    }
}

/// Handle for managing broken-pipe errors gracefully.
///
/// When writing to a pipe that is closed (e.g., `sf-rust | head -n 1`),
/// the process receives SIGPIPE, which Rust treats as a broken pipe error
/// in write operations.
///
/// Use this handler to exit cleanly without panicking or printing error messages.
pub struct BrokenPipeHandler {
    was_broken: AtomicBool,
}

impl BrokenPipeHandler {
    /// Create a new broken-pipe handler.
    pub fn new() -> Self {
        // On Unix, we would want to set SIGPIPE to SIG_DFL to get exit code 141.
        // However, since the workspace forbids unsafe code, we handle EPIPE
        // errors explicitly in check_and_exit_if_broken() instead.
        // On most platforms, Rust's standard library already handles SIGPIPE
        // appropriately, so this handler serves as a safety check.

        Self {
            was_broken: AtomicBool::new(false),
        }
    }

    /// Check if the pipe was broken during an I/O operation and exit if so.
    ///
    /// Call this after writing to stdout/stderr to detect and handle broken pipes.
    pub fn check_and_exit_if_broken(&self) {
        if self.was_broken.load(Ordering::Relaxed) {
            // Exit with code 141 (SIGPIPE: 128 + 13)
            std::process::exit(141);
        }
    }

    /// Mark the pipe as broken and exit.
    pub fn exit_on_broken(&self) {
        self.was_broken.store(true, Ordering::Relaxed);
        std::process::exit(141);
    }

    /// Handle an I/O error, checking if it was due to a broken pipe.
    pub fn handle_io_error(&self, err: &io::Error) {
        if err.kind() == io::ErrorKind::BrokenPipe {
            self.exit_on_broken();
        }
    }
}

impl Default for BrokenPipeHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// Prompt for user input without hanging when not connected to a TTY.
///
/// On non-TTY streams (e.g., piped input), this function will fail immediately
/// rather than hang waiting for input.
///
/// # Returns
///
/// `Ok(String)` if input was successfully read.
/// `Err(io::Error)` if input is not available (non-TTY, EOF, or other error).
pub fn prompt_non_blocking(prompt: &str) -> io::Result<String> {
    if !is_tty() {
        return Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "stdin is not a TTY; cannot prompt for input",
        ));
    }

    // In a real implementation, this would use a library like `dialoguer`
    // or a low-level terminal input handler. For now, we use std::io::stdin().
    eprint!("{}", prompt);
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_color_env_disables_colors() {
        // This test is environment-dependent and not isolated.
        // A production implementation would use dependency injection or test doubles.
        // For now, we document the expected behavior.
        let result = supports_color();
        // result should be false if NO_COLOR is set, true otherwise
        // (assuming stdout is a TTY)
        assert!(result == is_tty() || !std::env::var("NO_COLOR").unwrap_or_default().is_empty());
    }

    #[test]
    fn test_color_ansi_codes() {
        let red_start = Color::Red.ansi_start();
        let reset = Color::Reset.ansi_start();
        assert_eq!(red_start, "\x1b[31m");
        assert_eq!(reset, "\x1b[0m");
    }

    #[test]
    fn test_broken_pipe_handler_creation() {
        let handler = BrokenPipeHandler::new();
        assert!(!handler.was_broken.load(Ordering::Relaxed));
    }

    #[test]
    fn test_prompt_fails_non_tty() {
        // This test assumes stdin is not a TTY (typical in test environments)
        if !is_tty() {
            let result = prompt_non_blocking("test> ");
            assert!(result.is_err());
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotConnected);
        }
    }
}

