# macOS Network Extension Migration

This directory contains the native macOS pieces needed to move the Tauri client
from the current root helper route/DNS model to an Outline-style Packet Tunnel
Provider model.

## What This Replaces

The current runtime does this in a root helper:

- Starts `shadowsocks-rust` with `local-tun`.
- Discovers `utun` through `ifconfig`.
- Adds `/1` routes with `/sbin/route`.
- Changes physical service DNS with `networksetup`.
- Redirects loopback DNS with PF.

The Network Extension model moves route and DNS ownership to macOS:

- The app starts a `NETunnelProviderManager`.
- macOS launches a signed `.appex` implementing `NEPacketTunnelProvider`.
- The extension calls `setTunnelNetworkSettings` with default IPv4 route,
  excluded local routes, and `NEDNSSettings`.
- Packets flow through `NEPacketTunnelFlow`.

## Files

- `SocksTunnelControl.swift`: app-side controller equivalent to Outline's
  `OutlineVpn.swift`.
- `VpnExtension/PacketTunnelProvider.swift`: extension-side tunnel settings and
  lifecycle skeleton.
- `VpnExtension/Info.plist`: extension plist.
- `App.entitlements`: host app entitlements.
- `VpnExtension/SocksTunnelExtension.entitlements`: extension entitlements.

## Required Xcode Wiring

The files here are not compiled by Cargo or Vite. They must be added to an
Xcode target:

1. Add a macOS App Extension target of type Packet Tunnel Provider.
2. Set the extension bundle identifier to `com.tosone.socks.SocksTunnelExtension`.
3. Add `VpnExtension/PacketTunnelProvider.swift` to that extension target.
4. Use `VpnExtension/Info.plist` for the extension.
5. Apply `VpnExtension/SocksTunnelExtension.entitlements` to the extension.
6. Apply `App.entitlements` to the Tauri host app signing step.
7. Embed the built `.appex` in the final `.app` bundle under `Contents/PlugIns/`.
8. Ensure the host app and extension use the same App Group.

## Data Plane Gap

The packet data plane is intentionally not stubbed as successful. The extension
must connect `NEPacketTunnelFlow` to a Shadowsocks transport before it can
replace the helper at runtime.

The viable implementation options are:

- Adapt `shadowsocks-rust` behind a packet source/sink abstraction callable from
  Swift.
- Reuse Outline's Go tun2socks stack and gobind-generated framework.
- Introduce a narrow Swift/Rust bridge that batches packet reads and writes.

Do not switch `src-tauri/src/session.rs` to this backend until the extension
can actually relay packets. Otherwise the UI Connect action will only create a
VPN configuration that immediately fails.
