# Overview
A Tauri-based background app that monitors clipboard activity and alerts users when they attempt to paste content into a different app than where it was copied. Primary use case: preventing users from blindly pasting malicious commands from browsers into terminals.

Architecture
Frontend (Web/React)

Minimal tray-based UI — lives in the menu bar
Settings panel for configuring monitored apps, notification preferences, whitelist rules
Activity log showing recent clipboard events

Backend (Rust via Tauri)

Clipboard monitoring daemon
App focus tracking
Notification dispatch
Settings persistence (local JSON or SQLite)

macOS Implementation Plan
Phase 1 — Project Setup

Init Tauri project with React frontend
Configure as a menu bar app (no dock icon)
Set up LaunchAgent support for auto-start on login
Configure macOS code signing and notarization

Phase 2 — Clipboard Monitoring

Use NSPasteboard via Rust's objc crate to poll clipboard changes
Track changeCount on NSPasteboard.general to detect new copies
Store the source app when clipboard content changes

Phase 3 — Active App Detection

Use macOS Accessibility APIs (AXUIElement) to detect the frontmost app
Request Accessibility permissions via AXIsProcessTrustedWithOptions
Track app switches using NSWorkspace.didActivateApplicationNotification
Map bundle identifiers to friendly app names

Phase 4 — Cross-App Paste Detection

Compare the app that last wrote to the clipboard with the app attempting to paste
If they differ and the destination is a monitored app (e.g. Terminal, iTerm), trigger an alert
Use macOS native notifications (UNUserNotificationCenter) or a custom overlay

Phase 5 — Settings & UX

Tray menu with quick toggles (enabled/disabled, strict/notify mode)
Settings window: manage monitored apps list, whitelist trusted pairs (e.g. IDE ↔ Terminal is fine)
Optional: show clipboard content preview in the alert so users can verify what they're pasting

Phase 6 — Packaging & Distribution

Code sign with Apple Developer ID
Notarize via xcrun notarytool
Package as .dmg for distribution
Optional: Homebrew cask formula

Key macOS Permissions Required

Accessibility — for detecting active app and intercepting paste events
Notifications — for alerting users

Rust Crates to Investigate

objc / objc2 — Objective-C runtime bindings
cocoa — macOS framework bindings
tauri — app framework
tauri-plugin-notification — native notifications
tauri-plugin-autostart — launch on login

Risks & Considerations

Paste interception: macOS doesn't easily let you block a paste. V1 should focus on alerting, with blocking as a stretch goal
Privacy: clipboard content is sensitive — never log or transmit it, only compare source/destination apps
Performance: polling clipboard too aggressively could drain battery. A 200-500ms poll interval should be fine
