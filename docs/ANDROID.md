# Running DVP on Android (Termux aarch64)

> **Note:** Android is suitable for quick testing or setup, but it is not recommended for long-term, actual protection.

This guide provides step-by-step instructions for deploying and running the Discord Vanity Protector (DVP) engine on Android devices using **Termux** (64-bit ARM / `aarch64`).

---

## 1. Prerequisites & Termux Setup

### 1.1 Install Termux
Install the latest release of Termux from [F-Droid](https://f-droid.org/en/packages/com.termux/) or the [Termux GitHub Releases](https://github.com/termux/termux-app/releases). *(Do not use the outdated Google Play Store version).*

### 1.2 Update Packages & Install Base Dependencies
Open Termux and execute:

```bash
pkg update && pkg upgrade -y
pkg install -y git curl wget tar clang rust make cmake pkg-config openssl
```

### 1.3 Clone the Repository
```bash
git clone https://github.com/faiz4sure/discord-vanity-protector.git
cd discord-vanity-protector
```

### 1.4 Configure `config.toml`
Copy or edit `config.toml` in the repository root directory:

```bash
nano config.toml
```

Populate the required fields:
- `selfbot.token`: Your Discord account token.
- `selfbot.server_id`: The ID of the server to protect.
- `vanity.code`: The vanity URL slug (e.g. `myserver`).
- `vanity.password`: Your Discord account password (for automated MFA elevation).
- `logging.webhook_url`: Discord webhook URL for incident alerts.

---

## 2. Execution Pathways

Android (Termux) operates on Android's **Bionic C library** and LLVM `libc++` rather than standard GNU `glibc` / `libstdc++.so.6`. Choose the pathway best suited for your setup:

---

### Pathway A: Native Compilation inside Termux (Recommended)

Building directly on your device links the engine against Android's native Bionic runtime and `libc++`, guaranteeing 100% native stability without missing library dependencies.

> [!IMPORTANT]
> **Compilation Resource Notice**: Compiling DVP from source (especially BoringSSL C/C++ crypto components and Fat LTO optimizations) is CPU- and memory-intensive on mobile hardware and can take **5 to 15+ minutes** depending on your phone's processor.
>
> **Before starting compilation**, execute `termux-wake-lock` to keep the CPU awake and prevent Android from killing the compiler process in the background.

#### 1. Acquire Wake Lock & Install Build Tools
```bash
# Keep CPU awake during long compilation
termux-wake-lock

# Install required toolchains
pkg install -y rust clang make cmake binutils openssl
```

#### 2. Build Release Artifacts
```bash
cargo build --release
```

Compilation generates:
- `target/release/dvp` (Standalone native executable)
- `target/release/libdvp.so` (Dynamic shared library)

#### 3. Run Native Engine
```bash
# Direct binary execution:
./target/release/dvp

# Or via Python loader (loads local target/release/libdvp.so):
python3 main.py
```

---

### Pathway B: Zero-Compilation Prebuilt via PRoot Linux (Ubuntu)

The pre-compiled `libdvp-linux-aarch64.so` binaries in GitHub Releases target standard GNU Linux (`glibc` & `libstdc++.so.6`). To run these pre-compiled binaries on Android without compiling:

#### 1. Install PRoot Ubuntu Container
```bash
pkg install -y proot-distro
proot-distro install ubuntu
proot-distro login ubuntu
```

#### 2. Run Pre-Compiled Loaders inside PRoot
```bash
apt update && apt install -y python3 libstdc++6 git curl
git clone https://github.com/faiz4sure/discord-vanity-protector.git dvp
cd dvp
python3 main.py
```

---

## 3. Background Persistence & Continuous Execution

Android OS aggressively terminates long-running background processes. Follow these steps to ensure uninterrupted operation:

### 3.1 Acquire Termux Wake Lock
Prevent Android from putting the CPU into deep sleep:

```bash
termux-wake-lock
```

A persistent notification `Termux (wake lock held)` will appear in the Android notification tray.

---

### 3.2 Run inside `tmux` or `screen`
Use a terminal multiplexer so DVP continues running even if the Termux app is closed or backgrounded.

#### 1. Install `tmux`
```bash
pkg install -y tmux
```

#### 2. Start a Persistent Session
```bash
tmux new -s dvp
```

#### 3. Launch DVP inside `tmux`
```bash
# Using native binary:
./target/release/dvp

# Or using Python loader:
python3 main.py
```

#### 4. Detach from Session
Press `Ctrl + B`, then press `D`. The session will remain active in the background.

#### 5. Reattach to Session
To inspect logs or manage the process at any time:
```bash
tmux attach -t dvp
```

---

### 3.3 Disable Android Battery Optimization

To prevent Android OEM task killers (Samsung, Xiaomi/MIUI, OnePlus, Huawei) from killing Termux:

1. Open Android **Settings** ➔ **Apps** ➔ **Termux**.
2. Tap **Battery** or **App Battery Usage**.
3. Select **Unrestricted** (or disable "Battery Optimization").
4. On Xiaomi/MIUI: Enable **Autostart** and set Battery Saver to **No restrictions**.
5. On Samsung: Add Termux to **Never sleeping apps**.

