#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Install the CrownOS portal backend next to xdg-desktop-portal.
#
#   sudo ./install.sh              # build in release mode, install under /usr
#   sudo ./install.sh --uninstall
#
# xdg-desktop-portal only reads backend definitions from its own data
# directory, so this is a system install; PREFIX must be the prefix
# xdg-desktop-portal was built with.

set -euo pipefail

HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${PREFIX:-/usr}"
LIBEXEC="$PREFIX/lib"
DATA="$PREFIX/share"
UNINSTALL=0

for arg in "$@"; do
  case "$arg" in
    --uninstall) UNINSTALL=1 ;;
    -h|--help) sed -n '3,11p' "${BASH_SOURCE[0]}" | sed 's/^# \?//'; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

[ "$(id -u)" -eq 0 ] || { echo "run as root: the portal definition lives in $DATA" >&2; exit 1; }

FILES=(
  "$LIBEXEC/xdg-desktop-portal-crownos"
  "$LIBEXEC/crownos-portal-dialog"
  "$DATA/xdg-desktop-portal/portals/crownos.portal"
  "$DATA/xdg-desktop-portal/crownos-portals.conf"
  "$DATA/dbus-1/services/org.freedesktop.impl.portal.desktop.crownos.service"
  "$PREFIX/lib/systemd/user/xdg-desktop-portal-crownos.service"
)

if [ "$UNINSTALL" -eq 1 ]; then
  rm -fv "${FILES[@]}"
  exit 0
fi

BUILDER="${SUDO_USER:-$(id -un)}"
sudo -u "$BUILDER" cargo build --release --manifest-path "$HERE/Cargo.toml"

substitute() { sed "s|@libexecdir@|$LIBEXEC|g" "$1"; }

install -Dm755 "$HERE/target/release/xdg-desktop-portal-crownos" "${FILES[0]}"
install -Dm755 "$HERE/target/release/crownos-portal-dialog" "${FILES[1]}"
install -Dm644 "$HERE/data/crownos.portal" "${FILES[2]}"
install -Dm644 "$HERE/data/crownos-portals.conf" "${FILES[3]}"
substitute "$HERE/data/org.freedesktop.impl.portal.desktop.crownos.service.in" | install -Dm644 /dev/stdin "${FILES[4]}"
substitute "$HERE/data/xdg-desktop-portal-crownos.service.in" | install -Dm644 /dev/stdin "${FILES[5]}"
printf 'installed %s\n' "${FILES[@]}"

echo
echo "Log in to CrownOS (XDG_CURRENT_DESKTOP=CrownOS) for xdg-desktop-portal to pick it up,"
echo "or restart it in a running session:  systemctl --user restart xdg-desktop-portal"
