//! Fase 14.3 — Scripts de fine-tuning embebidos (`include_str!`).
//!
//! Se despliegan a `<base>/finetune/scripts/` (dentro de `/srv/enola-ai`,
//! root-owned — NO rutas predecibles en `/tmp`, regla SEC-001/N-01). El
//! script corre DENTRO del contenedor de entrenamiento (`ContainerPort`),
//! nunca en el host.

use crate::domain::error::{EnolaError, Result};
use std::path::{Path, PathBuf};

/// Script de entrenamiento LoRA embebido en el binario.
pub(crate) const TRAIN_LORA_PY: &str = include_str!("scripts/train_lora.py");
/// Conversor HF→GGUF (llama.cpp b4879, MIT) embebido — Fase 14.5.
pub(crate) const CONVERT_HF_TO_GGUF_PY: &str = include_str!("scripts/convert_hf_to_gguf.py");

/// Despliega los scripts de fine-tuning en `dir` (root-owned bajo
/// `/srv/enola-ai/<instance>/finetune/scripts/`) con permisos `0644`: el
/// contenedor de entrenamiento corre sin privilegios (A25-12) y solo necesita
/// leerlos (`python3 <script>`); siguen siendo de root (no modificables).
/// Devuelve la ruta al script principal dentro del host.
pub(crate) fn ensure_finetune_scripts(dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).map_err(|e| {
        EnolaError::InfrastructureError(format!(
            "No se pudo crear el directorio de scripts finetune '{}': {e}",
            dir.display()
        ))
    })?;
    let target = dir.join("train_lora.py");
    std::fs::write(&target, TRAIN_LORA_PY).map_err(|e| {
        EnolaError::InfrastructureError(format!("No se pudo escribir '{}': {e}", target.display()))
    })?;
    let convert = dir.join("convert_hf_to_gguf.py");
    std::fs::write(&convert, CONVERT_HF_TO_GGUF_PY).map_err(|e| {
        EnolaError::InfrastructureError(format!("No se pudo escribir '{}': {e}", convert.display()))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644));
        let _ = std::fs::set_permissions(&convert, std::fs::Permissions::from_mode(0o644));
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_script_is_present_and_valid_python() {
        assert!(TRAIN_LORA_PY.contains("def main()"));
        assert!(TRAIN_LORA_PY.contains("argparse"));
        assert!(TRAIN_LORA_PY.contains("LoraConfig"));
        // S5: carga el modelo desde el GGUF montado y el tokenizer del dir.
        assert!(TRAIN_LORA_PY.contains("gguf_file=args.gguf"));
        // E2E ft45862: `pretrained_model_name_or_path` debe ser el DIRECTORIO
        // del GGUF, no el fichero (un path a fichero falla en `cached_file`
        // al validarlo como repo_id — HFValidationError).
        assert!(TRAIN_LORA_PY.contains("os.path.dirname(args.gguf)"));
        assert!(TRAIN_LORA_PY.contains("--tokenizer"));
        assert!(TRAIN_LORA_PY.contains("AutoTokenizer.from_pretrained(args.tokenizer)"));
        // Fase 14.5 — export a GGUF (merge + conversión) gated por EXPORT_FILE.
        assert!(TRAIN_LORA_PY.contains("EXPORT_FILE"));
        assert!(TRAIN_LORA_PY.contains("merge_and_unload"));
        assert!(TRAIN_LORA_PY.contains("convert_hf_to_gguf.py"));
        assert!(TRAIN_LORA_PY.contains("NO_LOCAL_GGUF"));
    }

    #[test]
    fn convert_script_is_present_and_valid() {
        // Fase 14.5 — conversor HF→GGUF embebido (llama.cpp b4879).
        assert!(CONVERT_HF_TO_GGUF_PY.contains("def main()"));
        assert!(CONVERT_HF_TO_GGUF_PY.contains("--outtype"));
        assert!(CONVERT_HF_TO_GGUF_PY.contains("--outfile"));
        assert!(CONVERT_HF_TO_GGUF_PY.contains("class LlamaModel(Model):"));
    }

    #[test]
    fn ensure_deploys_script_with_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = ensure_finetune_scripts(dir.path()).unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("[train_lora]"));
        let convert = dir.path().join("convert_hf_to_gguf.py");
        assert!(convert.exists());
        let convert_content = std::fs::read_to_string(&convert).unwrap();
        assert!(convert_content.contains("class LlamaModel(Model):"));
        // Segundo despliegue idempotente
        let path2 = ensure_finetune_scripts(dir.path()).unwrap();
        assert_eq!(path, path2);
    }
}
