FROM python:3.11-slim
# CPU por defecto (ligero). Para GPU NVIDIA, build con --build-arg TORCH_INDEX=.../cu124
ARG TORCH_INDEX=https://download.pytorch.org/whl/cpu
RUN pip install --no-cache-dir torch --index-url ${TORCH_INDEX}
# `gguf` pineado a la versión del gguf-py de llama.cpp b4879: el conversor
# embebido `convert_hf_to_gguf.py` (Fase 14.5) depende de su API.
RUN pip install --no-cache-dir "transformers>=4.46,<5" peft datasets gguf==0.16.0 accelerate sentencepiece
