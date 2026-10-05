# macOS Network Extension

This directory contains the native macOS pieces for the Packet Tunnel backend.
Route and DNS ownership live in macOS (`NetworkExtension`), not in a root
helper.

## What this replaces

The old runtime used a root helper to:

- Start `shadowsocks-rust` with `local-tun`.
- Discover `utun` through `ifconfig`.
- Add `/1` routes with `/sbin/route`.
- Change the physical service DNS with `networksetup`.
- Redirect loopback DNS with PF.

The Network Extension model moves all of that to macOS:

- The app starts a `NETunnelProviderManager`.
- macOS launches a signed `.appex` implementing `NEPacketTunnelProvider`.
- The extension calls `setTunnelNetworkSettings` with the default IPv4 route,
  excluded local routes, and `NEDNSSettings`.
- Packets flow through `NEPacketTunnelFlow`.

## Building

The Xcode project is **generated** from `project.yml`; do not edit
`SocksTunnel.xcodeproj` by hand (it is gitignored).

```sh
make extension           # generate + build + sign target/extensions/vpn/SocksTunnelExtension.appex
make extension-project   # only regenerate SocksTunnel.xcodeproj from project.yml
```

`make extension` runs:

1. `xcodegen generate` → `SocksTunnel.xcodeproj`
2. `xcodebuild ... CODE_SIGNING_ALLOWED=NO` → unsigned `.appex`
3. `codesign --entitlements VpnExtension/SocksTunnelExtension.entitlements`
   (ad-hoc by default; override with `VPN_CODESIGN_IDENTITY=...`)

The build is deliberately unsigned inside Xcode because
`com.apple.developer.networking.networkextension` is a restricted entitlement:
Xcode refuses to sign it without a provisioning profile. The Makefile performs
the signing so the same flow works for ad-hoc development and for a real
identity + profile later (see T6.4 in `todo.md`).

## Files

- `project.yml`: xcodegen spec (source of truth for the extension target).
- `SocksTunnelControl.swift`: app-side controller (reference; the app currently
  drives `NETunnelProviderManager` from `src-tauri/native/packet_tunnel.m`).
- `VpnExtension/PacketTunnelProvider.swift`: extension-side tunnel lifecycle
  and settings.
- `VpnExtension/Info.plist`: extension plist.
- `App.entitlements`: host app entitlements.
- `VpnExtension/SocksTunnelExtension.entitlements`: extension entitlements.

## Required Apple configuration

1. App ID + extension App ID, both with the NetworkExtension capability enabled.
2. A provisioning profile that authorises `packet-tunnel-provider`.
3. Host app and extension must share the App Group `group.com.tosone.socks`.
4. Embed the built `.appex` under `Contents/PlugIns/` (handled by
   `make extension-embed`).

## Data plane gap

The packet data plane is intentionally not stubbed as successful. The extension
must connect `NEPacketTunnelFlow` to a Shadowsocks transport before it can
replace the helper at runtime.

The plan (see `todo.md` M2/M3) is to compile a Rust core statically into the
extension and bridge packets over a C ABI:

- Reuse `shadowsocks-service`'s `local-tun` (`TcpTun` / `UdpTun` / `smoltcp`).
- Replace the real `utun` device with a virtual device backed by
  `NEPacketTunnelFlow`.
- Batch packets across the FFI boundary.
