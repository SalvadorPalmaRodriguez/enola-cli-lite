"""URLs de enola/wagtail: las de `wagtail start` + servir `media/` (A25-N3).

En producción Django no sirve los ficheros subidos. Para un sitio de bajo
tráfico detrás de Nginx/Tor basta `django.views.static.serve`; la ruta va ANTES
de la ruta comodín de Wagtail, que si no respondería 404 a /media/...
"""
from django.conf import settings
from django.urls import re_path
from django.views.static import serve

from website.urls import urlpatterns as _base_urlpatterns

urlpatterns = [
    re_path(
        r"^media/(?P<path>.*)$",
        serve,
        {"document_root": settings.MEDIA_ROOT},
    ),
    *_base_urlpatterns,
]
