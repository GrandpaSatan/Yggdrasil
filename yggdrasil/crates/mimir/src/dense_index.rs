//! In-memory dense 384-dim embedding index for the novelty-gate Phase 0
//! shadow observer (Sprint 067).
//!
//! Mirrors `SdrIndex`'s public API shape so the handler can run both indexes
//! side-by-side with symmetric call sites. The key difference is the payload
//! type: each entry stores the full L2-normalized `Vec<f32>` embedding
//! instead of a 256-bit SDR, preserving the 128 dimensions that SDR
//! binarization discards.
//!
//! Similarity reuses `crate::sdr::dot_similarity` — for L2-normalized input
//! (our ONNX embedder's output) dot product equals cosine similarity, so no
//! new math is introduced.
//!
//! At current scale (~50k engrams × 384 floats × 4 bytes ≈ 77 MB, brute-force
//! dot ≈ 1–2 ms) no ANN structure is required. If the index grows past
//! ~500k entries this module should gain HNSW or flat-IVF ranking.
//!
//! Phase 0 role: the handler populates this index on every successful
//! insert/update and queries it purely to compute a shadow cosine that is
//! logged alongside the SDR Hamming similarity. No verdict decisions use
//! this index yet.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::sdr;

/// Partition key used for engrams with no project (global scope).
const GLOBAL_PARTITION: &str = "__global__";

/// Thread-safe in-memory dense embedding index, partitioned by project.
///
/// Read-biased: queries take a read lock, inserts take a write lock.
/// Each partition is a `Vec<(Uuid, Vec<f32>)>`; the embedding vectors are
/// expected to be 384-dim and L2-normalized, matching the ONNX embedder
/// output used elsewhere in Mimir.
///
/// The parallel `tag_index` mirrors `SdrIndex`'s Sprint 065 A·P1 design —
/// partition-prefix tags (e.g. `sprint:NNN`, `incident:NNN`) isolate
/// cross-sprint collisions for tag-filtered queries.
pub struct DenseIndex {
    partitions: RwLock<HashMap<String, Vec<(Uuid, Vec<f32>)>>>,
    tag_index: RwLock<HashMap<Uuid, HashSet<Arc<str>>>>,
}

impl DenseIndex {
    /// Create an empty index with no partitions.
    pub fn new() -> Self {
        Self {
            partitions: RwLock::new(HashMap::new()),
            tag_index: RwLock::new(HashMap::new()),
        }
    }

    /// Insert an embedding into a specific project partition, recording
    /// partition-prefix tags in the parallel tag_index.
    ///
    /// `embedding` is expected to be L2-normalized 384-dim. The index does
    /// not enforce this — callers that feed un-normalized vectors will get
    /// uncalibrated similarity scores.
    pub fn insert_scoped_with_tags(
        &self,
        project: Option<&str>,
        id: Uuid,
        embedding: Vec<f32>,
        tags: &[String],
    ) {
        let key = project.unwrap_or(GLOBAL_PARTITION);
        let mut partitions = self.partitions.write().unwrap();
        partitions
            .entry(key.to_string())
            .or_default()
            .push((id, embedding));
        drop(partitions);

        if !tags.is_empty() {
            let mut tag_index = self.tag_index.write().unwrap();
            let entry = tag_index.entry(id).or_default();
            for t in tags {
                entry.insert(Arc::<str>::from(t.as_str()));
            }
        }
    }

    /// Remove an engram from ALL partitions (handles project reassignment).
    pub fn remove(&self, id: Uuid) {
        let mut partitions = self.partitions.write().unwrap();
        for entries in partitions.values_mut() {
            entries.retain(|(eid, _)| *eid != id);
        }
        drop(partitions);
        self.tag_index.write().unwrap().remove(&id);
    }

    /// Query a specific project partition + optionally the global partition,
    /// then apply an OR-semantics tag filter before returning the top-K.
    ///
    /// `tag_filter` — candidates pass if their tag set contains AT LEAST ONE
    /// of these tags. Empty filter = pass-all (all candidates in scope).
    /// Mirrors `SdrIndex::query_scoped_with_tags` semantics exactly so the
    /// two indexes can run symmetric observer/decision pairs.
    pub fn query_scoped_with_tags(
        &self,
        target: &[f32],
        project: &str,
        include_global: bool,
        tag_filter: &[String],
        limit: usize,
    ) -> Vec<(Uuid, f64)> {
        if limit == 0 {
            return Vec::new();
        }

        let partitions = self.partitions.read().unwrap();
        let mut scored: Vec<(Uuid, f64)> = Vec::new();

        if let Some(entries) = partitions.get(project) {
            scored.extend(
                entries
                    .iter()
                    .map(|(id, emb)| (*id, sdr::dot_similarity(target, emb))),
            );
        }

        if include_global
            && let Some(entries) = partitions.get(GLOBAL_PARTITION)
        {
            scored.extend(
                entries
                    .iter()
                    .map(|(id, emb)| (*id, sdr::dot_similarity(target, emb))),
            );
        }

        drop(partitions);

        if !tag_filter.is_empty() {
            let tag_index = self.tag_index.read().unwrap();
            scored.retain(|(id, _)| {
                tag_index
                    .get(id)
                    .map(|tags| tag_filter.iter().any(|t| tags.contains(t.as_str())))
                    .unwrap_or(false)
            });
        }

        scored.sort_by(|(id_a, sim_a), (id_b, sim_b)| {
            sim_b
                .partial_cmp(sim_a)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| id_a.cmp(id_b))
        });

        scored.truncate(limit);
        scored
    }

    /// Query ALL partitions (backward compat / unscoped search).
    pub fn query(&self, target: &[f32], limit: usize) -> Vec<(Uuid, f64)> {
        if limit == 0 {
            return Vec::new();
        }

        let partitions = self.partitions.read().unwrap();
        let mut scored: Vec<(Uuid, f64)> = Vec::new();
        for entries in partitions.values() {
            scored.extend(
                entries
                    .iter()
                    .map(|(id, emb)| (*id, sdr::dot_similarity(target, emb))),
            );
        }

        scored.sort_by(|(id_a, sim_a), (id_b, sim_b)| {
            sim_b
                .partial_cmp(sim_a)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| id_a.cmp(id_b))
        });

        scored.truncate(limit);
        scored
    }

    /// Bulk load from PostgreSQL rows: `(id, embedding Vec<f32>, project, tags)`.
    ///
    /// Phase 0 reserves this for future PG backfill — at Phase 0 the index
    /// starts empty and populates from live inserts. Implemented now so the
    /// follow-up backfill sprint only needs to wire up the SQL side.
    pub fn load_from_rows_scoped_with_tags(
        &self,
        rows: &[(Uuid, Vec<f32>, Option<String>, Vec<String>)],
    ) {
        let mut partitions = self.partitions.write().unwrap();
        let mut tag_index = self.tag_index.write().unwrap();
        for (id, embedding, project, tags) in rows {
            let key = project.as_deref().unwrap_or(GLOBAL_PARTITION);
            partitions
                .entry(key.to_string())
                .or_default()
                .push((*id, embedding.clone()));
            if !tags.is_empty() {
                let entry = tag_index.entry(*id).or_default();
                for t in tags {
                    entry.insert(Arc::<str>::from(t.as_str()));
                }
            }
        }
    }

    /// Total number of entries across all partitions.
    pub fn len(&self) -> usize {
        self.partitions
            .read()
            .unwrap()
            .values()
            .map(|v| v.len())
            .sum()
    }

    /// Whether the index has zero entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Number of partitions (projects + global).
    pub fn partition_count(&self) -> usize {
        self.partitions.read().unwrap().len()
    }

    /// Number of entries in a specific partition.
    pub fn partition_len(&self, project: Option<&str>) -> usize {
        let key = project.unwrap_or(GLOBAL_PARTITION);
        self.partitions
            .read()
            .unwrap()
            .get(key)
            .map(|v| v.len())
            .unwrap_or(0)
    }
}

impl Default for DenseIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an L2-normalized 4-dim embedding from raw values.
    /// Short vectors keep the tests readable; the index does not enforce
    /// a specific dimension, only that target + stored entries match length.
    fn norm(v: &[f32]) -> Vec<f32> {
        let mag: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if mag == 0.0 {
            return v.to_vec();
        }
        v.iter().map(|x| x / mag).collect()
    }

    #[test]
    fn insert_and_query() {
        let index = DenseIndex::new();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        let a = norm(&[1.0, 1.0, 1.0, 1.0]);
        let b = norm(&[1.0, 1.0, 0.0, 0.0]);

        index.insert_scoped_with_tags(None, id1, a.clone(), &[]);
        index.insert_scoped_with_tags(None, id2, b, &[]);

        let results = index.query(&a, 2);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, id1);
        assert!((results[0].1 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn insert_scoped_and_query_scoped() {
        let index = DenseIndex::new();
        let id_ygg = Uuid::new_v4();
        let id_fen = Uuid::new_v4();
        let id_global = Uuid::new_v4();
        let vec_a = norm(&[1.0, 0.0, 0.0, 0.0]);

        index.insert_scoped_with_tags(Some("yggdrasil"), id_ygg, vec_a.clone(), &[]);
        index.insert_scoped_with_tags(Some("fenrir"), id_fen, vec_a.clone(), &[]);
        index.insert_scoped_with_tags(None, id_global, vec_a.clone(), &[]);

        // Query yggdrasil + global — should NOT see fenrir
        let results = index.query_scoped_with_tags(&vec_a, "yggdrasil", true, &[], 10);
        let ids: Vec<Uuid> = results.iter().map(|(id, _)| *id).collect();
        assert!(ids.contains(&id_ygg));
        assert!(ids.contains(&id_global));
        assert!(!ids.contains(&id_fen));

        // Query fenrir only (no global) — should only see fenrir
        let results = index.query_scoped_with_tags(&vec_a, "fenrir", false, &[], 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, id_fen);
    }

    #[test]
    fn query_all_spans_partitions() {
        let index = DenseIndex::new();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let v = norm(&[1.0, 1.0, 1.0, 1.0]);

        index.insert_scoped_with_tags(Some("a"), id1, v.clone(), &[]);
        index.insert_scoped_with_tags(Some("b"), id2, v.clone(), &[]);

        let results = index.query(&v, 10);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn remove_entry() {
        let index = DenseIndex::new();
        let id = Uuid::new_v4();
        let v = norm(&[1.0, 0.0, 0.0, 0.0]);
        index.insert_scoped_with_tags(None, id, v, &[]);
        assert_eq!(index.len(), 1);

        index.remove(id);
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn remove_crosses_partitions() {
        let index = DenseIndex::new();
        let id = Uuid::new_v4();
        let v = norm(&[1.0, 0.0, 0.0, 0.0]);
        index.insert_scoped_with_tags(Some("proj"), id, v, &[]);
        assert_eq!(index.len(), 1);

        index.remove(id);
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn remove_clears_tag_index() {
        let index = DenseIndex::new();
        let id = Uuid::new_v4();
        let v = norm(&[1.0, 0.0, 0.0, 0.0]);
        index.insert_scoped_with_tags(Some("p"), id, v, &["sprint:067".to_string()]);
        index.remove(id);

        assert!(index.tag_index.read().unwrap().get(&id).is_none());
    }

    #[test]
    fn empty_query_returns_empty() {
        let index = DenseIndex::new();
        let v = norm(&[1.0, 0.0, 0.0, 0.0]);
        let results = index.query(&v, 5);
        assert!(results.is_empty());
    }

    #[test]
    fn limit_zero_returns_empty() {
        let index = DenseIndex::new();
        let v = norm(&[1.0, 0.0, 0.0, 0.0]);
        index.insert_scoped_with_tags(None, Uuid::new_v4(), v.clone(), &[]);
        assert!(index.query(&v, 0).is_empty());
        assert!(index.query_scoped_with_tags(&v, "p", true, &[], 0).is_empty());
    }

    #[test]
    fn orthogonal_vectors_have_zero_similarity() {
        let index = DenseIndex::new();
        let a = norm(&[1.0, 0.0, 0.0, 0.0]);
        let b = norm(&[0.0, 1.0, 0.0, 0.0]);
        index.insert_scoped_with_tags(None, Uuid::new_v4(), a.clone(), &[]);

        let results = index.query(&b, 1);
        assert_eq!(results.len(), 1);
        assert!(results[0].1.abs() < 1e-6, "expected ~0, got {}", results[0].1);
    }

    #[test]
    fn identical_vectors_score_one() {
        let index = DenseIndex::new();
        let v = norm(&[0.5, 0.5, 0.5, 0.5]);
        index.insert_scoped_with_tags(None, Uuid::new_v4(), v.clone(), &[]);

        let results = index.query(&v, 1);
        assert_eq!(results.len(), 1);
        assert!((results[0].1 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn results_sorted_descending_by_similarity() {
        let index = DenseIndex::new();
        let target = norm(&[1.0, 0.0, 0.0, 0.0]);
        let near = norm(&[0.9, 0.1, 0.0, 0.0]); // high cosine
        let far = norm(&[0.0, 1.0, 0.0, 0.0]);  // low cosine

        let id_near = Uuid::new_v4();
        let id_far = Uuid::new_v4();
        index.insert_scoped_with_tags(None, id_far, far, &[]);
        index.insert_scoped_with_tags(None, id_near, near, &[]);

        let results = index.query(&target, 2);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, id_near, "nearest match must come first");
        assert!(results[0].1 > results[1].1);
    }

    // --- Sprint 065 A·P1 parallel: partition-prefix tag filter ---

    #[test]
    fn insert_with_tags_records_in_tag_index() {
        let index = DenseIndex::new();
        let id = Uuid::new_v4();
        let v = norm(&[1.0, 0.0, 0.0, 0.0]);
        let tags = vec!["sprint:067".to_string(), "phase:0".to_string()];
        index.insert_scoped_with_tags(Some("yggdrasil"), id, v, &tags);

        let guard = index.tag_index.read().unwrap();
        let recorded = guard.get(&id).expect("tag set present");
        assert!(recorded.contains("sprint:067"));
        assert!(recorded.contains("phase:0"));
    }

    #[test]
    fn query_with_sprint_tag_filter_isolates_sprints() {
        let index = DenseIndex::new();
        let id_066 = Uuid::new_v4();
        let id_067 = Uuid::new_v4();
        let shared = norm(&[1.0, 1.0, 1.0, 1.0]);

        // Identical embeddings — cross-sprint collision scenario.
        index.insert_scoped_with_tags(
            Some("yggdrasil"),
            id_066,
            shared.clone(),
            &["sprint:066".to_string()],
        );
        index.insert_scoped_with_tags(
            Some("yggdrasil"),
            id_067,
            shared.clone(),
            &["sprint:067".to_string()],
        );

        // Empty filter — both return.
        let all = index.query_scoped_with_tags(&shared, "yggdrasil", false, &[], 10);
        assert_eq!(all.len(), 2);

        // sprint:067 filter — only id_067 passes.
        let only_067 = index.query_scoped_with_tags(
            &shared,
            "yggdrasil",
            false,
            &["sprint:067".to_string()],
            10,
        );
        assert_eq!(only_067.len(), 1);
        assert_eq!(only_067[0].0, id_067);

        // sprint:066 filter — only id_066 passes.
        let only_066 = index.query_scoped_with_tags(
            &shared,
            "yggdrasil",
            false,
            &["sprint:066".to_string()],
            10,
        );
        assert_eq!(only_066.len(), 1);
        assert_eq!(only_066[0].0, id_066);
    }

    #[test]
    fn tag_filter_or_semantics() {
        let index = DenseIndex::new();
        let id = Uuid::new_v4();
        let v = norm(&[1.0, 0.0, 0.0, 0.0]);
        index.insert_scoped_with_tags(
            Some("p"),
            id,
            v.clone(),
            &["sprint:067".to_string(), "phase:P1".to_string()],
        );

        // Non-matching + matching tag — OR semantics passes the match.
        let results = index.query_scoped_with_tags(
            &v,
            "p",
            false,
            &["sprint:099".to_string(), "phase:P1".to_string()],
            10,
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, id);
    }

    #[test]
    fn untagged_candidate_filtered_out_when_filter_present() {
        let index = DenseIndex::new();
        let tagged = Uuid::new_v4();
        let untagged = Uuid::new_v4();
        let v = norm(&[1.0, 0.0, 0.0, 0.0]);

        index.insert_scoped_with_tags(
            Some("p"),
            tagged,
            v.clone(),
            &["sprint:067".to_string()],
        );
        index.insert_scoped_with_tags(Some("p"), untagged, v.clone(), &[]);

        let filtered = index.query_scoped_with_tags(
            &v,
            "p",
            false,
            &["sprint:067".to_string()],
            10,
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].0, tagged);
    }

    #[test]
    fn load_from_rows_populates_partitions_and_tags() {
        let index = DenseIndex::new();
        let id = Uuid::new_v4();
        let v = norm(&[1.0, 1.0, 0.0, 0.0]);

        index.load_from_rows_scoped_with_tags(&[(
            id,
            v.clone(),
            Some("yggdrasil".to_string()),
            vec!["sprint:067".to_string(), "core".to_string()],
        )]);

        let found = index.query_scoped_with_tags(
            &v,
            "yggdrasil",
            false,
            &["sprint:067".to_string()],
            10,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, id);
    }

    #[test]
    fn stats_len_and_partition_counts() {
        let index = DenseIndex::new();
        let v = norm(&[1.0, 0.0, 0.0, 0.0]);
        index.insert_scoped_with_tags(Some("a"), Uuid::new_v4(), v.clone(), &[]);
        index.insert_scoped_with_tags(Some("b"), Uuid::new_v4(), v.clone(), &[]);
        index.insert_scoped_with_tags(None, Uuid::new_v4(), v, &[]);

        assert_eq!(index.len(), 3);
        assert_eq!(index.partition_count(), 3);
        assert_eq!(index.partition_len(Some("a")), 1);
        assert_eq!(index.partition_len(None), 1);
    }
}
