/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

use std::collections::HashMap;
use std::collections::VecDeque;
use std::os::fd::AsFd;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use buck2_common::init::ResourceControlConfig;
use buck2_core::soft_error;
use buck2_error::BuckErrorContext;
use buck2_error::internal_error;
use buck2_events::dispatch::EventDispatcher;
use dupe::Dupe;
use nix::dir::Dir;
use nix::fcntl::OFlag;
use nix::fcntl::openat;
use nix::sys::stat::Mode;
use nix::unistd;
use tokio::fs::File;
use tokio::sync::Mutex;

use crate::CommandType;
use crate::cgroup_info::CGroupInfo;
use crate::memory_tracker::MemoryPressureState;
use crate::memory_tracker::MemoryReading;
use crate::memory_tracker::MemoryTrackerHandle;
use crate::memory_tracker::read_memory_current;
use crate::memory_tracker::read_memory_swap_current;
use crate::path::CgroupPath;
use crate::path::CgroupPathBuf;
use crate::pool::CgroupID;
use crate::pool::CgroupPool;
use crate::systemd::CgroupMemoryFile;

/// Memory constraints inherited from ancestor cgroups in the hierarchy.
///
/// When Buck2 operates within a systemd slice or cgroup hierarchy, ancestor cgroups
/// may impose memory limits that affect buck2 processes. This struct captures those
/// constraints by traversing up the cgroup tree to find the nearest non-"max" values.
/// None means that no ancestor cgroup has set a memory limit.
///
/// These constraints are particularly important for:
/// - Understanding the actual resource bounds available to Buck2 processes
/// - Respecting system-level resource policies set by administrators or orchestrators
/// - Most effective in non-container environments, note that in container environments, processes often cannot see memory limits
#[derive(Debug, Clone, Default)]
pub(crate) struct AncestorCgroupConstraints {
    pub memory_max: Option<u64>,
    pub memory_high: Option<u64>,
    pub memory_swap_max: Option<u64>,
    pub memory_swap_high: Option<u64>,
}

impl AncestorCgroupConstraints {
    pub(crate) async fn new() -> buck2_error::Result<Option<Self>> {
        let cgroup_info = CGroupInfo::read()?;
        if let Some(slice_path) = cgroup_info.get_slice() {
            let mut constraints = Self::default();
            let memory_fields = [
                (CgroupMemoryFile::MemoryMax, &mut constraints.memory_max),
                (CgroupMemoryFile::MemoryHigh, &mut constraints.memory_high),
                (
                    CgroupMemoryFile::MemorySwapMax,
                    &mut constraints.memory_swap_max,
                ),
                (
                    CgroupMemoryFile::MemorySwapHigh,
                    &mut constraints.memory_swap_high,
                ),
            ];

            for (memory_file, constraint_field) in memory_fields {
                if let Some(memory_limit) = memory_file
                    .find_ancestor_memory_limit_async(slice_path.as_path())
                    .await?
                {
                    *constraint_field = Some(memory_limit.parse::<u64>()?);
                }
            }

            tracing::trace!("Ancestor cgroup constraints: {constraints:?}");

            Ok(Some(constraints))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActionCgroupResult {
    pub memory_peak: Option<u64>,
    pub swap_peak: Option<u64>,
    pub error: Option<buck2_error::Error>,
    pub was_frozen: bool,
    pub freeze_duration: Option<Duration>,
}

impl ActionCgroupResult {
    fn from_info(cgroup_info: ActionCgroup) -> Self {
        Self {
            memory_peak: Some(cgroup_info.memory_peak),
            swap_peak: Some(cgroup_info.swap_peak),
            error: cgroup_info.error,
            was_frozen: cgroup_info.was_frozen,
            freeze_duration: cgroup_info.freeze_duration,
        }
    }

    pub fn from_error(e: buck2_error::Error) -> Self {
        Self {
            memory_peak: None,
            swap_peak: None,
            error: Some(e),
            was_frozen: false,
            freeze_duration: None,
        }
    }
}

#[derive(Debug)]
struct ActionCgroup {
    path: CgroupPathBuf,
    memory_current_file: File,
    memory_swap_current_file: File,
    memory_initial: u64,
    memory_peak: u64,
    memory_current: u64,
    swap_initial: u64,
    swap_current: u64,
    swap_peak: u64,
    error: Option<buck2_error::Error>,
    was_frozen: bool,
    freeze_start: Option<Instant>,
    freeze_duration: Option<Duration>,
    freeze_file: Option<OwnedFd>,
    command_type: CommandType,
    // memory.current value when this cgroup was frozen. Used to calculate whether we can unfreeze early
    memory_current_when_frozen: Option<u64>,
    action_digest: Option<String>,
    dispatcher: EventDispatcher,
}

pub(crate) struct ActionCgroups {
    enable_freezing: bool,
    active_cgroups: HashMap<CgroupPathBuf, ActionCgroup>,
    frozen_cgroups: VecDeque<CgroupPathBuf>,
    /// The original memory.high value from the cgroup slice of daemon, forkserver and workers cgroups, saved before unsetting it during
    /// memory pressure. Used to restore the limit when all cgroups are unfrozen.
    original_memory_high: Option<String>,
    last_freeze_time: Option<Instant>,
    last_unfreeze_time: Option<Instant>,
    // Total memory of buck2.slice (Which contains daemon, forkserver and workers cgroups) when the last freeze happened.
    // Used to calculate when we should unfreeze cgroups.
    total_memory_during_last_freeze: Option<u64>,
    // Buck2 metadata for telemetry logging purposes
    metadata: HashMap<String, String>,
    #[allow(dead_code)]
    // Constraints for the cgroup hierarchy
    ancestor_cgroup_constraints: Option<AncestorCgroupConstraints>,
}

// Interface between forkserver/executors and ActionCgroups used to report when commands
// are active and return cgroup results for a single command.
pub struct ActionCgroupSession {
    // Pointer to the cgroup pool and not owned by the session. This is mainly used for the session to mark a cgroup as being used
    // when starting a command and then releasing it back to the pool when the command finishes.
    action_cgroups: Arc<Mutex<ActionCgroups>>,
    cgroup_id: CgroupID,
    pub path: CgroupPathBuf,
}

impl ActionCgroupSession {
    pub async fn maybe_create(
        tracker: &Option<MemoryTrackerHandle>,
        dispatcher: EventDispatcher,
        command_type: CommandType,
        action_digest: Option<String>,
        pool: &CgroupPool,
    ) -> buck2_error::Result<Option<Self>> {
        let Some(tracker) = tracker else {
            return Ok(None);
        };

        let (cgroup_id, path) = pool.acquire()?;

        let mut action_cgroups = tracker.action_cgroups.lock().await;

        if let Err(e) = action_cgroups
            .command_started(
                path.clone(),
                dispatcher.dupe(),
                command_type,
                action_digest.clone(),
            )
            .await
        {
            // FIXME(JakobDegen): A proper drop impl on the id would be better than this
            pool.release(cgroup_id);
            return Err(e);
        }

        Ok(Some(ActionCgroupSession {
            action_cgroups: tracker.action_cgroups.dupe(),
            cgroup_id,
            path,
        }))
    }

    pub async fn command_finished(&mut self, pool: &CgroupPool) -> ActionCgroupResult {
        let res = self
            .action_cgroups
            .lock()
            .await
            .command_finished(&self.path);

        pool.release(self.cgroup_id);

        res
    }
}

impl ActionCgroups {
    pub async fn init(
        resource_control_config: &ResourceControlConfig,
    ) -> buck2_error::Result<Self> {
        let enable_freezing = resource_control_config
            .enable_action_freezing
            .unwrap_or(false);

        let ancestor_cgroup_constraints = AncestorCgroupConstraints::new().await?;

        Ok(Self::new(enable_freezing, ancestor_cgroup_constraints))
    }

    pub fn new(
        enable_freezing: bool,
        ancestor_cgroup_constraints: Option<AncestorCgroupConstraints>,
    ) -> Self {
        Self {
            enable_freezing,
            active_cgroups: HashMap::new(),
            frozen_cgroups: VecDeque::new(),
            original_memory_high: None,
            last_freeze_time: None,
            last_unfreeze_time: None,
            total_memory_during_last_freeze: None,
            metadata: buck2_events::metadata::collect(),
            ancestor_cgroup_constraints,
        }
    }

    #[cfg(test)]
    pub(crate) fn testing_new() -> Self {
        Self::new(false, None)
    }

    pub async fn command_started(
        &mut self,
        cgroup_path: CgroupPathBuf,
        dispatcher: EventDispatcher,
        command_type: CommandType,
        action_digest: Option<String>,
    ) -> buck2_error::Result<()> {
        let mut memory_current_file = File::open(cgroup_path.as_path().join("memory.current"))
            .await
            .with_buck_error_context(|| "failed to open memory.current")?;
        let mut memory_swap_current_file =
            File::open(cgroup_path.as_path().join("memory.swap.current"))
                .await
                .with_buck_error_context(|| "failed to open memory.swap.current")?;

        let memory_initial = read_memory_current(&mut memory_current_file).await?;
        let swap_initial = read_memory_swap_current(&mut memory_swap_current_file).await?;

        let existing = self.active_cgroups.insert(
            cgroup_path.clone(),
            ActionCgroup {
                path: cgroup_path,
                memory_current_file,
                memory_swap_current_file,
                memory_initial,
                memory_current: memory_initial,
                memory_peak: 0,
                swap_initial,
                swap_current: swap_initial,
                swap_peak: 0,
                error: None,
                was_frozen: false,
                freeze_start: None,
                freeze_duration: None,
                freeze_file: None,
                command_type,
                memory_current_when_frozen: None,
                action_digest,
                dispatcher,
            },
        );
        if let Some(existing) = existing {
            return Err(internal_error!(
                "cgroup already exists, reused cgroup pool worker? {:?}",
                existing.path,
            ));
        }

        Ok(())
    }

    pub fn command_finished(&mut self, cgroup_path: &CgroupPath) -> ActionCgroupResult {
        if let Some(mut cgroup) = self.active_cgroups.remove(cgroup_path) {
            // Command can finish after freezing a cgroup either because freezing may take some time
            // or because we started freezing after the command finished.
            // In this case we need to unfreeze the cgroup for the next command.
            if let Some(freeze_file) = cgroup.freeze_file.take() {
                unfreeze_cgroup(freeze_file);
                self.frozen_cgroups
                    .retain(|frozen_cgroup_path| *cgroup_path != **frozen_cgroup_path);
            }
            ActionCgroupResult::from_info(cgroup)
        } else {
            ActionCgroupResult::from_error(internal_error!(
                "cgroup not found for {:?}",
                cgroup_path
            ))
        }
    }

    pub async fn update(
        &mut self,
        pressure_state: MemoryPressureState,
        memory_reading: &MemoryReading,
    ) {
        for cgroup in self.active_cgroups.values_mut() {
            // Need to continuously poll memory.current when using a cgroup pool because we can't reset memory.peak
            // in kernels older than 6.12.
            match tokio::try_join!(
                read_memory_current(&mut cgroup.memory_current_file),
                read_memory_swap_current(&mut cgroup.memory_swap_current_file)
            ) {
                Ok((memory_current, swap_current)) => {
                    cgroup.memory_current = memory_current;
                    if let Some(memory_delta) =
                        cgroup.memory_current.checked_sub(cgroup.memory_initial)
                    {
                        cgroup.memory_peak = cgroup.memory_peak.max(memory_delta);
                    }
                    cgroup.swap_current = swap_current;
                    if let Some(swap_delta) = cgroup.swap_current.checked_sub(cgroup.swap_initial) {
                        cgroup.swap_peak = cgroup.swap_peak.max(swap_delta);
                    }
                }
                Err(e) => {
                    cgroup.error = Some(e);
                }
            }
        }

        if pressure_state == MemoryPressureState::AbovePressureLimit {
            self.maybe_freeze(memory_reading);

            // When we are under memory pressure and began freezing actions, we need to reset memory.high to max to prevent throttling unnecessarily.
            // original_memory_high is used to make sure we don't reset more than once
            if self.original_memory_high.is_none() && !self.frozen_cgroups.is_empty() {
                if let Err(e) = self.unset_memory_high() {
                    let _unused = soft_error!("unset_memory_high_error", e);
                }
            }
        }

        self.maybe_unfreeze(pressure_state, memory_reading);

        // When we are not under memory pressure and have no frozen actions, lower memory.high so we can react to memory pressure earlier
        if self.original_memory_high.is_some()
            && self.frozen_cgroups.is_empty()
            && pressure_state == MemoryPressureState::BelowPressureLimit
        {
            if let Err(e) = self.restore_memory_high() {
                let _unused = soft_error!("restore_memory_high_error", e);
            }
        }
    }

    // Currently we freeze the largest cgroup and we keep freezing until only one action is executing.
    // What we really want is to freeze the cgroup that will have the least impact on the critical path,
    // may be better to freeze at random, or freeze the cgroup with the most pressure stalls.
    fn maybe_freeze(&mut self, memory_reading: &MemoryReading) {
        // We freeze at most 1 action every second since memory changes of previous freezes will take a few seconds to take effect
        // This maybe not be enough, but we don't want to wait too long that we encounter OOMs
        if self
            .last_freeze_time
            .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
        {
            return;
        }

        let active_cgroups_count = self.active_cgroups.len();
        let running_actions = active_cgroups_count - self.frozen_cgroups.len();
        // Don't freeze if there's only one action
        if running_actions <= 1 {
            return;
        }

        let largest_cgroup = self
            .active_cgroups
            .values_mut()
            .filter(|cgroup| cgroup.freeze_file.is_none())
            .max_by_key(|cgroup| cgroup.memory_current);

        if let Some(cgroup) = largest_cgroup {
            if self.enable_freezing {
                match freeze_cgroup(&cgroup.path) {
                    Ok(freeze_file) => {
                        tracing::debug!(
                            "Froze action: {:?} (type: {:?})",
                            cgroup.path,
                            cgroup.command_type
                        );

                        cgroup.freeze_start = Some(Instant::now());
                        cgroup.freeze_file = Some(freeze_file);
                        cgroup.memory_current_when_frozen = Some(cgroup.memory_current);
                        self.total_memory_during_last_freeze =
                            Some(memory_reading.buck2_slice_memory_current);
                        self.last_freeze_time = Some(Instant::now());
                        self.frozen_cgroups.push_back(cgroup.path.clone());

                        emit_resource_control_event(
                            &cgroup.dispatcher,
                            memory_reading,
                            buck2_data::ResourceControlKind::Freeze,
                            cgroup,
                            self.frozen_cgroups.len() as u64,
                            active_cgroups_count as u64,
                            &self.metadata,
                            &self.ancestor_cgroup_constraints,
                        );
                    }
                    Err(e) => {
                        cgroup.error = Some(e);
                    }
                }
            }
        }
    }

    fn maybe_unfreeze(
        &mut self,
        pressure_state: MemoryPressureState,
        memory_reading: &MemoryReading,
    ) {
        // We unfreeze at most 1 action every 3 seconds since memory changes of previous freezes will take several seconds
        // to take effect. This ensures that we don't unfreeze too quickly
        if self
            .last_unfreeze_time
            .is_some_and(|t| t.elapsed() < Duration::from_secs(3))
        {
            return;
        }

        let active_cgroups_count = self.active_cgroups.len();
        let running_actions = active_cgroups_count - self.frozen_cgroups.len();
        let should_unfreeze = self.frozen_cgroups.front().is_some_and(|cgroup_path| {
            let frozen_cgroup = self
                .active_cgroups
                .get(cgroup_path)
                .expect("frozen cgroups must be in active cgroups");

            let memory_when_frozen = frozen_cgroup
                .memory_current_when_frozen
                .expect("frozen cgroups should have memory current set");
            let total_memory_during_last_freeze = self
                .total_memory_during_last_freeze
                .expect("total memory during last freeze should be set");

            // If the current memory use is less than the sum of memory of cgroup at the time it was frozen +
            // total memory when we last froze an action, we can start unfreezing earlier
            let can_unfreeze_early = memory_reading.buck2_slice_memory_current + memory_when_frozen
                < total_memory_during_last_freeze;

            // If we can unfreeze early or if there are no actions running, start unfreezing.
            // We unfreeze even if there are no actions running to prevent the build from getting stuck
            (pressure_state == MemoryPressureState::BelowPressureLimit && can_unfreeze_early)
                || running_actions == 0
        });

        // TODO: Should explore unfreezing what's on the critical path or what's using the least memory
        let cgroup_to_unfreeze = if should_unfreeze {
            self.frozen_cgroups.pop_front()
        } else {
            None
        };

        if let Some(frozen_cgroup_path) = cgroup_to_unfreeze {
            let frozen_cgroup = self
                .active_cgroups
                .get_mut(&frozen_cgroup_path)
                .expect("frozen cgroups must be in active cgroups");

            tracing::debug!(
                "Unfreezing action: {:?} (type: {:?})",
                frozen_cgroup_path,
                frozen_cgroup.command_type
            );

            frozen_cgroup.was_frozen = true;

            let freeze_elapsed = frozen_cgroup
                .freeze_start
                .expect("freeze start must exist if we are unfreezing")
                .elapsed();

            frozen_cgroup.freeze_duration = Some(match frozen_cgroup.freeze_duration {
                Some(duration) => duration + freeze_elapsed,
                None => freeze_elapsed,
            });

            let freeze_file = frozen_cgroup
                .freeze_file
                .take()
                .expect("frozen cgroups must have a freeze file");
            unfreeze_cgroup(freeze_file);
            self.last_unfreeze_time = Some(Instant::now());

            emit_resource_control_event(
                &frozen_cgroup.dispatcher,
                memory_reading,
                buck2_data::ResourceControlKind::Unfreeze,
                frozen_cgroup,
                self.frozen_cgroups.len() as u64,
                active_cgroups_count as u64,
                &self.metadata,
                &self.ancestor_cgroup_constraints,
            );
        }
    }

    /// Removes the memory.high limit from the cgroup slice
    fn unset_memory_high(&mut self) -> buck2_error::Result<()> {
        let cgroup_info = CGroupInfo::read()?;
        let slice_path = cgroup_info.get_slice().ok_or(buck2_error::buck2_error!(
            buck2_error::ErrorTag::Environment,
            "The closest slice of the daemon cgroup does not exist"
        ))?;
        let memory_high = CgroupMemoryFile::MemoryHigh.read(slice_path.as_path())?;
        self.original_memory_high = Some(memory_high);
        CgroupMemoryFile::MemoryHigh.set(slice_path.as_path(), "max")?;

        Ok(())
    }

    /// Restores the original memory.high limit to the cgroup slice
    fn restore_memory_high(&mut self) -> buck2_error::Result<()> {
        if let Some(original_memory_high) = self.original_memory_high.take() {
            let cgroup_info = CGroupInfo::read()?;
            let slice_path = cgroup_info.get_slice().ok_or(buck2_error::buck2_error!(
                buck2_error::ErrorTag::Environment,
                "The closest slice of the daemon cgroup does not exist"
            ))?;
            CgroupMemoryFile::MemoryHigh.set(slice_path.as_path(), &original_memory_high)?;
        }
        Ok(())
    }
}

fn emit_resource_control_event(
    dispatcher: &EventDispatcher,
    memory_reading: &MemoryReading,
    kind: buck2_data::ResourceControlKind,
    cgroup: &ActionCgroup,
    frozen_cgroup_count: u64,
    active_cgroup_count: u64,
    metadata: &HashMap<String, String>,
    ancestor_cgroup_constraints: &Option<AncestorCgroupConstraints>,
) {
    dispatcher.instant_event(buck2_data::ResourceControlEvents {
        uuid: dispatcher.trace_id().to_string(),
        action_digest: cgroup.action_digest.clone().unwrap_or_default(),

        command: cgroup.command_type.to_string(),
        kind: kind.into(),

        event_time: Some(SystemTime::now().into()),

        memory_current: memory_reading.buck2_slice_memory_current,
        memory_swap_current: memory_reading.buck2_slice_memory_swap_current,
        memory_pressure: memory_reading.buck2_slice_memory_pressure,

        daemon_memory_current: memory_reading.daemon_memory_current,
        daemon_swap_current: memory_reading.daemon_memory_swap_current,

        cgroup_memory_current: cgroup.memory_current,
        cgroup_memory_peak: cgroup.memory_peak,

        cgroup_swap_current: cgroup.swap_current,
        cgroup_swap_peak: cgroup.swap_peak,

        frozen_cgroup_count,
        active_cgroup_count,

        metadata: metadata.clone(),

        ancestor_cgroup_constraints: ancestor_cgroup_constraints.as_ref().map(|constraints| {
            buck2_data::AncestorCgroupConstraints {
                memory_max: constraints.memory_max,
                memory_high: constraints.memory_high,
                memory_swap_max: constraints.memory_swap_max,
                memory_swap_high: constraints.memory_swap_high,
            }
        }),
    });
}

// From https://docs.kernel.org/admin-guide/cgroup-v2.html
// Writing “1” to 'cgroup.freeze' causes freezing of the cgroup and all descendant cgroups. This means
// that all belonging processes will be stopped and will not run until the cgroup will be explicitly unfrozen.
fn freeze_cgroup(cgroup_path: &CgroupPath) -> buck2_error::Result<OwnedFd> {
    // Using nix APIs in case more precise control is needed for control file IO.
    let dir: Dir = Dir::open(cgroup_path.as_path(), OFlag::O_CLOEXEC, Mode::empty())
        .map_err(|e| buck2_error::Error::from(e).context("Failed to open cgroup directory"))?;
    let freeze_file: OwnedFd = openat(
        &dir,
        "cgroup.freeze",
        OFlag::O_CLOEXEC | OFlag::O_RDWR,
        Mode::empty(),
    )
    .map_err(|e| buck2_error::Error::from(e).context("Failed to open cgroup.freeze file"))?;

    static ONE: [u8; 1] = ["1".as_bytes()[0]];
    let bytes_written = unistd::write(freeze_file.as_fd(), &ONE)?;
    if bytes_written != 1 {
        return Err(internal_error!("Failed to write to cgroup.freeze file"));
    }
    Ok(freeze_file)
}

fn unfreeze_cgroup(freeze_file: OwnedFd) {
    fn unfreeze(freeze_file: OwnedFd) -> buck2_error::Result<()> {
        static ZERO: [u8; 1] = ["0".as_bytes()[0]];
        let bytes_written = unistd::write(freeze_file.as_fd(), &ZERO)?;
        if bytes_written != 1 {
            return Err(internal_error!("Failed to write to cgroup.freeze file"));
        }
        Ok(())
    }

    if let Err(e) = unfreeze(freeze_file) {
        let _unused = soft_error!("cgroup_unfreeze_failed", e);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use buck2_core::fs::paths::abs_norm_path::AbsNormPathBuf;

    use super::*;

    #[tokio::test]
    async fn test_peak_memory_and_swap() -> buck2_error::Result<()> {
        let cgroup_1 = tempfile::tempdir()?;
        let cgroup_2 = tempfile::tempdir()?;
        let cgroup_1 =
            CgroupPathBuf::new(AbsNormPathBuf::unchecked_new(cgroup_1.path().to_path_buf()));
        let cgroup_2 =
            CgroupPathBuf::new(AbsNormPathBuf::unchecked_new(cgroup_2.path().to_path_buf()));

        fs::write(cgroup_1.as_path().join("memory.current"), "10")?;
        fs::write(cgroup_2.as_path().join("memory.current"), "10")?;
        fs::write(cgroup_1.as_path().join("memory.swap.current"), "15")?;
        fs::write(cgroup_2.as_path().join("memory.swap.current"), "15")?;

        let mut action_cgroups = ActionCgroups::new(false, None);
        action_cgroups
            .command_started(
                cgroup_1.clone(),
                EventDispatcher::null(),
                CommandType::Build,
                Some("action_1".to_owned()),
            )
            .await?;
        action_cgroups
            .command_started(
                cgroup_2.clone(),
                EventDispatcher::null(),
                CommandType::Build,
                Some("action_2".to_owned()),
            )
            .await?;

        fs::write(cgroup_1.as_path().join("memory.current").as_path(), "20")?;
        fs::write(cgroup_2.as_path().join("memory.current").as_path(), "5")?;
        fs::write(cgroup_1.as_path().join("memory.swap.current"), "18")?;
        fs::write(cgroup_2.as_path().join("memory.swap.current"), "6")?;

        let memory_reading = MemoryReading {
            buck2_slice_memory_current: 10000,
            buck2_slice_memory_swap_current: 0,
            buck2_slice_memory_pressure: 12,
            daemon_memory_current: 8000,
            daemon_memory_swap_current: 0,
        };
        action_cgroups
            .update(MemoryPressureState::AbovePressureLimit, &memory_reading)
            .await;

        let cgroup_1_res = action_cgroups.command_finished(&cgroup_1);
        let cgroup_2_res = action_cgroups.command_finished(&cgroup_2);
        assert_eq!(cgroup_1_res.memory_peak, Some(10));
        assert_eq!(cgroup_2_res.memory_peak, Some(0));
        assert_eq!(cgroup_1_res.swap_peak, Some(3));
        assert_eq!(cgroup_2_res.swap_peak, Some(0));

        Ok(())
    }

    #[tokio::test]
    async fn test_freeze() -> buck2_error::Result<()> {
        let cgroup_1 = tempfile::tempdir()?;
        let cgroup_2 = tempfile::tempdir()?;
        let cgroup_1 =
            CgroupPathBuf::new(AbsNormPathBuf::unchecked_new(cgroup_1.path().to_path_buf()));
        let cgroup_2 =
            CgroupPathBuf::new(AbsNormPathBuf::unchecked_new(cgroup_2.path().to_path_buf()));

        fs::write(cgroup_1.as_path().join("memory.current"), "0")?;
        fs::write(cgroup_2.as_path().join("memory.current"), "0")?;
        fs::write(cgroup_1.as_path().join("cgroup.freeze"), "0")?;
        fs::write(cgroup_2.as_path().join("cgroup.freeze"), "0")?;
        fs::write(cgroup_1.as_path().join("memory.swap.current"), "0")?;
        fs::write(cgroup_2.as_path().join("memory.swap.current"), "0")?;

        let mut action_cgroups = ActionCgroups::new(true, None);
        action_cgroups
            .command_started(
                cgroup_1.clone(),
                EventDispatcher::null(),
                CommandType::Build,
                None,
            )
            .await?;
        action_cgroups
            .command_started(
                cgroup_2.clone(),
                EventDispatcher::null(),
                CommandType::Build,
                None,
            )
            .await?;

        fs::write(cgroup_1.as_path().join("memory.current"), "1")?;
        fs::write(cgroup_2.as_path().join("memory.current"), "2")?;

        let memory_reading = MemoryReading {
            buck2_slice_memory_current: 10000,
            buck2_slice_memory_swap_current: 0,
            buck2_slice_memory_pressure: 12,
            daemon_memory_current: 8000,
            daemon_memory_swap_current: 0,
        };
        action_cgroups
            .update(MemoryPressureState::AbovePressureLimit, &memory_reading)
            .await;

        let cgroup_1_res = action_cgroups.command_finished(&cgroup_1);
        assert_eq!(cgroup_1_res.was_frozen, false);

        let memory_reading_2 = MemoryReading {
            buck2_slice_memory_current: 0,
            buck2_slice_memory_swap_current: 0,
            buck2_slice_memory_pressure: 0,
            daemon_memory_current: 0,
            daemon_memory_swap_current: 0,
        };
        action_cgroups
            .update(MemoryPressureState::BelowPressureLimit, &memory_reading_2)
            .await;

        let cgroup_2_res = action_cgroups.command_finished(&cgroup_2);
        assert_eq!(cgroup_2_res.was_frozen, true);

        Ok(())
    }
}
