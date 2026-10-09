# Keyboard shortcuts / キーボードショートカット / 键盘快捷键

🟢 [Done] Open **Edit → Keyboard Shortcuts…** (編集 → キーボードショートカット / 编辑 → 键盘快捷键). The initial shortcut is **⌘⌥⇧K** on macOS and **Ctrl+Alt+Shift+K** on Windows.

The editor lists implemented menu commands and tools, including commands without an initial shortcut. Search supports matching case and category filtering. Selecting a command and pressing keys in the recording field prepares an assignment. Conflicts name the existing command, offer navigation to it, and require an explicit reassignment action. Clear removes a binding; Restore defaults restores the command's initial key. Duplicate Set and Delete Set manage named sets. Export Text produces a readable list. OK applies the entire draft; Cancel discards it.

🟢 [Done] Common defaults follow the Adobe tools where possible: B brush, E eraser, V selection, A direct selection, P pen, N pencil, T text, G gradient, S clone stamp, M marquee, H hand and Z zoom. File/history/clipboard use the familiar Command/Ctrl combinations. Place uses Command/Ctrl+D as in Illustrator/InDesign; selection and shape shortcuts avoid collisions in the combined application. U selects a vector rectangle, Shift+U a vector ellipse. Not every Adobe product can share identical defaults in one application; every listed command can be reassigned. Less common menu commands start unassigned rather than claiming arbitrary keys.

## Architecture and interaction

- Rust owns the shared settings, active set, command metadata used by native canvas input, atomic file persistence, validation and settings revision. The file is `shortcuts-v1.json` in the application's data directory.
- Each editor supplies a stable command ID, localized label/category and default binding. Menu and toolbar labels derive from the same settings as keyboard dispatch. Large image buffers never cross this bridge.
- The native canvas routes a recognized key to the owning editor's registered action. Previous hardcoded canvas and WebView tool/menu bindings were removed. Text fields and IME composition retain text input; Escape, Enter, Space and arrow/delete gesture handling stay with the active editor tool.
- Settings changes are broadcast to every editor. Saving a stale dialog fails with an instruction to reopen it, rather than overwriting another window's changes.
- macOS Command is represented portably as `Primary`, Windows uses Control. Additional Control, Option/Alt and Shift modifiers are explicit. Physical letter/digit/punctuation codes are shared with native macOS key codes. System Quit, Hide and Minimize combinations are reserved.
- Registration omits planned menu items without an action. Commands disabled by the current document are still editable in the shortcut dialog but cannot execute until available.

## Validation

🟢 [Done] Frontend production build; Rust native shortcut validation/conflict tests; browser flow covering Edit menu entry, remapping, removal of the old binding, persistence across reload, text field typing, set duplication, Cancel, conflict navigation/reassignment, all three locales and a 390px-wide viewport. Browser plugin was unavailable; QA used Playwright with installed Chrome against the actual components and bridge's browser fallback.

⭕️ [Pending] Physical JIS/US keyboard, OS-reserved key behavior and multiple native editor windows require manual platform checks. Browser QA confirms the WebView flow; it does not substitute for those checks.

## Reference specifications

The attached Illustrator and Photoshop dialogs guided the set selector, search, command list and conflict controls. Adobe's official documentation was consulted for customization and initial assignments:

- [Photoshop: Customize keyboard shortcuts](https://helpx.adobe.com/photoshop/using/customizing-keyboard-shortcuts.html)
- [Illustrator: Customize keyboard shortcuts](https://helpx.adobe.com/ca/illustrator/desktop/get-started/preferences-and-settings/customize-keyboard-shortcuts.html)
- [Illustrator: Default keyboard shortcuts](https://helpx.adobe.com/uk/illustrator/using/default-keyboard-shortcuts.html)
- [InDesign: Keyboard shortcuts](https://helpx.adobe.com/indesign/desktop/get-started/settings-and-preferences/keyboard-shortcuts.html)
