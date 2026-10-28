#!/bin/bash
# Entrypoint de enola/magnolia (A25-N3). Los secretos se leen de /run/secrets/*;
# nunca pasan por variables de entorno ni por argv (no salen en `docker inspect`
# ni en `ps`). La contraseña de la base de datos solo llega al fichero de
# configuración de Jackrabbit (0600, volumen /data del propio contenedor).
set -eu

: "${MAGNOLIA_DB_HOST:?falta MAGNOLIA_DB_HOST}"
DB_NAME="${MAGNOLIA_DB_NAME:-magnolia}"
DB_USER="${MAGNOLIA_DB_USER:-magnolia}"
DB_PORT="${MAGNOLIA_DB_PORT:-5432}"

umask 077
mkdir -p /data/conf /data/magnolia/keys /data/tmp

# Escapa un valor para un atributo XML y para el reemplazo de sed (delimitador |).
xml_sed() {
    printf '%s' "$1" | sed \
        -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' -e 's/"/\&quot;/g' \
        -e 's/[\\|&]/\\&/g'
}

DB_PASSWORD="$(cat /run/secrets/db_password)"
sed \
    -e "s|@DB_URL@|$(xml_sed "jdbc:postgresql://${MAGNOLIA_DB_HOST}:${DB_PORT}/${DB_NAME}")|" \
    -e "s|@DB_USER@|$(xml_sed "$DB_USER")|" \
    -e "s|@DB_PASSWORD@|$(xml_sed "$DB_PASSWORD")|" \
    /opt/enola/jackrabbit-postgres.xml.tmpl > /data/conf/jackrabbit-postgres.xml
unset DB_PASSWORD

# Postgres puede tardar unos segundos en aceptar conexiones: Jackrabbit falla al
# arrancar si no responde, así que se espera (máx. ~2 min) antes de lanzar Tomcat.
tries=0
until (exec 3<>"/dev/tcp/${MAGNOLIA_DB_HOST}/${DB_PORT}") 2>/dev/null; do
    tries=$((tries + 1))
    if [ "$tries" -ge 40 ]; then
        echo "enola-magnolia: la base de datos no responde tras 40 intentos" >&2
        exit 1
    fi
    sleep 3
done

export CATALINA_TMPDIR=/data/tmp
exec catalina.sh run
