import Foundation
import NetworkExtension

final class PacketTunnelProvider: NEPacketTunnelProvider {
  private enum ConfigKey {
    static let tunnelId = "id"
    static let transport = "transport"
  }

  private var relay: PacketRelay?
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
    guard let transportConfig = protocolConfig.providerConfiguration?[ConfigKey.transport] as? String,
      !transportConfig.isEmpty
    else {
      completionHandler(TunnelProviderError.invalidConfiguration("Missing transport config."))
      return
    }

    let settings = Self.networkSettings()
    setTunnelNetworkSettings(settings) { [weak self] error in
      if let error {
        completionHandler(error)
        return
      }
      guard let self else {
        completionHandler(TunnelProviderError.dataPlaneUnavailable("Tunnel provider was released."))
        return
      }
      do {
        self.relay = try Self.makeRelay(
          tunnelId: tunnelId,
          transportConfig: transportConfig,
          packetFlow: self.packetFlow
        )
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

  private static func networkSettings() -> NEPacketTunnelNetworkSettings {
    let settings = NEPacketTunnelNetworkSettings(tunnelRemoteAddress: "::")
    let vpnAddress = selectVpnAddress(interfaceAddresses: networkInterfaceAddresses())
    let ipv4Settings = NEIPv4Settings(addresses: [vpnAddress], subnetMasks: ["255.255.255.0"])
    ipv4Settings.includedRoutes = [NEIPv4Route.default()]
    ipv4Settings.excludedRoutes = excludedIpv4Routes()
    settings.ipv4Settings = ipv4Settings
    settings.dnsSettings = NEDNSSettings(servers: ["169.254.113.53"])
    return settings
  }

  private static func makeRelay(
    tunnelId: String,
    transportConfig: String,
    packetFlow: NEPacketTunnelFlow
  ) throws -> PacketRelay {
    throw TunnelProviderError.dataPlaneUnavailable(
      "Packet relay is not wired yet. Connect NEPacketTunnelFlow to the Shadowsocks transport before enabling this backend."
    )
  }

  private func removeDefaultPathObserver() {
    if observingDefaultPath {
      removeObserver(self, forKeyPath: "defaultPath")
      observingDefaultPath = false
    }
  }
}

private protocol PacketRelay {
  func stop()
  func notifyNetworkChanged()
}

private enum TunnelProviderError: LocalizedError {
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
