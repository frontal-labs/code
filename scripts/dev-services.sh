#!/usr/bin/env bash
# Starts the live services the test suites use, and prints the variables that point at them.
#
#   Postgres 17 over verified TLS on 127.0.0.1:55433 (worker tokens, TLS tests)
#   ClickHouse 26.3 on 127.0.0.1:8123 (audit and migration tests)
#
# Certificates are generated once into TLS_DIR (default ~/.frontal-code-dev/tls) with a private
# CA, so a test run never depends on files in a scratch directory. Re-running is safe: existing
# containers are started, and existing certificates are kept.
#
#   eval "$(scripts/dev-services.sh env)"       # start services and export the variables
#   TLS_SAN_EXTRA=IP:172.17.0.1 scripts/dev-services.sh up   # add a name to the certificate (new certs only)
#   scripts/dev-services.sh up                  # start services only
#   scripts/dev-services.sh down                # stop and remove the containers (data volumes kept)
set -euo pipefail

TLS_DIR="${TLS_DIR:-$HOME/.frontal-code-dev/tls}"
PG_NAME="${PG_NAME:-frontal-dev-pg}"
CH_NAME="${CH_NAME:-frontal-dev-clickhouse}"
PG_IMAGE="postgres:17"
CH_IMAGE="clickhouse/clickhouse-server:26.3.62.3"
PG_PASSWORD="${PG_PASSWORD:-dev-local-only}"
CH_PASSWORD="${CH_PASSWORD:-dev-local-only}"
PG_PORT=55433
CH_PORT=8123

command="${1:-up}"

need_docker() {
  docker info >/dev/null 2>&1 || { echo "error: Docker daemon is not reachable" >&2; exit 1; }
}

make_certs() {
  mkdir -p "$TLS_DIR"
  chmod 700 "$TLS_DIR"
  if [ ! -f "$TLS_DIR/ca.crt" ]; then
    openssl req -x509 -newkey rsa:2048 -nodes -keyout "$TLS_DIR/ca.key" -out "$TLS_DIR/ca.crt" \
      -days 3650 -subj "/CN=Frontal dev test CA" \
      -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
  fi
  if [ ! -f "$TLS_DIR/server.crt" ]; then
    openssl req -newkey rsa:2048 -nodes -keyout "$TLS_DIR/server.key" -out "$TLS_DIR/server.csr" \
      -subj "/CN=localhost" 2>/dev/null
    # TLS_SAN_EXTRA adds names for other hosts, for example IP:172.17.0.1 for the Docker bridge on a
    # native Linux host. Delete server.crt and run again to regenerate with a new value.
    sans="DNS:localhost,DNS:host.docker.internal,IP:127.0.0.1${TLS_SAN_EXTRA:+,$TLS_SAN_EXTRA}"
    printf "subjectAltName=%s\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n" "$sans" > "$TLS_DIR/leaf.ext"
    openssl x509 -req -in "$TLS_DIR/server.csr" -CA "$TLS_DIR/ca.crt" -CAkey "$TLS_DIR/ca.key" \
      -CAcreateserial -out "$TLS_DIR/server.crt" -days 825 -extfile "$TLS_DIR/leaf.ext" 2>/dev/null
    rm -f "$TLS_DIR/server.csr" "$TLS_DIR/leaf.ext"
  fi
  chmod 600 "$TLS_DIR/server.key" "$TLS_DIR/ca.key"
}

start_postgres() {
  if docker inspect "$PG_NAME" >/dev/null 2>&1; then
    docker start "$PG_NAME" >/dev/null
  else
    docker run -d --name "$PG_NAME" -p "127.0.0.1:$PG_PORT:5432" \
      -e POSTGRES_USER=frontal -e POSTGRES_PASSWORD="$PG_PASSWORD" -e POSTGRES_DB=frontal \
      "$PG_IMAGE" >/dev/null
    wait_for_postgres
    # Postgres refuses a key that other users can read, so the files are copied in and owned by postgres.
    docker exec "$PG_NAME" mkdir -p /etc/postgresql/tls
    for file in server.crt server.key ca.crt; do docker cp "$TLS_DIR/$file" "$PG_NAME:/etc/postgresql/tls/$file"; done
    docker exec "$PG_NAME" sh -c 'chown -R postgres:postgres /etc/postgresql/tls && chmod 600 /etc/postgresql/tls/server.key'
    # ALTER SYSTEM is safe once the server is up, and the setting survives the restart below.
    # One -c per statement: psql runs a multi-statement string in a transaction, which ALTER SYSTEM refuses.
    docker exec -u postgres "$PG_NAME" psql -U frontal -d frontal -q \
      -c "ALTER SYSTEM SET ssl = on" \
      -c "ALTER SYSTEM SET ssl_cert_file = '/etc/postgresql/tls/server.crt'" \
      -c "ALTER SYSTEM SET ssl_key_file = '/etc/postgresql/tls/server.key'" \
      -c "ALTER SYSTEM SET ssl_ca_file = '/etc/postgresql/tls/ca.crt'"
    docker restart "$PG_NAME" >/dev/null
  fi
  wait_for_postgres
  docker exec "$PG_NAME" sh -c "psql -U frontal -d frontal -tAc 'show ssl'" | grep -q on \
    || { echo "error: Postgres is not serving TLS" >&2; exit 1; }
}

wait_for_postgres() {
  for _ in $(seq 1 60); do
    docker exec "$PG_NAME" pg_isready -U frontal >/dev/null 2>&1 && return 0
    sleep 1
  done
  echo "error: Postgres did not start" >&2
  exit 1
}

start_clickhouse() {
  if docker inspect "$CH_NAME" >/dev/null 2>&1; then
    docker start "$CH_NAME" >/dev/null
  else
    docker run -d --name "$CH_NAME" -p "127.0.0.1:$CH_PORT:8123" \
      -e CLICKHOUSE_USER=frontal -e CLICKHOUSE_PASSWORD="$CH_PASSWORD" \
      -e CLICKHOUSE_DEFAULT_ACCESS_MANAGEMENT=1 "$CH_IMAGE" >/dev/null
  fi
  for _ in $(seq 1 60); do
    curl -s -m 2 -u "frontal:$CH_PASSWORD" "http://127.0.0.1:$CH_PORT/ping" 2>/dev/null | grep -q Ok && break
    sleep 1
  done
  curl -s -m 2 -u "frontal:$CH_PASSWORD" "http://127.0.0.1:$CH_PORT/ping" 2>/dev/null | grep -q Ok \
    || { echo "error: ClickHouse did not start" >&2; exit 1; }
}

print_env() {
  cat <<EOF
export FRONTAL_TEST_POSTGRES_URL='postgres://frontal:$PG_PASSWORD@127.0.0.1:$PG_PORT/frontal?sslmode=verify-full'
export FRONTAL_TEST_POSTGRES_CA_FILE='$TLS_DIR/ca.crt'
export FRONTAL_TEST_CLICKHOUSE_URL='http://127.0.0.1:$CH_PORT'
export FRONTAL_TEST_CLICKHOUSE_USER=frontal
export FRONTAL_TEST_CLICKHOUSE_PASSWORD='$CH_PASSWORD'
EOF
}

case "$command" in
  up)
    need_docker; make_certs; start_postgres; start_clickhouse
    echo "dev services are up: Postgres (TLS) on $PG_PORT, ClickHouse on $CH_PORT"
    ;;
  env)
    need_docker; make_certs; start_postgres; start_clickhouse
    print_env
    ;;
  down)
    need_docker
    docker rm -f "$PG_NAME" "$CH_NAME" >/dev/null 2>&1 || true
    echo "stopped $PG_NAME and $CH_NAME (data volumes kept)" >&2
    ;;
  *)
    echo "usage: $0 [up|env|down]" >&2
    exit 2
    ;;
esac
