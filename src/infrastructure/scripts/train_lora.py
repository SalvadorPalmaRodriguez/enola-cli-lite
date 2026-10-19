#!/usr/bin/env python3
# enola-cli Fase 14 — runner de entrenamiento LoRA embebido (include_str!).
#
# Se despliega por el CLI en <instance>/finetune/scripts/ y se ejecuta
# DENTRO del contenedor de entrenamiento sobre `ContainerPort`. Entrada:
#   python3 /opt/finetune/train_lora.py --train /data/train.jsonl \
#       --val /data/val.jsonl --out /out \
#       --gguf /model.gguf --tokenizer /tokenizer [--low-memory]
#
# El contenedor corre SIN red: el GGUF y el tokenizer se montan como
# volúmenes desde el host. El modelo se carga nativo desde el GGUF
# (`gguf_file`) y el tokenizer desde el directorio montado.
#
# Requiere `transformers` (>= 4.46, con soporte `gguf_file`), `peft`,
# `datasets`, `torch` en la imagen del contenedor. Si falta una dependencia,
# falla con mensaje accionable.

import argparse
import json
import os
import subprocess
import sys


def fail(msg: str):
    print(f"[train_lora] ERROR: {msg}", file=sys.stderr)
    sys.exit(1)


def load_jsonl(path: str):
    rows = []
    with open(path, "r", encoding="utf-8") as fh:
        for n, line in enumerate(fh, 1):
            line = line.strip()
            if not line:
                continue
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError as e:
                fail(f"{path}:{n} JSON inválido: {e}")
    return rows


def to_text(rec):
    """Serializa un registro ChatML a texto de entrenamiento."""
    parts = []
    for m in rec.get("messages", []):
        role = m.get("role", "user")
        content = m.get("content", "")
        parts.append("### " + role + "\n" + content + "\n")
    return "".join(parts)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--train", required=True)
    ap.add_argument("--val", default=None)
    ap.add_argument("--out", required=True)
    ap.add_argument("--gguf", required=True, help="Ruta del GGUF base montado")
    ap.add_argument("--tokenizer", required=True, help="Directorio del tokenizer HF montado")
    ap.add_argument("--low-memory", action="store_true")
    ap.add_argument("--epochs", type=int, default=1)
    ap.add_argument("--max-len", type=int, default=2048)
    args = ap.parse_args()

    if not os.path.isfile(args.gguf):
        fail(f"GGUF base no encontrado en '{args.gguf}'")
    if not os.path.isdir(args.tokenizer) or not os.path.isfile(
        os.path.join(args.tokenizer, "tokenizer_config.json")
    ):
        fail(f"tokenizer HF no encontrado en '{args.tokenizer}' (falta tokenizer_config.json)")

    os.makedirs(args.out, exist_ok=True)
    # El rootfs es read-only: TMPDIR apunta a un subdirectorio del volumen /out.
    tmpdir = os.environ.get("TMPDIR")
    if tmpdir:
        os.makedirs(tmpdir, exist_ok=True)
    train_rows = load_jsonl(args.train)
    if not train_rows:
        fail("dataset de entrenamiento vacío")
    val_rows = load_jsonl(args.val) if args.val and os.path.exists(args.val) else []
    print(
        f"[train_lora] train={len(train_rows)} val={len(val_rows)} "
        f"gguf={args.gguf} tokenizer={args.tokenizer} low_memory={args.low_memory}",
        flush=True,
    )

    try:
        import torch
        from datasets import Dataset
        from peft import LoraConfig, get_peft_model
        from transformers import (
            AutoModelForCausalLM,
            AutoTokenizer,
            Trainer,
            TrainingArguments,
        )
    except ImportError as e:
        fail(
            f"dependencia ausente en la imagen ({e}) — usa una imagen con "
            f"transformers+peft+datasets+torch"
        )

    tokenizer = AutoTokenizer.from_pretrained(args.tokenizer)

    def row_to_example(rec):
        enc = tokenizer(to_text(rec), truncation=True, max_length=args.max_len)
        return {
            "input_ids": enc["input_ids"],
            "attention_mask": enc["attention_mask"],
            # Sin labels el Trainer no puede calcular la loss (causal LM).
            "labels": enc["input_ids"],
        }

    train_ds = Dataset.from_list([row_to_example(r) for r in train_rows])
    eval_ds = (
        Dataset.from_list([row_to_example(r) for r in val_rows]) if val_rows else None
    )

    dtype = torch.bfloat16 if torch.cuda.is_available() else torch.float32
    # El GGUF trae su propia arquitectura/config en el header: transformers lo
    # descuantiza a fp32 y reconstruye el modelo sin red (offline).
    # `pretrained_model_name_or_path` debe ser el directorio que contiene el
    # GGUF: una ruta a fichero se valida como repo_id y lanza HFValidationError.
    model = AutoModelForCausalLM.from_pretrained(
        os.path.dirname(args.gguf) or ".",
        gguf_file=args.gguf,
        dtype=dtype,
    )
    lora = LoraConfig(
        r=8 if args.low_memory else 16,
        lora_alpha=16,
        lora_dropout=0.05,
        target_modules=["q_proj", "k_proj", "v_proj", "o_proj"],
        task_type="CAUSAL_LM",
    )
    model = get_peft_model(model, lora)

    targs = TrainingArguments(
        output_dir=args.out,
        per_device_train_batch_size=1 if args.low_memory else 4,
        gradient_accumulation_steps=8 if args.low_memory else 2,
        num_train_epochs=args.epochs,
        learning_rate=2e-4,
        logging_steps=10,
        save_strategy="epoch",
        fp16=False,
        bf16=torch.cuda.is_available(),
        report_to=[],
    )
    trainer = Trainer(model=model, args=targs, train_dataset=train_ds, eval_dataset=eval_ds)
    trainer.train()
    adapter_out = os.path.join(args.out, "adapter")
    model.save_pretrained(adapter_out)
    tokenizer.save_pretrained(adapter_out)
    print(f"[train_lora] adaptador guardado en {adapter_out}", flush=True)

    # Fase 14.5 — export del modelo resultante a GGUF. Solo si el CLI lo pide
    # (EXPORT_FILE = nombre del .gguf a escribir en /out; vacío → sin export).
    export_file = os.environ.get("EXPORT_FILE", "").strip()
    if export_file:
        if os.path.basename(export_file) != export_file or not export_file.lower().endswith(".gguf"):
            fail(f"EXPORT_FILE inválido '{export_file}' (debe ser un nombre .gguf sin directorios)")
        # Merge del adaptador LoRA en los pesos base (calidad f16 final:
        # merga en alta precisión y solo se cuantiza al convertir).
        merged = model.merge_and_unload()
        merged_dtype = torch.bfloat16 if torch.cuda.is_available() else torch.float32
        merged = merged.to(dtype=merged_dtype)
        merged_dir = os.path.join(args.out, "merged")
        merged.save_pretrained(merged_dir)
        tokenizer.save_pretrained(merged_dir)
        print(f"[train_lora] modelo mergeado guardado en {merged_dir}", flush=True)

        convert_script = os.path.join(
            os.path.dirname(os.path.abspath(__file__)), "convert_hf_to_gguf.py"
        )
        if not os.path.isfile(convert_script):
            fail(f"conversor no encontrado en '{convert_script}'")
        cmd = [
            sys.executable,
            convert_script,
            merged_dir,
            "--outfile",
            os.path.join(args.out, export_file),
            "--outtype",
            "f16",
        ]
        if args.low_memory:
            # Spool a temp para no duplicar pico de RAM en máquinas ajustadas.
            cmd.append("--use-temp-file")
        env = dict(os.environ)
        env["NO_LOCAL_GGUF"] = "1"
        print(f"[train_lora] convirtiendo a GGUF: {' '.join(cmd)}", flush=True)
        rc = subprocess.run(cmd, env=env)
        if rc.returncode != 0:
            fail(f"conversión HF→GGUF falló (exit {rc.returncode})")
        print(f"[train_lora] GGUF exportado en {os.path.join(args.out, export_file)}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())