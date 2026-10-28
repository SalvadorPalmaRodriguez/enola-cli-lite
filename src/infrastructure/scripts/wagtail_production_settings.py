"""Ajustes de producción de enola/wagtail (A25-N3).

Se instalan como `website/settings/production.py` del proyecto vacío generado
por `wagtail start`. Los secretos se leen de `/run/secrets/*`; el resto de la
configuración llega por variables de entorno no sensibles.
"""
import os
from pathlib import Path

from .base import *  # noqa: F401,F403


def _secret(name):
    try:
        return Path("/run/secrets", name).read_text().strip()
    except OSError:
        return None


def _csv(name, default=""):
    return [v.strip() for v in os.environ.get(name, default).split(",") if v.strip()]


DEBUG = False

SECRET_KEY = _secret("django_secret_key")
if not SECRET_KEY:
    if os.environ.get("WAGTAIL_IMAGE_BUILD") == "1":
        # Solo durante `collectstatic` en el build; no se usa en ejecución.
        SECRET_KEY = "build-only-not-a-secret"
    else:
        raise RuntimeError("falta el secreto /run/secrets/django_secret_key")

# El contenedor solo es alcanzable en 127.0.0.1 (Docker) y por el proxy de
# Nginx/Tor, cuyo nombre .onion no se conoce al crear la instancia.
ALLOWED_HOSTS = _csv("WAGTAIL_ALLOWED_HOSTS", "*")
CSRF_TRUSTED_ORIGINS = _csv("WAGTAIL_CSRF_TRUSTED_ORIGINS")
SECURE_PROXY_SSL_HEADER = ("HTTP_X_FORWARDED_PROTO", "https")

DATABASES = {
    "default": {
        "ENGINE": "django.db.backends.postgresql",
        "NAME": os.environ.get("WAGTAIL_DB_NAME", "wagtail"),
        "USER": os.environ.get("WAGTAIL_DB_USER", "wagtail"),
        "PASSWORD": _secret("db_password") or "",
        "HOST": os.environ.get("WAGTAIL_DB_HOST", "db"),
        "PORT": os.environ.get("WAGTAIL_DB_PORT", "5432"),
    }
}

MIDDLEWARE.insert(1, "whitenoise.middleware.WhiteNoiseMiddleware")  # noqa: F405
STORAGES = {
    "default": {"BACKEND": "django.core.files.storage.FileSystemStorage"},
    "staticfiles": {
        "BACKEND": "whitenoise.storage.CompressedManifestStaticFilesStorage"
    },
}

ROOT_URLCONF = "website.enola_urls"
WAGTAILADMIN_BASE_URL = os.environ.get("WAGTAIL_BASE_URL", "http://localhost")

SESSION_COOKIE_HTTPONLY = True
CSRF_COOKIE_HTTPONLY = True
SECURE_CONTENT_TYPE_NOSNIFF = True
