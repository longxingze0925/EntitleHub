#!/usr/bin/env bash
set -Eeuo pipefail

APP_NAME="EntitleHub"
PROJECT_NAME="${USER_ADMIN_PROJECT_NAME:-entitle-hub}"
INSTALL_DIR="${USER_ADMIN_INSTALL_DIR:-/opt/entitle-hub}"
USER_ADMIN_REPO="${USER_ADMIN_REPO:-longxingze0925/EntitleHub}"
USER_ADMIN_REF="${USER_ADMIN_REF:-main}"
USER_ADMIN_RAW_BASE="${USER_ADMIN_RAW_BASE:-https://raw.githubusercontent.com/${USER_ADMIN_REPO}/${USER_ADMIN_REF}}"
USER_ADMIN_ARCHIVE_URL="${USER_ADMIN_ARCHIVE_URL:-https://github.com/${USER_ADMIN_REPO}/archive/refs/heads/${USER_ADMIN_REF}.tar.gz}"

ENV_FILE=".env.compose"
STATE_FILE=".install-state"
BACKUP_DIR="backups/installer"
DIGEST_FILE="${USER_ADMIN_DIGEST_FILE:-compose.digests.yaml}"
PIN_DIGESTS="${USER_ADMIN_PIN_DIGESTS:-1}"
DEPLOY_MODE="${USER_ADMIN_DEPLOY_MODE:-image}"

LOCAL_SOURCE_ROOT=""
if [[ -n "${BASH_SOURCE[0]:-}" && -f "${BASH_SOURCE[0]}" ]]; then
  script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd -P || true)"
  if [[ -n "$script_dir" && -f "$script_dir/../compose.yaml" ]]; then
    LOCAL_SOURCE_ROOT="$(cd "$script_dir/.." && pwd -P)"
  fi
fi

log() {
  printf '\n==> %s\n' "$*"
}

warn() {
  printf '警告：%s\n' "$*" >&2
}

die() {
  printf '错误：%s\n' "$*" >&2
  exit 1
}

pause() {
  printf '\n按 Enter 继续...'
  read -r _ || true
}

ask() {
  local prompt="$1"
  local default="${2:-}"
  local value
  if [[ -n "$default" ]]; then
    printf '%s [%s]: ' "$prompt" "$default" >&2
  else
    printf '%s: ' "$prompt" >&2
  fi
  read -r value
  if [[ -z "$value" ]]; then
    printf '%s' "$default"
  else
    printf '%s' "$value"
  fi
}

ask_secret_optional() {
  local prompt="$1"
  local value
  printf '%s: ' "$prompt" >&2
  read -r -s value
  printf '\n' >&2
  printf '%s' "$value"
}

confirm() {
  local prompt="$1"
  local answer
  printf '%s [y/N]: ' "$prompt"
  read -r answer
  [[ "$answer" == "y" || "$answer" == "Y" || "$answer" == "yes" || "$answer" == "YES" || "$answer" == "是" ]]
}

pin_digests_enabled() {
  [[ "$PIN_DIGESTS" != "0" && "$PIN_DIGESTS" != "false" && "$PIN_DIGESTS" != "FALSE" ]]
}

require_root() {
  if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
    die "此操作需要 root 权限，请使用 sudo 或 root 重新运行。"
  fi
  assert_safe_install_dir
}

assert_safe_install_dir() {
  if [[ -z "$INSTALL_DIR" || "$INSTALL_DIR" == "/" || "$INSTALL_DIR" == "/opt" || "$INSTALL_DIR" == "/usr" || "$INSTALL_DIR" == "/var" ]]; then
    die "安装目录不安全：$INSTALL_DIR"
  fi
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "缺少必需命令：$1"
}

is_installed() {
  [[ -f "$INSTALL_DIR/compose.yaml" && -f "$INSTALL_DIR/$ENV_FILE" ]]
}

in_install_dir() {
  cd "$INSTALL_DIR"
}

compose_base() {
  local args=(-p "$PROJECT_NAME" --env-file "$ENV_FILE" -f compose.yaml)
  if [[ -f compose.proxy.yml ]]; then
    args+=(-f compose.proxy.yml)
  fi
  if [[ "$DEPLOY_MODE" != "source" && -f "$DIGEST_FILE" ]] && pin_digests_enabled; then
    args+=(-f "$DIGEST_FILE")
  fi
  docker compose "${args[@]}" "$@"
}

compose_unpinned() {
  local args=(-p "$PROJECT_NAME" --env-file "$ENV_FILE" -f compose.yaml)
  if [[ -f compose.proxy.yml ]]; then
    args+=(-f compose.proxy.yml)
  fi
  docker compose "${args[@]}" "$@"
}

get_env_value() {
  local key="$1"
  local file="${2:-$INSTALL_DIR/$ENV_FILE}"
  [[ -f "$file" ]] || return 1
  awk -F= -v key="$key" '$1 == key { sub(/^[^=]*=/, ""); print; exit }' "$file"
}

set_env_value() {
  local file="$1"
  local key="$2"
  local value="$3"
  local tmp
  tmp="$(mktemp)"
  awk -v key="$key" -v value="$value" '
    BEGIN { done = 0 }
    index($0, key "=") == 1 {
      print key "=" value
      done = 1
      next
    }
    { print }
    END {
      if (!done) {
        print key "=" value
      }
    }
  ' "$file" > "$tmp"
  cat "$tmp" > "$file"
  rm -f "$tmp"
}

random_urlsafe() {
  openssl rand -base64 32 | tr '+/' '-_' | tr -d '='
}

random_base64() {
  openssl rand -base64 32
}

detect_public_ip() {
  local ip=""
  if command -v curl >/dev/null 2>&1; then
    ip="$(curl -fsS --max-time 5 https://api.ipify.org 2>/dev/null || true)"
  fi
  if [[ -z "$ip" ]]; then
    ip="$(hostname -I 2>/dev/null | awk '{print $1}' || true)"
  fi
  printf '%s' "$ip"
}

check_port_available() {
  local port="$1"
  if command -v ss >/dev/null 2>&1; then
    ! ss -ltn "( sport = :$port )" | awk 'NR > 1 { found = 1 } END { exit found ? 0 : 1 }'
    return
  fi
  if command -v lsof >/dev/null 2>&1; then
    ! lsof -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1
    return
  fi
  return 0
}

install_docker_prompt() {
  if command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
    return
  fi

  warn "未检测到 Docker 或 Docker Compose 插件。"
  if confirm "是否现在使用 Docker 官方脚本安装？"; then
    require_command curl
    curl -fsSL https://get.docker.com | sh
    systemctl enable --now docker >/dev/null 2>&1 || true
  else
    die "请先安装 Docker 和 Docker Compose 插件，然后重新运行安装器。"
  fi
}

preflight() {
  require_command curl
  require_command tar
  require_command openssl
  case "$DEPLOY_MODE" in
    image | source) ;;
    *) die "USER_ADMIN_DEPLOY_MODE 只能是 image 或 source。" ;;
  esac
  install_docker_prompt
  docker version >/dev/null
  docker compose version >/dev/null
}

fetch_source() {
  local dest="$1"
  rm -rf "$dest"
  mkdir -p "$dest"

  if [[ -n "$LOCAL_SOURCE_ROOT" && "$LOCAL_SOURCE_ROOT" != "$INSTALL_DIR" ]]; then
    log "复制本地项目文件"
    (
      cd "$LOCAL_SOURCE_ROOT"
      tar \
        --exclude='./backend/target' \
        --exclude='./client-sdk/target' \
        --exclude='./admin/node_modules' \
        --exclude='./admin/dist' \
        --exclude='./target' \
        --exclude='./backups' \
        --exclude='./.tools' \
        -cf - .
    ) | (
      cd "$dest"
      tar -xf -
    )
    return
  fi

  log "下载最新源码包"
  curl -fsSL "$USER_ADMIN_ARCHIVE_URL" | tar -xz --strip-components=1 -C "$dest"
}

safe_refresh_source() {
  local tmp
  assert_safe_install_dir
  tmp="$(mktemp -d)"
  fetch_source "$tmp"

  mkdir -p "$INSTALL_DIR"
  if [[ -f "$INSTALL_DIR/$ENV_FILE" ]]; then
    cp "$INSTALL_DIR/$ENV_FILE" "$tmp/$ENV_FILE"
  fi
  if [[ -f "$INSTALL_DIR/$STATE_FILE" ]]; then
    cp "$INSTALL_DIR/$STATE_FILE" "$tmp/$STATE_FILE"
  fi
  if [[ -f "$INSTALL_DIR/$DIGEST_FILE" ]]; then
    cp "$INSTALL_DIR/$DIGEST_FILE" "$tmp/$DIGEST_FILE"
  fi
  if [[ -d "$INSTALL_DIR/certs" ]]; then
    mkdir -p "$tmp/certs"
    cp -a "$INSTALL_DIR/certs/." "$tmp/certs/"
  fi
  if [[ -f "$INSTALL_DIR/Caddyfile" ]]; then
    cp "$INSTALL_DIR/Caddyfile" "$tmp/Caddyfile"
  fi
  if [[ -f "$INSTALL_DIR/compose.proxy.yml" ]]; then
    cp "$INSTALL_DIR/compose.proxy.yml" "$tmp/compose.proxy.yml"
  fi
  if [[ -d "$INSTALL_DIR/backups" ]]; then
    mkdir -p "$tmp/backups"
    cp -a "$INSTALL_DIR/backups/." "$tmp/backups/"
  fi

  find "$INSTALL_DIR" -mindepth 1 -maxdepth 1 \
    ! -name 'backups' \
    ! -name '.env.compose' \
    ! -name "$DIGEST_FILE" \
    ! -name 'certs' \
    ! -name 'Caddyfile' \
    ! -name 'compose.proxy.yml' \
    ! -name '.install-state' \
    -exec rm -rf {} +

  cp -a "$tmp/." "$INSTALL_DIR/"
  rm -rf "$tmp"
}

write_state() {
  local mode="$1"
  local public_url="$2"
  local domain="${3:-}"
  cat > "$INSTALL_DIR/$STATE_FILE" <<EOF
MODE=$mode
PUBLIC_URL=$public_url
DOMAIN=$domain
INSTALLED_AT=$(date -u +%Y-%m-%dT%H:%M:%SZ)
SOURCE_REF=$USER_ADMIN_REF
EOF
  chmod 600 "$INSTALL_DIR/$STATE_FILE"
}

write_env_file() {
  local mode="$1"
  local public_url="$2"
  local backend_public_url="$3"
  local host_bind="$4"
  local cookie_secure="$5"
  local env_path="$INSTALL_DIR/$ENV_FILE"

  cp "$INSTALL_DIR/.env.compose.example" "$env_path"
  chmod 600 "$env_path"

  local postgres_db postgres_user postgres_password redis_password grafana_password
  postgres_db="$(get_env_value POSTGRES_DB "$env_path")"
  postgres_user="$(get_env_value POSTGRES_USER "$env_path")"
  postgres_password="$(random_urlsafe)"
  redis_password="$(random_urlsafe)"
  grafana_password="$(random_urlsafe)"

  set_env_value "$env_path" COMPOSE_ENV_FILE "$ENV_FILE"
  set_env_value "$env_path" COMPOSE_HOST_BIND "$host_bind"
  set_env_value "$env_path" POSTGRES_PASSWORD "$postgres_password"
  set_env_value "$env_path" REDIS_PASSWORD "$redis_password"
  set_env_value "$env_path" GRAFANA_ADMIN_USER "admin"
  set_env_value "$env_path" GRAFANA_ADMIN_PASSWORD "$grafana_password"
  set_env_value "$env_path" DATABASE_URL "postgres://${postgres_user}:${postgres_password}@postgres:5432/${postgres_db}"
  set_env_value "$env_path" REDIS_URL "redis://:${redis_password}@redis:6379"
  if [[ "$cookie_secure" == "true" ]]; then
    set_env_value "$env_path" APP_ENV "production"
  else
    set_env_value "$env_path" APP_ENV "development"
  fi
  set_env_value "$env_path" APP_BASE_URL "$public_url"
  set_env_value "$env_path" APP_LOG_LEVEL "info"
  set_env_value "$env_path" ALLOWED_ORIGINS "$public_url"
  set_env_value "$env_path" JWT_ISSUER "$backend_public_url"
  set_env_value "$env_path" COOKIE_SECURE "$cookie_secure"
  set_env_value "$env_path" SESSION_SECRET "$(random_urlsafe)"
  set_env_value "$env_path" TOKEN_HASH_PEPPER "$(random_urlsafe)"
  set_env_value "$env_path" REFRESH_TOKEN_PEPPER "$(random_urlsafe)"
  set_env_value "$env_path" CSRF_SECRET "$(random_urlsafe)"
  set_env_value "$env_path" MASTER_KEY "$(random_base64)"
  set_env_value "$env_path" ALERTMANAGER_WEBHOOK_TOKEN "$(random_urlsafe)"

  if [[ "$mode" == "domain-auto" || "$mode" == "domain-custom" || "$mode" == "external-proxy" ]]; then
    set_env_value "$env_path" BACKEND_HOST_PORT "18080"
    set_env_value "$env_path" ADMIN_HOST_PORT "15173"
  fi
}

write_caddy_files() {
  local domain="$1"
  local tls_mode="$2"
  local cert_path="${3:-}"
  local key_path="${4:-}"

  mkdir -p "$INSTALL_DIR/certs"
  if [[ "$tls_mode" == "custom" ]]; then
    [[ -f "$cert_path" ]] || die "证书文件不存在：$cert_path"
    [[ -f "$key_path" ]] || die "私钥文件不存在：$key_path"
    openssl x509 -in "$cert_path" -noout >/dev/null
    openssl pkey -in "$key_path" -noout >/dev/null
    cp "$cert_path" "$INSTALL_DIR/certs/fullchain.pem"
    cp "$key_path" "$INSTALL_DIR/certs/privkey.pem"
    chmod 600 "$INSTALL_DIR/certs/privkey.pem"
  fi

  cat > "$INSTALL_DIR/Caddyfile" <<EOF
$domain {
    encode zstd gzip
EOF

  if [[ "$tls_mode" == "custom" ]]; then
    cat >> "$INSTALL_DIR/Caddyfile" <<'EOF'
    tls /etc/caddy/certs/fullchain.pem /etc/caddy/certs/privkey.pem
EOF
  fi

  cat >> "$INSTALL_DIR/Caddyfile" <<'EOF'

    handle /api/* {
        reverse_proxy backend:8080
    }

    handle /.well-known/* {
        reverse_proxy backend:8080
    }

    handle {
        reverse_proxy admin:80
    }
}
EOF

  cat > "$INSTALL_DIR/compose.proxy.yml" <<'EOF'
services:
  caddy:
    image: ${CADDY_IMAGE:-caddy:2}
    depends_on:
      admin:
        condition: service_healthy
      backend:
        condition: service_healthy
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile:ro
      - ./certs:/etc/caddy/certs:ro
      - caddy-data:/data
      - caddy-config:/config
    restart: unless-stopped

volumes:
  caddy-data:
  caddy-config:
EOF
}

write_external_proxy_example() {
  local domain="$1"
  cat > "$INSTALL_DIR/reverse-proxy.nginx.example.conf" <<EOF
server {
    listen 443 ssl http2;
    server_name $domain;

    ssl_certificate     /path/to/fullchain.pem;
    ssl_certificate_key /path/to/privkey.pem;

    location /api/ {
        proxy_pass http://127.0.0.1:18080;
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
    }

    location /.well-known/ {
        proxy_pass http://127.0.0.1:18080;
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
    }

    location / {
        proxy_pass http://127.0.0.1:15173;
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
    }
}
EOF
}

wait_for_http() {
  local url="$1"
  local name="$2"
  local attempts="${3:-40}"
  local i
  for ((i = 1; i <= attempts; i++)); do
    if curl -fsS --max-time 5 "$url" >/dev/null 2>&1; then
      printf '%s 已就绪。\n' "$name"
      return 0
    fi
    sleep 3
  done
  return 1
}

run_migrations() {
  log "执行数据库迁移"
  in_install_dir
  compose_base up -d postgres redis
  compose_base run --rm backend user-admin-backend migrate
}

pin_image_digests() {
  if [[ "$DEPLOY_MODE" == "source" ]]; then
    warn "源码构建模式已跳过镜像 digest 锁定。"
    rm -f "$DIGEST_FILE"
    return
  fi
  if ! pin_digests_enabled; then
    warn "已跳过镜像 digest 锁定。"
    rm -f "$DIGEST_FILE"
    return
  fi

  log "锁定本次部署使用的镜像"
  in_install_dir

  local tmp_file
  tmp_file="$(mktemp)"
  {
    printf '# Generated by ops/user-adminctl.sh. Do not edit by hand.\n'
    printf 'services:\n'
  } > "$tmp_file"

  local count=0
  local service image digest
  while IFS=$'\t' read -r service image; do
    [[ -n "$service" && -n "$image" ]] || continue

    if [[ "$image" == *@sha256:* ]]; then
      digest="$image"
    else
      printf '拉取镜像：%s\n' "$image"
      docker pull "$image"
      digest="$(docker image inspect --format '{{if .RepoDigests}}{{index .RepoDigests 0}}{{end}}' "$image" 2>/dev/null || true)"
      [[ -n "$digest" ]] || die "无法获取镜像 digest：$image"
    fi

    {
      printf '  %s:\n' "$service"
      printf '    image: "%s"\n' "$digest"
    } >> "$tmp_file"
    count=$((count + 1))
  done < <(
    compose_unpinned config | awk '
      /^  [A-Za-z0-9_.-]+:$/ {
        service = $1
        sub(/:$/, "", service)
        next
      }
      service != "" && /^    image:[[:space:]]+/ {
        image = $0
        sub(/^    image:[[:space:]]+/, "", image)
        gsub(/^"|"$/, "", image)
        gsub(/^'\''|'\''$/, "", image)
        print service "\t" image
      }
    '
  )

  [[ "$count" -gt 0 ]] || die "未找到可锁定的镜像服务。"
  mv "$tmp_file" "$DIGEST_FILE"
  chmod 644 "$DIGEST_FILE"
  printf '镜像版本已锁定：%s\n' "$INSTALL_DIR/$DIGEST_FILE"
}

prepare_images() {
  in_install_dir
  if [[ "$DEPLOY_MODE" == "source" ]]; then
    log "源码构建服务镜像"
    rm -f "$DIGEST_FILE"
    compose_unpinned build --pull
    return
  fi

  log "拉取预构建服务镜像"
  if ! pin_digests_enabled; then
    compose_unpinned pull
  fi
  pin_image_digests
}

start_stack() {
  log "启动服务"
  in_install_dir
  compose_base up -d --no-build
}

init_owner() {
  log "按需初始化管理员"
  in_install_dir
  local public_url domain owner_email owner_name owner_password credentials_path
  public_url="$(get_env_value PUBLIC_URL "$STATE_FILE" || true)"
  domain="$(get_env_value DOMAIN "$STATE_FILE" || true)"
  if [[ -n "$domain" ]]; then
    owner_email="admin@${domain}"
  else
    owner_email="admin@entitlehub.local"
  fi
  owner_name="${APP_NAME} Owner"
  owner_password="$(random_urlsafe)"
  credentials_path="$INSTALL_DIR/owner-credentials.txt"

  if compose_base run --rm \
    -e INIT_TENANT_NAME="$APP_NAME" \
    -e INIT_TENANT_SLUG="entitlehub" \
    -e INIT_OWNER_EMAIL="$owner_email" \
    -e INIT_OWNER_NAME="$owner_name" \
    -e INIT_OWNER_PASSWORD="$owner_password" \
    backend user-admin-backend init-owner; then
    (
      umask 077
      cat > "$credentials_path" <<EOF
EntitleHub owner credentials
URL=${public_url}
TENANT=${APP_NAME}
EMAIL=${owner_email}
PASSWORD=${owner_password}
EOF
    )
    printf '初始管理员凭据已保存：%s\n' "$credentials_path"
  else
    warn "初始管理员未创建。若数据库已存在租户可忽略，否则请检查日志后手动运行 init-owner。"
  fi
}

run_smoke() {
  log "运行冒烟检测"
  in_install_dir
  local backend_port admin_port public_url backend_url
  backend_port="$(get_env_value BACKEND_HOST_PORT)"
  admin_port="$(get_env_value ADMIN_HOST_PORT)"
  public_url="$(awk -F= '$1 == "PUBLIC_URL" { sub(/^[^=]*=/, ""); print; exit }' "$STATE_FILE" 2>/dev/null || true)"
  backend_url="http://127.0.0.1:${backend_port}"

  wait_for_http "$backend_url/health" "后端健康检查" 60 || die "后端健康检查失败。"
  wait_for_http "http://127.0.0.1:${admin_port}/" "后台管理" 30 || warn "后台直连端口检查失败。"

  if [[ -n "$public_url" && "$public_url" == https://* ]]; then
    wait_for_http "$public_url" "公网 HTTPS" 40 || warn "公网 HTTPS 检查失败，请检查 DNS、防火墙和证书状态。"
  fi
}

install_flow() {
  require_root
  preflight

  if is_installed; then
    warn "$APP_NAME 已安装在 $INSTALL_DIR。"
    confirm "是否继续刷新安装文件并保留现有密钥？" || return
  fi

  printf '\n请选择访问方式：\n'
  printf '1) 不使用域名，仅本机访问\n'
  printf '2) 不使用域名，使用服务器 IP 访问\n'
  printf '3) 使用域名，自动申请 HTTPS 证书\n'
  printf '4) 使用域名，使用自有证书\n'
  printf '5) 已有反向代理 / 负载均衡\n'
  printf '请选择：'
  local choice
  read -r choice

  local mode public_url backend_public_url host_bind cookie_secure domain cert key
  case "$choice" in
    1)
      mode="local"
      host_bind="127.0.0.1"
      public_url="http://127.0.0.1:5173"
      backend_public_url="http://127.0.0.1:8080"
      cookie_secure="false"
      ;;
    2)
      mode="ip"
      host_bind="0.0.0.0"
      local detected_ip
      detected_ip="$(detect_public_ip)"
      detected_ip="$(ask "服务器 IP 或主机名" "$detected_ip")"
      public_url="http://${detected_ip}:5173"
      backend_public_url="http://${detected_ip}:8080"
      cookie_secure="false"
      ;;
    3)
      mode="domain-auto"
      domain="$(ask "域名")"
      [[ -n "$domain" ]] || die "必须填写域名。"
      check_port_available 80 || die "80 端口已被占用。"
      check_port_available 443 || die "443 端口已被占用。"
      host_bind="127.0.0.1"
      public_url="https://${domain}"
      backend_public_url="https://${domain}"
      cookie_secure="true"
      ;;
    4)
      mode="domain-custom"
      domain="$(ask "域名")"
      [[ -n "$domain" ]] || die "必须填写域名。"
      cert="$(ask "证书 fullchain.pem 路径")"
      key="$(ask "私钥路径")"
      check_port_available 80 || die "80 端口已被占用。"
      check_port_available 443 || die "443 端口已被占用。"
      host_bind="127.0.0.1"
      public_url="https://${domain}"
      backend_public_url="https://${domain}"
      cookie_secure="true"
      ;;
    5)
      mode="external-proxy"
      domain="$(ask "公网域名或 URL 主机")"
      [[ -n "$domain" ]] || die "必须填写域名。"
      host_bind="127.0.0.1"
      public_url="https://${domain}"
      backend_public_url="https://${domain}"
      cookie_secure="true"
      ;;
    *)
      warn "已取消。"
      return
      ;;
  esac

  log "准备安装目录"
  mkdir -p "$INSTALL_DIR"
  safe_refresh_source

  if [[ ! -f "$INSTALL_DIR/$ENV_FILE" ]]; then
    write_env_file "$mode" "$public_url" "$backend_public_url" "$host_bind" "$cookie_secure"
  else
    warn "已保留现有 $ENV_FILE，未覆盖密钥。"
  fi

  rm -f "$INSTALL_DIR/compose.proxy.yml" "$INSTALL_DIR/Caddyfile"
  case "$mode" in
    domain-auto)
      write_caddy_files "$domain" "auto"
      ;;
    domain-custom)
      write_caddy_files "$domain" "custom" "$cert" "$key"
      ;;
    external-proxy)
      write_external_proxy_example "$domain"
      ;;
  esac

  write_state "$mode" "$public_url" "${domain:-}"
  install_local_command
  prepare_images
  run_migrations
  start_stack
  init_owner
  run_smoke

  printf '\n安装完成。\n'
  printf '后台地址：%s\n' "$public_url"
  printf '安装目录：%s\n' "$INSTALL_DIR"
}

install_local_command() {
  if [[ -w /usr/local/bin || "${EUID:-$(id -u)}" -eq 0 ]]; then
    mkdir -p /usr/local/bin
    cat > /usr/local/bin/entitle-hub <<EOF
#!/usr/bin/env bash
set -Eeuo pipefail
export USER_ADMIN_INSTALL_DIR="${INSTALL_DIR}"
export USER_ADMIN_PROJECT_NAME="${PROJECT_NAME}"
export USER_ADMIN_REPO="${USER_ADMIN_REPO}"
export USER_ADMIN_REF="${USER_ADMIN_REF}"
export USER_ADMIN_RAW_BASE="${USER_ADMIN_RAW_BASE}"
export USER_ADMIN_DEPLOY_MODE="\${USER_ADMIN_DEPLOY_MODE:-${DEPLOY_MODE}}"
bash <(curl -fsSL "\$USER_ADMIN_RAW_BASE/ops/install.sh")
EOF
    chmod +x /usr/local/bin/entitle-hub
    ln -sf /usr/local/bin/entitle-hub /usr/local/bin/user-admin
  fi
}

backup_flow() {
  require_root
  is_installed || die "$APP_NAME 尚未安装。"
  in_install_dir
  mkdir -p "$BACKUP_DIR"
  local ts target postgres_user postgres_db
  ts="$(date -u +%Y%m%dT%H%M%SZ)"
  target="$BACKUP_DIR/user_admin_${ts}.dump"
  postgres_user="$(get_env_value POSTGRES_USER)"
  postgres_db="$(get_env_value POSTGRES_DB)"
  log "备份 PostgreSQL 到 $INSTALL_DIR/$target"
  compose_base exec -T postgres pg_dump --format=custom --no-owner --no-acl -U "$postgres_user" -d "$postgres_db" > "$target"
  sha256sum "$target" > "${target}.sha256" 2>/dev/null || true
  printf '%s\n' "$INSTALL_DIR/$target"
}

restore_flow() {
  require_root
  is_installed || die "$APP_NAME 尚未安装。"
  local file
  file="$(ask "备份文件路径")"
  [[ -f "$file" ]] || die "备份文件不存在：$file"
  confirm "恢复会修改当前数据库，是否先创建安全备份？" && backup_flow
  confirm "是否现在继续恢复？" || return

  in_install_dir
  local postgres_user postgres_db
  postgres_user="$(get_env_value POSTGRES_USER)"
  postgres_db="$(get_env_value POSTGRES_DB)"
  if [[ -f "${file}.sha256" ]]; then
    (cd "$(dirname "$file")" && sha256sum -c "$(basename "${file}.sha256")")
  fi
  compose_base exec -T postgres pg_restore --no-owner --no-acl --clean --if-exists -U "$postgres_user" -d "$postgres_db" < "$file"
}

update_flow() {
  require_root
  preflight
  is_installed || die "$APP_NAME 尚未安装。"

  printf '\n更新选项：\n'
  printf '1) 更新到最新稳定版本\n'
  printf '2) 取消\n'
  printf '请选择：'
  local choice
  read -r choice
  [[ "$choice" == "1" ]] || return

  backup_flow
  log "刷新源码并更新服务镜像"
  safe_refresh_source
  in_install_dir
  prepare_images
  run_migrations
  start_stack
  run_smoke
  printf '\n更新完成。\n'
}

uninstall_flow() {
  require_root
  is_installed || die "$APP_NAME 尚未安装。"

  printf '\n卸载选项：\n'
  printf '1) 安全卸载，保留数据卷和备份\n'
  printf '2) 彻底清除，包括 Docker 数据卷\n'
  printf '3) 取消\n'
  printf '请选择：'
  local choice
  read -r choice

  in_install_dir
  case "$choice" in
    1)
      compose_base down
      rm -f /usr/local/bin/user-admin /usr/local/bin/entitle-hub
      printf '服务已停止，数据卷和 %s 已保留。\n' "$INSTALL_DIR"
      ;;
    2)
      printf '请输入 DELETE USER-ADMIN DATA 确认清除：'
      local phrase
      read -r phrase
      [[ "$phrase" == "DELETE USER-ADMIN DATA" ]] || die "确认短语不匹配。"
      compose_base down -v --rmi local
      rm -f /usr/local/bin/user-admin /usr/local/bin/entitle-hub
      rm -rf "$INSTALL_DIR"
      printf '已彻底清除。\n'
      ;;
    *)
      warn "已取消。"
      ;;
  esac
}

translate_mode() {
  case "$1" in
    local) printf '不使用域名，仅本机访问' ;;
    ip) printf '不使用域名，服务器 IP 访问' ;;
    domain-auto) printf '使用域名，自动申请 HTTPS 证书' ;;
    domain-custom) printf '使用域名，自有证书' ;;
    external-proxy) printf '已有反向代理 / 负载均衡' ;;
    *) printf '%s' "$1" ;;
  esac
}

status_flow() {
  is_installed || die "$APP_NAME 尚未安装。"
  in_install_dir
  printf '\n安装目录：%s\n' "$INSTALL_DIR"
  if [[ -f "$STATE_FILE" ]]; then
    local mode public_url domain installed_at source_ref
    mode="$(get_env_value MODE "$STATE_FILE" || true)"
    public_url="$(get_env_value PUBLIC_URL "$STATE_FILE" || true)"
    domain="$(get_env_value DOMAIN "$STATE_FILE" || true)"
    installed_at="$(get_env_value INSTALLED_AT "$STATE_FILE" || true)"
    source_ref="$(get_env_value SOURCE_REF "$STATE_FILE" || true)"
    [[ -n "$mode" ]] && printf '访问方式：%s\n' "$(translate_mode "$mode")"
    [[ -n "$public_url" ]] && printf '访问地址：%s\n' "$public_url"
    [[ -n "$domain" ]] && printf '域名：%s\n' "$domain"
    [[ -n "$installed_at" ]] && printf '安装时间：%s\n' "$installed_at"
    [[ -n "$source_ref" ]] && printf '源码版本：%s\n' "$source_ref"
  fi
  if [[ -f "$DIGEST_FILE" ]]; then
    printf '镜像锁定：已启用（%s）\n' "$DIGEST_FILE"
  else
    printf '镜像锁定：未启用\n'
  fi
  compose_base ps
}

logs_flow() {
  is_installed || die "$APP_NAME 尚未安装。"
  in_install_dir
  printf '服务名，留空查看全部：'
  local service
  read -r service
  if [[ -n "$service" ]]; then
    compose_base logs --tail=200 -f "$service"
  else
    compose_base logs --tail=200 -f
  fi
}

restart_flow() {
  require_root
  is_installed || die "$APP_NAME 尚未安装。"
  in_install_dir
  compose_base restart
  run_smoke
}

reset_admin_password_flow() {
  require_root
  is_installed || die "$APP_NAME 尚未安装。"
  in_install_dir

  local email tenant_slug password disable_mfa
  email="$(ask "管理员邮箱")"
  tenant_slug="$(ask "租户 slug，同邮箱多租户时填写" "")"
  password="$(ask_secret_optional "新密码，留空则自动生成")"
  if confirm "是否同时关闭该账号 MFA"; then
    disable_mfa="true"
  else
    disable_mfa="false"
  fi

  local args=(run --rm -e RESET_ADMIN_EMAIL="$email" -e RESET_ADMIN_DISABLE_MFA="$disable_mfa")
  if [[ -n "$tenant_slug" ]]; then
    args+=(-e RESET_ADMIN_TENANT_SLUG="$tenant_slug")
  fi
  if [[ -n "$password" ]]; then
    args+=(-e RESET_ADMIN_PASSWORD="$password")
  fi

  compose_base "${args[@]}" backend user-admin-backend reset-admin-password
}

cert_flow() {
  require_root
  is_installed || die "$APP_NAME 尚未安装。"

  printf '\n证书管理：\n'
  printf '1) 查看证书状态\n'
  printf '2) 切换到自动申请证书\n'
  printf '3) 切换到自有证书\n'
  printf '4) 重载代理\n'
  printf '5) 返回\n'
  printf '请选择：'
  local choice
  read -r choice

  local domain cert key
  in_install_dir
  case "$choice" in
    1)
      compose_base logs --tail=100 caddy || true
      ;;
    2)
      domain="$(ask "域名")"
      write_caddy_files "$domain" "auto"
      set_env_value "$ENV_FILE" APP_BASE_URL "https://${domain}"
      set_env_value "$ENV_FILE" ALLOWED_ORIGINS "https://${domain}"
      set_env_value "$ENV_FILE" JWT_ISSUER "https://${domain}"
      set_env_value "$ENV_FILE" COOKIE_SECURE "true"
      write_state "domain-auto" "https://${domain}" "$domain"
      pin_image_digests
      compose_base up -d caddy backend admin
      ;;
    3)
      domain="$(ask "域名")"
      cert="$(ask "证书 fullchain.pem 路径")"
      key="$(ask "私钥路径")"
      write_caddy_files "$domain" "custom" "$cert" "$key"
      set_env_value "$ENV_FILE" APP_BASE_URL "https://${domain}"
      set_env_value "$ENV_FILE" ALLOWED_ORIGINS "https://${domain}"
      set_env_value "$ENV_FILE" JWT_ISSUER "https://${domain}"
      set_env_value "$ENV_FILE" COOKIE_SECURE "true"
      write_state "domain-custom" "https://${domain}" "$domain"
      pin_image_digests
      compose_base up -d caddy backend admin
      ;;
    4)
      compose_base exec -T caddy caddy reload --config /etc/caddy/Caddyfile || compose_base restart caddy
      ;;
    *)
      return
      ;;
  esac
}

doctor_flow() {
  printf '\n环境诊断：\n'
  command -v docker >/dev/null 2>&1 && printf 'docker: 正常\n' || printf 'docker: 缺失\n'
  docker compose version >/dev/null 2>&1 && printf 'docker compose: 正常\n' || printf 'docker compose: 缺失\n'
  command -v curl >/dev/null 2>&1 && printf 'curl: 正常\n' || printf 'curl: 缺失\n'
  command -v openssl >/dev/null 2>&1 && printf 'openssl: 正常\n' || printf 'openssl: 缺失\n'
  df -h "$INSTALL_DIR" 2>/dev/null || df -h /
  if is_installed; then
    status_flow || true
    run_smoke || true
  fi
}

print_header() {
  clear 2>/dev/null || true
  printf '========================================\n'
  printf ' %s 一键管理器\n' "$APP_NAME"
  printf '========================================\n'
  if is_installed; then
    printf '状态：已安装\n'
    printf '安装目录：%s\n' "$INSTALL_DIR"
    if [[ -f "$INSTALL_DIR/$STATE_FILE" ]]; then
      awk -F= '$1 == "PUBLIC_URL" { print "访问地址：" $2 }' "$INSTALL_DIR/$STATE_FILE"
      awk -F= '$1 == "SOURCE_REF" { print "源码版本：" $2 }' "$INSTALL_DIR/$STATE_FILE"
    fi
  else
    printf '状态：未安装\n'
    printf '安装目录：%s\n' "$INSTALL_DIR"
  fi
  printf '\n'
}

main_menu() {
  while true; do
    print_header
    if is_installed; then
      cat <<'EOF'
1) 更新到最新版
2) 查看状态
3) 查看日志
4) 备份数据
5) 恢复备份
6) 证书管理
7) 重置管理员密码
8) 运行诊断
9) 重启服务
10) 卸载
11) 退出
EOF
      printf '请选择：'
      local choice
      read -r choice
      case "$choice" in
        1) update_flow; pause ;;
        2) status_flow; pause ;;
        3) logs_flow ;;
        4) backup_flow; pause ;;
        5) restore_flow; pause ;;
        6) cert_flow; pause ;;
        7) reset_admin_password_flow; pause ;;
        8) doctor_flow; pause ;;
        9) restart_flow; pause ;;
        10) uninstall_flow; pause ;;
        11) exit 0 ;;
        *) warn "无效选择。"; pause ;;
      esac
    else
      cat <<'EOF'
1) 安装
2) 从备份恢复
3) 运行诊断
4) 退出
EOF
      printf '请选择：'
      local choice
      read -r choice
      case "$choice" in
        1) install_flow; pause ;;
        2) install_flow; restore_flow; pause ;;
        3) doctor_flow; pause ;;
        4) exit 0 ;;
        *) warn "无效选择。"; pause ;;
      esac
    fi
  done
}

main_menu
