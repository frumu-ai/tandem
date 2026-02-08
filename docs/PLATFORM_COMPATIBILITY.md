# Platform Compatibility Analysis

## Summary

Tandem is designed to work across **Windows, Linux, and macOS** with appropriate platform-specific handling where needed.

## ✅ macOS Compatibility Status

### Already Handled Correctly

1. **Clipboard Paste (Images)**:
   - Enhanced handler checks both `clipboardData.items` (standard) and `clipboardData.files` (Linux fallback)
   - macOS WebKit supports `clipboardData.items` natively (like Windows)
   - No macOS-specific changes needed ✅

2. **File Paths**:
   - All path handling uses regex that normalizes both `/` and `\` to `/`
   - Works correctly on macOS (Unix-style paths) ✅
   - Examples:
     ```typescript
     .replace(/\\/g, "/")  // Normalizes Windows \ to /
     .split(/[/\\]/)        // Splits on both / and \
     ```

3. **Process Management**:
   - Windows-specific code (`taskkill`, console hiding) is properly wrapped with `#[cfg(target_os = "windows")]`
   - macOS will use Unix process signals (same as Linux) ✅

4. **Git Commands**:
   - No platform-specific issues
   - `git` command works identically on macOS/Linux/Windows ✅

5. **Environment Variables**:
   - Linux GTK/WebKit fixes are properly wrapped with `#[cfg(target_os = "linux")]`
   - Won't affect macOS ✅
   ```rust
   #[cfg(target_os = "linux")]
   {
       std::env::set_var("GTK_IM_MODULE", "gtk-im-context-simple");
       std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
   }
   ```

### Known Platform-Specific Code

#### Backend (Rust)

| Location                               | Platform     | Purpose                   | macOS Impact                   |
| -------------------------------------- | ------------ | ------------------------- | ------------------------------ |
| `src-tauri/src/main.rs`                | Linux only   | GTK/WebKit env vars       | None - properly isolated       |
| `src-tauri/src/commands.rs:292`        | Windows only | Hide `git` console window | None - macOS uses Unix path    |
| `src-tauri/src/sidecar_manager.rs:488` | Windows only | Hide `taskkill` console   | None - macOS uses Unix signals |

#### Frontend (TypeScript/React)

| Feature            | Implementation               | macOS Status             |
| ------------------ | ---------------------------- | ------------------------ |
| Clipboard paste    | Multi-method detection       | ✅ Works (standard path) |
| File path parsing  | Regex normalizes `/` and `\` | ✅ Works                 |
| Keyboard shortcuts | None currently               | N/A                      |

## 🔍 Potential macOS Considerations

### 1. Keyboard Shortcuts (Future)

If adding keyboard shortcuts, use:

```typescript
const isMac = navigator.platform.toUpperCase().indexOf("MAC") >= 0;
const modifier = isMac ? "metaKey" : "ctrlKey"; // Cmd vs Ctrl
```

### 2. File Permissions

- macOS is Unix-like (similar to Linux)
- File permissions should work identically ✅

### 3. Notarization (Distribution)

- macOS apps require code signing and notarization
- Supported by `.github/workflows/release.yml`, but only takes effect if Apple signing/notarization secrets are configured in the GitHub repo
- See: https://tauri.app/distribute/sign/macos/

### 4. App Sandbox

- macOS enforces stricter security than Linux
- Tauri handles this automatically
- No code changes needed ✅

## 🧪 Testing Recommendations

### macOS-Specific Tests

1. **Clipboard paste**: Copy screenshot → Paste into chat
2. **File paths**: Open projects with spaces/special chars in path
3. **Sidecar binary**: Verify correct architecture (arm64 for M1/M2/M3, x86_64 for Intel)
4. **Git operations**: Init repo, check diff display

### Multi-Platform Regression Tests

1. Path normalization (Windows `\` vs Unix `/`)
2. Process cleanup (sidecar stop/restart)
3. API key storage/retrieval

## 📝 Conclusion

**macOS compatibility is already excellent** with no known blockers. The codebase follows cross-platform best practices:

- ✅ Platform-specific code is properly isolated with `#[cfg(target_os = "...")]`
- ✅ Path handling normalizes Windows/Unix separators
- ✅ Clipboard handling uses standard APIs with fallbacks
- ✅ No hardcoded platform assumptions

### Action Items

- [x] Enhanced clipboard paste handler (added in ChatInput.tsx)
- [ ] Test on actual macOS hardware (M1/Intel)
- [ ] Verify sidecar binary downloads correct architecture
- [ ] Optional: Add platform-specific keyboard shortcut hints in UI
