//! Acceptance tests for T-501: Project scoping + container detection.
//!
//! Acceptance:
//! - two different repos produce distinct project_ids and disjoint project-scoped recall
//! - in_container detection works inside/outside a container (mocked FS)

use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use shx_core::{
    ContextBundle, EnvInfo, Interaction, Profile, RiskLevel, Scope, SecretRedactor,
    is_in_container, resolve_in_container,
};
use shx_memory::{
    ContextBudget, ContextBuilder, InMemoryStore, MemoryStore, SqliteStore, resolve_project_scope,
};

fn unique_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = env::temp_dir().join(format!("shx-test-scope-{prefix}-{nanos}"));
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn make_interaction(id: i64, project_id: Option<String>, input: &str, cmd: &str) -> Interaction {
    Interaction {
        id: Some(id),
        ts: 1_000_000 + id,
        session_id: "test".into(),
        project_id,
        cwd: "/test".into(),
        os: "linux".into(),
        shell: "bash".into(),
        input_nl: input.into(),
        output_cmd: cmd.into(),
        explanation: None,
        backend: "mock".into(),
        model: "fixture".into(),
        confidence: Some(1.0),
        latency_ms: 5,
        risk_level: RiskLevel::Safe,
        risk_notes: vec![],
        from_cache: false,
        accepted: None,
        executed: None,
        tags: vec![],
    }
}

/// Acceptance 1: two different repos produce distinct `project_id`s and disjoint project-scoped recall.
#[test]
fn two_repos_produce_distinct_project_ids_and_disjoint_recall() {
    let repo_a = unique_dir("repo-a");
    fs::create_dir_all(repo_a.join(".git")).expect("git a");
    let repo_b = unique_dir("repo-b");
    fs::create_dir_all(repo_b.join(".git")).expect("git b");

    let (root_a, id_a) = resolve_project_scope(None, &repo_a);
    let (root_b, id_b) = resolve_project_scope(None, &repo_b);

    assert!(root_a.is_some(), "repo A must have a git root");
    assert!(root_b.is_some(), "repo B must have a git root");
    assert!(id_a.is_some(), "repo A must have project_id");
    assert!(id_b.is_some(), "repo B must have project_id");

    let id_a = id_a.unwrap();
    let id_b = id_b.unwrap();

    assert_ne!(
        id_a, id_b,
        "two distinct repos must produce distinct project_ids"
    );
    assert_eq!(id_a.len(), 16);
    assert_eq!(id_b.len(), 16);

    // Test disjoint recall on both InMemoryStore and SqliteStore
    let sqlite_file = repo_a.join("test.db");
    let sqlite_store = SqliteStore::open(&sqlite_file).expect("open sqlite");
    let memory_store = InMemoryStore::new();

    let stores: Vec<Box<dyn MemoryStore>> = vec![Box::new(sqlite_store), Box::new(memory_store)];

    for store in stores {
        // Record 3 interactions in repo A
        for i in 1..=3 {
            let item = make_interaction(
                i,
                Some(id_a.clone()),
                &format!("test in repo a {i}"),
                &format!("cmd_a_{i}"),
            );
            store.record_interaction(&item).expect("record in a");
        }

        // Record 3 interactions in repo B
        for i in 4..=6 {
            let item = make_interaction(
                i,
                Some(id_b.clone()),
                &format!("test in repo b {i}"),
                &format!("cmd_b_{i}"),
            );
            store.record_interaction(&item).expect("record in b");
        }

        // Project A recall
        let recall_a = store
            .recent(
                10,
                Scope::Project {
                    id: Some(id_a.clone()),
                },
            )
            .expect("recall a");
        assert_eq!(recall_a.len(), 3, "repo A must recall exactly 3 items");
        for rec in &recall_a {
            assert_eq!(
                rec.project_id.as_deref(),
                Some(id_a.as_str()),
                "recall A must only contain repo A interactions"
            );
            assert!(
                !rec.output_cmd.contains("cmd_b"),
                "repo A recall must be disjoint from repo B"
            );
        }

        // Project B recall
        let recall_b = store
            .recent(
                10,
                Scope::Project {
                    id: Some(id_b.clone()),
                },
            )
            .expect("recall b");
        assert_eq!(recall_b.len(), 3, "repo B must recall exactly 3 items");
        for rec in &recall_b {
            assert_eq!(
                rec.project_id.as_deref(),
                Some(id_b.as_str()),
                "recall B must only contain repo B interactions"
            );
            assert!(
                !rec.output_cmd.contains("cmd_a"),
                "repo B recall must be disjoint from repo A"
            );
        }

        // ContextBuilder integration: building context in repo A anchors to repo A only
        let redactor = SecretRedactor;
        let builder = ContextBuilder::new(&redactor);
        let env = EnvInfo {
            os: "linux".into(),
            shell: "bash".into(),
            cwd: repo_a.display().to_string(),
            git_root: Some(repo_a.display().to_string()),
            in_container: false,
        };
        let profile = Profile {
            name: "default".into(),
            ports: vec![],
            prefer_docker: false,
            notes: "".into(),
        };
        let budget = ContextBudget {
            recent: 5,
            relevance: 0,
            shell: 0,
            max_tokens: 1000,
        };

        let bundle_a: ContextBundle = builder
            .build(
                "run tests",
                &env,
                &profile,
                store.as_ref(),
                budget,
                Some(&id_a),
            )
            .expect("build context");
        assert_eq!(bundle_a.history.len(), 3);
        for h in &bundle_a.history {
            assert!(
                h.output_cmd.contains("cmd_a"),
                "repo A bundle must contain repo A command"
            );
            assert!(
                !h.output_cmd.contains("cmd_b"),
                "repo A bundle must never contain repo B command"
            );
        }
    }

    let _ = fs::remove_dir_all(repo_a);
    let _ = fs::remove_dir_all(repo_b);
}

/// Acceptance 2: in_container detection works inside/outside a container (mocked FS).
#[test]
fn in_container_detection_inside_outside_mocked_fs() {
    // 1. Outside container
    let clean_fs = |_p: &str| false;
    assert!(
        !is_in_container(clean_fs, None, None),
        "clean environment must not be flagged as container"
    );
    assert!(
        !is_in_container(
            clean_fs,
            Some("1:name=systemd:/\n2:cpu:/init.scope\n"),
            None
        ),
        "normal host cgroup must not be flagged as container"
    );

    // 2. Inside Docker container via /.dockerenv
    let docker_fs = |p: &str| p == "/.dockerenv";
    assert!(
        is_in_container(docker_fs, None, None),
        "presence of /.dockerenv flags container"
    );

    // 3. Inside Podman container via /run/.containerenv
    let podman_fs = |p: &str| p == "/run/.containerenv";
    assert!(
        is_in_container(podman_fs, None, None),
        "presence of /run/.containerenv flags container"
    );

    // 4. Container environment variable
    assert!(
        is_in_container(clean_fs, None, Some("podman")),
        "container=podman flags container"
    );
    assert!(
        is_in_container(clean_fs, None, Some("docker")),
        "container=docker flags container"
    );
    assert!(
        is_in_container(clean_fs, None, Some("oci")),
        "container=oci flags container"
    );
    assert!(
        !is_in_container(clean_fs, None, Some("")),
        "empty container env does not flag container"
    );
    assert!(
        !is_in_container(clean_fs, None, Some("0")),
        "container=0 does not flag container"
    );
    assert!(
        !is_in_container(clean_fs, None, Some("false")),
        "container=false does not flag container"
    );

    // 5. Cgroup markers
    assert!(
        is_in_container(clean_fs, Some("12:pids:/docker/43b7f14b3017a4216cdb"), None),
        "cgroup with docker flags container"
    );
    assert!(
        is_in_container(
            clean_fs,
            Some("1:name=systemd:/kubepods.slice/kubepods-burstable.slice/pod123"),
            None
        ),
        "cgroup with kubepods flags container"
    );
    assert!(
        is_in_container(
            clean_fs,
            Some("2:devices:/system.slice/containerd.service"),
            None
        ),
        "cgroup with containerd flags container"
    );
    assert!(
        is_in_container(clean_fs, Some("3:memory:/lxc/my-container"), None),
        "cgroup with lxc flags container"
    );

    // 6. Config resolution overrides
    assert!(
        resolve_in_container("true", clean_fs, None, None),
        "config setting 'true' forces true"
    );
    assert!(
        !resolve_in_container("false", docker_fs, None, None),
        "config setting 'false' forces false even when /.dockerenv exists"
    );
    assert!(
        resolve_in_container("auto", docker_fs, None, None),
        "config setting 'auto' inspects environment"
    );
    assert!(
        !resolve_in_container("auto", clean_fs, None, None),
        "config setting 'auto' returns false on clean environment"
    );
}
