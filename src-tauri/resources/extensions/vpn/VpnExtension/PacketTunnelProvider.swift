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
    /// Optional `[String]` of DNS server addresses to advertise to the system.
    static let dnsServers = "dnsServers"
  }

  /// DNS servers advertised to the system.
  ///
  /// They must be reachable *through* the tunnel and get resolved by the
  /// Shadowsocks server. We deliberately do NOT use a fake link-local address:
  /// `169.254.0.0/16` is in `excludedSubnets`, so queries to such an address
  /// bypass the tunnel and time out (verified on device).
  private static let defaultDnsServers = ["1.1.1.1", "8.8.8.8"]

  private var relay: RustRelay?
  private var observingDefaultPath = false

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

    let dnsServers = (protocolConfig.providerConfiguration?[ConfigKey.dnsServers] as? [String])?
      .filter { !$0.isEmpty } ?? Self.defaultDnsServers

    let network = Self.makeNetwork(dnsServers: dnsServers)
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
    if defaultPath?.status == .satisfied {
      relay?.notifyNetworkChanged()
      reasserting = false
    } else {
      reasserting = true
      setTunnelNetworkSettings(nil) { _ in }
    }
  }

  private static func makeNetwork(dnsServers: [String]) -> TunnelNetwork {
    let settings = NEPacketTunnelNetworkSettings(tunnelRemoteAddress: "::")
    let vpnAddress = selectVpnAddress(interfaceAddresses: networkInterfaceAddresses())
    let subnetMask = "255.255.255.0"
    let ipv4Settings = NEIPv4Settings(addresses: [vpnAddress], subnetMasks: [subnetMask])
    ipv4Settings.includedRoutes = [NEIPv4Route.default()]
    ipv4Settings.excludedRoutes = excludedIpv4Routes()
    settings.ipv4Settings = ipv4Settings
    settings.dnsSettings = NEDNSSettings(servers: dnsServers)
    return TunnelNetwork(address: vpnAddress, subnetMask: subnetMask, settings: settings)
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
