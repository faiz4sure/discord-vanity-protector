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

You can run DVP on Android via two distinct methods:

---

### Pathway A: Zero-Compilation Execution (Python, Bun, or Node.js)

DVP provides lightweight FFI runtime loaders (`main.py`, `bun.ts`, `index.js`) that automatically detect the `aarch64` architecture, fetch the pre-compiled `libdvp-linux-aarch64.so` binary from GitHub Releases, and execute the engine directly in background memory.

#### Option 1: Python Loader (Recommended for Android)
Install Python and run:

```bash
pkg install -y python
python3 main.py
```

#### Option 2: Bun Loader
Install Bun inside Termux and run:

```bash
curl -fsSL https://bun.sh/install | bash
source ~/.bashrc
bun run bun.ts
```

#### Option 3: Node.js Loader
Install Node.js, install runtime dependencies, and run:

```bash
pkg install -y nodejs
npm install
node index.js
```

---

### Pathway B: Native Compilation inside Termux

If you prefer compiling the engine directly on your Android device:

#### 1. Configure Rust & Toolchain Environment
```bash
pkg install -y rust clang binutils
export CC=clang
export CXX=clang++
```

#### 2. Build Release Artifacts
```bash
cargo build --release
```

Compilation produces:
- `target/release/dvp` (Standalone native executable)
- `target/release/libdvp.so` (Dynamic shared library)

#### 3. Run Native Executable
```bash
./target/release/dvp
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

