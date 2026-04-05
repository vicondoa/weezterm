.PHONY: all fmt build check test docs servedocs

all: build

test:
	cargo nextest run
	cargo nextest run -p wezterm-escape-parser # no_std by default

check:
	cargo check
	cargo check -p wezterm-escape-parser
	cargo check -p wezterm-cell
	cargo check -p wezterm-surface
	cargo check -p wezterm-ssh

build:
	cargo build $(BUILD_OPTS) -p wezterm
	cargo build $(BUILD_OPTS) -p wezterm-gui
	cargo build $(BUILD_OPTS) -p wezterm-mux-server
	cargo build $(BUILD_OPTS) -p strip-ansi-escapes

fmt:
	cargo +nightly fmt

docs:
	ci/build-docs.sh

servedocs:
	ci/build-docs.sh serve

# --- weezterm remote features ---
.PHONY: weezterm-windows-setup
weezterm-windows-setup: build
	@echo "Packaging Weezterm for Windows..."
	$(eval TAG_NAME ?= nightly)
	$(eval ZIPDIR := Weezterm-windows-$(TAG_NAME))
	$(eval INSTNAME := Weezterm-$(TAG_NAME)-setup)
	rm -rf $(ZIPDIR)
	mkdir -p $(ZIPDIR)
	cp target/release/wezterm.exe $(ZIPDIR)/
	cp target/release/wezterm-gui.exe $(ZIPDIR)/
	cp target/release/wezterm-mux-server.exe $(ZIPDIR)/
	cp target/release/strip-ansi-escapes.exe $(ZIPDIR)/
	-cp assets/windows/conhost/conpty.dll $(ZIPDIR)/ 2>/dev/null
	-cp assets/windows/conhost/OpenConsole.exe $(ZIPDIR)/ 2>/dev/null
	-cp assets/windows/angle/libEGL.dll $(ZIPDIR)/ 2>/dev/null
	-cp assets/windows/angle/libGLESv2.dll $(ZIPDIR)/ 2>/dev/null
	-mkdir -p $(ZIPDIR)/mesa && cp target/release/mesa/opengl32.dll $(ZIPDIR)/mesa/ 2>/dev/null
	@echo "Created $(ZIPDIR)/ with Weezterm binaries"
	@echo "To create ZIP: 7z a -tzip $(ZIPDIR).zip $(ZIPDIR)"
	@echo "To create installer: iscc.exe -DMyAppVersion=$(TAG_NAME) -F$(INSTNAME) ci/windows-installer.iss"
