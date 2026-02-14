#[cfg(target_os = "macos")]
mod macos;

// ============================================================================
// CHROME TAB DETECTION MODULE - Added for Chrome tab tracking
// ============================================================================
#[cfg(target_os = "macos")]
mod chrome_tab;
// ============================================================================

#[cfg(target_os = "macos")]
pub use self::macos::AppSenseProvider;
