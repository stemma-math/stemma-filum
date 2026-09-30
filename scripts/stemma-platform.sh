#!/usr/bin/env bash
# Installs or updates Stemma instances on this machine from the latest build
# of `main`, and leaves them running. Run it as root, as often as you like:
#
#     sudo ./stemma-platform.sh                    the instance called `default`
#     sudo ./stemma-platform.sh --instance dev     another instance, beside it
#     sudo ./stemma-platform.sh --with-caddy       also put Caddy in front, with a certificate
#     sudo ./stemma-platform.sh --force            reinstall the build even if it is current
#
# The first run of an instance asks for its configuration (the public address,
# who may register, outgoing mail) and offers to create its first
# administrator. The first run on the machine also asks for a GitHub token
# that can read the repository: a fine-grained token with read-only access to
# its contents. Later runs ask nothing, and change nothing that is already as
# it should be.
#
# Each instance has its own binary, configuration, data, port, service,
# backups and Caddy site, so instances can be at different builds:
#
#     /opt/stemma/NAME/stemma-server        the binary, and the one before it
#     /etc/stemma/NAME/server.toml          the configuration (secrets in server.env)
#     /var/lib/stemma-NAME                  the data
#     /var/backups/stemma/NAME              a backup before every update
#     stemma-server@NAME.service            the service
#     /usr/local/bin/stemma-server-NAME     the server's commands for this instance,
#                                           e.g. `sudo stemma-server-NAME status`
#
# An instance listens on a loopback port and serves no TLS itself, so it
# needs a reverse proxy in front. `--with-caddy` installs Caddy (on Debian or
# Ubuntu) and has it answer on the instance's public address, which needs the
# name to point at this machine and ports 80 and 443 to be reachable. Without
# it, the proxy is yours to set up.
#
# Needs: Linux with systemd, curl, jq, git, tar, sha256sum.

set -euo pipefail

REPO="stemma-math/stemma"
RELEASE="main-build"
ASSET="stemma-server-linux-x86_64.tar.gz"
USER_NAME="stemma"
TOKEN_FILE="/etc/stemma/github-token"
TEMPLATE="/etc/systemd/system/stemma-server@.service"
CADDYFILE="/etc/caddy/Caddyfile"
CADDY_SITES="/etc/caddy/sites"
FIRST_PORT=8080

say() { printf '\033[1m==> %s\033[0m\n' "$*"; }
die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

instance="default"
force=false
caddy=false
while [ $# -gt 0 ]; do
    case "$1" in
        --instance)
            [ $# -ge 2 ] || die "--instance needs a name"
            instance="$2"
            shift
            ;;
        --instance=*) instance="${1#--instance=}" ;;
        --force) force=true ;;
        --with-caddy) caddy=true ;;
        -h | --help) sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) die "unknown option: $1 (try --help)" ;;
    esac
    shift
done

[[ "$instance" =~ ^[a-z][a-z0-9-]{0,30}$ ]] \
    || die "an instance name is lowercase letters, digits and hyphens, starting with a letter"

HOME_DIR="/opt/stemma/$instance"
BINARY="$HOME_DIR/stemma-server"
PREVIOUS="$HOME_DIR/stemma-server.previous"
CONFIG="/etc/stemma/$instance/server.toml"
DATA="/var/lib/stemma-$instance"
BACKUPS="/var/backups/stemma/$instance"
UNIT="stemma-server@$instance.service"
COMMAND="/usr/local/bin/stemma-server-$instance"

[ "$(id -u)" -eq 0 ] || die "run it as root: sudo $0"
[ "$(uname -m)" = x86_64 ] || die "the builds are for x86_64 Linux only"
for tool in curl jq git tar sha256sum systemctl runuser flock; do
    command -v "$tool" >/dev/null || die "$tool is not installed"
done

# One run per instance at a time.
exec 9>"/run/stemma-platform-$instance.lock"
flock -n 9 || die "another run for $instance is in progress"

interactive() { [ -t 0 ] && [ -t 1 ]; }

# The server, for this instance, as the account it runs as.
server() { runuser -u "$USER_NAME" -- "$BINARY" --config "$CONFIG" "$@"; }

# A top-level string from the configuration file, as `setup` writes it.
setting() { # key [file]
    sed -n "s/^$1[[:space:]]*=[[:space:]]*\"\([^\"]*\)\".*/\1/p" "${2:-$CONFIG}" | head -n 1
}

# Writes a file only when its content differs, and says whether it did.
put() { # path mode content
    if [ "$(cat "$1" 2>/dev/null)" = "$3" ]; then
        return 1
    fi
    install -d -m 755 "$(dirname "$1")"
    printf '%s\n' "$3" >"$1.new"
    chmod "$2" "$1.new"
    mv -f "$1.new" "$1"
}

# --- The token ---------------------------------------------------------------

token="${STEMMA_GITHUB_TOKEN:-}"
if [ -z "$token" ] && [ -r "$TOKEN_FILE" ]; then
    token="$(<"$TOKEN_FILE")"
fi
if [ -z "$token" ]; then
    interactive || die "no GitHub token: put one in $TOKEN_FILE or STEMMA_GITHUB_TOKEN"
    read -rsp "GitHub token that can read $REPO: " token
    echo
    install -d -m 755 "$(dirname "$TOKEN_FILE")"
    (umask 077 && printf '%s\n' "$token" >"$TOKEN_FILE")
fi

api() {
    curl -fsSL -H "Authorization: Bearer $token" -H "X-GitHub-Api-Version: 2022-11-28" "$@"
}

# --- What is there, and what is installed ------------------------------------

say "Looking for the latest build"
release="$(api "https://api.github.com/repos/$REPO/releases/tags/$RELEASE")" \
    || die "cannot read the $RELEASE release: is the token right?"

download() { # asset-name destination
    local id
    id="$(jq -r --arg name "$1" '.assets[] | select(.name == $name) | .id' <<<"$release")"
    [ -n "$id" ] || die "the release has no $1"
    api -H "Accept: application/octet-stream" \
        "https://api.github.com/repos/$REPO/releases/assets/$id" -o "$2"
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

download build.json "$work/build.json"
latest="$(jq -r .version "$work/build.json")"
installed=""
if [ -x "$BINARY" ]; then
    installed="$("$BINARY" --version | awk '{print $2}')"
fi
echo "    instance:  $instance"
echo "    latest:    $latest"
echo "    installed: ${installed:-nothing}"

# --- The binary --------------------------------------------------------------

updated=false
backup=""
if [ "$installed" != "$latest" ] || $force; then
    say "Downloading $latest"
    download "$ASSET" "$work/$ASSET"
    download SHA256SUMS "$work/SHA256SUMS"
    (cd "$work" && grep " $ASSET\$" SHA256SUMS | sha256sum -c --quiet -) \
        || die "the download does not match its checksum"
    tar -xzf "$work/$ASSET" -C "$work"
    [ "$("$work/stemma-server" --version | awk '{print $2}')" = "$latest" ] \
        || die "the downloaded binary does not say it is $latest"

    if [ -n "$installed" ] && [ -f "$CONFIG" ]; then
        say "Backing up $instance"
        install -d -m 700 -o "$USER_NAME" -g "$USER_NAME" "$BACKUPS"
        server backup --to "$BACKUPS" --keep 10
        backup="$(find "$BACKUPS" -maxdepth 1 -type f -printf '%T@ %p\n' | sort -rn | head -n 1 | cut -d' ' -f2-)"
        echo "    $backup"
    fi

    say "Installing $latest"
    systemctl stop "$UNIT" 2>/dev/null || true
    install -d -m 755 "$HOME_DIR"
    if [ -x "$BINARY" ]; then
        cp -f "$BINARY" "$PREVIOUS"
    fi
    install -m 755 "$work/stemma-server" "$BINARY.new"
    mv -f "$BINARY.new" "$BINARY"
    updated=true
fi

# --- The instance ------------------------------------------------------------

say "Checking the installation"
fresh=false
[ -f "$CONFIG" ] || fresh=true

# The account, the directories, the configuration skeleton and the session
# key, or word that they are already there. The unit is this script's, so
# that each instance has its own.
"$BINARY" --config "$CONFIG" install --data-dir "$DATA" --user "$USER_NAME" --no-service

if $fresh; then
    # Each instance on its own port: the first free one from FIRST_PORT on.
    port=$FIRST_PORT
    while grep -qsE "^listen[[:space:]]*=[[:space:]]*\"[^\"]*:$port\"" /etc/stemma/*/server.toml \
        || { command -v ss >/dev/null && ss -Hltn "sport = :$port" | grep -q .; }; do
        port=$((port + 1))
    done
    sed -i -E "s|^listen[[:space:]]*=[[:space:]]*\"[^\"]*\"|listen = \"127.0.0.1:$port\"|" "$CONFIG"

    interactive || die "a new instance needs its configuration: run this again from a terminal"
    say "Configuring $instance"
    "$BINARY" --config "$CONFIG" setup
fi

# One unit for every instance; `%i` is the instance's name.
unit="# Written by stemma-platform.sh; it is rewritten on every run.
[Unit]
Description=Stemma server (%i)
After=network-online.target
Wants=network-online.target

[Service]
Type=exec
ExecStart=/opt/stemma/%i/stemma-server --config /etc/stemma/%i/server.toml serve
User=$USER_NAME
Group=$USER_NAME
EnvironmentFile=-/etc/stemma/%i/server.env
StateDirectory=stemma-%i
StateDirectoryMode=0750
CacheDirectory=stemma-%i
RuntimeDirectory=stemma-%i
Restart=on-failure
RestartSec=5s

# The process serves HTTP and writes one directory. Everything
# else on the machine is out of its reach.
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ProtectKernelTunables=true
ProtectControlGroups=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
RestrictSUIDSGID=true
LockPersonality=true

[Install]
WantedBy=multi-user.target"
if put "$TEMPLATE" 644 "$unit"; then
    systemctl daemon-reload
fi

put "$COMMAND" 755 "#!/bin/sh
# The server's commands for the $instance instance, as the account it runs as.
# Written by stemma-platform.sh.
exec runuser -u $USER_NAME -- $BINARY --config $CONFIG \"\$@\"" || true

say "Bringing the database up to date"
server migrate

say "Checking $instance"
if ! "$BINARY" --config "$CONFIG" check --user "$USER_NAME"; then
    if $updated && [ -f "$PREVIOUS" ]; then
        echo
        echo "The new build does not pass its checks. To go back:"
        echo "    sudo cp $PREVIOUS $BINARY"
        [ -n "$backup" ] && echo "and restore the database from $backup."
    fi
    die "$instance is not ready to start (\`stemma-server-$instance setup\` asks the configuration questions again)"
fi

say "Starting $instance"
systemctl enable --quiet "$UNIT"
systemctl restart "$UNIT"
sleep 3
if ! systemctl is-active --quiet "$UNIT"; then
    journalctl -u "$UNIT" -n 30 --no-pager
    die "the service did not stay up"
fi

# --- The first administrator -------------------------------------------------

if server admin list | grep -q '^Nobody maintains' && interactive; then
    echo
    read -rp "Create the first administrator of $instance now? [Y/n] " answer
    if [[ ! "$answer" =~ ^[Nn] ]]; then
        read -rp "  Their name here (letters, digits, hyphens): " name
        read -rp "  Their email address: " email
        server user create --email "$email" "$name"
        server admin grant "$name"
        echo "    Send them the link above to set their password."
    else
        echo "    Until there is an account, anyone who opens /register can create the first"
        echo "    one, even with registration closed. \`stemma-server-$instance user create\`"
        echo "    and \`admin grant\` make the administrator whenever you are ready."
    fi
fi

# --- The proxy ---------------------------------------------------------------

if $caddy; then
    public_url="$(setting public_url)"
    listen="$(setting listen)"
    listen="${listen:-127.0.0.1:$FIRST_PORT}"
    host="$(sed -E 's#^https?://##; s#[/:].*$##' <<<"$public_url")"
    case "$public_url" in
        https://*) ;;
        *) die "Caddy needs an https:// public_url in $CONFIG, not \`$public_url\`" ;;
    esac
    [ "$host" != stemma.example.org ] || die "public_url in $CONFIG is still the example"

    if ! command -v caddy >/dev/null; then
        command -v apt-get >/dev/null || die "Caddy is installed here only with apt; install it yourself"
        say "Installing Caddy"
        apt-get update -qq
        apt-get install -y -qq debian-keyring debian-archive-keyring apt-transport-https gnupg
        curl -1sLf https://dl.cloudsmith.io/public/caddy/stable/gpg.key \
            | gpg --dearmor --yes -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
        curl -1sLf https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt \
            >/etc/apt/sources.list.d/caddy-stable.list
        chmod o+r /usr/share/keyrings/caddy-stable-archive-keyring.gpg /etc/apt/sources.list.d/caddy-stable.list
        apt-get update -qq
        apt-get install -y -qq caddy
    fi

    say "Putting Caddy in front of $host"
    case "$listen" in
        unix:*) upstream="unix/${listen#unix:}" ;;
        *) upstream="$listen" ;;
    esac
    changed=false
    if put "$CADDY_SITES/stemma-$instance.caddy" 644 "# Written by stemma-platform.sh from $CONFIG; it is rewritten on every run.
$host {
	reverse_proxy $upstream
}"; then
        changed=true
    fi

    # The packaged Caddyfile serves a placeholder page on port 80; it gives
    # way to the sites, and a Caddyfile somebody wrote only gains the import.
    import="import $CADDY_SITES/*.caddy"
    if [ ! -f "$CADDYFILE" ] || grep -q '/usr/share/caddy' "$CADDYFILE"; then
        printf '%s\n' "$import" >"$CADDYFILE"
        changed=true
    elif ! grep -qxF "$import" "$CADDYFILE"; then
        printf '\n%s\n' "$import" >>"$CADDYFILE"
        changed=true
    fi

    caddy validate --config "$CADDYFILE" --adapter caddyfile >/dev/null 2>&1 \
        || die "Caddy does not accept $CADDYFILE: run \`caddy validate --config $CADDYFILE\`"
    systemctl enable --quiet caddy
    if ! systemctl is-active --quiet caddy; then
        systemctl start caddy
    elif $changed; then
        systemctl reload caddy
    fi
    echo "    $public_url -> $upstream"
fi

server status
say "Stemma $latest is running as $instance"
