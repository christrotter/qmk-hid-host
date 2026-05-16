// ============================================================================
// CHROME TAB DETECTION MODULE - Added for Chrome tab tracking
// This module provides functionality to detect the active Chrome tab using
// AppleScript. It executes AppleScript commands to query Chrome's active
// tab URL and title.
// ============================================================================

use std::process::Command;
use std::time::{Duration, Instant};

/// Represents information about the currently active Chrome tab
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromeTabInfo {
    pub url: String,
    pub title: String,
    // ========================================================================
    // DOMAIN EXTRACTION - Added domain field for stable identification
    // Contains subdomain.domain.tld (e.g., "www.google.com")
    // ========================================================================
    pub domain: String,
    // ========================================================================
}

/// Result of attempting to get Chrome tab information
pub type ChromeTabResult = Result<ChromeTabInfo, ChromeTabError>;

#[derive(Debug)]
pub enum ChromeTabError {
    ChromeNotRunning,
    NoActiveTab,
    ExecutionError(String),
}

// ============================================================================
// DOMAIN EXTRACTION - Extract domain from URL
// Parses a URL and extracts the subdomain.domain.tld portion
// Examples:
//   "https://www.google.com/search?q=rust" -> "www.google.com"
//   "https://github.com/user/repo" -> "github.com"
//   "http://localhost:3000" -> "localhost"
// ============================================================================
fn extract_domain(url: &str) -> String {
    // Remove protocol (http://, https://, etc.)
    let without_protocol = url.split("://").nth(1).unwrap_or(url);

    // Get everything before the first '/' or '?'
    let domain_with_port = without_protocol
        .split('/')
        .next()
        .unwrap_or(without_protocol)
        .split('?')
        .next()
        .unwrap_or(without_protocol);

    // Remove port if present
    let domain = domain_with_port.split(':').next().unwrap_or(domain_with_port);

    domain.to_string()
}
// ============================================================================

/// Gets the active Chrome tab's URL and title using AppleScript
///
/// This function executes AppleScript to query Chrome's current tab.
/// Returns an error if Chrome is not running or if there's no active tab.
/// OPTIMIZED: Simplified AppleScript for faster execution
pub fn get_active_chrome_tab() -> ChromeTabResult {
    // Optimized AppleScript - only get URL for faster execution
    // We derive domain from URL, so title is not needed for matching
    let script = "tell application \"Google Chrome\" to if it is running then if (count of windows) > 0 then return URL of active tab of front window";

    // Execute the AppleScript with optimized single-line format
    let output = Command::new("osascript")
        .arg("-ss") // Use strict mode for faster parsing
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| ChromeTabError::ExecutionError(format!("Failed to execute osascript: {}", e)))?;

    if !output.status.success() {
        // Check if it's just Chrome not running or no window
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("not running") || stderr.is_empty() {
            return Err(ChromeTabError::ChromeNotRunning);
        }
        return Err(ChromeTabError::ExecutionError(format!("AppleScript execution failed: {}", stderr)));
    }

    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Check for empty result (no windows)
    if result.is_empty() {
        return Err(ChromeTabError::NoActiveTab);
    }

    // We have a URL
    let url = result;
    // Extract domain from URL
    let domain = extract_domain(&url);
    // Use domain as title for simplicity (we don't need the full title for matching)
    let title = domain.clone();

    Ok(ChromeTabInfo { url, title, domain })
}

/// Cached Chrome tab poller that efficiently tracks tab changes
///
/// This struct maintains a cache of the last known tab information
/// and only reports changes when the tab actually changes.
pub struct ChromeTabPoller {
    last_tab: Option<ChromeTabInfo>,
    last_poll_time: Instant,
    poll_interval: Duration,
}

impl ChromeTabPoller {
    /// Creates a new ChromeTabPoller with the specified polling interval
    pub fn new(poll_interval_ms: u64) -> Self {
        Self {
            last_tab: None,
            last_poll_time: Instant::now() - Duration::from_secs(10), // Force first poll
            poll_interval: Duration::from_millis(poll_interval_ms),
        }
    }

    /// Checks if enough time has passed to poll again
    pub fn should_poll(&self) -> bool {
        self.last_poll_time.elapsed() >= self.poll_interval
    }

    /// Polls for the current Chrome tab and returns Some if it has changed
    /// Returns None if the tab hasn't changed or if there was an error
    pub fn poll(&mut self) -> Option<ChromeTabInfo> {
        if !self.should_poll() {
            return None;
        }

        self.last_poll_time = Instant::now();

        match get_active_chrome_tab() {
            Ok(tab_info) => {
                // Check if the tab has changed
                if self.last_tab.as_ref() != Some(&tab_info) {
                    // ============================================================
                    // DOMAIN EXTRACTION - Clean logging with domain at info level
                    // ============================================================
                    tracing::debug!("Chrome tab changed: {}", tab_info.domain);
                    tracing::debug!("Chrome tab URL: {}", tab_info.url);
                    // ============================================================
                    self.last_tab = Some(tab_info.clone());
                    Some(tab_info)
                } else {
                    None
                }
            }
            Err(e) => {
                // Only log if we previously had a tab (to avoid spam when Chrome closes)
                if self.last_tab.is_some() {
                    tracing::debug!("Chrome tab detection error: {:?}", e);
                    self.last_tab = None;
                }
                None
            }
        }
    }

    /// Resets the cached tab information
    pub fn reset(&mut self) {
        self.last_tab = None;
        self.last_poll_time = Instant::now() - Duration::from_secs(10); // Force next poll
    }
}

// ============================================================================
// END OF CHROME TAB DETECTION MODULE
// ============================================================================
