# Chrome Tab Detection Options

This document outlines the various approaches considered for detecting which Chrome browser tab is currently active on macOS.

## Requirements
- **Low Latency**: Minimal delay between tab change and detection
- **High Reliability**: Consistent operation without failures
- **No Manual Configuration**: Should work out-of-the-box for users

---

## Option 1: AppleScript/JXA ⭐ (Selected)

**Reliability:** ★★★★★
**Latency:** ★★★★☆
**Consistency:** ★★★★★

### Overview
Chrome officially exposes its window and tab information through AppleScript. We can directly query the active tab's URL and title using the macOS scripting bridge.

### How It Works
```applescript
tell application "Google Chrome"
    get URL of active tab of front window
    get title of active tab of front window
end tell
```

### Implementation Approach
1. When NSWorkspace detects Chrome is focused, immediately query active tab
2. Poll for tab changes every 250-500ms while Chrome remains focused
3. Cache last result to detect changes efficiently
4. Stop polling when Chrome loses focus

### Pros
- ✅ Official Chrome API support
- ✅ Very reliable and consistent
- ✅ ~10-50ms latency when cached properly
- ✅ Works without any Chrome configuration changes
- ✅ Can get URL, title, and other tab metadata
- ✅ Simple implementation that fits existing architecture

### Cons
- ❌ Requires spawning a process for each query (~10ms overhead)
- ❌ Slightly higher latency than native notifications
- ❌ Polling-based rather than event-driven

### Dependencies
- `osascript` (built into macOS)
- No additional Rust crates needed

---

## Option 2: macOS Accessibility API

**Reliability:** ★★★★☆
**Latency:** ★★★★☆
**Consistency:** ★★★☆☆

### Overview
Use the macOS Accessibility framework (`AXUIElement` APIs) to inspect Chrome's UI hierarchy and extract the active tab's title from window/tab UI elements.

### How It Works
- Query Chrome's window hierarchy using AX APIs
- Navigate to the active tab element
- Extract the tab title from the UI element

### Implementation Approach
1. Use `accessibility-sys` or similar crate for AX bindings
2. Query Chrome's AXWindows
3. Find the focused AXTab element
4. Extract title from AXTitle attribute

### Pros
- ✅ Native macOS API
- ✅ Works with any browser (not Chrome-specific)
- ✅ Can subscribe to UI change notifications (AXFocusedUIElementChanged)
- ✅ No process spawning required

### Cons
- ❌ Requires user to grant Accessibility permissions (security prompt)
- ❌ Tab titles might be truncated in UI
- ❌ More complex to parse UI hierarchy
- ❌ Chrome's UI structure might change between versions
- ❌ Cannot get full URL, only title
- ❌ Higher implementation complexity

### Dependencies
- `accessibility-sys` or similar crate
- Requires Accessibility permissions

---

## Option 3: Chrome Debugging Protocol (CDP)

**Reliability:** ★★★★★
**Latency:** ★★★★★
**Consistency:** ★★★★☆

### Overview
Connect to Chrome's debugging protocol over WebSocket to receive real-time tab information and events.

### How It Works
- Chrome exposes a WebSocket API on port 9222 (when enabled)
- Connect to `/json` endpoint to list all tabs
- Subscribe to Target events for real-time updates
- Query tab information from the protocol

### Implementation Approach
1. Connect to `http://localhost:9222/json`
2. Get list of all tabs and their debugging WebSocket URLs
3. Connect to the active tab's WebSocket
4. Subscribe to `Target.targetInfoChanged` events

### Pros
- ✅ Very low latency (<5ms)
- ✅ Real-time event notifications (not polling)
- ✅ Access to full tab state, network requests, console, etc.
- ✅ Official Chrome protocol
- ✅ Can control tabs programmatically

### Cons
- ❌ Requires Chrome to be launched with `--remote-debugging-port=9222`
- ❌ User must manually enable this flag or app must manage Chrome launch
- ❌ Security implications of having debugging enabled
- ❌ Only one debugger can connect at a time (conflicts with DevTools)
- ❌ Complex WebSocket handling and protocol implementation

### Dependencies
- WebSocket client crate (`tokio-tungstenite`)
- CDP protocol crate or manual implementation

---

## Option 4: Chrome Extension + Native Messaging

**Reliability:** ★★★★★
**Latency:** ★★★★★
**Consistency:** ★★★★★

### Overview
Create a Chrome extension that uses the Native Messaging API to communicate tab changes directly to the native app.

### How It Works
- Chrome extension listens to `chrome.tabs.onActivated` event
- Extension sends tab info to native host via stdin/stdout
- Native host (our app) receives messages in real-time

### Implementation Approach
1. Create a Chrome extension with `tabs` permission
2. Implement native messaging host in our app
3. Extension sends messages when active tab changes
4. App receives messages and processes tab information

### Pros
- ✅ Real-time notifications with minimal latency (<1ms)
- ✅ Most reliable - built into Chrome's event system
- ✅ Can capture tab changes instantly
- ✅ No polling required
- ✅ Full access to tab metadata
- ✅ Works even when Chrome is not focused

### Cons
- ❌ Requires users to install a Chrome extension
- ❌ More complex deployment (extension + native host setup)
- ❌ Extension needs to be published or side-loaded
- ❌ Maintenance overhead for extension updates
- ❌ Requires manifest.json and extension packaging

### Dependencies
- Chrome extension (JavaScript)
- Native messaging host manifest
- JSON message parsing in Rust

---

## Comparison Matrix

| Feature | AppleScript | Accessibility | CDP | Extension |
|---------|-------------|---------------|-----|-----------|
| Setup Required | None | Permissions | Chrome Flag | Extension Install |
| Latency | 10-50ms | 5-20ms | <5ms | <1ms |
| Reliability | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| URL Access | ✅ Full | ❌ No | ✅ Full | ✅ Full |
| Event-Driven | ❌ Polling | ✅ Events | ✅ Events | ✅ Events |
| User Friction | None | Permissions | Manual Setup | Extension |
| Implementation | Simple | Medium | Complex | Complex |

---

## Decision: Option 1 (AppleScript)

### Rationale
For the initial implementation, AppleScript provides the best balance of:
- **Zero user friction** - works out of the box
- **Simple implementation** - fits naturally into existing architecture
- **Good enough performance** - 250ms polling is acceptable for most use cases
- **Official API** - reliable and stable across Chrome versions

### Future Considerations
If lower latency is needed in the future, we can consider:
- **Option 4 (Extension)** for power users who want <1ms latency
- **Option 2 (Accessibility)** as a middle ground with event-driven updates

The AppleScript approach can be easily extended or replaced without affecting the rest of the system.
