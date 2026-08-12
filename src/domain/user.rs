// Minimal stub for `domain::user` to satisfy module reference in `mod.rs`
// Expand later with real types/behaviours as needed by the domain logic.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub name: Option<String>,
}

impl User {
    pub fn new(id: impl Into<String>, name: Option<String>) -> Self {
        Self {
            id: id.into(),
            name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::User;

    #[test]
    fn new_with_name() {
        let u = User::new("u1", Some("Alice".into()));
        assert_eq!(u.id, "u1");
        assert_eq!(u.name.as_deref(), Some("Alice"));
    }

    #[test]
    fn new_without_name() {
        let u = User::new("u2", None);
        assert_eq!(u.id, "u2");
        assert!(u.name.is_none());
    }

    #[test]
    fn id_accepts_string_and_str() {
        let from_str = User::new("abc", None);
        let from_string = User::new("abc".to_string(), None);
        assert_eq!(from_str.id, from_string.id);
    }

    #[test]
    fn clone_is_equal() {
        let u = User::new("x", Some("Bob".into()));
        let c = u.clone();
        assert_eq!(u, c);
    }

    #[test]
    fn eq_same_id_and_name() {
        let a = User::new("id1", Some("X".into()));
        let b = User::new("id1", Some("X".into()));
        assert_eq!(a, b);
    }

    #[test]
    fn ne_different_id() {
        let a = User::new("id1", None);
        let b = User::new("id2", None);
        assert_ne!(a, b);
    }

    #[test]
    fn ne_different_name() {
        let a = User::new("id1", Some("A".into()));
        let b = User::new("id1", Some("B".into()));
        assert_ne!(a, b);
    }

    #[test]
    fn debug_contains_id() {
        let u = User::new("debug-id", None);
        let s = format!("{:?}", u);
        assert!(s.contains("debug-id"));
    }

    // ── Error-path / edge-case tests ──

    #[test]
    fn new_with_empty_id() {
        let u = User::new("", None);
        assert_eq!(u.id, "");
        assert!(u.name.is_none());
    }

    #[test]
    fn new_with_empty_name() {
        let u = User::new("u1", Some("".into()));
        assert_eq!(u.name.as_deref(), Some(""));
    }

    #[test]
    fn debug_contains_name_when_present() {
        let u = User::new("id1", Some("Charlie".into()));
        let s = format!("{:?}", u);
        assert!(s.contains("Charlie"));
    }

    #[test]
    fn ne_when_one_has_name_and_other_doesnt() {
        let a = User::new("id1", Some("X".into()));
        let b = User::new("id1", None);
        assert_ne!(a, b);
    }
}
