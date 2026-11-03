#!/usr/bin/env bash
# ==============================================================================
# Oh My Works (OMW) - 1Panel Lite Server Deployment Script
# Optimized for 2-Core / 2GB RAM Linux VPS (Tencent Cloud / Debian / Ubuntu / Arch)
# ==============================================================================
set -e

echo "=== [1/6] Auditing Host Specs & RAM ==="
nproc
free -h

# 1. Ensure 2GB Swap exists with 600 permissions
if [ ! -f /swapfile ]; then
  echo "Creating 2GB swapfile..."
  fallocate -l 2G /swapfile 2>/dev/null || dd if=/dev/zero of=/swapfile bs=1M count=2048
  chmod 600 /swapfile
  mkswap /swapfile
  swapon /swapfile
  grep -q '/swapfile' /etc/fstab || echo '/swapfile none swap sw 0 0' >> /etc/fstab
fi

# 2. Free RAM: Stop heavy legacy processes if any
echo "=== [2/6] Stopping Bloated Runtimes (PM2 / Next.js) ==="
which pm2 >/dev/null 2>&1 && pm2 kill || true
pkill -9 -f 9router || true
pkill -9 -f next-server || true

# 3. Install OMW 1Panel Native Rust Daemon Binary (< 5MB RAM)
echo "=== [3/6] Installing OMW 1Panel Native Daemon ==="
mkdir -p /root/.omw/projects
mkdir -p /usr/local/bin

if [ -f "./omw-server-daemon" ]; then
  cp -f ./omw-server-daemon /usr/local/bin/omw-server-daemon
else
  echo "Downloading precompiled OMW Server Daemon from GitHub Releases..."
  curl -fsSL "https://github.com/IvanoWisnuBudiLaras/Oh-My-Works/releases/latest/download/omw-server-daemon" -o /usr/local/bin/omw-server-daemon || \
  curl -fsSL "https://github.com/IvanoWisnuBudiLaras/Oh-My-Works/releases/download/v0.1.0/omw-server-daemon" -o /usr/local/bin/omw-server-daemon
fi

chmod +x /usr/local/bin/omw-server-daemon

# 4. Create & Enable Systemd Service
echo "=== [4/6] Configuring Systemd Service (omw-daemon) ==="
cat << 'EOF' > /etc/systemd/system/omw-daemon.service
[Unit]
Description=Oh My Works (OMW) 1Panel Lite & OmniRoute Daemon
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/root
Environment="OMW_DAEMON_PORT=9180"
Environment="OMW_DAEMON_HOST=0.0.0.0"
ExecStart=/usr/local/bin/omw-server-daemon
Restart=always
RestartSec=3
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable omw-daemon
systemctl restart omw-daemon

# 5. Restore Hermes Multi-Agent Config if present
if [ -f "./hermes-server-backup-latest.tar.gz" ]; then
  echo "=== [5/6] Restoring Hermes Agent Configuration ==="
  tar -xzvf ./hermes-server-backup-latest.tar.gz -C /root/
fi

# 6. Status and Instructions
# 6. Auto-configure Caddy if installed
echo "=== [6/6] Auto-configuring Caddy Reverse Proxy ==="
if [ -f /etc/caddy/Caddyfile ]; then
  if ! grep -q "panel.ivanowisnubudilaras.web.id" /etc/caddy/Caddyfile; then
    echo "Adding panel.ivanowisnubudilaras.web.id to /etc/caddy/Caddyfile..."
    cat << 'CADDY_EOF' >> /etc/caddy/Caddyfile

panel.ivanowisnubudilaras.web.id {
    reverse_proxy 127.0.0.1:9180
}
CADDY_EOF
  fi
  systemctl reload caddy || true
  echo "Caddy reloaded with HTTPS on panel.ivanowisnubudilaras.web.id!"
fi

echo "=== [6/6] Verifying Daemon Status ==="
sleep 1
systemctl status omw-daemon --no-pager || true

echo ""
echo "=========================================================================="
echo "  Oh My Works (OMW) 1Panel Lite Deployed Successfully on Server!"
echo "  - Local Port: http://127.0.0.1:9180/dashboard"
echo "  - Footprint:  ~4.8MB binary / ~5.7MB RAM"
echo ""
echo "  Next: Connect via Caddy Reverse Proxy for HTTPS:"
echo "    echo 'panel.ivanowisnubudilaras.web.id { reverse_proxy 127.0.0.1:9180 }' >> /etc/caddy/Caddyfile"
echo "    systemctl reload caddy"
echo "=========================================================================="
