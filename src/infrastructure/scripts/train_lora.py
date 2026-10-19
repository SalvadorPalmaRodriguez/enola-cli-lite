#!/usr/bin/env python3
# enola-cli Fase 14 — runner de entrenamiento LoRA embebido (include_str!).
#
# Se despliega por el CLI en <instance>/finetune/scripts/ y se ejecuta
# DENTRO del contenedor de entrenamiento sobre `ContainerPort`. Entrada:
#   python3 /opt/finetune/train_lora.py --train /data/train.jsonl \
#       --val /data/val.jsonl --out /out [--base-model ID] [--low-memory]
#
# Requiere `transformers`, `peft`, `datasets`, `torch` en la imagen del
# contenedor. Si falta una dependencia, falla con mensaje accionable.

import argparse
import json
import os
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
    ap.add_argument("--base-model", default="llama3.1:8b")
    ap.add_argument("--low-memory", action="store_true")
    ap.add_argument("--epochs", type=int, default=1)
    ap.add_argument("--max-len", type=int, default=2048)
    args = ap.parse_args()

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
        f"base={args.base_model} low_memory={args.low_memory}",
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

    tokenizer = AutoTokenizer.from_pretrained(args.base_model)

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
    model = AutoModelForCausalLM.from_pretrained(args.base_model, dtype=dtype)
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
    return 0


if __name__ == "__main__":
    sys.exit(main())
