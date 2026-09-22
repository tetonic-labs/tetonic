use tetonic_domain::DataClass;

/// Explicit full-payload disclosure permission for a hosted inference binding.
/// This is independent of fabric trust: an enrolled worker grants no cloud access.
/// The composition root must derive this from the effective project/session policy.
#[derive(Debug, Clone, Default)]
pub struct HostedInferencePolicy {
    ceiling: Option<DataClass>,
}

impl HostedInferencePolicy {
    /// Permit full prompts up to this class. Secrets remain local under all ceilings.
    pub fn allow_up_to(ceiling: DataClass) -> Self {
        Self {
            ceiling: Some(ceiling),
        }
    }

    pub fn allows(&self, class: DataClass) -> bool {
        class != DataClass::Secret && self.ceiling.is_some_and(|ceiling| class <= ceiling)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_deny_and_secret_floor() {
        assert!(!HostedInferencePolicy::default().allows(DataClass::Public));
        let policy = HostedInferencePolicy::allow_up_to(DataClass::RepositorySource);
        assert!(policy.allows(DataClass::Public));
        assert!(policy.allows(DataClass::RepositorySource));
        assert!(!policy.allows(DataClass::SensitiveSource));
        assert!(!HostedInferencePolicy::allow_up_to(DataClass::Secret).allows(DataClass::Secret));
    }
}
