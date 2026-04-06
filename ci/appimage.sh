#!/bin/bash
set -x
rm -rf AppDir *.AppImage *.zsync
set -e

# --- weezterm remote features ---
# Create compat symlinks for renamed binaries (weezterm* -> wezterm*)
for old in wezterm wezterm-gui wezterm-mux-server; do
  new="${old/wezterm/weezterm}"
  if [ -f "target/release/$new" ] && [ ! -f "target/release/$old" ]; then
    ln -sf "$new" "target/release/$old"
  fi
done
# Icon compat symlink
if [ -f "assets/icon/weezterm/terminal.png" ] && [ ! -f "assets/icon/terminal.png" ]; then
  ln -sf weezterm/terminal.png assets/icon/terminal.png
fi
# Use extract-and-run to avoid FUSE requirement in containers
export APPIMAGE_EXTRACT_AND_RUN=1
# Override desktop/appdata with weezterm-branded versions
if [ -f "assets/weezterm.desktop" ]; then
  cp -f assets/weezterm.desktop assets/wezterm.desktop
fi
if [ -f "assets/weezterm.appdata.xml" ]; then
  cp -f assets/weezterm.appdata.xml assets/wezterm.appdata.xml
fi
# --- end weezterm remote features ---

mkdir AppDir

install -Dsm755 -t AppDir/usr/bin target/release/wezterm-mux-server
install -Dsm755 -t AppDir/usr/bin target/release/wezterm
install -Dsm755 -t AppDir/usr/bin target/release/wezterm-gui
install -Dsm755 -t AppDir/usr/bin target/release/strip-ansi-escapes
# --- weezterm remote features ---
# Also install under the real weezterm names so the desktop Exec= entry works
if [ -f "target/release/weezterm" ]; then
  install -Dsm755 target/release/weezterm AppDir/usr/bin/weezterm
fi
if [ -f "target/release/weezterm-gui" ]; then
  install -Dsm755 target/release/weezterm-gui AppDir/usr/bin/weezterm-gui
fi
if [ -f "target/release/weezterm-mux-server" ]; then
  install -Dsm755 target/release/weezterm-mux-server AppDir/usr/bin/weezterm-mux-server
fi
# --- end weezterm remote features ---
install -Dm644 assets/icon/terminal.png AppDir/usr/share/icons/hicolor/128x128/apps/org.wezfurlong.wezterm.png
# --- weezterm remote features ---
# Also install icon under the new app ID so linuxdeploy can find it
install -Dm644 assets/icon/terminal.png AppDir/usr/share/icons/hicolor/128x128/apps/com.vicondoa.weezterm.png
# --- end weezterm remote features ---
install -Dm644 assets/wezterm.desktop AppDir/usr/share/applications/org.wezfurlong.wezterm.desktop
install -Dm644 assets/wezterm.appdata.xml AppDir/usr/share/metainfo/org.wezfurlong.wezterm.appdata.xml
install -Dm644 assets/wezterm-nautilus.py AppDir/usr/share/nautilus-python/extensions/wezterm-nautilus.py

[ -x /tmp/linuxdeploy ] || ( curl -L 'https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage' -o /tmp/linuxdeploy && chmod +x /tmp/linuxdeploy )

TAG_NAME=${TAG_NAME:-$(git -c "core.abbrev=8" show -s "--format=%cd-%h" "--date=format:%Y%m%d-%H%M%S")}
distro=$(lsb_release -is 2>/dev/null || sh -c "source /etc/os-release && echo \$NAME")
distver=$(lsb_release -rs 2>/dev/null || sh -c "source /etc/os-release && echo \$VERSION_ID")

# Embed appropriate update info
# https://github.com/AppImage/AppImageSpec/blob/master/draft.md#github-releases
# --- weezterm remote features ---
if [[ "$BUILD_REASON" == "Schedule" ]] ; then
  UPDATE="gh-releases-zsync|vicondoa|weezterm|nightly|Weezterm-*.AppImage.zsync"
  OUTPUT=Weezterm-nightly-$distro$distver.AppImage
else
  UPDATE="gh-releases-zsync|vicondoa|weezterm|latest|Weezterm-*.AppImage.zsync"
  OUTPUT=Weezterm-$TAG_NAME-$distro$distver.AppImage
fi
# --- end weezterm remote features ---

# Munge the path so that it finds our appstreamcli wrapper
PATH="$PWD/ci:$PATH" \
VERSION="$TAG_NAME" \
UPDATE_INFORMATION="$UPDATE" \
OUTPUT="$OUTPUT" \
  /tmp/linuxdeploy \
  --exclude-library='libwayland-client.so.0' \
  --appdir AppDir \
  --output appimage \
  --desktop-file assets/wezterm.desktop

# Update the AUR build file.  We only really want to use this for tagged
# builds but it doesn't hurt to generate it always here.
SHA256=$(sha256sum $OUTPUT | cut -d' ' -f1)
sed -e "s/@TAG@/$TAG_NAME/g" -e "s/@SHA256@/$SHA256/g" < ci/PKGBUILD.template > PKGBUILD
sed -e "s/@TAG@/$TAG_NAME/g" -e "s/@SHA256@/$SHA256/g" < ci/wezterm-linuxbrew.rb.template > wezterm-linuxbrew.rb
