# Base pineada por digest (python:3.11-slim, 3.11.17) para builds reproducibles.
FROM python:3.11-slim@sha256:e88e9763f943ec1834f992a4b51e0f24500486803e8bc534e5767af9ea65f6ce
# CPU por defecto (ligero). Para GPU NVIDIA, build con --build-arg TORCH_INDEX=.../cu126
# (torch 2.14.1 no se publica para cu124; cu126 funciona con drivers >= 525).
ARG TORCH_INDEX=https://download.pytorch.org/whl/cpu
RUN pip install --no-cache-dir torch==2.14.1 --index-url ${TORCH_INDEX}
# Versiones de la imagen validada en el E2E real de `ai finetune` (2026-10-19).
# `gguf==0.16.2`: primera versión publicada con la API que usa el conversor
# embebido `convert_hf_to_gguf.py` (llama.cpp b4879, Fase 14.5). La 0.16.0 no
# tiene `MODEL_ARCH.GEMMA3` y el conversor falla al importarse (H-18).
# `sentencepiece==0.2.0`: gguf 0.16.2 exige <=0.2.0 (la imagen antigua llevaba
# 0.2.2 con gguf 0.19.0; ese par no es instalable junto a gguf 0.16.x).
RUN pip install --no-cache-dir transformers==4.57.6 peft==0.21.2 datasets==5.0.1 gguf==0.16.2 accelerate==1.15.0 sentencepiece==0.2.0
# Lock completo (también dependencias transitivas): fija la versión final de
# cada paquete aunque las capas anteriores resolvieran otra. Va en una capa
# aparte para no invalidar la caché de torch. Los paquetes CUDA solo en GPU,
# desde el mismo índice de torch. `pip check` falla si el entorno es incoherente.
COPY requirements.lock requirements-cu126.lock /tmp/
RUN pip install --no-cache-dir --no-deps -r /tmp/requirements.lock \
    && case "${TORCH_INDEX}" in \
         *cu126*) pip install --no-cache-dir --no-deps -r /tmp/requirements-cu126.lock --index-url "${TORCH_INDEX}" ;; \
       esac \
    && pip check
# A25-12: sin privilegios por defecto (enola además fija `User: 65532:65532` al
# lanzar el contenedor, así que una imagen antigua tampoco corre como root).
USER 65532:65532
