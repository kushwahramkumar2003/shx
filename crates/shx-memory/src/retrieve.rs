//! Budgeted recency + BM25 relevance context assembly (docs/03-MEMORY.md §4).

use shx_core::{
    ContextBundle, EnvInfo, Interaction, Profile, Redactor, Scope, Snippet, VocabEntry,
};

use crate::store::{MemoryStore, Result};

/// Knobs from `[memory.context]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextBudget {
    /// Anchor recency window.
    pub recent: u32,
    /// Extra relevance hits.
    pub relevance: u32,
    /// Nearby shell entries (unused until shell ingest).
    pub shell: u32,
    /// Hard cap on memory tokens (`chars/4`).
    pub max_tokens: u32,
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            recent: 10,
            relevance: 5,
            shell: 5,
            max_tokens: 1500,
        }
    }
}

/// Assembles a [`ContextBundle`] from a store. Deterministic.
pub struct ContextBuilder<'a> {
    redactor: &'a dyn Redactor,
}

impl<'a> ContextBuilder<'a> {
    /// Use a specific redactor (tests may pass `SecretRedactor`).
    pub fn new(redactor: &'a dyn Redactor) -> Self {
        Self { redactor }
    }

    /// Build context for `intent`. `project_id` is a git-root hash, if any.
    pub fn build(
        &self,
        intent: &str,
        env: &EnvInfo,
        profile: &Profile,
        store: &dyn MemoryStore,
        budget: ContextBudget,
        project_id: Option<&str>,
    ) -> Result<ContextBundle> {
        Ok(self
            .assemble(intent, env, profile, store, budget, project_id)?
            .0)
    }

    /// Like [`ContextBuilder::build`], plus whether the budget trimmed anything
    /// and the token count.
    pub fn build_with_stats(
        &self,
        intent: &str,
        env: &EnvInfo,
        profile: &Profile,
        store: &dyn MemoryStore,
        budget: ContextBudget,
        project_id: Option<&str>,
    ) -> Result<(ContextBundle, bool, usize)> {
        self.assemble(intent, env, profile, store, budget, project_id)
    }

    fn assemble(
        &self,
        intent: &str,
        env: &EnvInfo,
        profile: &Profile,
        store: &dyn MemoryStore,
        budget: ContextBudget,
        project_id: Option<&str>,
    ) -> Result<(ContextBundle, bool, usize)> {
        let tokens = crate::bm25::unique_tokens(intent);
        let mut history = self.anchor(store, budget.recent, project_id)?;
        let anchor_ids: Vec<Option<i64>> = history.iter().map(|i| i.id).collect();
        let extra = self.relevance(store, &tokens, budget.relevance, &anchor_ids)?;
        history.extend(extra);
        history.sort_by(|a, b| b.ts.cmp(&a.ts).then(b.id.cmp(&a.id)));
        history.dedup_by(|a, b| a.id.is_some() && a.id == b.id);
        let vocabulary = self.vocabulary(store, &tokens)?;
        let snippets = self.snippets(store, &tokens)?;
        let mut bundle = ContextBundle {
            env: env.clone(),
            profile: profile.clone(),
            history,
            vocabulary,
            snippets,
            shell: Vec::new(),
        };
        let pre = memory_tokens(&bundle);
        let dropped = cap_budget(&mut bundle, budget.max_tokens);
        redact_bundle(&mut bundle, self.redactor);
        let tokens = memory_tokens(&bundle);
        Ok((bundle, dropped || pre > budget.max_tokens as usize, tokens))
    }

    fn anchor(
        &self,
        store: &dyn MemoryStore,
        recent: u32,
        project_id: Option<&str>,
    ) -> Result<Vec<Interaction>> {
        let proj = store.recent(
            recent as usize,
            Scope::Project {
                id: project_id.map(str::to_string),
            },
        )?;
        if proj.len() >= 3 {
            Ok(proj)
        } else {
            store.recent(recent as usize, Scope::Tool)
        }
    }

    fn relevance(
        &self,
        store: &dyn MemoryStore,
        tokens: &[String],
        n: u32,
        exclude: &[Option<i64>],
    ) -> Result<Vec<Interaction>> {
        if n == 0 || tokens.is_empty() {
            return Ok(Vec::new());
        }
        let mut query = tokens.to_vec();
        for e in store.vocabulary(tokens)? {
            query.extend(crate::bm25::tokens(&e.expansion));
        }
        query.sort();
        query.dedup();
        let corpus = store.recent(crate::bm25::CORPUS_LIMIT, Scope::Tool)?;
        Ok(crate::bm25::top_n(&corpus, &query, exclude, n as usize))
    }

    fn vocabulary(&self, store: &dyn MemoryStore, tokens: &[String]) -> Result<Vec<VocabEntry>> {
        let mut matched = store.vocabulary(tokens)?;
        let mut top = store.vocabulary(&[])?;
        top.truncate(5);
        matched.extend(top);
        matched.retain(crate::vocab::is_applied);
        matched.sort_by(crate::vocab::cmp_rank);
        matched.dedup_by(|a, b| a.term == b.term);
        Ok(matched)
    }

    fn snippets(&self, store: &dyn MemoryStore, tokens: &[String]) -> Result<Vec<Snippet>> {
        let mut all = store.snippets()?;
        all.retain(|s| snippet_matches(s, tokens));
        all.sort_by(|a, b| a.name.cmp(&b.name));
        all.truncate(3);
        Ok(all)
    }
}

fn snippet_matches(s: &Snippet, tokens: &[String]) -> bool {
    if tokens.is_empty() {
        return false;
    }
    let hay = format!("{} {}", s.name, s.description.as_deref().unwrap_or("")).to_ascii_lowercase();
    tokens.iter().any(|t| hay.contains(t.as_str()))
}

fn est_tokens(s: &str) -> usize {
    s.chars().count().div_ceil(4)
}

/// Tokens counted toward `max_tokens` (memory blocks only).
pub fn memory_tokens(bundle: &ContextBundle) -> usize {
    let mut n = 0;
    for i in &bundle.history {
        n += est_tokens(&i.input_nl) + est_tokens(&i.output_cmd);
        if let Some(e) = &i.explanation {
            n += est_tokens(e);
        }
    }
    for v in &bundle.vocabulary {
        n += est_tokens(&v.term) + est_tokens(&v.expansion);
    }
    for s in &bundle.snippets {
        n += est_tokens(&s.name) + est_tokens(&s.command);
        if let Some(d) = &s.description {
            n += est_tokens(d);
        }
    }
    for sh in &bundle.shell {
        n += est_tokens(&sh.cmd);
    }
    n
}

fn cap_budget(bundle: &mut ContextBundle, max_tokens: u32) -> bool {
    let max = max_tokens as usize;
    let before = memory_tokens(bundle);
    while memory_tokens(bundle) > max && !bundle.history.is_empty() {
        bundle.history.pop();
    }
    while memory_tokens(bundle) > max && !bundle.snippets.is_empty() {
        bundle.snippets.pop();
    }
    while memory_tokens(bundle) > max && !bundle.vocabulary.is_empty() {
        bundle.vocabulary.pop();
    }
    while memory_tokens(bundle) > max && !bundle.shell.is_empty() {
        bundle.shell.pop();
    }
    before > max
}

fn redact_bundle(bundle: &mut ContextBundle, r: &dyn Redactor) {
    for i in &mut bundle.history {
        i.input_nl = r.redact(&i.input_nl).into_owned();
        i.output_cmd = r.redact(&i.output_cmd).into_owned();
        if let Some(e) = i.explanation.take() {
            i.explanation = Some(r.redact(&e).into_owned());
        }
    }
    for v in &mut bundle.vocabulary {
        v.term = r.redact(&v.term).into_owned();
        v.expansion = r.redact(&v.expansion).into_owned();
    }
    for s in &mut bundle.snippets {
        s.command = r.redact(&s.command).into_owned();
        if let Some(d) = s.description.take() {
            s.description = Some(r.redact(&d).into_owned());
        }
    }
    for sh in &mut bundle.shell {
        sh.cmd = r.redact(&sh.cmd).into_owned();
    }
    bundle.profile.notes = r.redact(&bundle.profile.notes).into_owned();
}
