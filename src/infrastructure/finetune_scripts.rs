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

/// Despliega los scripts de fine-tuning en `dir` (root-owned bajo
/// `/srv/enola-ai/<instance>/finetune/scripts/`) con permisos `0750`.
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o750));
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
    }

    #[test]
    fn ensure_deploys_script_with_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = ensure_finetune_scripts(dir.path()).unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("[train_lora]"));
        // Segundo despliegue idempotente
        let path2 = ensure_finetune_scripts(dir.path()).unwrap();
        assert_eq!(path, path2);
    }
}
