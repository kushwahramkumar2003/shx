//! Okapi BM25 ranking over the interaction corpus (docs/03-MEMORY.md §4 step 2).
//!
//! Deterministic: query tokens are sorted/deduped; ties break on `ts` then `id`
//! descending. No extra crates — this is a few dozen lines of f64 arithmetic.

use std::collections::{BTreeMap, BTreeSet};

use shx_core::Interaction;

/// Robertson / Lucene defaults. Documented in docs/03-MEMORY.md §4.
pub(crate) const K1: f64 = 1.2;
pub(crate) const B: f64 = 0.75;

/// Newest Tool-scope rows that form the BM25 collection. History beyond this
/// window is not scored (the CLI is short-lived; 2048 rows is well above a
/// typical 180-day store).
pub(crate) const CORPUS_LIMIT: usize = 2048;

const STOPWORDS: &[&str] = &[
    "a", "an", "the", "on", "in", "of", "to", "for", "and", "or", "with", "is", "at", "it", "as",
    "by", "from", "be", "this", "that", "my", "me",
];

/// Tokenize `text`: lowercase, drop 1-char tokens and stopwords, keep duplicates
/// (term frequency).
pub(crate) fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| t.len() > 1)
        .map(|t| t.to_ascii_lowercase())
        .filter(|t| !STOPWORDS.contains(&t.as_str()))
        .collect()
}

/// Unique, sorted tokens — the query form used by the context builder.
pub(crate) fn unique_tokens(text: &str) -> Vec<String> {
    let mut out = tokens(text);
    out.sort();
    out.dedup();
    out
}

fn doc_tokens(i: &Interaction) -> Vec<String> {
    let mut t = tokens(&i.input_nl);
    t.extend(tokens(&i.output_cmd));
    t
}

/// Lucene-style IDF: `ln(1 + (N - n + 0.5) / (n + 0.5))`. Always positive.
fn idf(n_docs: usize, df: usize) -> f64 {
    let n = n_docs as f64;
    let d = df as f64;
    (1.0 + (n - d + 0.5) / (d + 0.5)).ln()
}

fn score_doc(
    doc: &[String],
    query: &[String],
    n_docs: usize,
    avgdl: f64,
    df: &BTreeMap<&str, usize>,
) -> f64 {
    let dl = doc.len() as f64;
    let mut score = 0.0;
    for q in query {
        let tf = doc.iter().filter(|t| *t == q).count() as f64;
        if tf == 0.0 {
            continue;
        }
        let nq = df.get(q.as_str()).copied().unwrap_or(0);
        let denom = tf + K1 * (1.0 - B + B * dl / avgdl);
        score += idf(n_docs, nq) * (tf * (K1 + 1.0)) / denom;
    }
    score
}

/// Rank `corpus` by BM25 against `query`. Zero-score and `exclude`d rows are
/// dropped. At most `n` hits, highest score first.
pub(crate) fn top_n(
    corpus: &[Interaction],
    query: &[String],
    exclude: &[Option<i64>],
    n: usize,
) -> Vec<Interaction> {
    if n == 0 || query.is_empty() || corpus.is_empty() {
        return Vec::new();
    }
    let docs: Vec<Vec<String>> = corpus.iter().map(doc_tokens).collect();
    let n_docs = corpus.len();
    let mut df: BTreeMap<&str, usize> = BTreeMap::new();
    for tokens in &docs {
        let mut seen = BTreeSet::new();
        for t in tokens {
            if seen.insert(t.as_str()) {
                *df.entry(t.as_str()).or_insert(0) += 1;
            }
        }
    }
    let total_len: usize = docs.iter().map(Vec::len).sum();
    if total_len == 0 {
        return Vec::new();
    }
    let avgdl = total_len as f64 / n_docs as f64;

    let mut scored: Vec<(usize, f64)> = docs
        .iter()
        .enumerate()
        .filter_map(|(idx, tokens)| {
            if exclude.contains(&corpus[idx].id) {
                return None;
            }
            let s = score_doc(tokens, query, n_docs, avgdl, &df);
            if s > 0.0 { Some((idx, s)) } else { None }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.1.total_cmp(&a.1)
            .then(corpus[b.0].ts.cmp(&corpus[a.0].ts))
            .then(corpus[b.0].id.cmp(&corpus[a.0].id))
    });
    scored.truncate(n);
    scored
        .into_iter()
        .map(|(idx, _)| corpus[idx].clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shx_core::RiskLevel;

    fn ix(id: i64, ts: i64, input: &str, cmd: &str) -> Interaction {
        Interaction {
            id: Some(id),
            ts,
            session_id: "s".into(),
            project_id: None,
            cwd: "/tmp".into(),
            os: "macos".into(),
            shell: "zsh".into(),
            input_nl: input.into(),
            output_cmd: cmd.into(),
            explanation: None,
            backend: "mock".into(),
            model: "fixture".into(),
            confidence: None,
            latency_ms: 1,
            risk_level: RiskLevel::Safe,
            risk_notes: vec![],
            from_cache: false,
            accepted: None,
            executed: None,
            tags: vec![],
        }
    }

    #[test]
    fn empty_query_or_corpus_is_empty() {
        let corpus = vec![ix(1, 1, "run pg", "docker run pg")];
        assert!(top_n(&corpus, &[], &[], 5).is_empty());
        assert!(top_n(&[], &unique_tokens("run"), &[], 5).is_empty());
        assert!(top_n(&corpus, &unique_tokens("run"), &[], 0).is_empty());
    }

    #[test]
    fn rare_term_outranks_common_term_when_overlap_ties() {
        let mut corpus = vec![
            ix(1, 200, "run the tests", "cargo test"),
            ix(2, 100, "start postgres", "pg_ctl start"),
        ];
        for i in 3..12 {
            corpus.push(ix(i, 50 - i, &format!("run job {i}"), "true"));
        }
        let q = unique_tokens("run postgres");
        let top = top_n(&corpus, &q, &[], 3);
        assert_eq!(top[0].id, Some(2), "rare term postgres should win");
        assert_eq!(top[0].input_nl, "start postgres");
    }

    #[test]
    fn zero_score_and_excluded_ids_are_dropped() {
        let corpus = vec![
            ix(1, 3, "run postgres", "docker run postgres"),
            ix(2, 2, "list files", "ls -la"),
            ix(3, 1, "cargo test", "cargo test"),
        ];
        let q = unique_tokens("postgres");
        let hits = top_n(&corpus, &q, &[Some(1)], 5);
        assert!(hits.is_empty());
        let hits = top_n(&corpus, &q, &[], 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, Some(1));
    }

    #[test]
    fn ranking_is_deterministic() {
        let corpus = vec![
            ix(1, 10, "run postgres", "docker run postgres"),
            ix(2, 20, "run postgres", "docker run postgres"),
            ix(3, 20, "run postgres", "docker run postgres"),
        ];
        let q = unique_tokens("run postgres");
        let a = top_n(&corpus, &q, &[], 5);
        let b = top_n(&corpus, &q, &[], 5);
        assert_eq!(a, b);
        // Equal scores: newer ts, then higher id.
        assert_eq!(
            a.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![Some(3), Some(2), Some(1)]
        );
    }

    #[test]
    fn tokenize_drops_stopwords_and_short_tokens() {
        assert_eq!(unique_tokens("run pg on 7"), vec!["pg", "run"]);
        assert_eq!(tokens("Run Run PG"), vec!["run", "run", "pg"]);
    }

    /// Offline T6 stand-in: labeled relevant docs, score-order MRR / recall@5.
    /// T-506 will formalize this against `tools/eval/fixtures/translate.json`.
    #[test]
    fn t505_bm25_beats_keyword_baselines() {
        let mut corpus = Vec::new();
        for i in 0..10 {
            corpus.push(ix(100 + i, 100 + i, &format!("run job {i}"), "true"));
        }
        for i in 0..5 {
            corpus.push(ix(200 + i, 200 + i, &format!("list files {i}"), "ls"));
            corpus.push(ix(
                250 + i,
                250 + i,
                &format!("tail the logs {i}"),
                "tail -f app.log",
            ));
        }
        corpus.push(ix(
            50,
            50,
            "start postgres locally",
            "docker run -p 5432:5432 postgres",
        ));
        corpus.push(ix(
            51,
            51,
            "follow api pod logs",
            "kubectl logs -f deploy/api",
        ));
        corpus.push(ix(
            52,
            52,
            "remove stopped containers",
            "docker container prune",
        ));
        corpus.push(ix(
            53,
            53,
            "undo last commit keep changes",
            "git reset --soft HEAD~1",
        ));
        corpus.push(ix(54, 54, "print os name", "uname -s"));
        corpus.push(ix(55, 55, "kill process on port 3000", "lsof -i :3000"));

        struct Q {
            intent: &'static str,
            relevant: &'static str,
            expand: Option<&'static str>,
        }
        let queries = [
            Q {
                intent: "run postgres",
                relevant: "start postgres locally",
                expand: None,
            },
            Q {
                intent: "kubectl logs for the api",
                relevant: "follow api pod logs",
                expand: None,
            },
            Q {
                intent: "prune stopped docker containers",
                relevant: "remove stopped containers",
                expand: None,
            },
            Q {
                intent: "undo git commit but keep the work",
                relevant: "undo last commit keep changes",
                expand: None,
            },
            Q {
                intent: "os",
                relevant: "print os name",
                expand: None,
            },
            Q {
                intent: "port 3000",
                relevant: "kill process on port 3000",
                expand: None,
            },
            Q {
                intent: "pg",
                relevant: "start postgres locally",
                expand: Some("postgres"),
            },
            Q {
                intent: "boot postgres in docker on 5432",
                relevant: "start postgres locally",
                expand: None,
            },
        ];

        fn keyword_score(i: &Interaction, qtokens: &[String]) -> u32 {
            let hay = format!("{} {}", i.input_nl, i.output_cmd).to_ascii_lowercase();
            qtokens.iter().filter(|t| hay.contains(t.as_str())).count() as u32
        }
        fn rank_of(order: &[String], relevant: &str) -> Option<usize> {
            order.iter().position(|s| s == relevant).map(|i| i + 1)
        }
        fn metrics(ranks: &[Option<usize>], k: usize) -> (f64, f64) {
            let n = ranks.len() as f64;
            let mut mrr = 0.0;
            let mut hits = 0.0;
            for rank in ranks.iter().flatten() {
                mrr += 1.0 / *rank as f64;
                if *rank <= k {
                    hits += 1.0;
                }
            }
            (mrr / n, hits / n)
        }

        let mut search_ranks = Vec::new();
        let mut overlap_ranks = Vec::new();
        let mut bm25_ranks = Vec::new();

        for q in &queries {
            let mut qtokens = unique_tokens(q.intent);
            let hay_intent = q.intent.to_ascii_lowercase();
            let mut search: Vec<&Interaction> = corpus
                .iter()
                .filter(|i| {
                    i.input_nl.to_ascii_lowercase().contains(&hay_intent)
                        || i.output_cmd.to_ascii_lowercase().contains(&hay_intent)
                })
                .collect();
            search.sort_by(|a, b| {
                keyword_score(b, &qtokens)
                    .cmp(&keyword_score(a, &qtokens))
                    .then(b.ts.cmp(&a.ts))
                    .then(b.id.cmp(&a.id))
            });
            search.truncate(5);
            let search_order: Vec<String> = search.iter().map(|i| i.input_nl.clone()).collect();

            let mut overlap: Vec<&Interaction> = corpus.iter().collect();
            overlap.sort_by(|a, b| {
                keyword_score(b, &qtokens)
                    .cmp(&keyword_score(a, &qtokens))
                    .then(b.ts.cmp(&a.ts))
                    .then(b.id.cmp(&a.id))
            });
            overlap.retain(|i| keyword_score(i, &qtokens) > 0);
            overlap.truncate(5);
            let overlap_order: Vec<String> = overlap.iter().map(|i| i.input_nl.clone()).collect();

            if let Some(exp) = q.expand {
                qtokens.extend(tokens(exp));
                qtokens.sort();
                qtokens.dedup();
            }
            let bm25 = top_n(&corpus, &qtokens, &[], 5);
            let bm25_order: Vec<String> = bm25.iter().map(|i| i.input_nl.clone()).collect();

            search_ranks.push(rank_of(&search_order, q.relevant));
            overlap_ranks.push(rank_of(&overlap_order, q.relevant));
            bm25_ranks.push(rank_of(&bm25_order, q.relevant));
        }

        let (search_mrr, search_r5) = metrics(&search_ranks, 5);
        let (overlap_mrr, overlap_r5) = metrics(&overlap_ranks, 5);
        let (bm25_mrr, bm25_r5) = metrics(&bm25_ranks, 5);
        let table = format!(
            "T-505 score-order delta (k=5, n={n})\n\
             method              MRR    recall@5\n\
             search+overlap      {search_mrr:.3}  {search_r5:.3}\n\
             corpus overlap      {overlap_mrr:.3}  {overlap_r5:.3}\n\
             BM25                {bm25_mrr:.3}  {bm25_r5:.3}\n\
             delta vs search     {d_mrr:+.3}  {d_r5:+.3}",
            n = queries.len(),
            d_mrr = bm25_mrr - search_mrr,
            d_r5 = bm25_r5 - search_r5,
        );
        eprintln!("{table}");
        assert!(
            bm25_mrr > search_mrr && bm25_r5 > search_r5,
            "BM25 must beat pre-T-505 search+overlap\n{table}"
        );
        assert!(
            bm25_r5 >= overlap_r5,
            "BM25 recall@5 must match-or-beat corpus overlap\n{table}"
        );
        assert!(
            bm25_mrr + 1e-9 >= overlap_mrr || bm25_r5 > overlap_r5,
            "BM25 must not regress both metrics vs overlap\n{table}"
        );
    }
}
