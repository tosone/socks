SHELL := /bin/bash

ROOT_DIR := $(abspath .)
VPN_DIR := $(ROOT_DIR)/src-tauri/resources/extensions/vpn
VPN_APP_SWIFT := $(VPN_DIR)/SocksTunnelControl.swift
VPN_EXTENSION_SWIFT := $(VPN_DIR)/VpnExtension/PacketTunnelProvider.swift
VPN_XCODEPROJ ?= $(VPN_DIR)/SocksTunnel.xcodeproj
VPN_SCHEME ?= SocksTunnelExtension
VPN_CONFIGURATION ?= Release
VPN_DERIVED_DATA ?= $(ROOT_DIR)/target/xcode
VPN_EXTENSION_OUT_DIR ?= $(ROOT_DIR)/target/extensions/vpn
VPN_MODULE_NAME ?= SocksTunnelExtension
VPN_PRODUCT_NAME ?= SocksTunnelExtension
VPN_BUNDLE_ID ?= com.tosone.socks.SocksTunnelExtension
VPN_VERSION ?= 0.1.0
VPN_BUILD ?= 1
VPN_CODESIGN_IDENTITY ?= -
VPN_TMP_DIR ?= $(ROOT_DIR)/target/tmp
TAURI_BUNDLE_DIR ?= $(ROOT_DIR)/src-tauri/target/release/bundle/macos
TAURI_APP_BUNDLE ?= $(TAURI_BUNDLE_DIR)/socks.app

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

.PHONY: help all frontend rust-check extension extension-check extension-build extension-package extension-embed tauri package clean-extension

help:
	@printf "%s\n" \
		"Targets:" \
		"  make extension       Build/package VPN extension; uses xcodebuild when an Xcode project exists." \
		"  make tauri           Validate/build extension, then run Tauri packaging." \
		"  make package         Alias for tauri." \
		"  make frontend        Build the Vite frontend." \
		"  make rust-check      Run cargo check for src-tauri." \
		"  make extension-embed Embed built .appex into TAURI_APP_BUNDLE=.../socks.app." \
		"" \
		"Variables:" \
		"  VPN_XCODEPROJ=$(VPN_XCODEPROJ)" \
		"  VPN_SCHEME=$(VPN_SCHEME)" \
		"  VPN_CONFIGURATION=$(VPN_CONFIGURATION)" \
		"  VPN_DERIVED_DATA=$(VPN_DERIVED_DATA)" \
		"  VPN_EXTENSION_OUT_DIR=$(VPN_EXTENSION_OUT_DIR)" \
		"  VPN_BUNDLE_ID=$(VPN_BUNDLE_ID)" \
		"  MACOS_TARGET=$(MACOS_TARGET)" \
		"  TAURI_APP_BUNDLE=$(TAURI_APP_BUNDLE)"

all: package

frontend:
	bun run build

rust-check:
	cd src-tauri && cargo check

extension: extension-check
	@if [[ -d "$(VPN_XCODEPROJ)" ]]; then \
		$(MAKE) extension-build; \
	else \
		$(MAKE) extension-package; \
	fi

extension-check:
	@command -v xcrun >/dev/null 2>&1 || { echo "xcrun/Xcode is required to check the VPN extension." >&2; exit 1; }
	xcrun swiftc -typecheck -target $(MACOS_TARGET) -framework NetworkExtension "$(VPN_APP_SWIFT)"
	xcrun swiftc -typecheck -target $(MACOS_TARGET) -framework NetworkExtension "$(VPN_EXTENSION_SWIFT)"

extension-build:
	@test -d "$(VPN_XCODEPROJ)" || { echo "Missing VPN_XCODEPROJ: $(VPN_XCODEPROJ)" >&2; exit 1; }
	xcodebuild \
		-project "$(VPN_XCODEPROJ)" \
		-scheme "$(VPN_SCHEME)" \
		-configuration "$(VPN_CONFIGURATION)" \
		-derivedDataPath "$(VPN_DERIVED_DATA)" \
		build

extension-package:
	@command -v xcrun >/dev/null 2>&1 || { echo "xcrun/Xcode is required to build the VPN extension." >&2; exit 1; }
	@command -v codesign >/dev/null 2>&1 || { echo "codesign is required to package the VPN extension." >&2; exit 1; }
	rm -rf "$(VPN_EXTENSION_OUT_DIR)/$(VPN_PRODUCT_NAME).appex"
	mkdir -p "$(VPN_EXTENSION_OUT_DIR)/$(VPN_PRODUCT_NAME).appex/Contents/MacOS" "$(VPN_TMP_DIR)"
	TMPDIR="$(VPN_TMP_DIR)" xcrun swiftc \
		-emit-library \
		-parse-as-library \
		-target $(MACOS_TARGET) \
		-module-name "$(VPN_MODULE_NAME)" \
		-framework NetworkExtension \
		"$(VPN_EXTENSION_SWIFT)" \
		-o "$(VPN_EXTENSION_OUT_DIR)/$(VPN_PRODUCT_NAME).appex/Contents/MacOS/$(VPN_PRODUCT_NAME)"
	cp "$(VPN_DIR)/VpnExtension/Info.plist" "$(VPN_EXTENSION_OUT_DIR)/$(VPN_PRODUCT_NAME).appex/Contents/Info.plist"
	/usr/libexec/PlistBuddy -c "Set :CFBundleDevelopmentRegion en" "$(VPN_EXTENSION_OUT_DIR)/$(VPN_PRODUCT_NAME).appex/Contents/Info.plist"
	/usr/libexec/PlistBuddy -c "Set :CFBundleExecutable $(VPN_PRODUCT_NAME)" "$(VPN_EXTENSION_OUT_DIR)/$(VPN_PRODUCT_NAME).appex/Contents/Info.plist"
	/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $(VPN_BUNDLE_ID)" "$(VPN_EXTENSION_OUT_DIR)/$(VPN_PRODUCT_NAME).appex/Contents/Info.plist"
	/usr/libexec/PlistBuddy -c "Set :CFBundleName $(VPN_PRODUCT_NAME)" "$(VPN_EXTENSION_OUT_DIR)/$(VPN_PRODUCT_NAME).appex/Contents/Info.plist"
	/usr/libexec/PlistBuddy -c "Set :CFBundlePackageType XPC!" "$(VPN_EXTENSION_OUT_DIR)/$(VPN_PRODUCT_NAME).appex/Contents/Info.plist"
	/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $(VPN_VERSION)" "$(VPN_EXTENSION_OUT_DIR)/$(VPN_PRODUCT_NAME).appex/Contents/Info.plist"
	/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $(VPN_BUILD)" "$(VPN_EXTENSION_OUT_DIR)/$(VPN_PRODUCT_NAME).appex/Contents/Info.plist"
	/usr/libexec/PlistBuddy -c "Set :NSExtension:NSExtensionPrincipalClass $(VPN_MODULE_NAME).PacketTunnelProvider" "$(VPN_EXTENSION_OUT_DIR)/$(VPN_PRODUCT_NAME).appex/Contents/Info.plist"
	plutil -lint "$(VPN_EXTENSION_OUT_DIR)/$(VPN_PRODUCT_NAME).appex/Contents/Info.plist"
	codesign --force --sign "$(VPN_CODESIGN_IDENTITY)" --timestamp=none --entitlements "$(VPN_DIR)/VpnExtension/SocksTunnelExtension.entitlements" "$(VPN_EXTENSION_OUT_DIR)/$(VPN_PRODUCT_NAME).appex"

extension-embed:
	@test -n "$(TAURI_APP_BUNDLE)" || { echo "Set TAURI_APP_BUNDLE=/path/to/socks.app." >&2; exit 1; }
	@test -d "$(TAURI_APP_BUNDLE)" || { echo "Missing app bundle: $(TAURI_APP_BUNDLE)" >&2; exit 1; }
	@appex="$$(find "$(VPN_EXTENSION_OUT_DIR)" "$(VPN_DERIVED_DATA)" -name "*.appex" -type d 2>/dev/null | head -n 1)"; \
	if [[ -z "$$appex" ]]; then \
		echo "No .appex found under $(VPN_EXTENSION_OUT_DIR) or $(VPN_DERIVED_DATA). Run make extension first." >&2; \
		exit 1; \
	fi; \
	mkdir -p "$(TAURI_APP_BUNDLE)/Contents/PlugIns"; \
	rm -rf "$(TAURI_APP_BUNDLE)/Contents/PlugIns/$$(basename "$$appex")"; \
	cp -R "$$appex" "$(TAURI_APP_BUNDLE)/Contents/PlugIns/"

tauri: extension
	bun run tauri build
	@if [[ -d "$(TAURI_APP_BUNDLE)" ]]; then \
		$(MAKE) extension-embed TAURI_APP_BUNDLE="$(TAURI_APP_BUNDLE)"; \
	else \
		echo "Tauri app bundle not found at $(TAURI_APP_BUNDLE); skipping extension embed."; \
	fi

package: tauri

clean-extension:
	rm -rf "$(VPN_DERIVED_DATA)" "$(VPN_EXTENSION_OUT_DIR)" "$(VPN_TMP_DIR)"
