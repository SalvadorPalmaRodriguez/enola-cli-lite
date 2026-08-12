//! Domain types for Git/Forgejo services.
//!
//! # Historial de cambios
//!
//! - **2026-03-21**: `GitPortManager` eliminado (tarea 230).
//!   La gestión de puertos ahora usa `PortValidator` en `application/port_validator.rs`.
//!   Los rangos están definidos en `PortRanges::GIT_HTTP` y `PortRanges::GIT_SSH`.
//!
//! Este módulo actualmente está vacío porque:
//! - `GitServerInstance` solo se usaba para `GitPortManager::allocate_ports()`
//! - La validación de puertos ahora ocurre en el executor con `PortValidator`
//!
//! Se mantiene el módulo vacío temporalmente para evitar errores de compilación
//! hasta que se verifique que no hay otras dependencias.
//!
//! El módulo puede eliminarse en el futuro si no se añaden nuevos tipos de dominio Git.
