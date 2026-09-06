# Local development setup (Windows)

Team Principal is a Windows-native app. It must be developed **natively on
Windows** — not in WSL, not in a Linux container. WSL has no access to the Win32
display, window and HID APIs that are the entire point of the product, so code
written there cannot be compiled against the real target or tested at all.

One-time setup, roughly 20 minutes, mostly downloads.

## 1. Visual Studio Build Tools

Rust on Windows links with the MSVC toolchain.

Download **Build Tools for Visual Studio 2022** from
<https://visualstudio.microsoft.com/downloads/> (under "Tools for Visual
Studio"). In the installer, tick **Desktop development with C++**. That workload
includes the MSVC compiler, the linker and the Windows SDK. Accept the defaults
inside it.

This is the largest download, around 3–4 GB. Start it first.

## 2. Rust

<https://rustup.rs> → download and run `rustup-init.exe`. Accept the defaults;
it picks `x86_64-pc-windows-msvc`, which is what we want.

Verify in a new terminal:

```powershell
rustc --version
cargo --version
```

## 3. Node.js

<https://nodejs.org> → the LTS installer (22 or later). Verify:

```powershell
node --version
npm --version
```

## 4. Git for Windows

<https://git-scm.com/downloads/win> → defaults are fine. This also gives Claude
Code a Bash shell to work in, which it prefers over PowerShell.

## 5. WebView2

Already present on Windows 11. Nothing to do. (On Windows 10 it usually is too,
but if `tauri dev` complains, install the Evergreen Bootstrapper from
<https://developer.microsoft.com/microsoft-edge/webview2/>.)

## 6. Claude Code

Open **PowerShell** (press Win, type `powershell`, Enter) and run:

```powershell
irm https://claude.ai/install.ps1 | iex
```

Close and reopen the terminal, then check:

```powershell
claude --version
claude doctor
```

## 7. Clone the repo and start

```powershell
cd $env:USERPROFILE\Documents
git clone https://github.com/acamenzuli/Team-Principal-App.git
cd Team-Principal-App
git checkout claude/sim-racing-launcher-display-i60v81
claude
```

First run of `claude` opens a browser to log in.

## 8. Verify the toolchain

Once the skeleton exists, this should build and launch a window:

```powershell
npm install
npm run tauri dev
```

First build compiles the whole Rust dependency tree and takes several minutes.
Subsequent builds are seconds.

## Why native Windows, not WSL or the cloud

| Needs | Cloud (Linux) | WSL | Native Windows |
| --- | --- | --- | --- |
| Compile the app | no | no | yes |
| Enumerate real monitors and read EDID | no | no | yes |
| Detect wheelbase, pedals, vJoy | no | no | yes |
| Move a real game window | no | no | yes |
| Run the Let's race flow end to end | no | no | yes |

Everything from milestone 2 onward touches Win32. There is no useful way to
develop or verify it anywhere else.
