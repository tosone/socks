# socks

macOS desktop client for [shadowsocks-rust](./shadowsocks-rust). The UI is Tauri 2 + React + Tailwind. The backend embeds `shadowsocks-service` as an in-process `sslocal` TUN client and can take over the system IPv4 default route.

This build is **macOS only**. Install the privileged helper once (administrator password). After that, connect/disconnect changes routes through a LaunchDaemon and does not prompt again.

## Run

```bash
bun install
bun run tauri dev
```

Requirements: recent Rust (shadowsocks-service needs 1.91+), bun, and Xcode command line tools.

## Usage

1. Click **安装助手** once and approve the macOS administrator prompt. This copies the current app binary to `/Library/PrivilegedHelperTools` and loads `com.tosone.socks.helper` as a LaunchDaemon.
2. Click **创建** and fill in name, server, port, password, and cipher. Plugin fields are optional; the plugin binary must already be on `PATH`.
3. Use the card switch to connect. Only one profile can be active. The helper then adds:
   - host route: Shadowsocks server IP → original gateway
   - `0.0.0.0/1` and `128.0.0.0/1` → the TUN interface (`10.255.0.1/24`)
4. The card shows live up/down rates and a faint sparkline. Use **⋯** to edit or delete. Delete asks for confirmation and disconnects first if needed.
5. Disconnect or quit the app to restore the previous routes. Use **卸载** if you want to remove the helper.

Profiles are stored in the app data directory as `profiles.json`.

## Notes

- No App Sandbox, ACL/split routing, IPv6 default routes, SIP008 subscriptions, or bundled SIP003 plugins in this version.
- Changing only IPv4 default routes can leave system DNS on the original resolver.
- AEAD-2022 ciphers expect a Base64 key of the correct length (`ssservice genkey -m ...`).
