import Foundation
import NetworkExtension

/// Packet Tunnel Provider backed by the Rust `socks-core` data plane.
///
/// The extension owns `NEPacketTunnelFlow`; the Rust core owns the userspace
/// TCP/IP + Shadowsocks stack. Packets cross the boundary over the C ABI in
/// `socks_core.h`.
final class PacketTunnelProvider: NEPacketTunnelProvider {
  private enum ConfigKey {
    static let tunnelId = "id"
    static let transport = "transport"
    /// Optional `String` DNS-over-HTTPS endpoint (defaults to `defaultDohURL`).
    static let dohURL = "dohURL"
    /// Optional `String` SNI/certificate name for DNS-over-TLS.
    static let dotServerName = "dotServerName"
    /// Optional `[String]` of DNS server addresses (plain DNS, or DoT endpoints
    /// when `dotServerName` is set).
    static let dnsServers = "dnsServers"
  }

  /// DNS advertised to the system.
  ///
  /// Plain UDP DNS is useless here: this Shadowsocks server does not relay UDP
  /// at all (verified on device: `dig @1.1.1.1` times out, `dig +tcp @1.1.1.1`
  /// succeeds), and the old fake link-local address sits inside the excluded
  /// `169.254.0.0/16` route so it never even reaches the tunnel. DNS-over-HTTPS
  /// rides the working TCP path instead, and needs no packet interception.
  ///
  /// `https://8.8.8.8/dns-query` specifically: the address is an IP, so there is
  /// no chicken-and-egg bootstrap lookup, and Google's certificate carries an
  /// `IP Address:8.8.8.8` SAN, so TLS validation succeeds. Cloudflare's
  /// certificate only has `DNS:cloudflare-dns.com`, which is why
  /// `https://1.1.1.1/dns-query` would fail.
  private static let defaultDohURL = URL(string: "https://8.8.8.8/dns-query")!

  private var relay: RustRelay?
  private var observingDefaultPath = false
  /// Settings currently advertised to the system.
  ///
  /// Kept so they can be re-applied when the default path changes: clearing the
  /// settings while the path is down removes the tunnel routes and
  /// NetworkExtension does not restore them by itself.
  private var tunnelNetwork: TunnelNetwork?

  override func startTunnel(
    options: [String: NSObject]?,
    completionHandler: @escaping (Error?) -> Void
  ) {
    guard let protocolConfig = protocolConfiguration as? NETunnelProviderProtocol else {
      completionHandler(TunnelProviderError.invalidConfiguration("Missing tunnel protocol."))
      return
    }
    guard let tunnelId = protocolConfig.providerConfiguration?[ConfigKey.tunnelId] as? String,
      !tunnelId.isEmpty
    else {
      completionHandler(TunnelProviderError.invalidConfiguration("Missing tunnel id."))
      return
    }
    guard let configJSON = protocolConfig.providerConfiguration?[ConfigKey.transport] as? String,
      !configJSON.isEmpty
    else {
      completionHandler(TunnelProviderError.invalidConfiguration("Missing client config."))
      return
    }

    let network = Self.makeNetwork(dnsSettings: Self.makeDnsSettings(protocolConfig))
    setTunnelNetworkSettings(network.settings) { [weak self] error in
      if let error {
        completionHandler(error)
        return
      }
      guard let self else {
        completionHandler(TunnelProviderError.dataPlaneUnavailable("Tunnel provider was released."))
        return
      }
      do {
        let relay = RustRelay(
          configJSON: configJSON,
          tunnelAddress: network.address,
          subnetMask: network.subnetMask,
          packetFlow: self.packetFlow
        )
        try relay.start()
        self.relay = relay
        self.tunnelNetwork = network
        self.addObserver(self, forKeyPath: "defaultPath", options: [.old], context: nil)
        self.observingDefaultPath = true
        completionHandler(nil)
      } catch {
        completionHandler(error)
      }
    }
  }

  override func stopTunnel(
    with reason: NEProviderStopReason,
    completionHandler: @escaping () -> Void
  ) {
    removeDefaultPathObserver()
    relay?.stop()
    relay = nil
    completionHandler()
  }

  override func observeValue(
    forKeyPath keyPath: String?,
    of object: Any?,
    change: [NSKeyValueChangeKey: Any]?,
    context: UnsafeMutableRawPointer?
  ) {
    guard keyPath == "defaultPath" else {
      return
    }
    guard let currentPath = defaultPath else {
      return
    }

    // macOS and iOS fire this KVO repeatedly for the same path (a known issue
    // since iOS 11 that Outline also works around). Without this guard every
    // duplicate event clears the tunnel settings again, which can leave the
    // tunnel up with no routes at all. `description` carries details that
    // structural equality misses, so compare it (as Outline does). The type is
    // not named explicitly because two `NWPath` types are in scope here.
    if let previous = change?[.oldKey],
      String(describing: previous) == String(describing: currentPath)
    {
      return
    }

    if currentPath.status == .satisfied {
      // Drop in-flight state and pick up the new path.
      relay?.notifyNetworkChanged()

      guard let settings = tunnelNetwork?.settings else {
        reasserting = false
        return
      }
      // Routes and DNS were cleared while the path was down; put them back,
      // keeping the same tunnel address the core was started with.
      setTunnelNetworkSettings(settings) { [weak self] error in
        if let error {
          NSLog("[socks] failed to re-apply tunnel settings: %@", error.localizedDescription)
        }
        self?.reasserting = false
      }
    } else {
      reasserting = true
      setTunnelNetworkSettings(nil) { _ in }
    }
  }

  /// Pick the DNS settings to advertise, preferring (in order): an explicit
  /// DoH URL, an explicit DoT server name, plain DNS servers, then the default
  /// DoH endpoint.
  private static func makeDnsSettings(_ protocolConfig: NETunnelProviderProtocol) -> NEDNSSettings {
    let config = protocolConfig.providerConfiguration
    let servers = (config?[ConfigKey.dnsServers] as? [String])?.filter { !$0.isEmpty } ?? []

    if let raw = config?[ConfigKey.dohURL] as? String,
      let url = URL(string: raw), url.scheme?.lowercased() == "https"
    {
      return makeDohSettings(url: url)
    }

    if let serverName = config?[ConfigKey.dotServerName] as? String, !serverName.isEmpty {
      let settings = NEDNSOverTLSSettings(servers: servers.isEmpty ? ["1.1.1.1"] : servers)
      settings.serverName = serverName
      return settings
    }

    if !servers.isEmpty {
      return NEDNSSettings(servers: servers)
    }

    return makeDohSettings(url: defaultDohURL)
  }

  /// `NEDNSOverHTTPSSettings` only inherits `init(servers:)`; the endpoint is a
  /// property (`servers` is unused for DoH).
  private static func makeDohSettings(url: URL) -> NEDNSOverHTTPSSettings {
    let settings = NEDNSOverHTTPSSettings(servers: [])
    settings.serverURL = url
    return settings
  }

  private static func makeNetwork(dnsSettings: NEDNSSettings) -> TunnelNetwork {
    let settings = NEPacketTunnelNetworkSettings(tunnelRemoteAddress: "::")
    let vpnAddress = selectVpnAddress(interfaceAddresses: networkInterfaceAddresses())
    let subnetMask = "255.255.255.0"
    let ipv4Settings = NEIPv4Settings(addresses: [vpnAddress], subnetMasks: [subnetMask])
    ipv4Settings.includedRoutes = [NEIPv4Route.default()]
    ipv4Settings.excludedRoutes = excludedIpv4Routes()
    settings.ipv4Settings = ipv4Settings
    settings.ipv6Settings = makeIpv6Settings()
    settings.dnsSettings = dnsSettings
    return TunnelNetwork(address: vpnAddress, subnetMask: subnetMask, settings: settings)
  }

  /// Claim all IPv6 traffic so nothing can bypass the tunnel.
  ///
  /// Without this the IPv6 default route stays on the physical interface, so on
  /// an IPv6-capable network every connection would leave unproxied while the
  /// UI claims it is tunnelled. The core cannot relay IPv6 yet (the smoltcp
  /// interface only holds the IPv4 tunnel address), so IPv6 currently
  /// dead-ends and apps fall back to IPv4 through Happy Eyeballs. Leaking is
  /// worse than breaking, and this also puts the routing in place for the
  /// eventual IPv6 support tracked as M4.3 option A.
  private static func makeIpv6Settings() -> NEIPv6Settings {
    let settings = NEIPv6Settings(
      addresses: [vpnIpv6Address],
      networkPrefixLengths: [NSNumber(value: vpnIpv6PrefixLength)]
    )
    settings.includedRoutes = [NEIPv6Route.default()]
    settings.excludedRoutes = excludedIpv6Routes()
    return settings
  }

  private func removeDefaultPathObserver() {
    if observingDefaultPath {
      removeObserver(self, forKeyPath: "defaultPath")
      observingDefaultPath = false
    }
  }
}

private struct TunnelNetwork {
  let address: String
  let subnetMask: String
  let settings: NEPacketTunnelNetworkSettings
}

/// Bridges `NEPacketTunnelFlow` to the Rust `socks-core` data plane.
private final class RustRelay {
  private let configJSON: String
  private let tunnelAddress: String
  private let subnetMask: String
  private let packetFlow: NEPacketTunnelFlow
  private var running = false

  init(configJSON: String, tunnelAddress: String, subnetMask: String, packetFlow: NEPacketTunnelFlow) {
    self.configJSON = configJSON
    self.tunnelAddress = tunnelAddress
    self.subnetMask = subnetMask
    self.packetFlow = packetFlow
  }

  func start() throws {
    let context = Unmanaged.passUnretained(self).toOpaque()
    var errorBuffer = [CChar](repeating: 0, count: 2048)

    let status = configJSON.withCString { configPointer in
      tunnelAddress.withCString { addressPointer in
        subnetMask.withCString { maskPointer in
          socks_core_start(
            configPointer,
            addressPointer,
            maskPointer,
            socksCoreSend,
            socksCoreEvent,
            context,
            &errorBuffer,
            UInt(errorBuffer.count)
          )
        }
      }
    }

    guard status == 0 else {
      throw TunnelProviderError.dataPlaneUnavailable(Self.errorMessage(from: errorBuffer))
    }

    running = true
    readPackets()
  }

  func stop() {
    running = false
    _ = socks_core_stop()
  }

  func notifyNetworkChanged() {
    _ = socks_core_notify_network_changed()
  }

  /// Packets produced by the core, written back to the tunnel.
  func handleOutbound(_ packets: UnsafePointer<SocksPacket>?, count: UInt) {
    guard let packets, count > 0 else {
      return
    }

    var datas = [Data]()
    var protocols = [NSNumber]()
    datas.reserveCapacity(Int(count))
    protocols.reserveCapacity(Int(count))

    for index in 0..<Int(count) {
      let packet = packets[index]
      guard let data = packet.data, packet.len > 0 else {
        continue
      }
      let bytes = Data(bytes: data, count: Int(packet.len))
      let family = bytes.first.map { $0 >> 4 == 6 ? AF_INET6 : AF_INET } ?? AF_INET
      datas.append(bytes)
      protocols.append(NSNumber(value: family))
    }

    guard !datas.isEmpty else {
      return
    }
    packetFlow.writePackets(datas, withProtocols: protocols)
  }

  private func readPackets() {
    guard running else {
      return
    }
    packetFlow.readPackets { [weak self] packets, _ in
      guard let self, self.running else {
        return
      }
      self.push(packets)
      self.readPackets()
    }
  }

  /// Hands a batch of packets read from the tunnel to the core.
  private func push(_ packets: [Data]) {
    let total = packets.reduce(0) { $0 + $1.count }
    guard total > 0 else {
      return
    }

    // The core copies the packets synchronously, so a single scratch buffer is
    // enough to keep every pointer valid for the duration of the call.
    var scratch = [UInt8](repeating: 0, count: total)
    var corePackets = [SocksPacket]()
    corePackets.reserveCapacity(packets.count)

    scratch.withUnsafeMutableBufferPointer { buffer in
      guard let base = buffer.baseAddress else {
        return
      }
      var offset = 0
      for packet in packets where !packet.isEmpty {
        packet.copyBytes(to: base + offset, count: packet.count)
        corePackets.append(SocksPacket(data: UnsafePointer(base + offset), len: UInt(packet.count)))
        offset += packet.count
      }

      _ = corePackets.withUnsafeBufferPointer { pointer in
        socks_core_push(pointer.baseAddress, UInt(corePackets.count))
      }
    }
  }

  private static func errorMessage(from buffer: [CChar]) -> String {
    let message = String(cString: buffer).trimmingCharacters(in: .whitespacesAndNewlines)
    return message.isEmpty ? "Failed to start the data plane." : message
  }
}

/// Must match `SocksSendFn` in `socks_core.h`.
private func socksCoreSend(
  _ packets: UnsafePointer<SocksPacket>?,
  _ count: UInt,
  _ context: UnsafeMutableRawPointer?
) {
  guard let context else {
    return
  }
  let relay = Unmanaged<RustRelay>.fromOpaque(context).takeUnretainedValue()
  relay.handleOutbound(packets, count: count)
}

/// Must match `SocksEventFn` in `socks_core.h`.
private func socksCoreEvent(_ eventJSON: UnsafePointer<CChar>?, _ context: UnsafeMutableRawPointer?) {
  guard let eventJSON else {
    return
  }
  let message = String(cString: eventJSON)
  NSLog("[socks-core] %@", message)
}

enum TunnelProviderError: LocalizedError {
  case invalidConfiguration(String)
  case dataPlaneUnavailable(String)

  var errorDescription: String? {
    switch self {
    case .invalidConfiguration(let message), .dataPlaneUnavailable(let message):
      return message
    }
  }
}

private let vpnSubnetCandidates: [String: String] = [
  "10": "10.111.222.0",
  "172": "172.16.9.1",
  "192": "192.168.20.1",
  "169": "169.254.19.0",
]

private let excludedSubnets = [
  "10.0.0.0/8",
  "100.64.0.0/10",
  "169.254.0.0/16",
  "172.16.0.0/12",
  "192.0.0.0/24",
  "192.0.2.0/24",
  "192.31.196.0/24",
  "192.52.193.0/24",
  "192.88.99.0/24",
  "192.168.0.0/16",
  "192.175.48.0/24",
  "198.18.0.0/15",
  "198.51.100.0/24",
  "203.0.113.0/24",
  "240.0.0.0/4",
]

private func selectVpnAddress(interfaceAddresses: [String]) -> String {
  var candidates = vpnSubnetCandidates
  for address in interfaceAddresses {
    for prefix in vpnSubnetCandidates.keys where address.hasPrefix(prefix) {
      candidates.removeValue(forKey: prefix)
    }
  }
  return (candidates.isEmpty ? vpnSubnetCandidates : candidates).randomElement()!.value
}

private func excludedIpv4Routes() -> [NEIPv4Route] {
  excludedSubnets.compactMap { subnet in
    guard let parsed = Ipv4Subnet(cidr: subnet) else {
      return nil
    }
    return NEIPv4Route(destinationAddress: parsed.address, subnetMask: parsed.mask)
  }
}

/// IPv6 address of the tunnel interface.
///
/// A ULA, deliberately outside `excludedIpv6Subnets` so the tunnel's own subnet
/// stays routed (the same reasoning as the IPv4 tunnel address, which is picked
/// from `vpnSubnetCandidates` to dodge the excluded local ranges).
private let vpnIpv6Address = "fd00:0:0:1::1"
private let vpnIpv6PrefixLength: UInt8 = 64

/// IPv6 ranges that must stay on the physical interface.
///
/// Mirrors what `excludedSubnets` does for IPv4: loopback, unspecified,
/// link-local, unique-local addresses (the IPv6 analogue of RFC 1918),
/// multicast, documentation space and 6to4. Note that `vpnIpv6Address` itself is
/// a ULA and therefore inside `fc00::/7`; that is fine and matches the IPv4
/// setup, where the tunnel address also sits inside an excluded range
/// (e.g. 169.254.19.0 in 169.254.0.0/16) and still works, because macOS keeps
/// the interface's own subnet on the tunnel.
private let excludedIpv6Subnets = [
  "::/128",  // unspecified
  "::1/128",  // loopback
  "fe80::/10",  // link-local
  "fc00::/7",  // unique local addresses
  "ff00::/8",  // multicast
  "2001:db8::/32",  // documentation
  "2002::/16",  // 6to4
]

private func excludedIpv6Routes() -> [NEIPv6Route] {
  excludedIpv6Subnets.compactMap { subnet in
    let parts = subnet.split(separator: "/")
    guard parts.count == 2, let prefix = UInt8(parts[1]) else {
      return nil
    }
    return NEIPv6Route(
      destinationAddress: String(parts[0]),
      networkPrefixLength: NSNumber(value: prefix)
    )
  }
}

private func networkInterfaceAddresses() -> [String] {
  var interfaces: UnsafeMutablePointer<ifaddrs>?
  var addresses: [String] = []
  guard getifaddrs(&interfaces) == 0 else {
    return addresses
  }
  defer { freeifaddrs(interfaces) }

  var current = interfaces
  while current != nil {
    guard let address = current?.pointee.ifa_addr,
      address.pointee.sa_family == UInt8(AF_INET)
    else {
      current = current?.pointee.ifa_next
      continue
    }
    let ipv4 = address.withMemoryRebound(to: sockaddr_in.self, capacity: 1) {
      $0.pointee.sin_addr
    }
    if let value = String(cString: inet_ntoa(ipv4), encoding: .utf8) {
      addresses.append(value)
    }
    current = current?.pointee.ifa_next
  }
  return addresses
}

private struct Ipv4Subnet {
  let address: String
  let mask: String

  init?(cidr: String) {
    let parts = cidr.split(separator: "/")
    guard parts.count == 2,
      let prefix = UInt32(parts[1]),
      prefix <= 32
    else {
      return nil
    }
    address = String(parts[0])
    mask = Self.mask(prefix: prefix)
  }

  private static func mask(prefix: UInt32) -> String {
    let raw = prefix == 0 ? 0 : UInt32.max << (32 - prefix)
    return [
      (raw >> 24) & 0xff,
      (raw >> 16) & 0xff,
      (raw >> 8) & 0xff,
      raw & 0xff,
    ].map(String.init).joined(separator: ".")
  }
}
