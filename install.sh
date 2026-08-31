#!/bin/sh
set -eu

REPOSITORY="${HYPFORWARD_REPOSITORY:-greepar/HypForward}"
VERSION="${HYPFORWARD_VERSION:-latest}"
INSTALL_DIR="${HYPFORWARD_INSTALL_DIR:-/usr/local/bin}"
INSTALL_SERVICE="${HYPFORWARD_INSTALL_SERVICE:-auto}"

info() {
    printf '[*] %s\n' "$*"
}

fail() {
    printf '[-] %s\n' "$*" >&2
    exit 1
}

command_exists() {
    command -v "$1" >/dev/null 2>&1
}

download() {
    url=$1
    output=$2

    if command_exists curl; then
        curl -fL --retry 3 --connect-timeout 15 "$url" -o "$output"
    elif command_exists wget; then
        wget -O "$output" "$url"
    else
        fail "curl or wget is required"
    fi
}

sha256() {
    if command_exists sha256sum; then
        sha256sum "$1" | cut -d ' ' -f 1
    elif command_exists shasum; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    else
        fail "sha256sum or shasum is required"
    fi
}

run_as_root() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
    elif command_exists sudo; then
        sudo "$@"
    else
        fail "root privileges are required; run as root or install sudo"
    fi
}

install_systemd_service() {
    service_file="${tmp_dir}/hypforward.service"
    cat >"$service_file" <<EOF
[Unit]
Description=HypForward Minecraft forwarding proxy
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=${INSTALL_DIR}/hypforward
Restart=on-failure
RestartSec=3
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true

[Install]
WantedBy=multi-user.target
EOF
    run_as_root install -m 0644 "$service_file" /etc/systemd/system/hypforward.service
    run_as_root systemctl daemon-reload
    run_as_root systemctl enable --now hypforward.service
    info "Enabled and started hypforward.service with systemd"
}

install_openrc_service() {
    service_file="${tmp_dir}/hypforward.openrc"
    cat >"$service_file" <<EOF
#!/sbin/openrc-run

name="HypForward"
description="HypForward Minecraft forwarding proxy"
command="${INSTALL_DIR}/hypforward"
command_background="yes"
pidfile="/run/hypforward.pid"

depend() {
    need net
    after firewall
}
EOF
    run_as_root install -m 0755 "$service_file" /etc/init.d/hypforward
    run_as_root rc-update add hypforward default
    if run_as_root rc-service hypforward status >/dev/null 2>&1; then
        run_as_root rc-service hypforward restart
    else
        run_as_root rc-service hypforward start
    fi
    info "Enabled and started hypforward with OpenRC"
}

install_sysv_service() {
    service_file="${tmp_dir}/hypforward.sysv"
    cat >"$service_file" <<EOF
#!/bin/sh
### BEGIN INIT INFO
# Provides:          hypforward
# Required-Start:    \$network
# Required-Stop:     \$network
# Default-Start:     2 3 4 5
# Default-Stop:      0 1 6
# Short-Description: HypForward Minecraft forwarding proxy
### END INIT INFO

DAEMON="${INSTALL_DIR}/hypforward"
PIDFILE="/run/hypforward.pid"
LOGFILE="/var/log/hypforward.log"

is_running() {
    [ -f "\$PIDFILE" ] && kill -0 "\$(cat "\$PIDFILE")" 2>/dev/null
}

case "\${1:-}" in
    start)
        if is_running; then
            exit 0
        fi
        nohup "\$DAEMON" >>"\$LOGFILE" 2>&1 &
        echo \$! >"\$PIDFILE"
        ;;
    stop)
        if is_running; then
            kill "\$(cat "\$PIDFILE")"
        fi
        rm -f "\$PIDFILE"
        ;;
    restart)
        "\$0" stop
        "\$0" start
        ;;
    status)
        is_running
        ;;
    *)
        echo "Usage: \$0 {start|stop|restart|status}" >&2
        exit 2
        ;;
esac
EOF
    run_as_root install -m 0755 "$service_file" /etc/init.d/hypforward
    if command_exists update-rc.d; then
        run_as_root update-rc.d hypforward defaults
    elif command_exists chkconfig; then
        run_as_root chkconfig --add hypforward
        run_as_root chkconfig hypforward on
    fi
    if run_as_root service hypforward status >/dev/null 2>&1; then
        run_as_root service hypforward restart
    else
        run_as_root service hypforward start
    fi
    info "Enabled and started hypforward with SysV init"
}

install_runit_service() {
    run_file="${tmp_dir}/hypforward.run"
    cat >"$run_file" <<EOF
#!/bin/sh
exec 2>&1
exec ${INSTALL_DIR}/hypforward
EOF
    run_as_root mkdir -p /etc/sv/hypforward
    run_as_root install -m 0755 "$run_file" /etc/sv/hypforward/run

    if [ -d /var/service ]; then
        service_dir=/var/service
    else
        service_dir=/etc/service
        run_as_root mkdir -p "$service_dir"
    fi
    run_as_root ln -sfn /etc/sv/hypforward "${service_dir}/hypforward"
    if command_exists sv; then
        run_as_root sv up hypforward
    fi
    info "Enabled and started hypforward with runit"
}

detect_init_system() {
    if command_exists systemctl && [ -d /run/systemd/system ]; then
        printf '%s\n' systemd
    elif command_exists rc-service && command_exists rc-update; then
        printf '%s\n' openrc
    elif command_exists sv && { [ -d /etc/sv ] || [ -d /etc/service ] || [ -d /var/service ]; }; then
        printf '%s\n' runit
    elif command_exists service && [ -d /etc/init.d ]; then
        printf '%s\n' sysv
    else
        printf '%s\n' none
    fi
}

case "$(uname -s)" in
    Linux)
        os=linux
        ;;
    Darwin)
        os=macos
        ;;
    *)
        fail "unsupported operating system: $(uname -s)"
        ;;
esac

case "$(uname -m)" in
    x86_64 | amd64)
        arch=x86_64
        ;;
    arm64 | aarch64)
        arch=aarch64
        ;;
    *)
        fail "unsupported architecture: $(uname -m)"
        ;;
esac

archive="hypforward-${os}-${arch}.tar.gz"
if [ "$VERSION" = latest ]; then
    base_url="https://github.com/${REPOSITORY}/releases/latest/download"
else
    case "$VERSION" in
        v*) release_tag=$VERSION ;;
        *) release_tag="v${VERSION}" ;;
    esac
    base_url="https://github.com/${REPOSITORY}/releases/download/${release_tag}"
fi

tmp_dir=$(mktemp -d 2>/dev/null || mktemp -d -t hypforward)
trap 'rm -rf "$tmp_dir"' EXIT HUP INT TERM

info "Downloading ${archive}"
download "${base_url}/${archive}" "${tmp_dir}/${archive}"
download "${base_url}/SHA256SUMS" "${tmp_dir}/SHA256SUMS"

expected=$(awk -v file="$archive" '$2 == file || $2 == "*" file { print $1; exit }' "${tmp_dir}/SHA256SUMS")
[ -n "$expected" ] || fail "${archive} is missing from SHA256SUMS"
actual=$(sha256 "${tmp_dir}/${archive}")
[ "$actual" = "$expected" ] || fail "checksum verification failed"
info "Checksum verified"

tar -xzf "${tmp_dir}/${archive}" -C "$tmp_dir"
[ -f "${tmp_dir}/hypforward" ] || fail "archive does not contain hypforward"

run_as_root mkdir -p "$INSTALL_DIR"
run_as_root install -m 0755 "${tmp_dir}/hypforward" "${INSTALL_DIR}/hypforward"
info "Installed ${INSTALL_DIR}/hypforward"

if [ "$os" != linux ] || [ "$INSTALL_SERVICE" = 0 ] || [ "$INSTALL_SERVICE" = none ]; then
    info "Run hypforward to start the proxy"
else
    if [ "$INSTALL_SERVICE" = auto ] || [ "$INSTALL_SERVICE" = 1 ]; then
        init_system=$(detect_init_system)
    else
        init_system=$INSTALL_SERVICE
    fi

    case "$init_system" in
        systemd)
            command_exists systemctl || fail "systemctl is required for systemd installation"
            install_systemd_service
            ;;
        openrc)
            command_exists rc-service || fail "rc-service is required for OpenRC installation"
            command_exists rc-update || fail "rc-update is required for OpenRC installation"
            install_openrc_service
            ;;
        sysv)
            command_exists service || fail "service is required for SysV installation"
            install_sysv_service
            ;;
        runit)
            command_exists sv || fail "sv is required for runit installation"
            install_runit_service
            ;;
        none)
            if [ "$INSTALL_SERVICE" = 1 ]; then
                fail "no supported init system was detected"
            fi
            info "No supported init system detected; run hypforward manually"
            ;;
        *)
            fail "unsupported service manager: ${init_system}"
            ;;
    esac
fi
