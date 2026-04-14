# SinglePDF Extension Alpha

This folder contains the first browser-extension integration slice for SinglePDF.

Current behavior:

1. User clicks the toolbar button.
2. Extension uses vendored SingleFile runtime code to capture a frozen HTML snapshot of the active page.
3. Extension sends that frozen HTML to a native messaging host named `singlepdf.host`.
4. Native host renders the PDF and writes it directly into the user Downloads folder.
5. Extension reports success or failure back in the browser UI.

The extension now also surfaces state in the toolbar badge:

- `CAP`: capture in progress
- `PDF`: native PDF rendering in progress
- `DL`: download in progress
- `OK`: completed successfully
- `ERR`: failed, with a browser notification and structured console error details

Important implementation detail:

- the native host now saves PDFs directly into Downloads instead of returning raw PDF bytes over native messaging
- this avoids Firefox native-messaging response size limits on real pages with images

Current build flow:

1. `.\tools\build-singlepdf.ps1` builds the Rust binary.
2. The same script prepares a loadable extension bundle under `build\extension`.
3. The built binary is copied to `build\release\singlepdf.exe` by default.

The extension copies the SingleFile runtime files it needs from `SingleFile/lib` into the build output `build\extension\lib` instead of mutating the source `extension\` tree.

## Build

From the repository root in PowerShell:

```powershell
.\tools\build-singlepdf.ps1
```

For a faster debug build:

```powershell
.\tools\build-singlepdf.ps1 -Debug
```

Outputs:

- extension folder to load:
  - `build\extension`
- native host binary to register:
  - `build\release\singlepdf.exe`
  - or `build\debug\singlepdf.exe` when `-Debug` is used

## Native Host

Expected host name:

- `singlepdf.host`

The installed manifest should point directly to the built `singlepdf.exe`. The binary now auto-detects browser native-messaging stdio launches, so the browser does not need to pass any extra CLI arguments.

## Windows Install

Build first, then register the native host against the produced binary path.

Firefox only:

```powershell
.\tools\install-native-host.ps1 -ExePath .\build\release\singlepdf.exe -FirefoxOnly
```

Edge only:

```powershell
.\tools\install-native-host.ps1 -ExePath .\build\release\singlepdf.exe -EdgeOnly -EdgeExtensionId <your-edge-extension-id>
```

Both:

```powershell
.\tools\install-native-host.ps1 -ExePath .\build\release\singlepdf.exe -EdgeExtensionId <your-edge-extension-id>
```

To remove the registration:

```powershell
.\tools\uninstall-native-host.ps1
```

The installer writes manifests under:

- `%LOCALAPPDATA%\SinglePDF\NativeMessagingHosts`

and sets the expected registry keys for Firefox and Edge under `HKCU`.

## Firefox Dev Loop

1. Run `.\tools\build-singlepdf.ps1`.
2. Open `about:debugging#/runtime/this-firefox`.
3. Choose `Load Temporary Add-on`.
4. Select `build\extension\manifest.json`.
5. Register the native host:

```powershell
.\tools\install-native-host.ps1 -ExePath .\build\release\singlepdf.exe -FirefoxOnly
```

6. Reload the temporary add-on after installing the host if the browser was already open.

## Edge Dev Loop

1. Run `.\tools\build-singlepdf.ps1`.
2. Open `edge://extensions`.
3. Enable `Developer mode`.
4. Choose `Load unpacked` and select `build\extension`.
5. Open the extension details page and copy the extension ID.
6. Register the native host with that ID:

```powershell
.\tools\install-native-host.ps1 -ExePath .\build\release\singlepdf.exe -EdgeOnly -EdgeExtensionId <copied-id>
```

7. Click `Reload` on the unpacked extension after installing the host.

## Failure Modes

If something goes wrong, the extension now exposes the failure in three places:

- toolbar badge switches to `ERR`
- a transient browser notification is shown when available
- the background page console logs a structured error object

Expected common failures:

- missing native host registration
- Edge extension ID mismatch in the native host manifest
- unsupported browser-internal pages such as `about:` or `edge:`
- missing capture content script after install or reload changes

## Why this exists

This is the smallest useful extension bridge:

- one click
- native local conversion
- no user-facing rendering options
- SingleFile capture instead of raw outerHTML
