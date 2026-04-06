.PHONY: all fmt build check test docs servedocs precommit

all: build

# --- weezterm remote features ---
# Run this before creating a PR. Mirrors what CI checks.
precommit: fmt check test
	@echo "\n✓ All precommit checks passed. Safe to push."
# --- end weezterm remote features ---

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
	cp target/release/weezterm.exe $(ZIPDIR)/
	cp target/release/weezterm-gui.exe $(ZIPDIR)/
	cp target/release/weezterm-mux-server.exe $(ZIPDIR)/
	cp target/release/strip-ansi-escapes.exe $(ZIPDIR)/
	-cp assets/windows/conhost/conpty.dll $(ZIPDIR)/ 2>/dev/null
	-cp assets/windows/conhost/OpenConsole.exe $(ZIPDIR)/ 2>/dev/null
	-cp assets/windows/angle/libEGL.dll $(ZIPDIR)/ 2>/dev/null
	-cp assets/windows/angle/libGLESv2.dll $(ZIPDIR)/ 2>/dev/null
	-mkdir -p $(ZIPDIR)/mesa && cp target/release/mesa/opengl32.dll $(ZIPDIR)/mesa/ 2>/dev/null
	@echo "Created $(ZIPDIR)/ with Weezterm binaries"
	@echo "To create ZIP: 7z a -tzip $(ZIPDIR).zip $(ZIPDIR)"
	@echo "To create installer: iscc.exe -DMyAppVersion=$(TAG_NAME) -F$(INSTNAME) ci/windows-installer.iss"

# Ensure Strawberry Perl is available on Windows (required for OpenSSL build).
# Downloads and installs to C:\Strawberry if missing, then prepends to PATH.
.PHONY: ensure-strawberry-perl
ensure-strawberry-perl:
ifeq ($(OS),Windows_NT)
	@if not exist "C:\Strawberry\perl\bin\perl.exe" ( \
		echo "Strawberry Perl not found, installing..." && \
		powershell -NoProfile -ExecutionPolicy Bypass -Command "\
			$$url = 'https://github.com/StrawberryPerl/Perl-Dist-Strawberry/releases/download/SP_54201_64bit/strawberry-perl-5.42.0.1-64bit.msi'; \
			$$msi = \"$$env:TEMP\\strawberry-perl.msi\"; \
			Write-Host 'Downloading Strawberry Perl...'; \
			[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; \
			Invoke-WebRequest -Uri $$url -OutFile $$msi -UseBasicParsing; \
			Write-Host 'Installing Strawberry Perl to C:\\Strawberry ...'; \
			Start-Process msiexec.exe -ArgumentList '/i', $$msi, '/qn', '/norestart', 'INSTALLDIR=C:\\Strawberry' -Wait -NoNewWindow; \
			Remove-Item $$msi -Force; \
			Write-Host 'Strawberry Perl installed.'" \
	) else ( echo "Strawberry Perl found." )
	@set "PATH=C:\Strawberry\perl\bin;%PATH%"
endif

# Convenience target: set up PATH for Windows builds
.PHONY: windows-build-env
windows-build-env: ensure-strawberry-perl
ifeq ($(OS),Windows_NT)
	@echo "Build environment ready. PATH includes Strawberry Perl."
	@echo "Run: set PATH=C:\Strawberry\perl\bin;%PATH%"
endif
# --- end weezterm remote features ---
