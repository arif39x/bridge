use crate::engine::query::QueryValue;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct EntitySnapshot {
    pub table_name: String,
    pub values: HashMap<String, QueryValue>,
}

pub struct DirtyTracker {
    pub snapshots: HashMap<String, EntitySnapshot>,
}

impl DirtyTracker {
    pub fn new() -> Self {
        Self {
            snapshots: HashMap::new(),
        }
    }

    pub fn take_snapshot(
        &mut self,
        key: String,
        table_name: String,
        values: HashMap<String, QueryValue>,
    ) {
        self.snapshots
            .insert(key, EntitySnapshot { table_name, values });
    }

    pub fn remove_snapshot(&mut self, key: &str) {
        self.snapshots.remove(key);
    }

    pub fn compute_diff(
        &self,
        key: &str,
        current_values: &HashMap<String, QueryValue>,
    ) -> Option<HashMap<String, QueryValue>> {
        let snapshot = self.snapshots.get(key)?;
        let mut diff = HashMap::new();

        for (col, current_val) in current_values {
            if let Some(original_val) = snapshot.values.get(col) {
                if current_val != original_val {
                    diff.insert(col.clone(), current_val.clone());
                }
            }
        }

        if diff.is_empty() {
            None
        } else {
            Some(diff)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_values(pairs: &[(&str, QueryValue)]) -> HashMap<String, QueryValue> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    #[test]
    fn take_and_remove_snapshot() {
        let mut tracker = DirtyTracker::new();
        let values = make_values(&[("name", QueryValue::String("alice".into()))]);
        tracker.take_snapshot("key1".into(), "users".into(), values);

        let snapshot = tracker.snapshots.get("key1");
        assert!(snapshot.is_some());
        assert_eq!(snapshot.unwrap().table_name, "users");

        tracker.remove_snapshot("key1");
        assert!(tracker.snapshots.is_empty());
    }

    #[test]
    fn compute_diff_no_changes() {
        let mut tracker = DirtyTracker::new();
        let values = make_values(&[
            ("name", QueryValue::String("alice".into())),
            ("age", QueryValue::Int(30)),
        ]);
        tracker.take_snapshot("key1".into(), "users".into(), values.clone());

        let diff = tracker.compute_diff("key1", &values);
        assert!(diff.is_none());
    }

    #[test]
    fn compute_diff_with_changes() {
        let mut tracker = DirtyTracker::new();
        let original = make_values(&[
            ("name", QueryValue::String("alice".into())),
            ("age", QueryValue::Int(30)),
        ]);
        tracker.take_snapshot("key1".into(), "users".into(), original);

        let current = make_values(&[
            ("name", QueryValue::String("bob".into())),
            ("age", QueryValue::Int(30)),
        ]);
        let diff = tracker.compute_diff("key1", &current).unwrap();
        assert_eq!(diff.len(), 1);
        assert_eq!(diff.get("name").unwrap(), &QueryValue::String("bob".into()));
        assert!(diff.get("age").is_none());
    }

    #[test]
    fn compute_diff_nonexistent_key() {
        let tracker = DirtyTracker::new();
        let values = make_values(&[("x", QueryValue::Int(1))]);
        let diff = tracker.compute_diff("no_such_key", &values);
        assert!(diff.is_none());
    }

    #[test]
    fn compute_diff_new_column_added() {
        let mut tracker = DirtyTracker::new();
        let original = make_values(&[("name", QueryValue::String("alice".into()))]);
        tracker.take_snapshot("k".into(), "t".into(), original);

        let current = make_values(&[
            ("name", QueryValue::String("alice".into())),
            ("extra", QueryValue::Int(99)),
        ]);
        let diff = tracker.compute_diff("k", &current);
        assert!(diff.is_none());
    }

    #[test]
    fn compute_diff_all_changed() {
        let mut tracker = DirtyTracker::new();
        let original = make_values(&[
            ("a", QueryValue::Int(1)),
            ("b", QueryValue::Bool(false)),
        ]);
        tracker.take_snapshot("k".into(), "t".into(), original);

        let current = make_values(&[
            ("a", QueryValue::Int(2)),
            ("b", QueryValue::Bool(true)),
        ]);
        let diff = tracker.compute_diff("k", &current).unwrap();
        assert_eq!(diff.len(), 2);
    }
}
