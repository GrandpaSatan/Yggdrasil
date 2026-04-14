/// Memory-event router — CALM-inspired zero-injection influence layer.
///
/// Memory events returned by Mimir's `POST /api/v1/recall` are used to
/// structurally refine routing decisions **without** injecting any text into
/// the LLM prompt.  This is the core of the Sprint 015 zero-injection
/// architecture: memory shapes *how* the model is chosen and *which* intent
/// is applied, but the model never reads raw engram text.
///
/// ## Design invariants
/// - Only events with `similarity >= 0.6` are considered (noise filter).
/// - `Pattern` triggers override `intent` only when `similarity > 0.85`
///   and an `intent_hint` is present.
/// - `Fact` triggers are informational — no routing change, logged for
///   observability.
/// - `Decision` triggers are informational only.
/// - Tag-based refinement promotes a `"default"` or `"general"` intent to
///   a specific intent when tags carry strong domain signal.
use ygg_domain::engram::{EngramTrigger, RecallResponse};

use crate::router::RoutingDecision;

/// Process memory events to refine a routing decision in place.
///
/// Called after the keyword-based `SemanticRouter::classify()` step and
/// before RAG context assembly so that any intent override takes effect
/// before Muninn's HA-skip logic is evaluated.
///
/// Memory influences behavior structurally — no text enters the prompt.
pub fn apply_memory_events(events: &RecallResponse, decision: &mut RoutingDecision) {
    let all_events = events.events.iter().chain(events.core_events.iter());

    for event in all_events {
        // Noise filter: only act on high-confidence matches.
        if event.similarity < 0.6 {
            continue;
        }

        match &event.trigger {
            EngramTrigger::Pattern { intent_hint, label } => {
                // A Pattern trigger with a strong similarity score and a
                // populated intent_hint can refine an unclassified intent.
                // Only overrides "default"/"general" — never overrides intents
                // that were positively classified by keyword matching (coding,
                // home_automation, etc.) or explicit model selection.
                if !intent_hint.is_empty()
                    && event.similarity > 0.85
                    && (decision.intent == "default" || decision.intent == "general")
                {
                    tracing::debug!(
                        pattern_label = %label,
                        old_intent = %decision.intent,
                        new_intent = %intent_hint,
                        similarity = event.similarity,
                        "memory pattern refining unclassified intent"
                    );
                    decision.intent = intent_hint.clone();
                }
            }

            EngramTrigger::Fact { label } => {
                // Facts are informational in the routing layer.  They confirm
                // that the system has processed a similar topic before and
                // that domain context is stable.  No routing change applied.
                tracing::debug!(
                    fact_label = %label,
                    similarity = event.similarity,
                    "memory fact trigger observed (no routing change)"
                );
            }

            EngramTrigger::Decision { label } => {
                // Decisions are recorded for observability.  They may be used
                // in future sprints to enforce consistency constraints, but for
                // now they are passive.
                tracing::debug!(
                    decision_label = %label,
                    similarity = event.similarity,
                    "memory decision trigger observed"
                );
            }
        }

        // ── Tag-based domain promotion ────────────────────────────────────
        // When the keyword router lands on "default" (no keyword match), strong
        // tag signals from past interactions can promote the intent to a
        // specific domain.  Only promotes from neutral/unset intents.
        if decision.intent == "default" || decision.intent == "general" {
            for tag in &event.tags {
                match tag.as_str() {
                    "coding" | "code" | "programming" => {
                        tracing::debug!(
                            tag = %tag,
                            "memory tag promoted intent to coding"
                        );
                        decision.intent = "coding".to_string();
                        break;
                    }
                    "home_automation" | "ha" | "smart_home" => {
                        tracing::debug!(
                            tag = %tag,
                            "memory tag promoted intent to home_automation"
                        );
                        decision.intent = "home_automation".to_string();
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
}
