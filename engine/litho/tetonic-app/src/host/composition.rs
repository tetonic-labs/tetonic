//! The single service-wiring path used by real hosts and injected test applications.
use crate::services::*;
use crate::{
    approval, commands, errors, events, host, services, Application, ApplicationDependencies,
};
use std::sync::Arc;

struct RunCommitEventHook;

impl tetonic_run::RunEventHook for RunCommitEventHook {
    fn on_run_committed(&self, result: &tetonic_domain::RunCommandResult) {
        tracing::debug!(
            run_id = %result.run_id,
            sequence = result.sequence,
            "run command committed"
        );
    }
}

pub(crate) fn build_supervisor(
    store: Option<tetonic_memory::SharedStore>,
) -> Arc<dyn tetonic_run::RunSupervisor> {
    // Startup storage failures enter Safe Mode inside the manager. An individual
    // interrupted run stays quarantined by its own RecoveryRequired state.
    let sup = tetonic_run::DurableRunSupervisor::new(store).with_hook(Arc::new(RunCommitEventHook));
    Arc::new(sup)
}

impl Application {
    /// Bootstrap workspace + runtime, returning a fully wired `Application`.
    pub async fn bootstrap(
        init_cmd: commands::InitializeCommand,
        store: Option<tetonic_memory::SharedStore>,
        event_sink: Arc<dyn events::ApplicationEventSink>,
        index_db: Option<std::path::PathBuf>,
    ) -> Result<
        (
            Self,
            commands::InitializeResultPayload,
            commands::InitializeBootstrapPayload,
        ),
        errors::AppError,
    > {
        let init: Arc<dyn InitializationService> =
            Arc::new(services::DefaultInitializationService::new());
        let init_result = init.initialize_workspace(init_cmd.clone()).await?;
        let bootstrap = init.bootstrap_runtime(&init_cmd, &store)?;
        let app = Self::from_bootstrap(init, &bootstrap, store, event_sink, index_db);
        Ok((app, init_result, bootstrap))
    }

    /// Wire services from a completed bootstrap using the same init service instance.
    pub fn from_bootstrap(
        init: Arc<dyn InitializationService>,
        bootstrap: &commands::InitializeBootstrapPayload,
        store: Option<tetonic_memory::SharedStore>,
        event_sink: Arc<dyn events::ApplicationEventSink>,
        index_db: Option<std::path::PathBuf>,
    ) -> Self {
        Self::from_bootstrap_with_supervisor(
            init,
            bootstrap,
            store.clone(),
            event_sink,
            index_db,
            build_supervisor(store),
        )
    }

    /// Same as [`Self::from_bootstrap`] but reuses an existing RunSupervisor
    /// (so the compute plane can bind it before index/submit).
    pub fn from_bootstrap_with_supervisor(
        init: Arc<dyn InitializationService>,
        bootstrap: &commands::InitializeBootstrapPayload,
        store: Option<tetonic_memory::SharedStore>,
        event_sink: Arc<dyn events::ApplicationEventSink>,
        index_db: Option<std::path::PathBuf>,
        supervisor: Arc<dyn tetonic_run::RunSupervisor>,
    ) -> Self {
        Self::compose(
            init,
            ApplicationDependencies {
                runtime: bootstrap.runtime.clone(),
                store,
                policy: bootstrap.policy.clone(),
                event_sink,
                index_db,
            },
            bootstrap.workspace_root.clone(),
            bootstrap.inference_defaults.num_ctx,
            supervisor,
        )
    }

    pub fn new(deps: ApplicationDependencies) -> Self {
        let supervisor = build_supervisor(deps.store.clone());
        Self::compose(
            Arc::new(services::DefaultInitializationService::new()),
            deps,
            String::new(),
            tetonic_capacity::InferenceDefaults::fallback().num_ctx,
            supervisor,
        )
    }

    fn compose(
        init: Arc<dyn InitializationService>,
        deps: ApplicationDependencies,
        workspace_root: String,
        num_ctx: u32,
        supervisor: Arc<dyn tetonic_run::RunSupervisor>,
    ) -> Self {
        let run_manager = Arc::new(services::DefaultRunService::new(
            deps.store.clone(),
            deps.policy.clone(),
            deps.event_sink.clone(),
            supervisor.clone(),
            deps.runtime.artifact_store().clone(),
        ));
        let runs: Arc<dyn services::RunService> = run_manager.clone();
        let approvals: Arc<dyn approval::ApprovalService> = Arc::new(
            approval::DefaultApprovalService::new(deps.store.clone(), deps.event_sink.clone()),
        );
        run_manager.attach_approvals(approvals.clone());
        run_manager.attach_runtime(deps.runtime.clone());
        Self {
            init,
            runs,
            run_manager,
            supervisor,
            policies: Arc::new(services::DefaultPolicyService::new(
                deps.policy.clone(),
                deps.store.clone(),
                deps.event_sink.clone(),
            )),
            approvals,
            estate: Arc::new(services::DefaultEstateService::new(
                deps.store.clone(),
                Some(deps.policy.clone()),
            )),
            capacity: Arc::new(services::DefaultCapacityService::new(
                deps.store.clone(),
                deps.event_sink.clone(),
            )),
            host: host::HostServices::new(
                deps.runtime.clone(),
                deps.store,
                deps.index_db,
                workspace_root,
                num_ctx,
                deps.event_sink,
                deps.policy.clone(),
            ),
        }
    }
}
