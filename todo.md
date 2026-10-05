# TODO：基于 NetworkExtension 重做数据面（不再用 root helper）

> 参考实现：`~/code/github/outline-apps`（Outline，Go + NetworkExtension）
> 目标形态：`~/code/github/outline-apps/new.md`（Tauri + Rust `shadowsocks` + `smoltcp` + `NEPacketTunnelProvider`）
> 范围：**仅 macOS**。iOS 暂不考虑，后续单独立项（见附录 A）。

---

## 0. TL;DR 结论（初次评估）

> 以下是**动手前的**差距判断，保留作为背景。其中第 2、4 条已于 0.5 节完成。

1. **root helper 在代码里其实已经删掉了**（commit `2cb1223`），现在控制面走 `NETunnelProviderManager`。
   残留问题只有：根目录 `README.md` 还在写 LaunchDaemon helper（**文档过期**），以及 `password`/`outline_config` 里的 Outline 遗留。
2. **真正的缺口是扩展里的数据面为空**：`PacketTunnelProvider.swift` 的 `makeRelay()` 直接抛
   `dataPlaneUnavailable`，`NETunnelProviderSession` 起来后没有任何包转发。
3. 所以「用 NetworkExtension 不要用 roothelper」这句话落到工程上 =
   **在 `.appex` 里实现一个 userspace tun2socks**（`NEPacketTunnelFlow` ↔ `shadowsocks`），
   把 `shadowsocks-service` 的 `local-tun` 从「绑定真实 utun fd」改成「绑定虚拟设备」，
   再通过 C ABI 从 Swift 喂包/收包。
4. 另一个被忽略的点：**扩展目前不是由 Xcode 工程构建的**，Makefile 用 `swiftc -emit-library`
   手工拼 `.appex`，`SocksTunnelControl.swift` 根本没进任何 target。这条路撑不到签名/公证/上架。

---

## 0.5 进度快照

### 已完成

- **M1/T1.1** ✅ Xcode 工程化：`project.yml`（xcodegen）→ `xcodebuild` → 统一签名，产出正规 universal `.appex`。
- **M2/T2.1-T2.3** ✅ `core/` crate（`socks-core`）+ cbindgen C ABI + 静态链接进 `.appex`。
- **M3/T3.1-T3.4** ✅ `VirtualDevice` + `TunBuilder::build_with_device`（submodule patch）+ 标准 SS JSON 配置。
- **M3/T3.4** ✅ 集成测试 `core/tests/dataplane.rs`：push 的 TCP SYN 真实建连到 shadowsocks 服务器，并回收 SYN-ACK。
- **M1/T6.4** ✅ portal + 真签名接入构建；**M4.3 IPv6** ✅（意外做成完整支持）；**T4.2 DNS** ✅（方案 C：DoH）。
- **T5.1 网络切换 a+b** ✅（待真机路径切换验证）。
- **日志** ✅ core 的滚动文件日志：`~/Library/Group Containers/group.com.tosone.socks/Logs/socks.log`；20 MiB/文件 × 最多 4 个（`socks.log`/`.1`/`.2`/`.3`，最旧的先删）；默认 `debug`（可选 `error`/`warn`/`info`/`trace`/`off`）；本地时间戳 + 级别 + target；每行 flush。
  - 位置（App Group）通过 `socks_core_start` 新增的 `log_dir` 参数传入；网络切换重启 core 后仍继续写同一文件。
  - 注：日志含**目标 IP/端口**等浏览元数据，对外分享时注意。

关键命令：`make submodule-patch` → `make core` → `make extension`。
测试：`cd core && cargo test`（7 单测 + 1 端到端）；`cd src-tauri && cargo test --lib`。

**明确边界：以上“✅”仅表示代码/离线测试通过，尚未在真机扩展里跑通。**
真机验证受限于 T6.4（NetworkExtension 受限权限 + provisioning profile）。

### 未完成（待办清单）

> 按优先级排序；详细拆解见第 4 节。

**M1 — 扩展生命周期（未做）**

- [ ] T1.2 App Group 共享：`group.com.tosone.socks` 已在 entitlements 声明，但代码未使用；配置/状态/错误共享文件均未实现。
- [ ] T1.3 状态回调：App 侧未观察 `NEVPNStatusDidChange`；无 `vpn://status` 事件；`session.rs` 仍是「调用成功即认为已连接」。
- [ ] T1.4 错误上报：无 `last_error` 持久化，无 `fetchLastDisconnectError`/IPC 兜底，前端拿不到失败原因。
- [ ] T1.5 on-demand：未设 `NEOnDemandRuleConnect`（当前 `onDemandEnabled = NO`）。

**M2 — 遗留问题（未做）**

- [ ] T2.4 core deployment target：`ld` 警告 `built for newer 'macOS' (27.0) than being linked (13.0)`；**不要**全局设 `MACOSX_DEPLOYMENT_TARGET`（会损坏 proc-macro dylib）。

**M3 — 遗留验证（未做，需真机 + NE 权限）**

- [x] 真机验证 TCP 数据面：**已通过**（2026-10-05）。隧道连上后 `curl https://1.1.1.1` → 301、`https://8.8.8.8` → 302、`https://9.9.9.9` → 404，三个 IP 均通 → 扩展加载、`socks_core_start` 成功、smoltcp + shadowsocks 转发生效。
- [x] 真机浏览可用：**已通过**。方案 C（DoH）后，未缓存域名解析 + 访问均成功（`www.wikipedia.org`/`www.rust-lang.org`/`www.apple.com` 全 HTTP 200）；app 的 `check_dns_google` 两步也都能过。
  - 注：长期仍需按 **T4.2 方案 B** 做包级 DNS 拦截（见下）。
- [ ] 验证 ICMP echo reply（smoltcp `auto-icmp-echo-reply`）。

**M4 — UDP + DNS（方案 C 已用 / 方案 B 待做）**

- [ ] T4.1 UDP：`mode` 已是 `TcpAndUdp`，链路上已有 `UdpTun`；缺显式配置、集成测试、超时/并发调参。
- [x] T4.2 **方案 C（当前实现，已真机验证）**：用 `NEDNSOverHTTPSSettings` 把系统 DNS 改成 **DoH `https://8.8.8.8/dns-query`**，全程走 TCP（已可用），**不需拦截、不需假 DNS、不需 NAT**，顺带得到加密 DNS。真机：`www.wikipedia.org`/`www.rust-lang.org`/`www.apple.com`（均未缓存）解析成功且 HTTP 200；`https://8.8.8.8` + `Host: dns.google` → 200；`dns.google` 解析成功。
  - 为什么用 `8.8.8.8` 而不是 `1.1.1.1`：Cloudflare 证书只有 `DNS:cloudflare-dns.com`（无 IP SAN），`https://1.1.1.1/dns-query` 会校验失败；Google 证书含 `IP Address:8.8.8.8`，且用 IP 无 bootstrap 解析问题。
- [x] T4.2 **方案 A（保留为可选覆盖）**：providerConfiguration 的 `dnsServers` 仍可指定明文 DNS；另外支持 `dohURL` / `dotServerName` 覆盖。仅在服务器支持 UDP 或需要调试时用。
- [ ] T4.2 **方案 B（最终目标，后续必须执行）**：保留假 DNS + 在 core 内按目标地址拦截 UDP/53 与 TCP/53，重写为远端 resolver；UDP 不通时回 truncated 逼 OS 走 TCP；DNS 短超时 + 短命 association。
  - 为什么还需要 B：C 只能管**系统 DNS**；若应用自带 DoH/DoT 或直连 IP、或希望“无论系统怎么配都强制走隧道”，需要包级拦截。
- [x] T4.2a 已实测确认：假 DNS `169.254.113.53` 落在被排除的 `169.254.0.0/16`，包不会进隧道（方案 B 必须给 DNS 地址加 /32 included route）。
- [ ] T4.3 IPv6 策略决定。

**M5 — 稳定性（未开始）**

- [ ] T5.1 网络切换：a+b 已实现（待真机路径切换验证）；c 域名重解析未做。
- [ ] T5.2 健康检查：App 侧 `https://8.8.8.8` 的 HTTP 语义未改，未与扩展核心对齐。
- [x] T5.3 流量统计：已实现（方案 A，App Group 共享文件）—— 扩展侧已验证；待 UI 真机确认。
- [ ] T5.4 内存：未做缓冲复用/连接数限制。

**M6 — 打包 / 签名 / 公证（T6.4 已完成 / 公证未做）**

- [x] T6.1 逐层签名（扩展 → 内嵌二进制 → App）—— 已接入构建并验证。
- [ ] T6.2 公证：`notarytool submit --wait` + `stapler staple`。
- [ ] T6.3 CI（macOS runner + 公证）。
- [x] T6.4 **NetworkExtension 受限权限 + provisioning profile**（portal + 签名接入均已完成）
  - [x] **portal 侧完成（2026-10-05，经 Chrome 自动化操作）**
    - 注册 App Group **`group.com.tosone.socks`**
    - 给 `com.tosone.socks` 挂上 App Groups（旧 profile 因此变为 Invalid）
    - 注册 App ID **`com.tosone.socks.SocksTunnelExtension`**（Network Extensions + App Groups）
    - 生成两个 macOS Development profile（均勾本机 `00006020-000E29000E33C01E`，有效期至 2027-10-05）：
      | Profile | App ID | UUID |
      |---|---|---|
      | `socks dev` | `4J2GZTPK37.com.tosone.socks` | `c073e707-1310-40e7-b1cd-1f7f84256fd0` |
      | `socks extension dev` | `4J2GZTPK37.com.tosone.socks.SocksTunnelExtension` | `045fdde2-9123-4f58-a410-edd154fbf392` |
    - 两者均含 `packet-tunnel-provider` + `group.com.tosone.socks`
    - 已安装到 `~/Library/MobileDevice/Provisioning Profiles/`；旧的 invalid `socks` profile 已删
    - 下载副本：`~/Downloads/socks_dev.provisionprofile`、`~/Downloads/socks_extension_dev.provisionprofile`
  - 签名证书：`Apple Development: Tosone Guo (846M4R7XD9)`，Team `4J2GZTPK37`
  - [x] **签名已接进构建（2026-10-05）**
    - 扩展：由 `xcodebuild` 真签名（`CODE_SIGN_STYLE=Manual` + `PROVISIONING_PROFILE_SPECIFIER="socks extension dev"` + `Apple Development: Tosone Guo (846M4R7XD9)`），不再用 `CODE_SIGNING_ALLOWED=NO`；Xcode 自动放入 `embedded.provisionprofile`
    - App：`extension-embed` 里拷入 `socks dev` profile 到 `Contents/embedded.provisionprofile`，再用真身份 + `App.entitlements` + `--options runtime` 重签
    - `App.entitlements` / `SocksTunnelExtension.entitlements` 补上了 `com.apple.application-identifier` 与 `com.apple.developer.team-identifier`
    - Makefile 新增可覆盖变量：`VPN_DEVELOPMENT_TEAM`、`VPN_CODE_SIGN_IDENTITY`、`VPN_PROFILES_DIR`、`VPN_EXT_PROFILE_NAME`、`VPN_APP_PROFILE_NAME`；profile 按 Name 在 `~/Library/MobileDevice/Provisioning Profiles/` 里查找，缺失则报错
    - 验证通过：
      ```
      codesign --verify --strict  → valid on disk / satisfies its Designated Requirement（app + appex）
      app   entitlements: app-id=4J2GZTPK37.com.tosone.socks
                          networkextension=[packet-tunnel-provider]
                          application-groups=[group.com.tosone.socks]
      appex entitlements: app-id=4J2GZTPK37.com.tosone.socks.SocksTunnelExtension（同上）
      两个 bundle 均有 embedded.provisionprofile（socks dev / socks extension dev）
      ```
    - 遗留：app 主二进制是 thin arm64，appex 是 universal；分发前需统一（本地测试不受影响）
  - 注：这是 **Development** profile，只能本机测试；公证分发（T6.2）后续需 **Developer ID Application** + 对应 profile。
  - [x] 已把 profile 装入 `~/Library/MobileDevice/Provisioning Profiles/ac8a0f09-19d33-...provisionprofile`。
  - [ ] 接上 Makefile/xcodegen：`DEVELOPMENT_TEAM=4J2GZTPK37` + app/扩展两个 profile + 真签名（不再 ad-hoc）。
  - 注：这是 **Development** profile，只能本机测试；公证分发（T6.2）后续需 **Developer ID Application** + 对应 profile。

**清理项（未做）**

- [ ] 根目录 `README.md` 仍在描述 LaunchDaemon helper（过期文档）。
- [ ] `SocksTunnelControl.swift` 仍是死代码（未被任何 target 引用）。
- [ ] SIP003 插件（`v2ray-plugin`）未做，且警告会忽略 `plugin`/`plugin_opts`。
- [ ] `server_installer.rs`/`proxy_installer.rs`/`systemd`/`build/*` 与服务端强耦合，未确认与本客户端打包解耦。

---

## 1. 现状 vs Outline 差距

> 状态：✅ 已做（离线验证）；🟡 部分；🔴 未做。具体见第 0.5 节未完成清单与第 4 节。

| 维度 | Outline（参考实现） | socks 现状 | 状态 |
|---|---|---|---|
| 系统 VPN 类型 | `NEPacketTunnelProvider` (`.appex`) | 同名骨架 | ✅ |
| 隧道设置 | 随机 VPN 地址、默认路由、排除网段、假 DNS `169.254.113.53` | 已实现 | ✅ |
| **数据面** | Go `tun2socks` | Rust `socks-core`（smoltcp + `shadowsocks-service`），离线 e2e 通过 | ✅（真机未验证）|
| 扩展工程 | Xcode 工程 + framework | xcodegen + xcodebuild + 统一签名 | ✅ |
| 配置文件格式 | Outline transport YAML | 标准 SS JSON（`client_config.rs`） | ✅ |
| App 侧控制 | `OutlineVpn.swift`：状态观察 + on-demand + IPC | `packet_tunnel.m`：同步阻塞，无状态观察/on-demand | 🔴 T1.3-T1.5 |
| 网络切换 | KVO → `notifyNetworkChanged()` + `reasserting` | KVO 已接上 core，但 core 侧是 no-op | 🟡 T5.1 |
| 共享存储 | App Group `group.org.getoutline.client` | entitlements 已声明，代码未用 | 🔴 T1.2 |
| 错误上报 | `saveLastError` + `fetchLastDisconnectError`（含 IPC 兜底） | 无 | 🔴 T1.4 |
| DNS | 假 DNS + UDP/53 拦截 + truncate 兜底 | **方案 C：DoH `https://8.8.8.8/dns-query`（已真机验证）**；方案 B 的拦截未做 | 🟡 T4.2（**方案 B 待做**）|
| UDP | 完整 UDP 关联（lwIP + PacketRelay） | `UdpTun` 已在链路上且 `mode` 默认 `TcpAndUdp`，但无测试/无显式配置 | 🟡 T4.1 |
| 健康检查 | 扩展内 `getHealthStatus()` | App 侧 HTTP `https://8.8.8.8` | 🔴 T5.2 |
| 流量统计 | 有 | 已实现（core 计数 → `traffic.json` → app 轮询 → `traffic` 事件）| ✅ 待 UI 验证 |
| 公证/装订 | 完整 | 仅 ad-hoc 签名 | 🔴 M6 |
| iOS | 支持 | macOS only | **不在当前范围**（附录 A）|

---

## 2. 目标架构

```
┌──────────── Tauri App (macOS) ────────────┐
│ React UI  ── invoke/emit ──  Rust backend │
│   Rust backend:                           │
│     - profiles / ss:// 解析               │
│     - VPN 控制: shim → NETunnelProviderManager (状态观察 + on-demand + IPC)
│     - 状态/流量: 来自扩展 IPC + App Group │
└───────────────────────┬───────────────────┘
                        │ App Group (group.com.tosone.socks)
                        │   config.json / status.json / last_error.json
┌───────────────────────▼───────────────────┐
│  SocksTunnelExtension (.appex)            │
│  Swift: PacketTunnelProvider             │
│    - NEPacketTunnelNetworkSettings        │
│    - packetFlow.readPackets/writePackets  │
│         │ C ABI (批量 packet + 回调)      │
│  Rust: libsocks_core (staticlib)          │
│    - VirtualDevice (AsyncRead+Write)      │
│    - shadowsocks-service TcpTun/UdpTun    │
│    - smoltcp Interface                    │
│    - DNS 拦截 (UDP/53 → 远端)             │
└───────────────────────┬───────────────────┘
                        │ Shadowsocks (AEAD / 2022)
                 Shadowsocks Server
```

**为什么必须在扩展里跑 Rust 核心（而不是再起一个 sslocal 进程）：**

- NetworkExtension 的 `packetFlow` 只给 Swift/ObjC API，**没有 fd**，无法像传统 `sslocal --tun` 那样把 utun 交给别的进程。
- macOS 扩展作为 appex 运行在沙箱中，依赖外部可执行文件（如 `sslocal`）会引入签名/公证/沙箱路径等一堆麻烦。
- 因此数据面必须**编译进扩展**，通过 C ABI 与 Swift 交互（对应 new.md 的 `core/` + cbindgen）。
- （iOS 有更严格的进程限制，但当前范围只做 macOS；见附录 A。）

> `shadowsocks-rust` 的 `local-tun` 已经用 `smoltcp` 实现了 `TcpTun` / `UdpTun` / `VirtTunDevice`，
> **不要从零写**；只需把它绑定的「真实 tun 设备」换成「`NEPacketTunnelFlow` 虚拟设备」。

---

## 3. Roadmap（里程碑）

| 阶段 | 目标 | 验收 |
|---|---|---|
| M1 | 扩展能加载、能启停、能上报状态与错误 | 连接后 `NEVPNStatus == .connected`，App 能拿到状态与最近错误 |
| M2 | Rust 核心能编译进 `.appex`，C ABI 握手成功 | 扩展进程内 `socks_core_start` 返回 0 |
| M3 | 单条 TCP 走通 | `curl https://example.com` 经隧道成功 |
| M4 | UDP + DNS | `dig`/浏览器域名解析经代理成功 |
| M5 | 网络切换 / 重连 / 健康检查 | 切换 Wi-Fi↔热点 后自动恢复 |
| M6 | 打包、签名、公证 | `notarytool` 通过、`stapler` 成功 |

> 全部里程碑仅针对 macOS。iOS 不在当前范围（见附录 A）。

---

## 4. 待办清单

### M1 · 扩展工程化与生命周期（T1.1 ✅ / T1.2-T1.5 未做）

- [x] **T1.1 用 Xcode 工程替代 `swiftc` 手工打包**
  - 新建 `src-tauri/resources/extensions/vpn/SocksTunnel.xcodeproj`（或 `project.yml` + xcodegen）。
  - target：macOS App Extension / Packet Tunnel Provider，bundle id `com.tosone.socks.SocksTunnelExtension`。
  - 把 `VpnExtension/PacketTunnelProvider.swift`、`Info.plist`、`SocksTunnelExtension.entitlements` 纳入 target。
  - 修掉当前 `swiftc -emit-library` 生成 dylib 的问题（appex 的可执行文件应是 executable）。
  - 保留 Makefile 的 `extension-build` 走 `xcodebuild`，删除/收敛 `extension-package` 兜底逻辑。
- [ ] **T1.2 扩展通过 App Group 与 App 交换配置**
  - 统一 group：`group.com.tosone.socks`（entitlements 已有，代码补上）。
  - 约定文件：`config.json`（活动 profile 的 SS 配置）、`status.json`、`last_error.json`。
  - 扩展 `startTunnel` 从 App Group 读配置（替代只依赖 `providerConfiguration`）。
- [ ] **T1.3 打通状态回调**
  - App 侧用 `NEVPNStatusDidChange` 观察（替代 `packet_tunnel.m` 的一次性同步调用）。
  - 新增 Tauri event：`vpn://status { state, profileId, message }`。
  - `session.rs` 改为以扩展真实状态为准，而不是「调用成功就认为已连接」。
- [ ] **T1.4 错误上报（对齐 Outline）**
  - 扩展保存 `last_error`（UserDefaults + App Group 文件）。
  - macOS 13+ 用 `fetchLastDisconnectError`；老系统用 `sendProviderMessage` IPC 兜底。
  - App 把错误透传到前端 `vpn://error` / 现有 ErrorDialog。
- [ ] **T1.5 on-demand 规则**
  - 连接成功后设置 `NEOnDemandRuleConnect`（`.any`），参考 Outline `OutlineVpn.start`。
  - 失败前先关闭 on-demand，避免系统无限重试。

### M2 · Rust 核心 crate 与 C ABI（T2.1-T2.3 ✅ / T2.4 未做）

- [x] **T2.1 新建 `core/`（或 `src-tauri/core`）crate**
  - `Cargo.toml`：`crate-type = ["staticlib", "cdylib", "rlib"]`。
  - 依赖：`shadowsocks`（本仓 submodule）、`shadowsocks-service`（features `local-tun`）、`tokio`、`smoltcp`、`serde`。
  - 交叉编译 target（仅 macOS）：`aarch64-apple-darwin` / `x86_64-apple-darwin`，再用 `lipo` 合成通用二进制。
- [x] **T2.2 定义 C ABI（`cbindgen` 生成头文件）**
  ```c
  typedef struct { const uint8_t *data; size_t len; } socks_packet;

  // Swift 实现，Rust 通过它把回包写回隧道
  typedef void (*socks_send_fn)(const socks_packet *packets, size_t count, void *ctx);
  typedef void (*socks_event_fn)(const char *json, void *ctx);   // status/traffic/error

  int  socks_core_start(const char *config_json, socks_send_fn send, socks_event_fn event, void *ctx);
  int  socks_core_push(const socks_packet *packets, size_t count);
  int  socks_core_stop(void);
  int  socks_core_notify_network_changed(void);
  ```
  - **必须批量传包**，避免每包一次 FFI。
  - 核心侧独立线程 + Tokio runtime，不阻塞 `readPackets` 回调。
  - 错误用返回码 + event 回调，不跨语言抛异常。
- [x] **T2.3 把核心编进扩展**
  - 静态库链接进 `.appex`（Xcode target 增加 `-lsocks_core` / link static lib）。
  - `build.rs` 或脚本：`cargo build --release --target ...` + `lipo`（macOS 通用二进制）。
- [ ] **T2.4 修正 core 的 deployment target**
  - 现象：`ld` 警告 `object file ... was built for newer 'macOS' version (27.0) than being linked (13.0)`。
  - 原因：`cc`/cmake 编译的 C 依赖（aws-lc、blake3）默认用 SDK 版本。
  - **坑**：直接全局 `MACOSX_DEPLOYMENT_TARGET=13.0` 会让 Xcode 27 生成损坏的 proc-macro dylib（`mis-aligned LINKEDIT string pool`），**不要这么做**。
  - 方向：只对 C 编译设 `CFLAGS_<target>` / cmake 变量，或升级工具链后重试。

### M3 · 数据面：TCP（T3.1-T3.4 ✅ / 真机验证 ✅）

- [x] **T3.1 实现 `VirtualDevice`（`AsyncRead + AsyncWrite`）**
  - `read()`：从 `socks_core_push` 攒下的队列取 IP 包。
  - `write()`：调用 `socks_send_fn` 把回包交给 Swift → `packetFlow.writePackets`。
  - 参考 `shadowsocks-service/src/local/tun/virt_device.rs` 的 `VirtTunDevice` 设计。
- [x] **T3.2 让 `shadowsocks-service` 支持外部注入设备**
  - 采用方案 A：在 `local/tun/mod.rs` 新增 `TunStream` trait + `Device` 枚举 + `TunBuilder::build_with_device()`；`Tun`/`Server` 的对外类型不变。
  - patch 记录在 `patches/shadowsocks-rust-tun-device-injection.patch`，由 `make submodule-patch` 幂等应用（`core-build` 会自动先跑）。
  - 后续升级 submodule 时需 rebase 该 patch；理想情况是把注入 API 提 PR 回上游。
- [x] **T3.3 配置来源改为标准 Shadowsocks**
  - 删除/替换 `src-tauri/src/outline_config.rs` 的 Outline transport YAML。
  - 改为 `{server, server_port, method, password[, plugin, plugin_opts]}` 结构体（SIP002 解析也放这里）。
  - `core` 侧用它构造 `shadowsocks::config::ServerConfig` / `ServiceContext`。
- [x] **T3.4 TCP 冒烟测试（core 层）**
  - `core/tests/dataplane.rs` 已验证：push SYN → 建立到 SS 服务器的连接 → 回 SYN-ACK。
  - [x] 真机 TCP 验证：已通过（`curl` 三个 IP 均返回 HTTP 响应）。
  - [x] 真机浏览验证：**已通过**（配合 T4.2 方案 A 的真实 resolver）—— `dig example.com` NOERROR、`curl https://example.com` 与 `https://www.cloudflare.com` 均 HTTP 200。
  - [ ] 验证 `TcpTun.drive_interface_state` 的 ICMP echo reply。

### M4 · UDP + DNS（部分已完成 / DNS 未做）

> Outline 的参照实现（已核对代码，非 `new.md` 描述）：
> - Apple 路径用户态栈是 **lwIP**（`outline-sdk/network/lwip2transport.ConfigureDeviceWithRelay(sd, pr)`），
>   TCP 走 `transport.StreamDialer`，UDP 走 `transport.PacketRelay`；`go-tun2socks`(gVisor) 只在 electron/Windows 用。
> - DNS 不是“把假 DNS 转发一下就完”——而是：
>   1. `NEDNSSettings = [169.254.113.53]`；
>   2. 在 packet relay / stream dialer 层**按目标地址拦截** `169.254.113.53:53`（UDP 和 TCP 都拦）；
>   3. 重写目标为**随机选定的公共 resolver**（`1.1.1.1` / `9.9.9.9` / `208.67.222.222` / `208.67.220.220`）；
>   4. DNS 用独立的 5s 超时（普通 relay 30s），每条查询用短命 association；
>   5. **UDP 健康门控**：`CheckUDPConnectivity` 失败时改返回 *truncated* DNS 响应，逼 OS 改用 TCP/53 重试。

- [x] **T4.1 UDP 关联（已默认打开，待验证）**
  - `core` 的 `mode` 默认就是 `Mode::TcpAndUdp`（`StartConfig::mode()`），`UdpTun` 已在链路上。
  - [ ] 显式化：在 `client_config.rs` 输出 `mode`，不要依赖默认值。
  - [ ] 加 UDP 集成测试（对应 `core/tests/dataplane.rs` 的 UDP 版）。
  - [ ] 调 `udp_timeout` / `udp_max_associations`（已透传到 `TunBuilder`，但 app 未传）。
- [x] **T4.2 DNS（已解决：方案 C）**
  - 实测背景：TCP 通、UDP 全不通。`dig @1.1.1.1` 超时8/8，`dig @8.8.8.8` 也超时，但 `dig +tcp @1.1.1.1` NOERROR，DoH 也通 → **该 SS 服务器不中继 UDP**。
  - [x] **方案 C（当前实现，已真机验证）**：把 `NEDNSSettings` 换成 `NEDNSOverHTTPSSettings(serverURL: https://8.8.8.8/dns-query)`，系统全程用 TCP 发 DNS，走已可用的 TCP 路径，**无需拦截/假 DNS/NAT**。
    - 真机结果：三个全新域名（`www.wikipedia.org` / `www.rust-lang.org` / `www.apple.com`）均解析成功且 HTTP 200；`https://8.8.8.8` + `Host: dns.google` → 200；`dns.google` 解析成功。
    - 选 `8.8.8.8` 而不选 `1.1.1.1`：经隧道实测 Cloudflare 证书 SAN 只有 `DNS:cloudflare-dns.com`（无 IP SAN）→ `https://1.1.1.1/dns-query` 会 TLS 校验失败；Google 证书含 `IP Address:8.8.8.8`，且 URL 用 IP 无 bootstrap 解析问题。
    - 代码：`VpnExtension/PacketTunnelProvider.swift`（`defaultDohURL` + `makeDohSettings` + `makeDnsSettings`）。
    - 覆盖入口（providerConfiguration）：`dohURL`（DoH 地址）、`dotServerName` + `dnsServers`（DoT）、`dnsServers`（明文 DNS）。
  - [x] **方案 A 降级为可选覆盖**：`dnsServers` 仍可指定明文 resolver；仅在服务器支持 UDP 或调试时用。（早前一次“DNS 成功”其实是 macOS 缓存命中，不是真的走出了隧道。）
  - [ ] **方案 B（最终目标，后续必须执行）**：回到 Outline 同款设计 —— 保留假 DNS `169.254.113.53` + 在 core 里按目标地址拦截。
    - [ ] 拦截 UDP/53 与 TCP/53 到假 DNS → 重写为远端 resolver。
    - [ ] resolver 选择：硬编码公共 resolver（随机）还是可配置。
    - [ ] 已无 UDP 可用，truncated 响应（逼 OS 走 TCP/53）将是**主路径**而不是兑底。
    - [ ] DNS 短超时（~5s）+ 短命 association，避免端口耗尽。
    - [ ] 前提：给假 DNS 地址加一条 `/32` included route（见 T4.2a）。
    - [ ] 为什么还需要 B：C 只管**系统 DNS**；应用自带 DoH/DoT、或直接写 IP 的场景无法控制。
  - [x] **T4.2a 已实测确认**：`169.254.113.53` 落在被排除的 `169.254.0.0/16`，假 DNS 的包**确实不会进隧道**。方案 B 必须给 DNS 地址加一条 /32 included route（或从 excluded 里挖掉该 /32）。
- [x] **T4.3 IPv6（已完成，且是完整支持，不只是堵塞）**
  - 背景实测：原本完全没设 IPv6 → IPv6 默认路由留在物理网卡（潜在泄漏）。本机网络的 IPv6 当时连不出去，所以没有实际泄漏；但配置是错的。
  - 实现：`NEIPv6Settings`（隧道地址 `fd00:0:0:1::1/64` + `::/0` included + 排除 `::/128`、`::1/128`、`fe80::/10`、`fc00::/7`、`ff00::/8`、`2001:db8::/32`、`2002::/16`）。
  - 结果（超出预期）：**IPv6 直接可用且被代理**。原因：smoltcp 已启用 `proto-ipv6`、`TcpTun` 用 `IpAddr` 通用处理、Swift 回包已按 IP 版本号选 `AF_INET6`，接上路由就通。
    - 真机：IPv6 出口 = `2406:da18:8e5:6100:...`（SS 服务器的 AWS 新加坡 IPv6），**不是**本机 ISP 的 `2408:8207:...` → 无泄漏
    - IPv4 出口 = `18.140.64.254`；`www.wikipedia.org` 200、`www.rust-lang.org` 301
    - 路由：IPv6 default → utun10（主），en0 的变为 `ifscope`
  - [ ] 已知问题（待评估）：隧道开启时 **对任意 IPv6 地址** 的 ICMP echo 都会被本地合成回应（`ping6 2001:db8::1` 也是 0% 丢包、RTT ~0.8ms）→ `ping` 有假阳性。来源是上游 smoltcp 的 `auto-icmp-echo-reply`；IPv4 需同样验证。

### M5 · 稳定性（未开始）

- [ ] **T5.1 网络切换（a+b 已实现，待真机切换验证）**
  - 原缺口（已修）：
    1. 路径不可用时 `setTunnelNetworkSettings(nil)` 会**清掉隧道路由**，恢复时只把 `reasserting` 设回 false → 路由永远不会回来 → 显示已连接但流量绕过隧道（既不通也是泄漏）
    2. `socks_core_notify_network_changed()` 是 no-op，in-flight smoltcp 状态不重置
    3. 没有 KVO 假阳性防护（iOS 11 起同一路径会重复触发）
  - [x] **a. Swift 路径处理**（`PacketTunnelProvider.swift`）：
    - 假阳性过滤：比较 `change[.oldKey]` 与当前 path 的 `description`（Outline 同款；因作用域内有两个 `NWPath` 类型，故不显式写类型名）
    - 网络恢复时**重新下发 `setTunnelNetworkSettings`**，复用启动时那份 `TunnelNetwork`，保证**隧道地址不变**（与 core 启动时用的地址一致）
  - [x] **b. core 自重启**（`core/src/engine.rs`）：`socks_core_notify_network_changed()` 用保存的 `StartParams`（config/地址/掩码/回调/ctx）**重启整个数据面** —— 一次性丢掉所有 in-flight TCP/UDP 与出站 socket；好处是不用再给 `TcpTun` 加 reset hook（避免又一次 submodule patch）
    - 集成测试覆盖：`core/tests/dataplane.rs` 推 SYN → 服务器收到连接 → `notify_network_changed()` → 再推一个 SYN → **第二次连接也到达**（证明重启后可用）
    - 注：重启期间 `socks_core_push` 返回非 0，Swift 忽略；中断约 100ms
  - [ ] **真机验证（待做）**：切换主网络（拔插网线 / 关开 Wi-Fi / 休眠唤醒）后确认路由自动恢复、无需手动重连
  - [ ] **c. 服务器地址为域名时的 bootstrap 重解析**（未做）：core 目前完全没设 DNS resolver，换网后重解析不能用系统 DNS（会被自己的隧道困住），需要一个绑定物理接口的 resolver
  - [ ] d. 健康检查（属 T5.2）
- [ ] **T5.2 健康检查**
  - 把 App 侧的 `check_dns_google`（`session.rs`）与扩展核心的连通性对齐。
  - 建议：扩展 `startTunnel` 里做一次健康检查，失败则 `cancelTunnelWithError`，App 通过 `fetchLastDisconnectError` 拿到原因。
  - 移除 App 侧「HTTP 成功才算连接」的语义（应改为状态回调驱动）。
- [x] **T5.3 流量统计（已实现，待 UI 真机确认）**
  - 背景：从 `2cb1223` 删掉 root helper 起就彻底失效了 —— `d8beea5` 把流量轮询搬进 helper，`2cb1223` 把 helper 删了，`emit("traffic")` 一起没了。所以速度和累计一直是 0，`traffic/<id>.json` 从未写过。
  - 链路：**core 计数 → 事件回调 → 扩展写 App Group 文件 → app 轮询 → `traffic` 事件 → 前端**
    - [x] **core**：`TrafficCounters`（`push` 计 tx = 上传；send 回调计 rx = 下载），每 1s 经现有 event 回调发 `{"type":"traffic","tx":..,"rx":..}`（空闲也发，让速度能回 0）。注：`shadowsocks-service` 的 `FlowStat` 只在 server 侧自增，tun 路径用不了（已 grep 验证）。
    - [x] **扩展**：`RustRelay.handleEvent` 把 `traffic` 事件原子写入 `<App Group>/traffic.json`（含 `updatedAtMs`，便于识别被杀的扩展留下的陈旧文件）。
    - [x] **app**：`src-tauri/src/traffic.rs` 轮询该文件 → 差值算 bps → 累加 lifetime totals（每 10s 及断开时落盘）→ `emit("traffic", ...)`。计数器变小 = 数据面重启 → 当作基线重置。
    - [x] **前端**：无需改动。
  - [x] `packet_tunnel.m` 新增 `socks_shared_container_path`（app 是沙箱进程，必须问系统要 App Group 容器路径，不能自己拼）。
  - 已验证：扩展侧计数递增（浏览后 rx 54KB → 8.17MB）；差值/stale 检测/文件格式均有单测；集成测试断言 `traffic` 事件的 tx 与 rx 均 > 0。
  - [ ] **待 UI 真机确认**：点连接后卡片右上角应显示实时 ↑/↓ 速度，点一下变累计；断开后累计持久化。
  - 注：日志与 `traffic.json` 同在 App Group，为 T1.2/T1.3（状态与错误上报）铺好了路。
- [ ] **T5.4 内存**
  - 扩展内存/资源受限——参考 outline 的 GC 调优思路，Rust 侧控制缓冲与连接数、复用包缓冲（`TokenBuffer` 池）。
  - 为将来 iOS 的 ~50MB 上限留余地（见附录 A）。

### M6 · 打包 / 签名 / 公证（T6.4 ✅ / 公证未做）

- [x] **T6.1 逐层签名**：已完成 —— 扩展由 xcodebuild 真签名（带 profile），app 由 `extension-embed` 拷入 profile + 真身份重签；`codesign --verify --strict` 通过。
- [x] **T6.1b dmg 顺序修复**：Tauri 的 dmg bundler 跑在 `extension-embed` **之前**，会打出「无 appex、无 entitlement、ad-hoc」的坏 app（装上去后连 VPN 会报 `permission denied`）。现改为：`bundle.targets = ["app"]`（Tauri 只出 .app）+ Makefile 新增 `dmg` target（hdiutil，在 embed 后执行）；`make tauri` = extension → tauri build → extension-embed → dmg。已验证 dmg 内含 appex + 两个 embedded.provisionprofile + 真签名 + NE entitlements。
- [ ] **T6.2 公证**：`xcrun notarytool submit --wait` + `xcrun stapler staple`。
- [ ] **T6.3 CI**：macOS runner 上跑 Rust 交叉编译 + Tauri + 扩展 + 公证。
- [ ] **T6.4 NetworkExtension 受限权限**：portal 侧已完成（App Group / 扩展 App ID / 两个 development profile 均已创建并安装，见 0.5 节）；**剩签名接入构建**。

### M7 · iOS（暂缓）

> 当前只做 macOS，本节不执行。iOS 的 Xcode target、交叉编译、UI 适配、deep-link 推迟到 macOS 数据面稳定后单独立项（要点见附录 A）。

### 清理项（未做）

- [ ] 更新 `README.md`：删除 LaunchDaemon helper / 路由 / PF DNS 的过期描述，改为 NE 架构。
- [ ] 处理 `src-tauri/resources/extensions/vpn/SocksTunnelControl.swift`：要么纳入 App target 并用它替代 `packet_tunnel.m`，要么删除（当前是死代码）。
- [ ] 评估 `v2ray-plugin`（SIP003）：在扩展沙箱内运行插件需要打包独立可执行文件并单独签名/公证，复杂度高；MVP 先不做。
- [ ] `server_installer.rs` / `proxy_installer.rs` / `systemd` / `build/*`：与服务端相关，确认与本客户端打包解耦。

---

## 5. 关键技术决策（记录）

| 决策 | 选择 | 理由 |
|---|---|---|
| 用户态栈 | `smoltcp`（复用 shadowsocks-service） | 已有 `TcpTun`/`UdpTun` 生产实现，避免重写 |
| 与 Service 集成方式 | 小改 submodule 暴露「注入设备」API（方案 A） | 远比复制代码可维护 |
| FFI 形式 | C ABI + cbindgen，**批量 packet** | 避免每包 FFI；new.md 明确要求 |
| 配置格式 | 标准 Shadowsocks JSON（弃 Outline transport YAML） | 本产品是标准 SS 客户端 |
| 扩展构建 | Xcode 工程（xcodebuild） | `swiftc` 手工拼 appex 撑不到签名/公证 |
| App↔扩展通信 | 状态回调 + App Group + IPC | 扩展可被系统回收，不能只靠内存状态 |
| IPv6 | 待定（M4.3） | 影响 excluded routes 与 DNS |

---

## 6. 风险

1. **`NEPacketTunnelFlow ↔ smoltcp` 胶水**是最大工作量与 bug 来源（尤其 UDP/DNS）。
2. **submodule patch 与上游升级冲突** —— 需要固定 fork 或把注入 API 提 PR 回上游。
3. **扩展内存/资源受限**：包缓冲、连接数、Rust 分配器都要盯（将来 iOS 更紧）。
4. **NetworkExtension 权限审批**有周期，阻塞发版，尽早申请。
5. **`swiftc` 产物形态**可能一直是坏的，M1 必须先验证扩展真能被系统加载。
6. **SIP003 插件**在扩展沙箱内运行需要额外打包可执行文件并处理签名/公证，复杂度高。

---

## 7. 完成定义（DoD）

- [ ] `make tauri` 一条命令产出可公证的 `.app`，含已签名的 `.appex`。
- [ ] 连接后系统流量真实经 Shadowsocks，`curl`/浏览器/`dig` 全部走通。
- [ ] 断开/退出/切换网络不残留路由或 DNS。
- [ ] 前端连接状态、流量速度、总流量、错误提示均来自扩展真实数据。
- [ ] README 与代码一致，无 root helper 残留描述。

---

## 附录 A · iOS（后续单独立项，当前不做）

macOS 打通后再启动，要点预记录：

- `cargo tauri ios init` → 生成 `src-tauri/gen/apple/` Xcode 工程，加入 VpnExtension target（建议用 xcodegen + `project.yml` 维护，避免手改生成文件被覆盖）。
- 同一 `core` 交叉编译到 `aarch64-apple-ios` / `aarch64-apple-ios-sim`。
- UI：安全区 `env(safe-area-inset-*)`、触控目标 ≥ 44pt、键盘弹出行为。
- `ss://` deep-link：URL Scheme / Universal Links。
- iOS 扩展内存约 50MB 上限，对 UDP/DNS 缓冲与连接数更敏感。
- 分发只能走 App Store / TestFlight（无官网直发 + 公证）。
- 架构上 macOS/iOS 共用同一 `NEPacketTunnelProvider` 与 `core`，所以 macOS 阶段的 C ABI 与设备注入应在设计上保持平台无关。
