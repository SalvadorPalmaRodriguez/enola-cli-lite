#!/bin/sh
# Entrypoint de enola/wagtail (A25-N3). Los secretos se leen de /run/secrets/*
# (ficheros montados en solo lectura); nunca pasan por variables de entorno ni
# por argv, así que no aparecen en `docker inspect` ni en `ps`.
set -eu
cd /srv/app

# Postgres puede tardar unos segundos en aceptar conexiones.
tries=0
until python manage.py migrate --noinput; do
    tries=$((tries + 1))
    if [ "$tries" -ge 40 ]; then
        echo "enola-wagtail: la base de datos no responde tras 40 intentos" >&2
        exit 1
    fi
    sleep 3
done

# Superusuario inicial (idempotente): la contraseña sale del secreto, no del entorno.
python manage.py shell <<'PY'
from django.contrib.auth import get_user_model

User = get_user_model()
if not User.objects.filter(is_superuser=True).exists():
    with open("/run/secrets/admin_password") as fh:
        password = fh.read().strip()
    User.objects.create_superuser("admin", "admin@localhost", password)
PY

exec gunicorn website.wsgi:application \
    --bind 0.0.0.0:8000 \
    --workers 2 \
    --access-logfile - \
    --error-logfile -
