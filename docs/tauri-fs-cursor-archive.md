# Tauri capabilities and Cursor archive reads

Second Brain Lite’s **Cursor workspaceStorage** access runs in **Rust invoke handlers** using `std::fs` and `walkdir` under the desktop process. That does **not** go through `@tauri-apps/plugin-fs` and is **not** governed by the minimal `capabilities/default.json` allowlist the same way **frontend** file plugins are.

[`src-tauri/capabilities/default.json`](../src-tauri/capabilities/default.json) keeps `core:default` and `dialog:default` for the UI you already use (e.g. vault folder picker). No extra scope was required in testing for backend-only reads of:

- macOS: `~/Library/Application Support/Cursor/User/workspaceStorage`
- Linux: `~/.config/Cursor/User/workspaceStorage`
- Windows: `%APPDATA%\Cursor\User\workspaceStorage`

If you later expose transcript paths to the **webview** via `plugin-fs`, add **narrow** `fs` scopes for those directories only — avoid blanket `fs:default`.
