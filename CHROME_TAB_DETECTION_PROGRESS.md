# Chrome Tab Detection Implementation Progress

This document tracks the implementation progress of Chrome tab detection capability.

## Iteration 1: Initial AppleScript Implementation

**Started:** 2026-02-14

**Goal:** Add the ability to detect which Chrome tab is active using AppleScript when Chrome is the focused application.

**Approach:**
- Use AppleScript to query Chrome's active tab URL and title
- Poll for tab changes every 250-500ms while Chrome is focused
- Integrate with existing NSWorkspace notification system
- Cache last result to detect changes efficiently

**Tasks:**
- [ ] Create AppleScript execution module
- [ ] Integrate Chrome tab polling into AppSenseProvider
- [ ] Update command structure to include tab information
- [ ] Test implementation

**Changes Made:**

1. **Created `chrome_tab.rs` module** (`src/providers/app_sense/chrome_tab.rs`)
   - Added `ChromeTabInfo` struct to represent tab URL and title
   - Implemented `get_active_chrome_tab()` function using AppleScript via `osascript`
   - Created `ChromeTabPoller` for efficient tab change detection with caching
   - Polling interval set to 250ms to balance responsiveness and CPU usage

2. **Updated `app_sense.rs`**
   - Added `mod chrome_tab` declaration to expose the module

3. **Updated `macos.rs` AppSenseProvider**
   - Added `chrome_focused: Arc<Mutex<bool>>` field to track Chrome focus state
   - Added global pointer `CHROME_FOCUSED_PTR` for callback access
   - Modified `handle_notification` callback to set/clear `chrome_focused` flag
   - Integrated `ChromeTabPoller` in main event loop
   - Added polling logic that only runs when Chrome is focused
   - Created `create_chrome_tab_command()` helper function

4. **Command Structure for Chrome Tabs**
   - Format: `[186, 206, 2, 2, hash[8 bytes], title[20 bytes]]`
   - Bytes 0-1: Provider ID (0xBACE = 186, 206)
   - Byte 2: Command type (2 = Chrome tab info)
   - Byte 3: App code (2 = Chrome)
   - Bytes 4-11: URL hash (8-byte u64 for unique identification)
   - Bytes 12-31: First 20 chars of title (UTF-8, zero-padded)

**Testing Results:**
Build test pending - cargo not available in current environment.
Manual testing required by user.

**How It Works:**

1. NSWorkspace notification fires when app focus changes
2. If Chrome gains focus, `chrome_focused` flag is set to `true`
3. Main loop polls Chrome every 250ms (via AppleScript) while focused
4. When tab URL/title changes, `ChromeTabPoller` detects it
5. New command is sent to device with tab hash and title
6. When Chrome loses focus, polling stops and poller resets

**Notes:**

- All new code is clearly marked with comments starting with "CHROME TAB DETECTION"
- AppleScript execution has ~10-50ms latency per call
- Polling is only active when Chrome is focused (minimal CPU impact)
- URL is hashed to 8 bytes for unique identification without transmitting full URL
- Title is truncated to 20 bytes to fit in 32-byte command structure
- Implementation is non-intrusive and can be easily extended or disabled

---

## Iteration 2: Fix Mutex Crash Bug

**Started:** 2026-02-14 (shortly after iteration 1)

**Issue:**
Application crashed when switching apps with error:
```
thread 'main' (1596203) panicked at library/std/src/sys/pal/unix/sync/mutex.rs:69:13:
failed to lock mutex: Invalid argument (os error 22)
```

**Root Cause:**
The `CHROME_FOCUSED_PTR` global pointer was pointing to a stack-allocated `Arc<Mutex<bool>>` that was cloned in the `start()` method. When `start()` returned, the stack frame was invalidated, but the pointer was still being dereferenced in the NSWorkspace notification callback, causing undefined behavior and mutex corruption.

**Solution:**
- Removed `CHROME_FOCUSED_PTR` global pointer entirely
- Access `chrome_focused` through the existing `ACTIVE_APP_PROVIDER_PTR` pointer
- Changed from `&*CHROME_FOCUSED_PTR` to `(&*ACTIVE_APP_PROVIDER_PTR).chrome_focused`
- This ensures we're always accessing valid memory since `self` lives as long as the provider

**Changes Made:**
1. Removed `static mut CHROME_FOCUSED_PTR` declaration
2. Removed code that set `CHROME_FOCUSED_PTR` in `start()`
3. Updated all callback handlers to access `chrome_focused` via provider pointer:
   - Code app handler
   - Fusion app handler
   - Google Chrome app handler
   - KiCad app handler
   - Default ("Other") app handler
4. Removed `CHROME_FOCUSED_PTR` cleanup in `stop()`

**Testing:**
✅ Verified - no longer crashes when switching apps.

---

## Iteration 3: Switch from Title to Domain Name

**Started:** 2026-02-14 (after iteration 2)

**Issue:**
Tab titles change frequently and aren't useful for stable identification. Titles can be long, dynamic, and provide inconsistent identification of the actual site being visited.

**Solution:**
Extract and use the domain name (subdomain.domain.tld) from the URL instead of the page title. Domains are:
- More stable (change less frequently than titles)
- Shorter (fit better in the 20-byte command space)
- Better identifiers for which website/service is active
- More useful for automation and device logic

**Changes Made:**

1. **Updated `chrome_tab.rs`:**
   - Added `extract_domain()` function to parse URLs
   - Handles protocol removal, path/query stripping, port removal
   - Examples:
     - `https://www.google.com/search?q=rust` → `www.google.com`
     - `https://github.com/user/repo` → `github.com`
     - `http://localhost:3000` → `localhost`
   - Added `domain: String` field to `ChromeTabInfo` struct
   - Updated `get_active_chrome_tab()` to extract and populate domain
   - Updated logging to show domain instead of title

2. **Updated `macos.rs`:**
   - Updated `create_chrome_tab_command()` documentation
   - Changed parameter from `title: &str` to `domain: &str`
   - Updated command to send domain bytes instead of title bytes
   - Updated call site to pass `tab_info.domain` instead of `tab_info.title`
   - Updated logging to show domain

**Command Structure (Updated):**
```
[186, 206, 2, 2, hash[8 bytes], domain[20 bytes]]
```
- Bytes 0-1: Provider ID (0xBACE = 186, 206)
- Byte 2: Command type (2 = Chrome tab info)
- Byte 3: App code (2 = Chrome)
- Bytes 4-11: URL hash (8 bytes, u64)
- Bytes 12-31: Domain name (e.g., "www.google.com")

**Example Outputs:**
- `www.google.com`
- `github.com`
- `stackoverflow.com`
- `mail.google.com`
- `docs.google.com`

**Benefits:**
- Stable identification (domains rarely change compared to page titles)
- Fits well in 20-byte space (most domains are < 20 characters)
- Useful for website-specific automation
- Clear, human-readable identification

**Testing:**
Ready for user testing with domain extraction.

---

## Iteration 4: Clean Up Logging

**Started:** 2026-02-14 (after iteration 3)

**Issue:**
The logging output was cluttered with both domain and full URL at the info level:
```
Chrome tab changed: docs.google.com (https://docs.google.com/document/d/abc123/edit)
```

**Solution:**
- Keep info-level logs clean and concise with just the domain
- Move full URL to debug-level logging for troubleshooting

**Changes Made:**

1. **Updated `chrome_tab.rs`:**
   - Changed from single info log with domain and URL to:
     - Info level: `"Chrome tab changed: {domain}"`
     - Debug level: `"Chrome tab URL: {url}"`

2. **Updated `macos.rs`:**
   - Same logging split for consistency
   - Added clear comment explaining the logging strategy

**New Log Output:**

Info level (default):
```
Chrome tab changed: docs.google.com
Chrome tab changed: github.com
Chrome tab changed: mail.google.com
```

Debug level (when enabled):
```
Chrome tab changed: docs.google.com
Chrome tab URL: https://docs.google.com/document/d/abc123/edit
Chrome tab changed: github.com
Chrome tab URL: https://github.com/user/repo/pull/42
```

**Benefits:**
- Cleaner console output during normal operation
- Full URL still available when debugging is enabled
- Easy to scan and understand what sites are being visited

**Testing:**
Ready for user testing with clean logging.

---
