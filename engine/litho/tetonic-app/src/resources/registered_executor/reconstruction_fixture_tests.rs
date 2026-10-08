use super::*;
use crate::{events::RecordingEventSink, Application, ComputePlaneRequest};

pub(super) struct Fixture {
    pub dir: tempfile::TempDir,
    pub local: LocalControl,
    pub credential: IssuedCredential,
    pub digest: String,
}

impl Fixture {
    pub async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("workspace")).unwrap();
        std::fs::write(dir.path().join("workspace/fixture.txt"), "cobalt orchard").unwrap();
        let local = LocalControl::open(dir.path().join("control.db"), "test".into())
            .await
            .unwrap();
        local
            .bootstrap("admin".into(), "org".into(), "Org".into())
            .await
            .unwrap();
        let credential = local
            .credentials()
            .issue("admin".into(), 3600)
            .await
            .unwrap();
        let secret = credential.expose_secret();
        let resources = local.resources();
        resources
            .create_team(secret, "org".into(), "team".into(), "Team".into())
            .await
            .unwrap();
        local
            .contexts()
            .create(
                secret,
                "shared".into(),
                ContextOwner::Team {
                    org_id: "org".into(),
                    team_id: "team".into(),
                },
            )
            .await
            .unwrap();
        let agent = resources.register_agent(secret, "org".into(), "agent".into(), "general".into(),
            serde_json::json!({"instructions":"Read the fixture and ask about the audience before answering.","requested_tools":["read_file"]})).await.unwrap();
        let digest = agent.identity.bound_definition_digest;
        let prepared = resources
            .prepare_general_revision(
                secret,
                "org".into(),
                "agent".into(),
                digest.clone(),
                "Read the fixture and tailor it for the audience".into(),
                Self::limits(),
            )
            .await
            .unwrap();
        resources
            .issue_execution_grant(
                secret,
                tetonic_memory::ExecutionGrant {
                    grant_id: "grant".into(),
                    scope: tetonic_domain::ExecutionScope {
                        principal_id: "admin".into(),
                        organization_id: "org".into(),
                        information_context_id: "shared".into(),
                    },
                    job: prepared.start_command("job".into()).unwrap().job_spec,
                    expires_at: chrono::Utc::now().timestamp() + 3600,
                },
            )
            .await
            .unwrap();
        resources
            .create_team_work_item(
                secret,
                CreateTeamWorkItem {
                    org: "org".into(),
                    team: "team".into(),
                    work_id: "work".into(),
                    title: "Read the fixture".into(),
                    request_id: "request".into(),
                    goal_id: None,
                },
            )
            .await
            .unwrap();
        resources
            .authorize_work_budget(
                secret,
                "org".into(),
                "team".into(),
                "work".into(),
                "fund".into(),
                100,
            )
            .await
            .unwrap();
        Self {
            dir,
            local,
            credential,
            digest,
        }
    }
    fn limits() -> HarnessPreparationLimits {
        HarnessPreparationLimits {
            human_handoff: true,
            work_director: false,
            max_steps: 4,
            max_input_bytes: 1024,
        }
    }
    pub fn request(&self) -> RegisteredAgentJob {
        RegisteredAgentJob {
            request_id: tetonic_memory::work_activation_request_id("request"),
            organization_id: "org".into(),
            information_context_id: "shared".into(),
            agent_key: "agent".into(),
            definition_digest: self.digest.clone(),
            execution_grant_id: "grant".into(),
            input: "Read the fixture and tailor it for the audience".into(),
            recovery_id: "job".into(),
        }
    }
    pub fn store(&self) -> SharedStore {
        SharedStore::open(self.dir.path().join("control.db"), 1).unwrap()
    }
    pub fn settings(&self) -> RegisteredExecutionSettings {
        let (sender, _) = tokio::sync::mpsc::channel(1);
        RegisteredExecutionSettings {
            skills: None,
            mcp: None,
            hosted: None,
            response_schema: None,
            plan_dispatch: Some(super::super::super::plan_dispatch::PlanDispatch {
                director: None,
                human: Some(super::super::super::plan_dispatch::HumanHandoff {
                    store: self.store(),
                    actor: "admin".into(),
                    org: "org".into(),
                    team: "team".into(),
                    work: "work".into(),
                    durable_wait_seconds: Some(600),
                    prepared_stop_binding: None,
                }),
                binding: "work".into(),
                sender,
                assignment_keys: vec![],
                remaining: Default::default(),
            }),
            max_elapsed_seconds: 30,
            reported_token_ceiling: Some(95),
            workspace_root: Some(self.dir.path().join("workspace")),
            model: "qwen3.5:latest".into(),
            num_ctx: 8192,
            data_class: tetonic_domain::DataClass::Secret,
            allowed_tools: ["read_file".into(), "ask_human".into()]
                .into_iter()
                .collect(),
            limits: Self::limits(),
        }
    }
    pub async fn app(&self, url: &str) -> Arc<Application> {
        let store = self.store();
        let (sink, _) = RecordingEventSink::new();
        let app = Application::bootstrap_mock_with_store(
            self.dir.path(),
            Some(store.clone()),
            sink,
            vec![],
        );
        let guard = Arc::new(tetonic_egress::EgressGuard::new());
        guard.configure_loopback_inference(url.rsplit(':').next().unwrap().parse().unwrap());
        let plane = crate::build_compute_plane(ComputePlaneRequest {
            guard,
            ollama_base: url.into(),
            policy: app.host.runtime.policy().clone(),
            workspace_root: self.dir.path().join("workspace"),
            artifact_store: app.host.runtime.artifact_store().clone(),
            store: Some(store),
            coordinator: None,
            placement_sink: None,
            previous_pooled: None,
        })
        .await;
        app.install_compute_services(&plane);
        app
    }
    pub fn context(
        receipt: Option<tetonic_run::managed::ActivationReceipt>,
    ) -> preparation::RegisteredJobContext {
        preparation::RegisteredJobContext {
            work: Some(("team".into(), "work".into())),
            parent: None,
            restore: receipt,
        }
    }
    pub async fn harness(
        &self,
        app: &Application,
        receipt: Option<tetonic_run::managed::ActivationReceipt>,
    ) -> assembly::RegisteredHarness {
        let plan = app
            .prepare_registered_harness(
                self.credential.expose_secret(),
                self.local.credentials().clone(),
                self.request(),
                self.settings(),
                Self::context(receipt),
            )
            .await
            .unwrap();
        let preparation::HarnessPreparation::Ready(plan) = plan else {
            panic!("expected a harness")
        };
        app.assemble_registered_harness(self.credential.expose_secret(), *plan)
            .await
            .unwrap()
    }
}
