SHELL := /bin/bash

ROOT_DIR := $(abspath .)
VPN_DIR := $(ROOT_DIR)/src-tauri/resources/extensions/vpn
VPN_APP_SWIFT := $(VPN_DIR)/SocksTunnelControl.swift
VPN_EXTENSION_SWIFT := $(VPN_DIR)/VpnExtension/PacketTunnelProvider.swift
VPN_PROJECT_YML ?= $(VPN_DIR)/project.yml
VPN_XCODEPROJ ?= $(VPN_DIR)/SocksTunnel.xcodeproj
VPN_SCHEME ?= SocksTunnelExtension
VPN_CONFIGURATION ?= Release
VPN_DERIVED_DATA ?= $(ROOT_DIR)/target/xcode
VPN_EXTENSION_OUT_DIR ?= $(ROOT_DIR)/target/extensions/vpn
VPN_MODULE_NAME ?= SocksTunnelExtension
VPN_PRODUCT_NAME ?= SocksTunnelExtension
VPN_BUNDLE_ID ?= com.tosone.socks.SocksTunnelExtension
VPN_APP_ENTITLEMENTS ?= $(VPN_DIR)/App.entitlements
VPN_VERSION ?= 0.1.0
VPN_BUILD ?= 1
VPN_TMP_DIR ?= $(ROOT_DIR)/target/tmp

# --- Signing -----------------------------------------------------------------
# The host app and the extension each need their own provisioning profile
# (NetworkExtension App IDs cannot be wildcards). Install the .provisionprofile
# files by double-clicking them, or drop them into VPN_PROFILES_DIR.
VPN_DEVELOPMENT_TEAM ?= 4J2GZTPK37
VPN_CODE_SIGN_IDENTITY ?= Apple Development: Tosone Guo (846M4R7XD9)
VPN_PROFILES_DIR ?= $(HOME)/Library/MobileDevice/Provisioning Profiles
VPN_EXT_PROFILE_NAME ?= socks extension dev
VPN_APP_PROFILE_NAME ?= socks dev

SUBMODULE_DIR := $(ROOT_DIR)/shadowsocks-rust
TUN_DEVICE_PATCH := $(ROOT_DIR)/patches/shadowsocks-rust-tun-device-injection.patch

CORE_DIR := $(ROOT_DIR)/core
CORE_TARGET_DIR ?= $(CORE_DIR)/target
CORE_OUT_DIR ?= $(ROOT_DIR)/target/core
CORE_LIB ?= $(CORE_OUT_DIR)/libsocks_core.a

TAURI_BUNDLE_DIR ?= $(ROOT_DIR)/src-tauri/target/release/bundle/macos
TAURI_APP_BUNDLE ?= $(TAURI_BUNDLE_DIR)/socks.app
TAURI_UNIVERSAL_TARGET ?= universal-apple-darwin
TAURI_UNIVERSAL_APP_BUNDLE ?= $(ROOT_DIR)/src-tauri/target/$(TAURI_UNIVERSAL_TARGET)/release/bundle/macos/socks.app

UNAME_M := $(shell uname -m)
ifeq ($(UNAME_M),arm64)
MACOS_ARCH ?= arm64
else ifeq ($(UNAME_M),x86_64)
MACOS_ARCH ?= x86_64
else
MACOS_ARCH ?= $(UNAME_M)
endif
MACOS_DEPLOYMENT_TARGET ?= 13.0
MACOS_TARGET := $(MACOS_ARCH)-apple-macos$(MACOS_DEPLOYMENT_TARGET)

# --- DMG ---------------------------------------------------------------------
# Tauri's own dmg bundler runs *before* `extension-embed`, so it would ship an
# app without the .appex and without the NetworkExtension entitlement. We
# therefore build the .app with Tauri (bundle.targets = ["app"]) and create the
# dmg ourselves, after the extension is embedded and the app is re-signed.
DMG_OUT_DIR ?= $(ROOT_DIR)/src-tauri/target/release/bundle/dmg
DMG_STAGE_DIR ?= $(ROOT_DIR)/target/dmg-stage
DMG_VOLUME_NAME ?= socks
# Match Tauri's artifact naming (arm64 -> aarch64).
DMG_ARCH ?= $(if $(filter arm64,$(MACOS_ARCH)),aarch64,$(MACOS_ARCH))

.PHONY: help all frontend rust-check submodule-patch core core-build extension extension-check extension-project extension-build extension-embed tauri package dmg dmg-universal clean-extension clean-core clean-dmg

help:
	@printf "%s\n" \
		"Targets:" \
		"  make extension       Generate + build + sign the VPN extension (.appex) via xcodegen/xcodebuild." \
		"  make extension-project  Regenerate SocksTunnel.xcodeproj from project.yml." \
		"  make core            Build the Rust data plane (universal libsocks_core.a)." \
		"  make submodule-patch Apply the shadowsocks-rust device-injection patch." \
		"  make tauri           Validate/build extension, then run Tauri packaging." \
		"  make package         Alias for tauri." \
		"  make frontend        Build the Vite frontend." \
		"  make rust-check      Run cargo check for src-tauri." \
		"  make extension-embed Embed built .appex into TAURI_APP_BUNDLE=.../socks.app." \
		"  make dmg             Create the .dmg from the embedded+signed app (run after extension-embed)." \
		"  make dmg-universal   Build a universal (Intel + Apple Silicon) app + dmg in one step." \
		"" \
		"Variables:" \
		"  VPN_XCODEPROJ=$(VPN_XCODEPROJ)" \
		"  VPN_SCHEME=$(VPN_SCHEME)" \
		"  VPN_CONFIGURATION=$(VPN_CONFIGURATION)" \
		"  VPN_DERIVED_DATA=$(VPN_DERIVED_DATA)" \
		"  VPN_EXTENSION_OUT_DIR=$(VPN_EXTENSION_OUT_DIR)" \
		"  VPN_BUNDLE_ID=$(VPN_BUNDLE_ID)" \
		"  MACOS_TARGET=$(MACOS_TARGET)" \
		"  TAURI_APP_BUNDLE=$(TAURI_APP_BUNDLE)" \
		"  TAURI_UNIVERSAL_APP_BUNDLE=$(TAURI_UNIVERSAL_APP_BUNDLE)" \
		"" \
		"Signing (already wired up as defaults; make tauri needs no extra flags):" \
		"  VPN_DEVELOPMENT_TEAM=$(VPN_DEVELOPMENT_TEAM)" \
		"  VPN_CODE_SIGN_IDENTITY=$(VPN_CODE_SIGN_IDENTITY)" \
		"  VPN_EXT_PROFILE_NAME=$(VPN_EXT_PROFILE_NAME)" \
		"  VPN_APP_PROFILE_NAME=$(VPN_APP_PROFILE_NAME)" \
		"  VPN_PROFILES_DIR=$(VPN_PROFILES_DIR)" \
		"" \
		"Override example:" \
		"  make tauri VPN_CODE_SIGN_IDENTITY=\"Apple Development: ...\" VPN_EXT_PROFILE_NAME=..."

all: package

frontend:
	bun run build

rust-check:
	cd src-tauri && cargo check

# Build the Rust data plane for both macOS slices and lipo them together, so it
# can be linked into the universal .appex.
core: core-build

# `socks-core` needs `TunBuilder::build_with_device`, which is not upstream yet.
# The change lives in patches/ so a fresh clone can reproduce it.
submodule-patch:
	@cd "$(SUBMODULE_DIR)" && \
	if git apply --reverse --check "$(TUN_DEVICE_PATCH)" >/dev/null 2>&1; then \
		echo "shadowsocks-rust device-injection patch already applied"; \
	else \
		echo "Applying $(notdir $(TUN_DEVICE_PATCH))"; \
		git apply "$(TUN_DEVICE_PATCH)"; \
	fi

# NOTE: do NOT export MACOSX_DEPLOYMENT_TARGET for the whole cargo invocation.
# With Xcode 27 it corrupts proc-macro dylibs ("mis-aligned LINKEDIT string
# pool"). See todo.md for the deployment-target follow-up.
core-build: submodule-patch
	cargo build --manifest-path "$(CORE_DIR)/Cargo.toml" --release --target aarch64-apple-darwin
	cargo build --manifest-path "$(CORE_DIR)/Cargo.toml" --release --target x86_64-apple-darwin
	mkdir -p "$(CORE_OUT_DIR)"
	lipo -create \
		"$(CORE_TARGET_DIR)/aarch64-apple-darwin/release/libsocks_core.a" \
		"$(CORE_TARGET_DIR)/x86_64-apple-darwin/release/libsocks_core.a" \
		-output "$(CORE_LIB)"
	@echo "Built $(CORE_LIB)"

extension: extension-check extension-build

# The generated SocksTunnel.xcodeproj is not checked in; project.yml is the
# source of truth. Regenerate after changing project.yml or adding sources.
extension-project:
	@command -v xcodegen >/dev/null 2>&1 || { echo "xcodegen is required to generate the VPN extension project (brew install xcodegen)." >&2; exit 1; }
	cd "$(VPN_DIR)" && xcodegen generate --spec "$(notdir $(VPN_PROJECT_YML))"

extension-check: core-build
	@command -v xcrun >/dev/null 2>&1 || { echo "xcrun/Xcode is required to check the VPN extension." >&2; exit 1; }
	xcrun swiftc -typecheck -target $(MACOS_TARGET) -framework NetworkExtension "$(VPN_APP_SWIFT)"
	xcrun swiftc -typecheck -target $(MACOS_TARGET) \
		-framework NetworkExtension \
		-I "$(CORE_DIR)/include" \
		-import-objc-header "$(VPN_DIR)/VpnExtension/VpnExtension-Bridging-Header.h" \
		"$(VPN_EXTENSION_SWIFT)"

extension-build: extension-project core-build
	@test -d "$(VPN_XCODEPROJ)" || { echo "Missing VPN_XCODEPROJ: $(VPN_XCODEPROJ)" >&2; exit 1; }
	@command -v codesign >/dev/null 2>&1 || { echo "codesign is required to sign the VPN extension." >&2; exit 1; }
	@found=0; for f in "$(VPN_PROFILES_DIR)"/*.provisionprofile; do \
		n="$$(security cms -D -i "$$f" 2>/dev/null | plutil -extract Name raw - 2>/dev/null || true)"; \
		if [[ "$$n" == "$(VPN_EXT_PROFILE_NAME)" ]]; then found=1; break; fi; \
	done; \
	if [[ "$$found" != "1" ]]; then \
		echo "ERROR: provisioning profile '$(VPN_EXT_PROFILE_NAME)' not found in '$(VPN_PROFILES_DIR)'." >&2; \
		echo "       Install the .provisionprofile, or set VPN_EXT_PROFILE_NAME." >&2; \
		exit 1; \
	fi
	xcodebuild \
		-project "$(VPN_XCODEPROJ)" \
		-scheme "$(VPN_SCHEME)" \
		-configuration "$(VPN_CONFIGURATION)" \
		-derivedDataPath "$(VPN_DERIVED_DATA)" \
		CORE_INCLUDE_DIR="$(CORE_DIR)/include" \
		CORE_STATIC_LIB="$(CORE_LIB)" \
		CODE_SIGN_STYLE=Manual \
		DEVELOPMENT_TEAM="$(VPN_DEVELOPMENT_TEAM)" \
		CODE_SIGN_IDENTITY="$(VPN_CODE_SIGN_IDENTITY)" \
		PROVISIONING_PROFILE_SPECIFIER="$(VPN_EXT_PROFILE_NAME)" \
		build
	rm -rf "$(VPN_EXTENSION_OUT_DIR)/$(VPN_PRODUCT_NAME).appex"
	mkdir -p "$(VPN_EXTENSION_OUT_DIR)"
	cp -R "$(VPN_DERIVED_DATA)/Build/Products/$(VPN_CONFIGURATION)/$(VPN_PRODUCT_NAME).appex" "$(VPN_EXTENSION_OUT_DIR)/"
	@test -f "$(VPN_EXTENSION_OUT_DIR)/$(VPN_PRODUCT_NAME).appex/Contents/embedded.provisionprofile" \
		|| { echo "ERROR: appex has no embedded.provisionprofile; Xcode did not sign with '$(VPN_EXT_PROFILE_NAME)'." >&2; exit 1; }
	@echo "--- signed appex ---"
	@codesign -dvvv "$(VPN_EXTENSION_OUT_DIR)/$(VPN_PRODUCT_NAME).appex" 2>&1 | grep -E "Identifier|Authority|TeamIdentifier|Signature" || true
	@echo "embedded.provisionprofile: OK"

extension-embed:
	@test -n "$(TAURI_APP_BUNDLE)" || { echo "Set TAURI_APP_BUNDLE=/path/to/socks.app." >&2; exit 1; }
	@test -d "$(TAURI_APP_BUNDLE)" || { echo "Missing app bundle: $(TAURI_APP_BUNDLE)" >&2; exit 1; }
	@appex="$$(find "$(VPN_EXTENSION_OUT_DIR)" -maxdepth 1 -name "*.appex" -type d 2>/dev/null | head -n 1)"; \
	if [[ -z "$$appex" ]]; then \
		echo "No .appex found under $(VPN_EXTENSION_OUT_DIR). Run make extension first." >&2; \
		exit 1; \
	fi; \
	mkdir -p "$(TAURI_APP_BUNDLE)/Contents/PlugIns"; \
	rm -rf "$(TAURI_APP_BUNDLE)/Contents/PlugIns/$$(basename "$$appex")"; \
	cp -R "$$appex" "$(TAURI_APP_BUNDLE)/Contents/PlugIns/"; \
	profile=""; \
	for f in "$(VPN_PROFILES_DIR)"/*.provisionprofile; do \
		n="$$(security cms -D -i "$$f" 2>/dev/null | plutil -extract Name raw - 2>/dev/null || true)"; \
		if [[ "$$n" == "$(VPN_APP_PROFILE_NAME)" ]]; then profile="$$f"; break; fi; \
	done; \
	if [[ -z "$$profile" ]]; then \
		echo "ERROR: provisioning profile '$(VPN_APP_PROFILE_NAME)' not found in '$(VPN_PROFILES_DIR)'." >&2; \
		exit 1; \
	fi; \
	cp "$$profile" "$(TAURI_APP_BUNDLE)/Contents/embedded.provisionprofile"; \
	codesign --force --sign "$(VPN_CODE_SIGN_IDENTITY)" --options runtime --timestamp=none \
		--entitlements "$(VPN_APP_ENTITLEMENTS)" "$(TAURI_APP_BUNDLE)"; \
	echo "--- signed app ---"; \
	codesign -dvvv "$(TAURI_APP_BUNDLE)" 2>&1 | grep -E "Identifier|Authority|TeamIdentifier|Signature" || true; \
	echo "embedded.provisionprofile: OK"

tauri: extension
	bun run tauri build
	@if [[ -d "$(TAURI_APP_BUNDLE)" ]]; then \
		$(MAKE) extension-embed TAURI_APP_BUNDLE="$(TAURI_APP_BUNDLE)"; \
		$(MAKE) dmg TAURI_APP_BUNDLE="$(TAURI_APP_BUNDLE)"; \
	else \
		echo "Tauri app bundle not found at $(TAURI_APP_BUNDLE); skipping extension embed + dmg."; \
	fi

# Build a single universal (Intel + Apple Silicon) app, embed the signed
# extension and produce a universal dmg in one step.
dmg-universal: extension
	bun run tauri build --target $(TAURI_UNIVERSAL_TARGET)
	@test -d "$(TAURI_UNIVERSAL_APP_BUNDLE)" \
		|| { echo "Missing universal app bundle: $(TAURI_UNIVERSAL_APP_BUNDLE)" >&2; exit 1; }
	$(MAKE) extension-embed TAURI_APP_BUNDLE="$(TAURI_UNIVERSAL_APP_BUNDLE)"
	$(MAKE) dmg TAURI_APP_BUNDLE="$(TAURI_UNIVERSAL_APP_BUNDLE)" DMG_ARCH=universal

# Must run AFTER extension-embed; see the DMG block above.
dmg:
	@test -d "$(TAURI_APP_BUNDLE)" || { echo "Missing app bundle: $(TAURI_APP_BUNDLE)" >&2; exit 1; }
	@command -v hdiutil >/dev/null 2>&1 || { echo "hdiutil is required to build the dmg." >&2; exit 1; }
	@test -e "$(TAURI_APP_BUNDLE)/Contents/PlugIns/SocksTunnelExtension.appex" \
		|| { echo "ERROR: $(TAURI_APP_BUNDLE) has no embedded .appex; run make extension-embed first." >&2; exit 1; }
	@test -f "$(TAURI_APP_BUNDLE)/Contents/embedded.provisionprofile" \
		|| { echo "ERROR: $(TAURI_APP_BUNDLE) is not signed with an app profile; run make extension-embed first." >&2; exit 1; }
	@ver="$$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$(TAURI_APP_BUNDLE)/Contents/Info.plist" 2>/dev/null || echo 0.1.0)"; \
	out="$(DMG_OUT_DIR)/socks_$${ver}_$(DMG_ARCH).dmg"; \
	stage="$(DMG_STAGE_DIR)"; \
	rm -rf "$$stage"; \
	mkdir -p "$$stage" "$(DMG_OUT_DIR)"; \
	ditto "$(TAURI_APP_BUNDLE)" "$$stage/$$(basename "$(TAURI_APP_BUNDLE)")"; \
	ln -sfn /Applications "$$stage/Applications"; \
	rm -f "$$out"; \
	hdiutil create -quiet -volname "$(DMG_VOLUME_NAME)" -srcfolder "$$stage" -ov -format UDZO "$$out"; \
	rm -rf "$$stage"; \
	echo "Built $$out"

package: tauri

clean-extension:
	rm -rf "$(VPN_DERIVED_DATA)" "$(VPN_EXTENSION_OUT_DIR)" "$(VPN_TMP_DIR)"

clean-dmg:
	rm -rf "$(DMG_OUT_DIR)" "$(DMG_STAGE_DIR)"

clean-core:
	cargo clean --manifest-path "$(CORE_DIR)/Cargo.toml"
	rm -rf "$(CORE_OUT_DIR)"
