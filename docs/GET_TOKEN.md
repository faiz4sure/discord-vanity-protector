# Extracting Your Discord Account Token Safely

This guide explains how to safely extract your personal Discord account token for use in `config.toml`.

---

## Security Advisory

> [!CAUTION]
> **CRITICAL SECURITY WARNING**
> - Your Discord token acts as your digital account key. Anyone with access to your token possesses full access to your account.
> - **NEVER** share your token, paste it into untrusted websites, or send it to other individuals.
> - DVP runs **100% locally** on your own machine. It never sends your token or password to third-party servers.
> - Beware of malicious browser extensions, phishing links, and fake token grabber executables claiming to optimize your account.

---

## Method 1: Web Browser Network Inspection (Recommended & Most Reliable)

This method extracts the token directly from active Discord REST API network requests.

1. Open your web browser (Chrome, Brave, Edge, or Firefox) and navigate to **[https://discord.com/app](https://discord.com/app)**.
2. Log in to your Discord account.
3. Open Developer Tools by pressing **`F12`** (or **`Ctrl + Shift + I`** on Windows/Linux, **`Cmd + Option + I`** on macOS).
4. Switch to the **Network** tab in the Developer Tools panel.
5. In the filter box at the top, type `api` or `science` or `messages`.
6. Click on any Discord channel or server to trigger a network request.
7. Select any network request directed to `discord.com/api/v9/...` (such as `messages`, `ack`, or `@me`).
8. In the right-hand inspection pane, select the **Headers** tab.
9. Scroll down to the **Request Headers** section and locate the **`authorization`** header.
10. Copy the token string (e.g., `MTE4...` or `mfa...`).

---

## Method 2: Application Local Storage Inspection

1. Open **[https://discord.com/app](https://discord.com/app)** in your browser and log in.
2. Press **`F12`** (or **`Ctrl + Shift + I`**) to open Developer Tools.
3. Switch to the **Application** tab (in Chrome/Brave/Edge) or **Storage** tab (in Firefox).
4. In the left sidebar, expand **Local Storage** and click on `https://discord.com`.
5. Toggle Device Toolbar by pressing **`Ctrl + Shift + M`** (or **`Cmd + Shift + M`**) if the `token` key is hidden.
6. In the filter box, type `token`.
7. Locate the row with the Key named `token` and copy its Value (strip the enclosing double quotes `"` if present).

---

## Method 3: Browser Console Extraction

1. Open **[https://discord.com/app](https://discord.com/app)** in your browser and log in.
2. Press **`F12`** and navigate to the **Console** tab.
3. If Discord displays a `Stop!` or pasting warning, type `allow pasting` and press **Enter**.
4. Paste the following extraction snippet into the console and press **Enter**:

```javascript
(function() {
    window.dispatchEvent(new Event('beforeunload'));
    let iframe = document.createElement('iframe');
    iframe.style.display = 'none';
    document.body.appendChild(iframe);
    let token = iframe.contentWindow.localStorage.getItem('token');
    iframe.remove();
    if (token) {
        console.log("%c[DVP] Your Discord Token:", "color: #00ff00; font-weight: bold; font-size: 14px;");
        console.log(token.replace(/^"|"$/g, ''));
    } else {
        console.error("[DVP] Token not found in Local Storage. Use Method 1 (Network Tab).");
    }
})();
```

5. Copy the printed token string from the console output.

---

## Method 4: Configuring `config.toml`

Once you have copied your token:

1. Open `config.toml` in your favorite text editor.
2. Paste your token into the `[selfbot]` section:

```toml
[selfbot]
token = "YOUR_DISCORD_TOKEN_HERE"
server_id = "123456789012345678"
```

3. Save the file.

