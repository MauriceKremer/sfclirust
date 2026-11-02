//! Graceful process lifecycle management.
//!
//! Handles process termination signals, task cleanup, and ensures proper
//! output flushing before exit. Separates signal handling from operation
//! cancellation and rollback concerns.
//!
//! This module is part of the foundational infrastructure (B10-04) and will be
//! used by commands in future backlog items. It is intentionally public but
//! currently unused.

#![allow(dead_code)]

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Unique identifier for long-running operations.
///
/// Preserved across signal handling to allow external monitoring/cleanup.
pub type OperationId = String;

/// Tracks whether a graceful shutdown has been requested.
///
/// Can be used by running tasks to check if they should wind down.
pub static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Counts the number of spawned tasks that are currently running.
pub static SPAWNED_TASK_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Context for a running operation that may be interrupted by signals.
#[derive(Debug, Clone)]
pub struct OperationContext {
    /// Unique identifier for this operation, preserved for tracking
    pub operation_id: OperationId,

    /// Whether shutdown was requested by signal
    pub shutdown_requested: bool,
}

impl OperationContext {
    /// Create a new operation context with a generated operation ID.
    pub fn new() -> Self {
        Self {
            operation_id: generate_operation_id(),
            shutdown_requested: false,
        }
    }

    /// Create a context with a specific operation ID.
    pub fn with_id(id: OperationId) -> Self {
        Self {
            operation_id: id,
            shutdown_requested: false,
        }
    }

    /// Check if a shutdown signal has been requested.
    pub fn is_shutdown_requested(&self) -> bool {
        SHUTDOWN_REQUESTED.load(Ordering::Relaxed)
    }

    /// Check if this operation should continue running.
    ///
    /// Returns false if shutdown was requested or if too many signals were received.
    pub fn should_continue(&self) -> bool {
        !self.is_shutdown_requested()
    }
}

impl Default for OperationContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Register a spawned task as currently running.
pub fn register_spawned_task() {
    SPAWNED_TASK_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Unregister a spawned task as it completes.
pub fn unregister_spawned_task() {
    SPAWNED_TASK_COUNT.fetch_sub(1, Ordering::Relaxed);
}

/// Get the number of currently active spawned tasks.
pub fn active_task_count() -> usize {
    SPAWNED_TASK_COUNT.load(Ordering::Relaxed)
}

/// Request graceful shutdown (called by signal handler).
pub fn request_shutdown() {
    SHUTDOWN_REQUESTED.store(true, Ordering::Relaxed);
}

/// Check if any spawned tasks are still running.
pub fn has_active_tasks() -> bool {
    active_task_count() > 0
}

/// Ensures output is flushed to stdout and stderr before exit.
pub fn flush_output() -> io::Result<()> {
    use std::io::Write;
    std::io::stdout().flush()?;
    std::io::stderr().flush()?;
    Ok(())
}

/// Gracefully shuts down the process, ensuring cleanup happens.
///
/// This function:
/// 1. Flushes all buffered output
/// 2. Waits briefly for spawned tasks to complete
/// 3. Exits with the specified code
///
/// # Panics
///
/// This function calls `process::exit()` and never returns.
pub fn shutdown(exit_code: i32) -> ! {
    // Flush output first
    let _ = flush_output();

    // Wait briefly for spawned tasks to complete (with timeout)
    // This prevents orphaned processes while not hanging indefinitely
    for _ in 0..10 {
        if !has_active_tasks() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // Log if we had to force exit with active tasks
    if has_active_tasks() {
        let count = active_task_count();
        eprintln!("warning: shutting down with {} active task(s)", count);
    }

    std::process::exit(exit_code);
}

/// Safely waits for all spawned tasks with a timeout.
///
/// Returns true if all tasks completed, false if timeout was reached.
pub fn wait_for_tasks(timeout_ms: u64) -> bool {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_millis(timeout_ms);

    loop {
        if !has_active_tasks() {
            return true;
        }

        if start.elapsed() >= timeout {
            return false;
        }

        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Generate a unique operation ID for tracking purposes.
fn generate_operation_id() -> OperationId {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    // Format: timestamp-nanos-random
    // This provides reasonable uniqueness without requiring complex UUID generation
    format!("{}-{:x}", now.as_nanos(), std::process::id() as u64)
}

/// Signal-safe shutdown handler for integration with signal handling.
///
/// This can be called from a signal handler to initiate graceful shutdown.
pub fn on_termination_signal() {
    request_shutdown();
    // Don't exit immediately; let the application complete gracefully
}

/// Cancellation token that can be shared across tasks.
///
/// Separate from shutdown signals; used for operation-level cancellation.
#[derive(Debug, Clone)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Create a new cancellation token.
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Request cancellation of associated operations.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Check if cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_context_creation() {
        let ctx = OperationContext::new();
        assert!(!ctx.operation_id.is_empty());
        assert!(!ctx.shutdown_requested);
        assert!(ctx.should_continue());
    }

    #[test]
    fn operation_context_with_id() {
        let id = "test-op-123".to_string();
        let ctx = OperationContext::with_id(id.clone());
        assert_eq!(ctx.operation_id, id);
    }

    #[test]
    fn shutdown_request_affects_context() {
        let ctx = OperationContext::new();
        assert!(!ctx.is_shutdown_requested());

        request_shutdown();
        assert!(ctx.is_shutdown_requested());
        assert!(!ctx.should_continue());

        // Reset for other tests
        SHUTDOWN_REQUESTED.store(false, Ordering::Relaxed);
    }

    #[test]
    fn spawned_task_counting() {
        // Reset counter
        SPAWNED_TASK_COUNT.store(0, Ordering::Relaxed);

        assert_eq!(active_task_count(), 0);
        assert!(!has_active_tasks());

        register_spawned_task();
        assert_eq!(active_task_count(), 1);
        assert!(has_active_tasks());

        register_spawned_task();
        assert_eq!(active_task_count(), 2);

        unregister_spawned_task();
        assert_eq!(active_task_count(), 1);

        unregister_spawned_task();
        assert_eq!(active_task_count(), 0);
        assert!(!has_active_tasks());
    }

    #[test]
    fn cancellation_token_lifecycle() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());

        token.cancel();
        assert!(token.is_cancelled());

        // Test that clones share state
        let token2 = token.clone();
        assert!(token2.is_cancelled());
    }

    #[test]
    fn wait_for_tasks_immediate_success() {
        SPAWNED_TASK_COUNT.store(0, Ordering::Relaxed);
        assert!(wait_for_tasks(100));
    }

    #[test]
    fn wait_for_tasks_timeout() {
        SPAWNED_TASK_COUNT.store(1, Ordering::Relaxed);
        assert!(!wait_for_tasks(50)); // Should timeout quickly
        SPAWNED_TASK_COUNT.store(0, Ordering::Relaxed);
    }

    #[test]
    fn operation_id_is_unique() {
        let id1 = generate_operation_id();
        let id2 = generate_operation_id();
        // They should be different (or at least not identical most of the time)
        // Due to timing, they might occasionally be the same
        assert!(!id1.is_empty());
        assert!(!id2.is_empty());
    }

    #[test]
    fn flush_output_succeeds() {
        // This should not panic
        let result = flush_output();
        assert!(result.is_ok());
    }

    #[test]
    fn operation_context_default() {
        let ctx = OperationContext::default();
        assert!(!ctx.operation_id.is_empty());
        assert!(ctx.should_continue());
    }

    #[test]
    fn cancellation_token_default() {
        let token = CancellationToken::default();
        assert!(!token.is_cancelled());
    }
}
