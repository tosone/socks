import Foundation
import NetworkExtension

public final class SocksTunnelControl {
  private enum ConfigKey {
    static let tunnelId = "id"
    static let transport = "transport"
  }

  private static let providerSuffix = ".SocksTunnelExtension"

  public init() {}

  public func start(tunnelId: String, name: String, transportConfig: String) async throws {
    if let activeManager = await tunnelManager(), isActive(activeManager.connection) {
      await stop(manager: activeManager)
    }

    let manager = try await setupManager(
      tunnelId: tunnelId,
      name: name,
      transportConfig: transportConfig
    )
    guard let session = manager.connection as? NETunnelProviderSession else {
      throw TunnelControlError.invalidSession
    }

    try session.startTunnel(options: [:])
  }

  public func stop(tunnelId: String) async {
    guard let manager = await tunnelManager(),
      tunnelIdFor(manager: manager) == tunnelId,
      isActive(manager.connection)
    else {
      return
    }
    await stop(manager: manager)
  }

  public func isRunning(tunnelId: String) async -> Bool {
    guard let manager = await tunnelManager() else {
      return false
    }
    return tunnelIdFor(manager: manager) == tunnelId && isActive(manager.connection)
  }

  private func setupManager(
    tunnelId: String,
    name: String,
    transportConfig: String
  ) async throws -> NETunnelProviderManager {
    let managers = try await NETunnelProviderManager.loadAllFromPreferences()
    let manager = managers.first ?? NETunnelProviderManager()
    manager.localizedDescription = name
    manager.onDemandRules = nil

    let protocolConfig = NETunnelProviderProtocol()
    protocolConfig.serverAddress = "socks"
    protocolConfig.providerBundleIdentifier =
      "\(Bundle.main.bundleIdentifier ?? "com.tosone.socks")\(Self.providerSuffix)"
    protocolConfig.providerConfiguration = [
      ConfigKey.tunnelId: tunnelId,
      ConfigKey.transport: transportConfig,
    ]

    manager.protocolConfiguration = protocolConfig
    manager.isEnabled = true
    try await manager.saveToPreferences()
    try await manager.loadFromPreferences()
    return manager
  }

  private func tunnelManager() async -> NETunnelProviderManager? {
    do {
      return try await NETunnelProviderManager.loadAllFromPreferences().first
    } catch {
      return nil
    }
  }

  private func stop(manager: NETunnelProviderManager) async {
    do {
      try await manager.loadFromPreferences()
      manager.isOnDemandEnabled = false
      try await manager.saveToPreferences()
      manager.connection.stopVPNTunnel()
    } catch {
      manager.connection.stopVPNTunnel()
    }
  }

  private func tunnelIdFor(manager: NETunnelProviderManager) -> String? {
    guard let protocolConfig = manager.protocolConfiguration as? NETunnelProviderProtocol else {
      return nil
    }
    return protocolConfig.providerConfiguration?[ConfigKey.tunnelId] as? String
  }

  private func isActive(_ connection: NEVPNConnection) -> Bool {
    switch connection.status {
    case .connected, .connecting, .reasserting:
      return true
    default:
      return false
    }
  }
}

public enum TunnelControlError: Error {
  case invalidSession
}
