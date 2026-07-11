# Flypaste Tutorial

Welcome to ![[../icons/ghost.svg]]Flypaste. This is a lightweight clipboard management tool designed specifically for macOS. Flypaste automatically records your copy history in the background and lets you quickly retrieve it through a shortcut panel.

## 1. Core Work Modes

To minimize interruptions to your current workflow, Flypaste provides two work modes:

- **Stealth Mode (Default)**:
  The panel floats and **does not steal the input focus of the current application**. In this mode, when you click or select a record via a shortcut, the content is **automatically pasted** into your original working window.
  *(Note: Since the focus remains on the original window, search queries cannot be performed in this mode.)*

- **Focus Mode**:
  Click the search box at the top of the panel to enter. At this time, the panel will acquire the system's keyboard input focus. You can type text to search through history records, or press the `Space` bar to preview detailed content.
  *(Note: In this mode, clicking any history record will only copy it back to the clipboard and **will not** automatically execute a paste action.)*

> **Tip:** In Focus Mode, clicking any blank area outside the panel will automatically release the focus and return to Stealth Mode.

## 2. Basic Operations Guide

![](./paste-window.png)

- **Summon the panel**: The default shortcut is ``` Cmd + ` ``` (the key to the left of the number 1). To modify this, go to "Settings -> Shortcuts".
- **Paste**: Click a paste item directly or use the shortcut `Cmd + 1` to `Cmd + 9` to select a paste item.
  - Stealth Mode: After clicking a paste item, the content will be **automatically pasted** into your original working window.
  - Focus Mode: After clicking a paste item, the content will be **copied** to the clipboard.
- **Pin window**: Use the shortcut `Cmd + Option + P` or click the ![[../icons/pin.svg]] icon in the top right corner of the panel to pin the panel to the top layer of the screen, suitable for consecutive multi-paste scenarios.
- **Shortcuts**: You can customize shortcuts, including:
  - Summon the panel
  - ![[../icons/pin.svg]] Pin window
  - ![[../icons/case_sensitive.svg]] Case sensitive
  - ![[../icons/regex.svg]] Regular expressions

## 3. Search and Preview

The search and preview functions need to be used in Focus Mode. Click the search box to enter Focus Mode. In Focus Mode, typing directly will input into the search box, and the paste history list will update synchronously.

- **Advanced Search Rules**:
  - **Regular Expressions**: Default shortcut `Cmd + Option + R` or click the ![[../icons/regex.svg]] icon on the right side of the search box to enable.
  - **Case Sensitive**: Default shortcut `Cmd + Option + C` or click the ![[../icons/case_sensitive.svg]] icon on the right side of the search box to enable.
- **Content Preview**: For long text or large images, hover the mouse over a paste record and press `Space` to pop up a preview window; press `Space` again to close it.
![](./preview.png)

## 4. Appearance Settings

In the **General** tab of the "Settings" interface, the following customizations can be made:

- **Interface Theme**: Supports Light, Dark, and System Default.
- **Transparency Control**: The application supports independently configuring the panel transparency in "Stealth Mode" and "Focus Mode".

## 5. Storage and Data Management

In the **Storage** tab of the "Settings" interface, you can maintain history data:

- **Retention Period**: Supports setting expiration times separately for "Text" and "Images". Expired data will be automatically cleaned up on the next startup, or you can manually click the button to clean it up.
- **Rebuild Index**: If there are anomalies in search results or the index takes up too much space, executing "Rebuild Index" can reconstruct the index.
- **Clear History Records**: This will clear all history records, including text, images, and index files. Please **proceed with caution**.
