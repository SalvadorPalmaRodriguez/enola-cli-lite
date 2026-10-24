// MCP-005 — Resources: documentación de usuario embebida.
//
// Expone el catálogo cerrado `domain::docs::DOC_CATALOG` vía JSON-RPC:
// `resources/list` → [{uri, name, description, mimeType}]
// `resources/read` → {contents: [{uri, mimeType, text}]}
// La resolución de URIs es estricta: solo `enola://docs/<path>` del catálogo
// (sin traversal — `doc_by_uri` solo hace match exacto sobre paths fijos).

use crate::application::mcp::registry::ToolHandler;
use crate::domain::docs::{doc_by_uri, doc_title, search_docs, DOC_CATALOG};
use crate::domain::error::{EnolaError, Result};
use serde_json::{json, Value};
use std::sync::Arc;

/// Resultado de `resources/list`: todos los documentos embebidos.
pub(crate) fn list_resources() -> Value {
    let resources: Vec<Value> = DOC_CATALOG
        .iter()
        .map(|d| {
            json!({
                "uri": d.uri(),
                "name": d.path,
                "description": doc_title(d),
                "mimeType": "text/markdown",
            })
        })
        .collect();
    json!({ "resources": resources })
}

/// Resultado de `resources/read` para una URI `enola://docs/<path>`.
///
/// Solo resuelve URIs del catálogo cerrado; cualquier otra (incluido
/// traversal `..`) devuelve `EnolaError::NotFound`/`ValidationError`.
pub(crate) fn read_resource(uri: &str) -> Result<Value> {
    if uri.is_empty() {
        return Err(EnolaError::ValidationError("missing 'uri'".into()));
    }
    let doc =
        doc_by_uri(uri).ok_or_else(|| EnolaError::NotFound(format!("unknown resource '{uri}'")))?;
    Ok(json!({
        "contents": [{
            "uri": doc.uri(),
            "mimeType": "text/markdown",
            "text": doc.content,
        }]
    }))
}

/// Tool `docs_search`: búsqueda de texto sobre todo el catálogo.
struct DocsSearchHandler;

#[async_trait::async_trait]
impl ToolHandler for DocsSearchHandler {
    async fn call(&self, args: Value) -> Result<Value> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EnolaError::ValidationError("missing 'query'".into()))?;
        let matches: Vec<Value> = search_docs(query)
            .iter()
            .map(|(doc, excerpt)| {
                json!({
                    "uri": doc.uri(),
                    "doc": doc.path,
                    "excerpt": excerpt,
                })
            })
            .collect();
        Ok(json!({ "query": query, "matches": matches }))
    }
}

// ── Fase 12.5 — docs_lookup: búsqueda semántica RAG sobre la documentación ──
//
// Alternativa OPCIONAL a `docs_search` (texto plano). Consulta SOLO la
// colección `enola_docs` (documentación embebida, indexada con
// `ai rag ingest-docs`), nunca el corpus del usuario `rag_<instancia>` (H-12).
// Sin documentación indexada devuelve `results: []` + nota, no un error.

/// Tool `docs_lookup`: consulta semántica RAG sobre la colección `enola_docs`.
struct DocsLookupHandler {
    instances: Arc<dyn crate::ports::ai::AiAdapterPort + Send + Sync>,
    provision: Arc<dyn crate::ports::rag::AiVectorStorePort + Send + Sync>,
    store: Arc<dyn crate::ports::rag::AiRagStorePort + Send + Sync>,
}

impl DocsLookupHandler {
    /// Vector store y config de `instance` si tiene `enola_docs` indexada
    /// (vía registrada por `ai rag ingest-docs`); `None` en otro caso.
    async fn indexed_docs(
        &self,
        instance: &str,
    ) -> Result<
        Option<(
            crate::ports::rag::VectorStoreInfo,
            crate::domain::rag::RagConfig,
        )>,
    > {
        let Some(info) = self.provision.vector_store_endpoint(instance).await? else {
            return Ok(None);
        };
        let cfg = self.store.load_rag_config(instance).await?;
        let key = crate::domain::rag::embedding_kind_key(
            info.backend,
            crate::domain::rag::DOCS_COLLECTION,
        );
        Ok(cfg
            .embedding_kinds
            .contains_key(&key)
            .then_some((info, cfg)))
    }
}

/// Respuesta degradada (sin error) cuando no hay documentación indexada.
fn docs_not_indexed(query: &str, instance: Option<&str>) -> Value {
    let target = instance.unwrap_or("<instance>");
    json!({
        "query": query,
        "results": [],
        "note": format!(
            "enola-cli docs are not indexed for semantic lookup; run `enola-cli ai rag \
             ingest-docs {target}` or use the `docs_search` tool"
        )
    })
}

#[async_trait::async_trait]
impl ToolHandler for DocsLookupHandler {
    async fn call(&self, args: Value) -> Result<Value> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EnolaError::ValidationError("missing 'query'".into()))?;
        let n_results = args
            .get("n_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(5)
            .clamp(1, 20) as usize;
        let instance_arg = args.get("instance").and_then(|v| v.as_str());

        // H-12: solo cuentan las instancias con `enola_docs` indexada. Una
        // instancia indicada debe existir (si no, error); sin docs, degrada.
        let found = match instance_arg {
            Some(i) => {
                crate::domain::ai::validate_ai_instance_name(i)?;
                self.instances.get_instance(i).await?;
                self.indexed_docs(i).await?.map(|f| (i.to_string(), f))
            }
            None => {
                let mut found = None;
                for inst in self.instances.list_instances().await.unwrap_or_default() {
                    if let Ok(Some(f)) = self.indexed_docs(&inst.name).await {
                        found = Some((inst.name, f));
                        break;
                    }
                }
                found
            }
        };
        let Some((instance, (info, cfg))) = found else {
            return Ok(docs_not_indexed(query, instance_arg));
        };

        let ollama_port = self
            .instances
            .get_instance(&instance)
            .await?
            .api_port
            .ok_or_else(|| {
                EnolaError::ValidationError(format!("instancia '{instance}' sin API Ollama activa"))
            })?;
        let vs = self
            .provision
            .build_vector_store(&info, ollama_port, &cfg.embedding_model);
        // D6: consultar con la vía con que se indexó la colección.
        let kind_key = crate::domain::rag::embedding_kind_key(
            info.backend,
            crate::domain::rag::DOCS_COLLECTION,
        );
        let mode = crate::domain::rag::query_mode_for(
            cfg.embedding_kinds.get(&kind_key).map(String::as_str),
        );
        let results = vs
            .query(crate::domain::rag::DOCS_COLLECTION, query, n_results, mode)
            .await?;
        let matches: Vec<Value> = results
            .iter()
            .map(|r| {
                json!({
                    "doc_id": r.document.id,
                    "excerpt": r.document.content.chars().take(500).collect::<String>(),
                    "distance": r.distance,
                    "source": r.document.metadata.get("source"),
                    "path": r.document.metadata.get("path"),
                })
            })
            .collect();
        Ok(json!({
            "query": query,
            "instance": instance,
            "collection": crate::domain::rag::DOCS_COLLECTION,
            "backend": info.backend,
            "results": matches,
        }))
    }
}

/// Registra la tool `docs_lookup` en el registry (dependencias inyectadas).
pub(crate) fn register_docs_lookup_tool(
    registry: &mut crate::application::mcp::registry::ToolRegistry,
    instances: Arc<dyn crate::ports::ai::AiAdapterPort + Send + Sync>,
    provision: Arc<dyn crate::ports::rag::AiVectorStorePort + Send + Sync>,
    store: Arc<dyn crate::ports::rag::AiRagStorePort + Send + Sync>,
) -> Result<()> {
    use crate::domain::tool::{ToolKind, ToolSpec};
    let spec = ToolSpec::new(
        "docs_lookup",
        "Semantic RAG lookup over the enola-cli docs indexed by \
         `ai rag ingest-docs <instance>` (optional alternative to docs_search; \
         returns no results with a note if the docs are not indexed).",
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "minLength": 1, "maxLength": 256},
                "instance": {"type": "string", "minLength": 1, "maxLength": 63},
                "n_results": {"type": "integer", "minimum": 1, "maximum": 20},
            },
            "required": ["query"],
            "additionalProperties": false
        }),
        ToolKind::ReadOnly,
    )
    .map_err(EnolaError::ValidationError)?;
    registry.register(
        spec,
        Arc::new(DocsLookupHandler {
            instances,
            provision,
            store,
        }),
    )
}

/// Registra la tool `docs_search` en el registry.
pub(crate) fn register_search_tool(
    registry: &mut crate::application::mcp::registry::ToolRegistry,
) -> Result<()> {
    use crate::domain::tool::{ToolKind, ToolSpec};
    let spec = ToolSpec::new(
        "docs_search",
        "Search the embedded user documentation (all enola-cli docs).",
        json!({
            "type": "object",
            "properties": {"query": {"type": "string", "minLength": 1, "maxLength": 256}},
            "required": ["query"],
            "additionalProperties": false
        }),
        ToolKind::ReadOnly,
    )
    .map_err(EnolaError::ValidationError)?;
    registry.register(spec, Arc::new(DocsSearchHandler))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_resources_exposes_every_doc() {
        let out = list_resources();
        let resources = out["resources"].as_array().unwrap();
        assert_eq!(resources.len(), DOC_CATALOG.len());
        assert!(resources.iter().all(|r| r["mimeType"] == "text/markdown"));
        assert!(resources
            .iter()
            .any(|r| r["uri"] == "enola://docs/wp/commands-wp.md"));
    }

    #[test]
    fn read_resource_returns_markdown() {
        let out = read_resource("enola://docs/wp/commands-wp.md").unwrap();
        let text = out["contents"][0]["text"].as_str().unwrap();
        assert!(text.len() > 100);
        assert_eq!(out["contents"][0]["mimeType"], "text/markdown");
    }

    #[test]
    fn read_resource_rejects_traversal_and_unknown() {
        assert!(read_resource("enola://docs/../Cargo.toml").is_err());
        assert!(read_resource("enola://docs/nope.md").is_err());
        assert!(read_resource("file:///etc/passwd").is_err());
        assert!(read_resource("").is_err());
    }

    // ── H-12: docs_lookup solo lee `enola_docs` y degrada sin error ──

    use crate::domain::ai::{AiInstance, AiInstanceStatus};
    use crate::domain::rag::{EmbeddingMode, RagConfig, VectorStoreBackend};
    use crate::ports::ai::MockAiAdapterPort;
    use crate::ports::rag::{
        MockAiRagStorePort, MockAiVectorStorePort, MockRagVectorStorePort, RagDocument,
        RagSearchResult, VectorStoreInfo,
    };

    fn ai_inst(name: &str) -> AiInstance {
        AiInstance {
            name: name.into(),
            model: "llama3.1:8b".into(),
            api_port: Some(11435),
            webui_port: Some(8081),
            status: AiInstanceStatus::Running,
            ollama_status: "Up".into(),
            webui_status: "Up".into(),
            gpu_enabled: false,
            onion_address: None,
        }
    }

    /// `docs_lookup` sobre instancias `(nombre, ¿vector store?, ¿enola_docs indexada?)`.
    /// La query solo acepta la colección `enola_docs`.
    fn lookup(instances: &[(&'static str, bool, bool)]) -> DocsLookupHandler {
        let all: Vec<_> = instances.to_vec();
        let mut insts = MockAiAdapterPort::new();
        let names: Vec<AiInstance> = all.iter().map(|(n, _, _)| ai_inst(n)).collect();
        insts
            .expect_list_instances()
            .returning(move || Ok(names.clone()));
        let known: Vec<&'static str> = all.iter().map(|(n, _, _)| *n).collect();
        insts.expect_get_instance().returning(move |n| {
            if known.contains(&n) {
                Ok(ai_inst(n))
            } else {
                Err(EnolaError::NotFound(format!("instancia '{n}' no existe")))
            }
        });
        let with_vs: Vec<&'static str> = all.iter().filter(|i| i.1).map(|i| i.0).collect();
        let mut prov = MockAiVectorStorePort::new();
        prov.expect_vector_store_endpoint().returning(move |n| {
            Ok(with_vs.contains(&n).then(|| VectorStoreInfo {
                backend: VectorStoreBackend::Chroma,
                host_port: 12001,
                endpoint: "http://127.0.0.1:12001".into(),
            }))
        });
        prov.expect_build_vector_store().returning(|_, _, _| {
            let mut vs = MockRagVectorStorePort::new();
            vs.expect_query()
                .withf(|c, _, _, mode| c == "enola_docs" && *mode == EmbeddingMode::Ollama)
                .returning(|_, _, _, _| {
                    Ok(vec![RagSearchResult {
                        document: RagDocument {
                            id: "enola://docs/guia/quickstart.md#0".into(),
                            content: "texto".into(),
                            metadata: Default::default(),
                        },
                        distance: 0.1,
                    }])
                });
            Arc::new(vs)
        });
        let with_docs: Vec<&'static str> = all.iter().filter(|i| i.2).map(|i| i.0).collect();
        let mut store = MockAiRagStorePort::new();
        store.expect_load_rag_config().returning(move |n| {
            let mut c = RagConfig::default();
            c.embedding_kinds
                .insert("chromadb/rag_x".into(), "ollama:nomic-embed-text".into());
            if with_docs.contains(&n) {
                c.embedding_kinds.insert(
                    "chromadb/enola_docs".into(),
                    "ollama:nomic-embed-text".into(),
                );
            }
            Ok(c)
        });
        DocsLookupHandler {
            instances: Arc::new(insts),
            provision: Arc::new(prov),
            store: Arc::new(store),
        }
    }

    fn assert_degraded(out: &Value) {
        assert!(out["results"].as_array().unwrap().is_empty(), "{out}");
        let note = out["note"].as_str().unwrap();
        assert!(note.contains("ai rag ingest-docs"), "{note}");
        assert!(note.contains("docs_search"), "{note}");
    }

    #[tokio::test]
    async fn docs_lookup_degrades_when_no_vector_store_anywhere() {
        let h = lookup(&[("a", false, false)]);
        assert_degraded(&h.call(json!({"query": "onion"})).await.unwrap());
    }

    #[tokio::test]
    async fn docs_lookup_degrades_when_docs_not_indexed() {
        // Vector store presente pero sin `enola_docs` (antes: error «colección no existe»).
        let h = lookup(&[("a", true, false)]);
        assert_degraded(&h.call(json!({"query": "onion"})).await.unwrap());
        assert_degraded(
            &h.call(json!({"query": "onion", "instance": "a"}))
                .await
                .unwrap(),
        );
    }

    #[tokio::test]
    async fn docs_lookup_explicit_instance_without_vector_store_degrades() {
        let h = lookup(&[("a", false, false)]);
        assert_degraded(
            &h.call(json!({"query": "onion", "instance": "a"}))
                .await
                .unwrap(),
        );
    }

    #[tokio::test]
    async fn docs_lookup_explicit_unknown_or_invalid_instance_is_error() {
        let h = lookup(&[("a", true, true)]);
        assert!(h
            .call(json!({"query": "onion", "instance": "nope"}))
            .await
            .is_err());
        assert!(h
            .call(json!({"query": "onion", "instance": "../etc"}))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn docs_lookup_picks_first_instance_with_indexed_docs() {
        // `a` tiene vector store pero no docs; `b` sí: se elige `b`.
        let h = lookup(&[("a", true, false), ("b", true, true)]);
        let out = h.call(json!({"query": "onion"})).await.unwrap();
        assert_eq!(out["instance"], "b");
        assert_eq!(out["collection"], "enola_docs");
        assert_eq!(out["results"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn docs_lookup_schema_bounds_instance() {
        let mut reg = crate::application::mcp::registry::ToolRegistry::new();
        let h = lookup(&[]);
        register_docs_lookup_tool(&mut reg, h.instances, h.provision, h.store).unwrap();
        assert!(reg
            .validate("docs_lookup", &json!({"query": "x", "instance": "dev"}))
            .is_ok());
        assert!(reg
            .validate(
                "docs_lookup",
                &json!({"query": "x", "instance": "a".repeat(64)})
            )
            .is_err());
        assert!(reg
            .validate("docs_lookup", &json!({"query": "x", "instance": ""}))
            .is_err());
    }

    #[tokio::test]
    async fn docs_search_returns_matching_docs() {
        let h = DocsSearchHandler;
        let out = h.call(json!({"query": "onion"})).await.unwrap();
        assert!(!out["matches"].as_array().unwrap().is_empty());
        assert!(h.call(json!({})).await.is_err());
    }
}
