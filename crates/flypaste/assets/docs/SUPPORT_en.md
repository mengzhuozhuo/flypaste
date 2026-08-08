# Support

Welcome to the Flypaste Support page. This page includes a user guide and frequently asked questions. If you encounter any issues or have suggestions, we are here to help!

---

## 🚀 Contact Us

If you need further assistance or want to report a bug, you can reach out to the developer directly:

- **Email**: 1002025363@qq.com

---

## 📖 User Guide

Welcome to 👻 Flypaste. This is a lightweight clipboard management tool designed specifically for macOS. Flypaste automatically records your copy history in the background and lets you quickly retrieve it through a shortcut panel.

### 1. Core Work Modes

To minimize interruptions to your current workflow, Flypaste provides two work modes:

- **Stealth Mode (Default)**:
  The panel floats and **does not steal the input focus of the current application**. In this mode, when you click or select a record via a shortcut, the content is **automatically pasted** into your original working window.
  *(Note: Since the focus remains on the original window, search queries cannot be performed in this mode.)*

- **Focus Mode**:
  Click the search box at the top of the panel to enter. At this time, the panel will acquire the system's keyboard input focus. You can type text to search through history records, or press the `Space` bar to preview detailed content.
  *(Note: In this mode, clicking any history record will only copy it back to the clipboard and **will not** automatically execute a paste action.)*

> **Tip:** In Focus Mode, clicking any blank area outside the panel will automatically release the focus and return to Stealth Mode.

### 2. Basic Operations Guide

![](./paste-window.png)

- **Summon the panel**: The default shortcut is `Alt + \`` (the key to the left of the number 1). To modify this, click the 👻 icon in the menu bar, choose **"Settings…"**, and configure it in the **"Shortcuts"** tab.
- **Paste**: Click a paste item directly or use the shortcut `Alt + 1` to `Alt + 9` to select a paste item.
  - Stealth Mode: After clicking a paste item, the content will be **automatically pasted** into your original working window.
  - Focus Mode: After clicking a paste item, the content will be **copied** to the clipboard.
- **Pin window**: Use the shortcut `Cmd + Option + P` or click the 📌 icon in the top right corner of the panel to pin the panel to the top layer of the screen, suitable for consecutive multi-paste scenarios.
- **Shortcuts**: You can customize shortcuts, including:
  - Summon the panel
  - 📌 Pin window
  - Aa Case sensitive
  - .* Regular expressions

### 3. Search and Preview

The search and preview functions need to be used in Focus Mode. Click the search box to enter Focus Mode. In Focus Mode, typing directly will input into the search box, and the paste history list will update synchronously.

- **Advanced Search Rules**:
  - **Regular Expressions**: Default shortcut `Cmd + Option + R` or click the .* icon on the right side of the search box to enable.
  - **Case Sensitive**: Default shortcut `Cmd + Option + C` or click the Aa icon on the right side of the search box to enable.
- **Content Preview**: For long text or large images, hover the mouse over a paste record and press `Space` to pop up a preview window; press `Space` again to close it.
![](./preview.png)

### 4. Appearance Settings

Click the 👻 icon in the menu bar, choose **"Settings…"**, and in the **"General"** tab you can customize the following:

- **Interface Theme**: Supports Light, Dark, and System Default.
- **Transparency Control**: The application supports independently configuring the panel transparency in "Stealth Mode" and "Focus Mode".

### 5. Storage and Data Management

Click the 👻 icon in the menu bar, choose **"Settings…"**, and in the **"Storage"** tab you can maintain history data:

- **Retention Period**: Supports setting expiration times separately for "Text" and "Images". Expired data will be automatically cleaned up on the next startup, or you can manually click the button to clean it up.
- **Rebuild Index**: If there are anomalies in search results or the index takes up too much space, executing "Rebuild Index" can reconstruct the index.
- **Clear History Records**: This will clear all history records, including text, images, and index files. Please **proceed with caution**.

---

## ❓ Frequently Asked Questions (FAQ)

### 1. Why does Flypaste require "Accessibility" permissions?

Flypaste uses the Accessibility permission **only** to simulate the "Command + V" keystroke. This allows the app to automatically paste the clipboard history item you selected into your active window. Without this permission, the app cannot paste the content for you.

### 2. How to grant Accessibility permissions?

1. Open your Mac's **System Settings**.
2. Navigate to **Privacy & Security** -> **Accessibility**.
3. Find **Flypaste** in the list and toggle the switch to turn it ON.
   *(If it's already enabled but pasting still doesn't work, try selecting it and clicking the `-` button to remove it, then launch the app again to re-authorize.)*

### 3. Why doesn't clicking a paste item automatically paste to the expected location?

1. Auto-pasting requires **Accessibility** permissions. Go to System Settings -> Privacy & Security -> Accessibility and make sure Flypaste is enabled (see sections 1 and 2 above).
   ![](./setting-accessibility.png)
2. Auto-pasting is not available in **Focus Mode**. Please paste manually with `Cmd + V`, or switch back to **Stealth Mode**.
   > Tip: In Focus Mode, the 👻 icon will light up.
   ![](./focus-mode.png)

### 4. Does Flypaste upload my clipboard data?

No. Flypaste operates 100% offline. All clipboard history is stored safely within the encrypted App Sandbox on your local hard drive.

### 5. How to delete all my history?

Click the 👻 icon in the menu bar at the top of the screen, choose **"Settings…"**, go to the **"Storage"** tab, and click **"Clear All History…"**. This will delete all local records and cannot be undone. Flypaste will quit when finished.

### 6. Why isn't the configured data retention period taking effect?

The retention period is applied during **the next launch**, not immediately. To clean up expired data right away, go to **"Settings…"** -> **"Storage"** and click **"Clean Expired Data"**.

### 7. What if the shortcut conflicts? The default `Alt + \`` doesn't work?

If the default shortcut doesn't respond, it's usually taken by another app (such as an input method or certain IDEs). You can click the 👻 icon in the menu bar at the top of the screen, choose **"Settings…"**, and record your own global summon shortcut in the **"Shortcuts"** tab.

### 8. Why doesn't pressing Space preview content?

Space preview requires two conditions:

1. You must be in **Focus Mode** (click the search box to enter).
2. The mouse must be hovering over a paste record.
