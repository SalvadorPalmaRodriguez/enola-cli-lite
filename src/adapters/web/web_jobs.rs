//! A25-03/I5 (P1.5) — Jobs largos del dashboard con patrón
//! *POST arranca / GET observa*.
//!
//! El POST autenticado (`/api/setup` con `pqc_tls`) lanza el job y emite un
//! ticket aleatorio de un solo uso (TTL 60 s). El `EventSource` del navegador
//! no puede enviar `Authorization`, así que el GET SSE se autentica con ese
//! ticket y **solo lee** el buffer del job: un GET nunca ejecuta nada (SD-11).

use rand::Rng;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Vida máxima de un ticket SSE sin canjear.
pub(crate) const TICKET_TTL: Duration = Duration::from_secs(60);
/// Máximo de tickets pendientes (anti acumulación).
const MAX_PENDING_TICKETS: usize = 64;
/// Máximo de líneas retenidas por job (las más antiguas se descartan).
const MAX_JOB_LINES: usize = 20_000;

#[derive(Default)]
struct JobState {
    /// Índice absoluto de `lines[0]` (crece al descartar las antiguas).
    start: usize,
    lines: Vec<String>,
    done: bool,
}

/// Buffer de salida de un job, observable por varios suscriptores.
#[derive(Default)]
pub(crate) struct JobLog {
    state: Mutex<JobState>,
    notify: tokio::sync::Notify,
}

impl JobLog {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, JobState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Añade una línea y despierta a los observadores.
    pub(crate) fn push(&self, line: impl Into<String>) {
        {
            let mut st = self.lock();
            st.lines.push(line.into());
            if st.lines.len() > MAX_JOB_LINES {
                let drop = st.lines.len() - MAX_JOB_LINES;
                st.lines.drain(..drop);
                st.start += drop;
            }
        }
        self.notify.notify_waiters();
    }

    /// Marca el job como terminado.
    pub(crate) fn finish(&self) {
        self.lock().done = true;
        self.notify.notify_waiters();
    }

    pub(crate) fn is_done(&self) -> bool {
        self.lock().done
    }

    /// Líneas desde el índice absoluto `from`, el siguiente índice y si el
    /// job terminó.
    pub(crate) fn read_from(&self, from: usize) -> (Vec<String>, usize, bool) {
        let st = self.lock();
        let from = from.max(st.start);
        let rel = from - st.start;
        let out: Vec<String> = st.lines.get(rel..).map(|s| s.to_vec()).unwrap_or_default();
        (out, st.start + st.lines.len(), st.done)
    }

    /// Espera a nuevas líneas (o a `max`, para no depender de un wakeup).
    pub(crate) async fn wait(&self, max: Duration) {
        let _ = tokio::time::timeout(max, self.notify.notified()).await;
    }
}

/// Tickets monouso que autorizan a observar un job por SSE.
#[derive(Default)]
pub(crate) struct TicketStore {
    pending: Mutex<HashMap<String, (Arc<JobLog>, Instant)>>,
}

impl TicketStore {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, (Arc<JobLog>, Instant)>> {
        self.pending.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Emite un ticket aleatorio (128 bit, hex) para observar `job`.
    pub(crate) fn issue(&self, job: Arc<JobLog>) -> String {
        let ticket: String = {
            let mut rng = rand::thread_rng();
            (0..16)
                .map(|_| format!("{:02x}", rng.gen::<u8>()))
                .collect()
        };
        let now = Instant::now();
        let mut map = self.lock();
        map.retain(|_, (_, at)| now.duration_since(*at) < TICKET_TTL);
        if map.len() >= MAX_PENDING_TICKETS {
            // Descarta el más antiguo: nunca crece sin límite.
            if let Some(oldest) = map
                .iter()
                .min_by_key(|(_, (_, at))| *at)
                .map(|(k, _)| k.clone())
            {
                map.remove(&oldest);
            }
        }
        map.insert(ticket.clone(), (job, now));
        ticket
    }

    /// Canjea un ticket: lo elimina siempre y devuelve el job si no caducó.
    pub(crate) fn redeem(&self, ticket: &str) -> Option<Arc<JobLog>> {
        let (job, at) = self.lock().remove(ticket)?;
        (Instant::now().duration_since(at) < TICKET_TTL).then_some(job)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_is_single_use() {
        let store = TicketStore::default();
        let job = JobLog::new();
        let t = store.issue(job);
        assert_eq!(t.len(), 32);
        assert!(store.redeem(&t).is_some());
        assert!(
            store.redeem(&t).is_none(),
            "un ticket no se canjea dos veces"
        );
        assert!(store.redeem("deadbeef").is_none());
    }

    #[test]
    fn expired_ticket_is_rejected() {
        let store = TicketStore::default();
        let job = JobLog::new();
        let t = store.issue(job.clone());
        // Envejece el ticket artificialmente.
        if let Some(entry) = store.lock().get_mut(&t) {
            entry.1 = Instant::now() - TICKET_TTL - Duration::from_secs(1);
        }
        assert!(store.redeem(&t).is_none());
    }

    #[test]
    fn pending_tickets_are_bounded() {
        let store = TicketStore::default();
        let job = JobLog::new();
        for _ in 0..(MAX_PENDING_TICKETS + 10) {
            store.issue(job.clone());
        }
        assert!(store.lock().len() <= MAX_PENDING_TICKETS);
    }

    #[test]
    fn job_log_replays_and_tracks_offset() {
        let log = JobLog::new();
        log.push("a");
        log.push("b");
        let (lines, next, done) = log.read_from(0);
        assert_eq!(lines, vec!["a", "b"]);
        assert_eq!(next, 2);
        assert!(!done);
        log.push("c");
        log.finish();
        let (lines, next, done) = log.read_from(next);
        assert_eq!(lines, vec!["c"]);
        assert_eq!(next, 3);
        assert!(done);
    }

    #[test]
    fn job_log_is_bounded() {
        let log = JobLog::new();
        for i in 0..(MAX_JOB_LINES + 5) {
            log.push(i.to_string());
        }
        let (lines, next, _) = log.read_from(0);
        assert_eq!(lines.len(), MAX_JOB_LINES);
        assert_eq!(next, MAX_JOB_LINES + 5);
        assert_eq!(lines[0], "5");
    }
}
