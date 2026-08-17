# socks

`socks` is a macOS desktop client for [shadowsocks-rust](./shadowsocks-rust). The UI is built with Tauri 2, React, and Tailwind CSS. The backend embeds `shadowsocks-service` and runs an in-process `sslocal` TUN client.

This project is macOS-only for now.

## Features

- Manage Shadowsocks server profiles.
- Connect through an in-process `sslocal` TUN client.
- Route system IPv4 traffic through the TUN interface.
- Install a privileged LaunchDaemon helper once, then connect and disconnect without repeated administrator prompts.
- Show live upload and download speed with a sparkline for the recent traffic window.
- Persist per-profile total upload and download traffic.
- Bundle SIP003 plugin executables as app resources.

## Requirements

- macOS.
- Rust 1.91 or newer.
- Bun.
- Xcode Command Line Tools.

## Development

Install dependencies:

```bash
bun install
```

Run the Tauri development app:

```bash
bun run tauri dev
```

Build the frontend only:

```bash
bun run build
```

Check the Rust backend:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

## Packaging

Build the macOS app package:

```bash
bun run tauri build
```

The app icon is generated from `public/socks.svg`. To regenerate the macOS icon resources:

```bash
bun run tauri icon public/socks.svg
```

Only the macOS icon resources are kept in the repository:

```text
src-tauri/icons/icon.icns
src-tauri/icons/icon.png
```

## Usage

1. Install the helper once from the app. This requires an administrator password and installs a LaunchDaemon helper under `/Library/PrivilegedHelperTools`.
2. Create a profile with name, server, port, password, and encryption method.
3. Optionally set a SIP003 plugin name and plugin options.
4. Use the profile card menu to connect, disconnect, edit, or delete a profile.
5. Only one profile can be active at a time. Connecting another profile disconnects the current one first.
6. Disconnect or quit the app to restore the previous routes.

When connected, the helper applies IPv4 routes similar to:

```text
<server-ip>      -> original gateway
0.0.0.0/1        -> TUN interface
128.0.0.0/1      -> TUN interface
```

The TUN interface address is:

```text
10.255.0.1/24
```

## Data Files

Profiles are stored in the Tauri app data directory:

```text
profiles.json
```

Per-profile traffic totals are stored separately:

```text
traffic/<profile-id>.json
```

On macOS, the app data directory is typically:

```text
~/Library/Application Support/com.tosone.socks/
```

Traffic sparkline samples are kept in memory only. They cover the recent traffic window and are discarded on disconnect, reconnect, or app restart.

## Bundled Plugins

SIP003 plugins can be bundled as Tauri resources. Put plugin executables under:

```text
src-tauri/resources/plugins/
```

The current bundle configuration maps:

```text
src-tauri/resources/plugins/v2ray-plugin/v2ray-plugin -> plugins/v2ray-plugin
```

When a profile uses a plugin name without path separators, the app first checks the bundled plugin resource directory. For example, `v2ray-plugin` resolves to:

```text
<App>.app/Contents/Resources/plugins/v2ray-plugin
```

If no bundled executable is found, the app falls back to the system `PATH`. Absolute plugin paths are used as-is.

## Limitations

- IPv6 default routes are not managed yet.
- DNS may still use the original system resolver.
- SIP008 subscriptions are not implemented yet.
- App Sandbox is not enabled.
- Bundled plugin binaries must match the target macOS CPU architecture.
- AEAD-2022 methods require a Base64 key of the correct length.
