#!/bin/bash
set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
  echo "ERROR: This script must be run as root." >&2; exit 1
fi

# Check if already installed
if [ -x /opt/CrowdStrike/falconctl ]; then
  echo "CrowdStrike Falcon sensor is already installed. Skipping."
  exit 0
fi

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

# Detect package type
if command -v dpkg &>/dev/null; then
  PKG_TYPE="deb"
elif command -v rpm &>/dev/null; then
  PKG_TYPE="rpm"
else
  echo "ERROR: Unsupported system — neither dpkg nor rpm found." >&2; exit 1
fi

ARCH=$(uname -m)
HOST=$(hostname)

# Detect distro from os-release
DISTRO_ID=""
DISTRO_VERSION=""
if [ -f /etc/os-release ]; then
  . /etc/os-release
  DISTRO_ID="${ID:-}"
  DISTRO_VERSION="${VERSION_ID%%.*}"  # major version only
fi

echo "Registering with pike server..."
echo "  host=$HOST pkg=$PKG_TYPE arch=$ARCH distro=$DISTRO_ID version=$DISTRO_VERSION"
HTTP_CODE=$(curl -sS -o "$TMPDIR/cb_response" -w "%{http_code}" -X POST "{{BASE_URL}}/cb" -d "${HOST}|${PKG_TYPE}|${ARCH}|${DISTRO_ID}|${DISTRO_VERSION}")
RESPONSE=$(cat "$TMPDIR/cb_response")

if [ "$HTTP_CODE" -ne 200 ]; then
  echo "ERROR: Server returned HTTP $HTTP_CODE: $RESPONSE" >&2; exit 1
fi

FILENAME=$(echo "$RESPONSE" | cut -d'|' -f1)
SHA256=$(echo "$RESPONSE" | cut -d'|' -f2)

if [ -z "$FILENAME" ] || [ -z "$SHA256" ]; then
  echo "ERROR: Failed to get sensor info from server." >&2; exit 1
fi

echo "Downloading sensor ($FILENAME)..."
HTTP_CODE=$(curl -sS -o "$TMPDIR/$FILENAME" -w "%{http_code}" "{{BASE_URL}}/s/$FILENAME")

if [ "$HTTP_CODE" -ne 200 ]; then
  echo "ERROR: Failed to download sensor (HTTP $HTTP_CODE)." >&2; exit 1
fi

echo "Verifying checksum..."
echo "$SHA256  $TMPDIR/$FILENAME" | sha256sum -c --quiet

echo "Installing sensor..."
if [ "$PKG_TYPE" = "deb" ]; then
  dpkg -i "$TMPDIR/$FILENAME"
else
  if command -v dnf &>/dev/null; then
    dnf install -y "$TMPDIR/$FILENAME"
  elif command -v zypper &>/dev/null; then
    zypper --non-interactive install "$TMPDIR/$FILENAME"
  elif command -v yum &>/dev/null; then
    yum install -y "$TMPDIR/$FILENAME"
  else
    rpm -ivh "$TMPDIR/$FILENAME"
  fi
fi

echo "Configuring CID..."
/opt/CrowdStrike/falconctl -s --cid='{{CID}}' {{CLOUD_FLAG}}

echo "Starting falcon-sensor..."
if command -v systemctl &>/dev/null; then
  systemctl start falcon-sensor
else
  service falcon-sensor start
fi

# Report success
curl -fsS -X POST "{{BASE_URL}}/done" -d "${HOST}|ok" >/dev/null 2>&1 || true

echo "CrowdStrike Falcon sensor installed and running."
