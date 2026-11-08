<p align="center">
  <img src="src-tauri/icons/source/icon-full.svg" width="112" alt="Pomodoro Pill tomato logo">
</p>

<h1 align="center">Pomodoro Pill</h1>

<p align="center">
  A tiny, always-on-top Pomodoro timer for Windows.<br>
  Focus for 25 minutes. Break for 5. Stay in flow.
</p>

<p align="center">
  <a href="#install">Install</a> ·
  <a href="#how-to-use">How to use</a> ·
  <a href="#settings">Settings</a> ·
  <a href="#troubleshooting">Troubleshooting</a>
</p>

Pomodoro Pill sits above your windows as a frameless pill. At rest it shows only
a dot, the time, and a thin progress line. Hover to reveal pause, stop, and skip.
It never takes keyboard focus and stays out of the taskbar and Alt-Tab.

## What it does

- Runs a 25-minute focus session and a 5-minute break.
- Never starts the next phase by itself. You choose when to continue.
- Floats above other windows without stealing focus.
- Stays out of the taskbar and Alt-Tab.
- Pauses, resumes, stops, and skips from the hover controls.
- Starts, pauses, and resumes with the global `Ctrl+Alt+P` hotkey.
- Remembers where you put it.
- Plays a sound when a phase ends.

## Install

1. Download the latest installer from the
   [Releases page](https://github.com/itsmusa/pomodoro-pill/releases/latest).
2. Run the downloaded installer.
3. If Windows SmartScreen says "Windows protected your PC", choose
   **More info** → **Run anyway**. The installer is not code-signed.
4. Launch Pomodoro Pill from the Start menu.

The installer is per-user and does not need administrator rights. It installs
under your local app data. WebView2 is installed automatically if it is missing.

## How to use

### The cycle

1. Click **Start**. The focus timer begins at `25:00`.
2. At `00:00` the pill flashes and plays a sound. The break is ready, but it
   does not start.
3. Click **Start** to begin the `05:00` break.
4. At `00:00` the pill flashes again. Click **Start** for the next focus
   session.

### Controls

| Action | How |
|---|---|
| Start focus or break | Click **Start**, or press `Ctrl+Alt+P` |
| Pause or resume | Hover the pill and click the pause/resume button, or press `Ctrl+Alt+P` |
| Stop and reset | Hover, click **Stop**, then click **Stop?** to confirm |
| Skip a break | Hover during a break and click **Skip** |
| Move the pill | Drag the pill |
| Hide or show | Left-click the tray icon |
| Quit | Right-click the tray icon → **Quit** |

The hover controls appear when you hover over the pill. During focus the second
button is **Stop**. During a break it is **Skip**.

### Tray menu

Right-click the tray icon for:

- **Sound** — turn phase-end sounds on or off.
- **Start with Windows** — launch the app when you sign in.
- **Hide in fullscreen** — hide the pill while a fullscreen app is active.
- **Reset position** — move the pill back to the top-right of the primary
  monitor.
- **Open settings file** — open `settings.json` in Notepad.
- **Quit** — exit the app.

## Settings

There is no settings window. Right-click the tray icon and choose
**Open settings file**, or edit:

```text
%APPDATA%\PomodoroPill\settings.json
```

The default file looks like this:

```json
{
  "focus_min": 25,
  "break_min": 5,
  "idle_reset_min": 15,
  "sound": true,
  "volume": 0.6,
  "autostart": false,
  "hide_in_fullscreen": false,
  "pos": null
}
```

| Field | Default | What it does |
|---|---:|---|
| `focus_min` | 25 | Focus length in minutes |
| `break_min` | 5 | Break length in minutes |
| `idle_reset_min` | 15 | Reset an idle paused or finished session |
| `sound` | true | Play a sound when a phase ends |
| `volume` | 0.6 | Sound volume from `0.0` to `1.0` |
| `autostart` | false | Start Pomodoro Pill with Windows |
| `hide_in_fullscreen` | false | Hide the pill in fullscreen apps |
| `pos` | null | Last window position; managed by the app |

Durations must be at least 1 minute. The volume is clamped to `0.0`–`1.0`.

The settings file is read when the app starts. **Restart Pomodoro Pill after
editing it.** Tray toggles save immediately and do not need a restart.

## Good to know

- Locking Windows, sleeping, or hibernating pauses a running timer. It stays
  paused when you return.
- A paused timer or a finished phase resets to `25:00` after 15 idle minutes.
- Only one copy runs. Launching the app again shows the existing pill.
- Phase-end notifications are sound only. There are no toast notifications.
- The pill does not appear over exclusive-fullscreen games.
- Switching virtual desktops can cause a brief flicker.
- This is a small v1 on purpose: no stats, task lists, themes, long breaks, or
  auto-update.

## Troubleshooting

**Windows says "Windows protected your PC"**

The installer is not code-signed. Choose **More info** → **Run anyway**.

**I cannot find the pill**

Left-click the tray icon. If it still does not appear, right-click the tray icon
and check **Hide in fullscreen**. Use **Reset position** if the window is
off-screen.

**Ctrl+Alt+P does nothing**

Another app may already use that shortcut. Use the hover controls instead, or
close the other app.

**There is no sound**

Check **Sound** in the tray menu and the Windows output device. The volume
setting accepts values from `0.0` to `1.0`.

**My settings edits are ignored**

The file is read at startup. Save the file and restart Pomodoro Pill. Tray
toggles do not need a restart.

**The timer will not quit**

The pill has no taskbar button. Right-click the tray icon and choose **Quit**.

**How do I uninstall it?**

Open Windows Settings → Apps → Installed apps → Pomodoro Pill → Uninstall.

## Build from source

Prerequisites:

- Windows 10/11 x64
- Rust stable with the MSVC toolchain
- Node.js 20 or newer
- Microsoft C++ Build Tools with the Windows SDK
- WebView2 runtime (the installer bootstraps it if missing)

Run the app in development:

```powershell
npm install
npm run tauri dev
```

Build the installer:

```powershell
npm install
npm run tauri build
```

The NSIS installer is written to:

```text
src-tauri/target/release/bundle/nsis/Pomodoro Pill_0.1.0_x64-setup.exe
```

The npm Tauri CLI is used here, so `cargo tauri` is not required. The installer
is per-user and is not code-signed, so SmartScreen may warn.

For a deeper look at the architecture, timer rules, and theme, see
[`docs/DESIGN.md`](docs/DESIGN.md).

## License

MIT. See [LICENSE](LICENSE).
