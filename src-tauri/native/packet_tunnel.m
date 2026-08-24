#import <Foundation/Foundation.h>
#import <NetworkExtension/NetworkExtension.h>

static NSString *const SocksTunnelConfigIdKey = @"id";
static NSString *const SocksTunnelConfigTransportKey = @"transport";
static NSString *const SocksTunnelProviderSuffix = @".SocksTunnelExtension";

static void socks_copy_error(NSString *message, char *buffer, uintptr_t buffer_len) {
  if (buffer == NULL || buffer_len == 0) {
    return;
  }

  const char *utf8 = [message UTF8String];
  if (utf8 == NULL) {
    buffer[0] = '\0';
    return;
  }

  strlcpy(buffer, utf8, buffer_len);
}

static NSArray<NETunnelProviderManager *> *socks_load_managers(NSError **error) {
  dispatch_semaphore_t semaphore = dispatch_semaphore_create(0);
  __block NSArray<NETunnelProviderManager *> *result = nil;
  __block NSError *load_error = nil;

  [NETunnelProviderManager loadAllFromPreferencesWithCompletionHandler:^(
                               NSArray<NETunnelProviderManager *> *managers,
                               NSError *manager_error) {
    result = managers;
    load_error = manager_error;
    dispatch_semaphore_signal(semaphore);
  }];
  dispatch_semaphore_wait(semaphore, DISPATCH_TIME_FOREVER);

  if (load_error != nil) {
    if (error != nil) {
      *error = load_error;
    }
    return nil;
  }
  return result ?: @[];
}

static BOOL socks_save_manager(NETunnelProviderManager *manager, NSError **error) {
  dispatch_semaphore_t semaphore = dispatch_semaphore_create(0);
  __block NSError *save_error = nil;

  [manager saveToPreferencesWithCompletionHandler:^(NSError *manager_error) {
    save_error = manager_error;
    dispatch_semaphore_signal(semaphore);
  }];
  dispatch_semaphore_wait(semaphore, DISPATCH_TIME_FOREVER);

  if (save_error != nil) {
    if (error != nil) {
      *error = save_error;
    }
    return NO;
  }
  return YES;
}

static BOOL socks_reload_manager(NETunnelProviderManager *manager, NSError **error) {
  dispatch_semaphore_t semaphore = dispatch_semaphore_create(0);
  __block NSError *load_error = nil;

  [manager loadFromPreferencesWithCompletionHandler:^(NSError *manager_error) {
    load_error = manager_error;
    dispatch_semaphore_signal(semaphore);
  }];
  dispatch_semaphore_wait(semaphore, DISPATCH_TIME_FOREVER);

  if (load_error != nil) {
    if (error != nil) {
      *error = load_error;
    }
    return NO;
  }
  return YES;
}

static BOOL socks_is_active(NEVPNConnection *connection) {
  return connection.status == NEVPNStatusConnecting ||
         connection.status == NEVPNStatusConnected ||
         connection.status == NEVPNStatusReasserting;
}

static NSString *socks_tunnel_id(NETunnelProviderManager *manager) {
  NETunnelProviderProtocol *protocol =
      (NETunnelProviderProtocol *)manager.protocolConfiguration;
  if (![protocol isKindOfClass:[NETunnelProviderProtocol class]]) {
    return nil;
  }
  return protocol.providerConfiguration[SocksTunnelConfigIdKey];
}

int32_t socks_packet_tunnel_start(const char *tunnel_id,
                                  const char *name,
                                  const char *transport_config,
                                  char *error_buffer,
                                  uintptr_t error_buffer_len) {
  @autoreleasepool {
    NSError *error = nil;
    NSArray<NETunnelProviderManager *> *managers = socks_load_managers(&error);
    if (error != nil) {
      socks_copy_error(error.localizedDescription, error_buffer, error_buffer_len);
      return 1;
    }

    for (NETunnelProviderManager *manager in managers) {
      if (socks_is_active(manager.connection)) {
        manager.onDemandRules = nil;
        manager.onDemandEnabled = NO;
        [manager.connection stopVPNTunnel];
      }
    }

    NETunnelProviderManager *manager = managers.firstObject ?: [NETunnelProviderManager new];
    manager.localizedDescription = [NSString stringWithUTF8String:name];
    manager.onDemandRules = nil;
    manager.onDemandEnabled = NO;

    NSString *bundleIdentifier = NSBundle.mainBundle.bundleIdentifier ?: @"com.tosone.socks";
    NETunnelProviderProtocol *protocol = [NETunnelProviderProtocol new];
    protocol.serverAddress = @"socks";
    protocol.providerBundleIdentifier =
        [bundleIdentifier stringByAppendingString:SocksTunnelProviderSuffix];
    protocol.providerConfiguration = @{
      SocksTunnelConfigIdKey : [NSString stringWithUTF8String:tunnel_id],
      SocksTunnelConfigTransportKey : [NSString stringWithUTF8String:transport_config],
    };

    manager.protocolConfiguration = protocol;
    manager.enabled = YES;

    if (!socks_save_manager(manager, &error) || !socks_reload_manager(manager, &error)) {
      socks_copy_error(error.localizedDescription, error_buffer, error_buffer_len);
      return 1;
    }

    NETunnelProviderSession *session = (NETunnelProviderSession *)manager.connection;
    if (![session isKindOfClass:[NETunnelProviderSession class]]) {
      socks_copy_error(@"Invalid tunnel provider session.", error_buffer, error_buffer_len);
      return 1;
    }

    if (![session startTunnelWithOptions:@{} andReturnError:&error]) {
      socks_copy_error(error.localizedDescription, error_buffer, error_buffer_len);
      return 1;
    }
    return 0;
  }
}

int32_t socks_packet_tunnel_stop(const char *tunnel_id,
                                 char *error_buffer,
                                 uintptr_t error_buffer_len) {
  @autoreleasepool {
    NSError *error = nil;
    NSArray<NETunnelProviderManager *> *managers = socks_load_managers(&error);
    if (error != nil) {
      socks_copy_error(error.localizedDescription, error_buffer, error_buffer_len);
      return 1;
    }

    NSString *targetId = [NSString stringWithUTF8String:tunnel_id];
    for (NETunnelProviderManager *manager in managers) {
      if ([socks_tunnel_id(manager) isEqualToString:targetId] && socks_is_active(manager.connection)) {
        manager.onDemandRules = nil;
        manager.onDemandEnabled = NO;
        socks_save_manager(manager, NULL);
        [manager.connection stopVPNTunnel];
      }
    }
    return 0;
  }
}
