#!/usr/bin/env bash
# demo/start_postgres.sh — 기능 시험 항목 (14) PostgreSQL 커넥터용 로컬 DB 기동.
#
# root(sudo)·Docker 없이 동작하도록 공식 배포 패키지를 사용자 디렉터리에 풀어
# initdb + pg_ctl 로 띄운다. 접속 정보는 examples/duckdb/postgres_demo.xzz 의
# `postgres://postgres@localhost:5432/mydb` 와 일치한다(trust 인증, 비밀번호 없음).
#
#   bash demo/start_postgres.sh start    # 기동 (+ mydb 생성)
#   bash demo/start_postgres.sh stop     # 종료
#   bash demo/start_postgres.sh status   # 상태
#
# 환경변수: XAZZ_PG_DIR(기본 ~/.xazz-pg), XAZZ_PG_PORT(기본 5432)
set -u

DIR="${XAZZ_PG_DIR:-$HOME/.xazz-pg}"
PORT="${XAZZ_PG_PORT:-5432}"
ROOT="$DIR/root"
PGBIN="$ROOT/usr/lib/postgresql/18/bin"
PGDATA="$DIR/data"
LIBDIR="$ROOT/usr/lib/x86_64-linux-gnu"
DEBDIR="$DIR/deb"
export LD_LIBRARY_PATH="$LIBDIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

PKGS=(postgresql-18 postgresql-client-18 postgresql-common libpq5 libicu78 libnuma1 liburing2)

log() { printf '[xazz-pg] %s\n' "$1"; }
die() { printf '[xazz-pg] ERROR: %s\n' "$1" >&2; exit 1; }

port_open() { ss -ltn 2>/dev/null | grep -q ":$PORT "; }

ensure_binaries() {
    [ -x "$PGBIN/initdb" ] && [ -x "$PGBIN/pg_ctl" ] && return 0
    command -v apt-get >/dev/null 2>&1 || die "apt-get 이 없어 패키지를 받을 수 없습니다."
    log "PostgreSQL 패키지를 내려받아 $ROOT 에 추출합니다 (최초 1회, 약 20MB)."
    mkdir -p "$DEBDIR" "$ROOT"
    ( cd "$DEBDIR" && apt-get download "${PKGS[@]}" ) || die "패키지 다운로드 실패(네트워크 확인)."
    for d in "$DEBDIR"/*.deb; do dpkg-deb -x "$d" "$ROOT" || die "추출 실패: $d"; done
    [ -x "$PGBIN/initdb" ] || die "initdb 를 찾을 수 없습니다."
}

ensure_data() {
    [ -f "$PGDATA/PG_VERSION" ] && return 0
    log "데이터 디렉터리를 초기화합니다: $PGDATA"
    mkdir -p "$DIR"
    "$PGBIN/initdb" -D "$PGDATA" -U postgres --auth=trust -E UTF8 >/dev/null \
        || die "initdb 실패 (로그: $DIR/log.txt)"
}

start() {
    if port_open; then
        log "포트 $PORT 가 이미 열려 있습니다 — 기존 서버를 사용합니다."
        return 0
    fi
    ensure_binaries
    ensure_data
    log "기동: 127.0.0.1:$PORT (socket=$DIR)"
    "$PGBIN/pg_ctl" -D "$PGDATA" -l "$DIR/log.txt" \
        -o "-p $PORT -k $DIR -c listen_addresses=127.0.0.1" start >/dev/null \
        || die "기동 실패 (로그: $DIR/log.txt)"
    for _ in $(seq 1 30); do port_open && break; sleep 0.3; done
    port_open || die "포트 $PORT 미개방 (로그: $DIR/log.txt)"
    "$PGBIN/createdb" -h 127.0.0.1 -p "$PORT" -U postgres mydb 2>/dev/null || true
    log "준비 완료. 검증: ./target/release/xazz run examples/duckdb/postgres_demo.xzz"
}

stop() {
    [ -f "$PGDATA/PG_VERSION" ] || { log "초기화된 데이터가 없습니다."; return 0; }
    ensure_binaries
    "$PGBIN/pg_ctl" -D "$PGDATA" stop >/dev/null 2>&1 && log "종료됨." || log "이미 종료됨."
}

status() {
    if port_open; then log "실행 중: 127.0.0.1:$PORT"; else log "중지됨."; fi
}

case "${1:-start}" in
    start)  start ;;
    stop)   stop ;;
    status) status ;;
    *) die "사용법: $0 {start|stop|status}" ;;
esac
