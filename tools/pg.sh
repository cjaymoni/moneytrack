#!/usr/bin/env bash
# A Postgres for development and for the server's tests, in a container on port 55432.
#
# Port 55432 rather than 5432 so it cannot collide with a Postgres someone already runs,
# and so a connection string pointing at the wrong one fails loudly instead of writing test
# rows into a real database.
set -euo pipefail

name=moneytrack-pg
port=55432
image=postgres:18-alpine

case "${1:-up}" in
    up)
        if docker ps -a --format '{{.Names}}' | grep -qx "$name"; then
            docker start "$name" > /dev/null
        else
            docker run -d --name "$name" \
                -e POSTGRES_USER=moneytrack \
                -e POSTGRES_PASSWORD=moneytrack \
                -e POSTGRES_DB=moneytrack \
                -p "$port:5432" "$image" > /dev/null
        fi
        for _ in $(seq 1 30); do
            docker exec "$name" pg_isready -U moneytrack > /dev/null 2>&1 && break
            sleep 1
        done
        echo "postgres://moneytrack:moneytrack@127.0.0.1:$port/moneytrack"
        ;;
    down)
        docker stop "$name" > /dev/null && echo "stopped"
        ;;
    reset)
        docker rm -f "$name" > /dev/null 2>&1 || true
        exec "$0" up
        ;;
    *)
        echo "usage: $0 [up|down|reset]" >&2
        exit 2
        ;;
esac
