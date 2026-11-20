# Configuring 2FA (TOTP) for Automatic MFA Reversion

This guide explains how to configure a 2FA TOTP secret key in `config.toml` so DVP can automatically pass Discord's MFA challenge when restoring your server's vanity URL.

---

## Why Use TOTP Instead of Password?

Discord requires Multi-Factor Authentication (MFA) elevation (`code: 60003`) before allowing changes to a server's vanity URL.

Historically, accounts without 2FA could complete this challenge using their account password. Recently, however, Discord's `/api/v9/mfa/finish` endpoint has been rejecting password-based MFA with:

```text
status=400 Bad Request body={"message": "Password does not match", "code": 60008}
```

This happens even when the password in `config.toml` is 100% correct. Enabling Authenticator App 2FA on your Discord account and setting `totp_secret` in `config.toml` avoids this issue completely.

When both `totp_secret` and `password` are present, DVP always prefers `totp` whenever Discord offers it.

---

## Security Advisory

> [!CAUTION]
> **KEEP YOUR TOTP SECRET PRIVATE**
> - Your `totp_secret` is the master seed used to generate 6-digit 2FA codes for your account.
> - **NEVER** share it, commit `config.toml` to Git, or paste it into untrusted websites.
> - DVP computes TOTP codes **100% locally** using RFC 6238 (HMAC-SHA1) and scrubs the derived key material from memory on drop.

---

## Step 1: Get Your Discord 2FA Setup Key (Base32 Secret)

You need the **16+ character Base32 setup key** (for example: `JBSWY3DPEHPK3PXP` or `ABCD EFGH IJKL MNOP`), **not** the 6-digit rotating code.

### Option A: 2FA Is Not Enabled Yet

1. Open Discord (Desktop, Web, or Mobile) and go to **User Settings -> My Account**.
2. Under **Password and Authentication**, click **Enable Authenticator App**.
3. Enter your Discord account password when prompted.
4. On the QR code screen, look below the QR code for the manual setup key (labelled **"Or enter this key manually"** / **"Copy Discord Auth Key"**).
5. Copy that setup key and save it temporarily.
6. Still on the same screen, scan the QR code (or enter the same setup key) into your own authenticator app (such as Google Authenticator, 2FAS, Ente Auth, Aegis, or Authy) so you can still log in normally.
7. Enter the 6-digit code shown by your authenticator app into Discord to finish enabling 2FA.
8. Save Discord's backup codes somewhere safe.

### Option B: 2FA Is Already Enabled on Your Account

Discord does not display your existing TOTP secret again after setup. You have two choices:

- **Export from your authenticator app**: Apps like **Ente Auth**, **2FAS**, **Aegis**, or **Bitwarden** let you view or export the Base32 secret for your existing Discord entry directly.
- **Re-enable 2FA**: In **User Settings -> My Account**, click **Remove Authenticator App**, then follow **Option A** above to generate a fresh setup key and copy it before confirming.

---

## Step 2: Add `totp_secret` to `config.toml`

1. Open `config.toml` in a text editor.
2. Under the `[vanity]` section, paste your Base32 setup key into `totp_secret`:

```toml
[vanity]
code = "myserver"
mode = "audit"
totp_secret = "ABCD EFGH IJKL MNOP"
password = ""
```

- Spaces, hyphens (`-`), padding (`=`), and lowercase/uppercase letters are handled automatically.
- You can leave `password = ""` empty once `totp_secret` is configured.
- DVP validates `totp_secret` on startup and will exit immediately with a clear error if the key is malformed or shorter than 16 Base32 characters.

---

## Step 3: Verify Your System Clock

TOTP codes depend on your machine's UTC clock (30-second windows). If your PC or VPS clock drifts by more than a few seconds, Discord will reject generated codes.

- **Linux (systemd)**: Run `timedatectl status` and make sure `System clock synchronized: yes`.
- **Windows**: Open **Settings -> Time & language -> Date & time** and enable **Set time automatically**, then click **Sync now**.
- **Android (Termux)**: Ensure **Automatic date & time** is enabled in Android system settings.
