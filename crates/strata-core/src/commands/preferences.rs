use super::CommandError;
use crate::DocumentError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use strata_pixels::memory::{MemoryLimits, MemoryPool, MemoryStats};

const MIB: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    #[default]
    AskFirst,
    Direct,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AiPreferences {
    pub mode: ExecutionMode,
    #[schemars(range(min = 1, max = 200))]
    pub max_steps: usize,
    #[schemars(range(min = 5, max = 120))]
    pub request_timeout_seconds: u64,
    #[schemars(range(min = 30, max = 1800))]
    pub turn_timeout_seconds: u64,
}

impl Default for AiPreferences {
    fn default() -> Self {
        Self {
            mode: ExecutionMode::AskFirst,
            max_steps: 50,
            request_timeout_seconds: 60,
            turn_timeout_seconds: 300,
        }
    }
}

impl AiPreferences {
    pub fn validate(&self) -> Result<(), CommandError> {
        if !(1..=200).contains(&self.max_steps)
            || !(5..=120).contains(&self.request_timeout_seconds)
            || !(30..=1800).contains(&self.turn_timeout_seconds)
        {
            return Err(CommandError::Invalid(
                "AI steps must be 1..200, request timeout 5..120s and turn timeout 30..1800s"
                    .into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryPreferences {
    #[schemars(range(min = 256, max = 16384))]
    pub cpu_mib: u64,
    #[schemars(range(min = 64, max = 4096))]
    pub gpu_mib: u64,
    #[schemars(range(min = 1024, max = 262144))]
    pub disk_mib: u64,
}

impl MemoryPreferences {
    fn from_pool(pool: &MemoryPool) -> Self {
        let limits = pool.limits();
        Self {
            cpu_mib: limits.cpu_bytes / MIB,
            gpu_mib: limits.gpu_bytes / MIB,
            disk_mib: limits.disk_bytes / MIB,
        }
    }

    fn limits(&self) -> MemoryLimits {
        MemoryLimits {
            cpu_bytes: self.cpu_mib * MIB,
            gpu_bytes: self.gpu_mib * MIB,
            disk_bytes: self.disk_mib * MIB,
        }
    }

    fn validate(&self) -> Result<(), CommandError> {
        if !(256..=16384).contains(&self.cpu_mib)
            || !(64..=4096).contains(&self.gpu_mib)
            || !(1024..=262144).contains(&self.disk_mib)
        {
            return Err(CommandError::Invalid(
                "memory budgets in MiB require CPU 256..16384, GPU 64..4096 and disk 1024..262144"
                    .into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppPreferences {
    pub version: u64,
    pub ai: AiPreferences,
    pub memory: MemoryPreferences,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetPreferences {
    #[serde(default)]
    pub ai: Option<AiPreferences>,
    #[serde(default)]
    pub memory: Option<MemoryPreferences>,
    #[serde(default)]
    pub expected_version: Option<u64>,
}

#[derive(Clone)]
pub struct AppPreferencesStore {
    state: Arc<Mutex<AppPreferences>>,
    pool: MemoryPool,
}

impl AppPreferencesStore {
    pub fn new(pool: MemoryPool) -> Self {
        Self {
            state: Arc::new(Mutex::new(AppPreferences {
                version: 0,
                ai: AiPreferences::default(),
                memory: MemoryPreferences::from_pool(&pool),
            })),
            pool,
        }
    }

    pub(super) fn pool(&self) -> MemoryPool {
        self.pool.clone()
    }

    pub fn get(&self) -> Result<AppPreferences, CommandError> {
        self.state
            .lock()
            .map(|state| state.clone())
            .map_err(|_| CommandError::Invalid("application preferences unavailable".into()))
    }

    pub(super) fn prepare(
        &self,
        params: SetPreferences,
    ) -> Result<(u64, AppPreferences), CommandError> {
        let old = self.get()?;
        if params
            .expected_version
            .is_some_and(|version| version != old.version)
        {
            return Err(CommandError::Stale);
        }
        if params.ai.is_none() && params.memory.is_none() {
            return Err(CommandError::Invalid(
                "provide AI or memory preferences".into(),
            ));
        }
        let mut next = old.clone();
        if let Some(ai) = params.ai {
            ai.validate()?;
            next.ai = ai;
        }
        if let Some(memory) = params.memory {
            memory.validate()?;
            next.memory = memory;
        }
        if next != old {
            next.version = old
                .version
                .checked_add(1)
                .ok_or_else(|| CommandError::Invalid("preferences version exhausted".into()))?;
        }
        Ok((old.version, next))
    }

    pub(super) fn commit(&self, expected: u64, next: AppPreferences) -> Result<(), CommandError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| CommandError::Invalid("application preferences unavailable".into()))?;
        if state.version != expected {
            return Err(CommandError::Stale);
        }
        if state.memory != next.memory {
            self.pool
                .set_limits(next.memory.limits())
                .map_err(DocumentError::from)?;
        }
        *state = next;
        Ok(())
    }
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryStatus {
    pub limits: MemoryLimits,
    pub usage: MemoryStats,
}

impl MemoryStatus {
    pub(super) fn read(pool: &MemoryPool) -> Self {
        Self {
            limits: pool.limits(),
            usage: pool.stats(),
        }
    }
}
