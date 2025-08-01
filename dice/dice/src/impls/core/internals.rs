/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

use std::thread;

use dice_error::result::CancellableResult;
use dice_error::result::CancellationReason;
use gazebo::prelude::SliceExt;

use super::graph::types::RejectedReason;
use crate::api::key::InvalidationSourcePriority;
use crate::api::storage_type::StorageType;
use crate::arc::Arc;
use crate::impls::cache::SharedCache;
use crate::impls::core::graph::introspection::VersionedGraphIntrospectable;
use crate::impls::core::graph::storage::InvalidateKind;
use crate::impls::core::graph::storage::ValueReusable;
use crate::impls::core::graph::storage::VersionedGraph;
use crate::impls::core::graph::types::VersionedGraphKey;
use crate::impls::core::graph::types::VersionedGraphResult;
use crate::impls::core::versions::VersionEpoch;
use crate::impls::core::versions::VersionTracker;
use crate::impls::core::versions::introspection::VersionIntrospectable;
use crate::impls::deps::graph::SeriesParallelDeps;
use crate::impls::key::DiceKey;
use crate::impls::task::dice::DiceTask;
use crate::impls::task::dice::TerminationObserver;
use crate::impls::transaction::ChangeType;
use crate::impls::value::DiceComputedValue;
use crate::impls::value::DiceValidValue;
use crate::impls::value::TrackedInvalidationPaths;
use crate::metrics::Metrics;
use crate::versions::VersionNumber;

/// Core state of DICE, holding the actual graph and version information
#[derive(allocative::Allocative)]
pub(super) struct CoreState {
    version_tracker: VersionTracker,
    graph: VersionedGraph,
    pending_termination_tasks: Vec<DiceTask>,
}

impl CoreState {
    pub(super) fn new() -> Self {
        Self {
            version_tracker: VersionTracker::new(),
            graph: VersionedGraph::new(),
            pending_termination_tasks: Vec::new(),
        }
    }

    pub(super) fn update_state(
        &mut self,
        updates: impl IntoIterator<Item = (DiceKey, ChangeType, InvalidationSourcePriority)>,
    ) -> VersionNumber {
        let version_update = self.version_tracker.write();
        let v = version_update.version();

        let mut changes_recorded = false;
        for (key, change, invalidation_priority) in updates {
            changes_recorded |= self.graph.invalidate(
                VersionedGraphKey::new(v, key),
                match change {
                    ChangeType::Invalidate => InvalidateKind::ForceDirty,
                    ChangeType::UpdateValue(v, s) => InvalidateKind::Update(v, s),
                    #[cfg(test)]
                    ChangeType::TestingSoftDirty => InvalidateKind::Invalidate,
                },
                invalidation_priority,
            );
        }
        if changes_recorded {
            version_update.commit()
        } else {
            version_update.undo()
        }
    }

    pub(super) fn ctx_at_version(&mut self, v: VersionNumber) -> (VersionEpoch, SharedCache) {
        self.version_tracker.at(v)
    }

    pub(super) fn current_version(&self) -> VersionNumber {
        self.version_tracker.current()
    }

    pub(super) fn drop_ctx_at_version(&mut self, v: VersionNumber) {
        if let Some(evicted_cache) = self.version_tracker.drop_at_version(v) {
            self.pending_termination_tasks
                .retain(|task| task.is_pending());
            self.pending_termination_tasks
                .extend(evicted_cache.cancel_pending_tasks());
        }
    }

    pub(super) fn lookup_key(&mut self, key: VersionedGraphKey) -> VersionedGraphResult {
        if self.version_tracker.should_reject(key.v) {
            VersionedGraphResult::Rejected(RejectedReason::RejectedDueToGraphClear)
        } else {
            self.graph.get(key)
        }
    }

    pub(super) fn update_computed(
        &mut self,
        key: VersionedGraphKey,
        epoch: VersionEpoch,
        storage: StorageType,
        value: DiceValidValue,
        reusability: ValueReusable,
        deps: Arc<SeriesParallelDeps>,
        invalidation_paths: TrackedInvalidationPaths,
    ) -> CancellableResult<DiceComputedValue> {
        if !self.version_tracker.is_relevant(key.v, epoch) {
            debug!(msg = "update is rejected due to outdated epoch", k = ?key.k, v = %key.v, v_epoch = %epoch);
            Err(CancellationReason::OutdatedEpoch)
        } else if self.version_tracker.should_reject(key.v) {
            debug!(msg = "update is rejected due to invalid version", k = ?key.k, v = %key.v);
            Err(CancellationReason::Rejected)
        } else {
            debug!(msg = "update graph entry", k = ?key.k, v = %key.v, v_epoch = %epoch);
            Ok(self
                .graph
                .update(key, value, reusability, deps, storage, invalidation_paths)
                .0)
        }
    }

    pub(super) fn get_tasks_pending_cancellation(&mut self) -> Vec<TerminationObserver> {
        self.pending_termination_tasks
            .retain(|task| task.is_pending());

        self.pending_termination_tasks
            .map(|task| task.await_termination())
    }

    pub(super) fn unstable_drop_everything(&mut self) {
        self.version_tracker.clear();

        // Do the actual drop on a different thread because we may have to drop a lot of stuff
        // here.
        let map = std::mem::take(&mut self.graph.nodes);
        thread::Builder::new()
            .name("dice-drop-everything".to_owned())
            .spawn(move || drop(map))
            .expect("failed to spawn thread");
    }

    pub(super) fn metrics(&self) -> Metrics {
        let mut currently_running_key_count = 0;
        let mut active_transaction_count = 0;

        let currently_active = self.version_tracker.currently_active();
        for active in currently_active {
            active_transaction_count += active.0;
            currently_running_key_count += active.1.active_tasks_count();
        }

        Metrics {
            key_count: self.graph.nodes.len(),
            currently_active_key_count: currently_running_key_count,
            active_transaction_count: active_transaction_count as u32, // probably won't support more than u32 transactions
        }
    }

    pub(super) fn introspection(&self) -> (VersionedGraphIntrospectable, VersionIntrospectable) {
        let graph = self.graph.introspect();
        let version_data = self.version_tracker.introspect();

        (graph, version_data)
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use allocative::Allocative;
    use async_trait::async_trait;
    use buck2_futures::cancellation::CancellationContext;
    use buck2_futures::spawner::TokioSpawner;
    use derive_more::Display;
    use dice_error::result::CancellableResult;
    use dice_error::result::CancellationReason;
    use dupe::Dupe;
    use futures::FutureExt;
    use tokio::sync::Semaphore;

    use crate::api::computations::DiceComputations;
    use crate::api::key::InvalidationSourcePriority;
    use crate::api::key::Key;
    use crate::arc::Arc;
    use crate::impls::cache::DiceTaskRef;
    use crate::impls::core::graph::storage::ValueReusable;
    use crate::impls::core::graph::types::VersionedGraphKey;
    use crate::impls::core::internals::CoreState;
    use crate::impls::core::internals::StorageType;
    use crate::impls::core::versions::VersionEpoch;
    use crate::impls::deps::graph::SeriesParallelDeps;
    use crate::impls::key::DiceKey;
    use crate::impls::key::ParentKey;
    use crate::impls::task::dice::DiceTask;
    use crate::impls::task::spawn_dice_task;
    use crate::impls::transaction::ChangeType;
    use crate::impls::value::DiceComputedValue;
    use crate::impls::value::DiceKeyValue;
    use crate::impls::value::DiceValidValue;
    use crate::impls::value::MaybeValidDiceValue;
    use crate::impls::value::TrackedInvalidationPaths;
    use crate::versions::VersionNumber;
    use crate::versions::VersionRanges;

    #[test]
    fn update_state_gets_next_version() {
        let mut core = CoreState::new();

        assert_eq!(
            core.update_state([(
                DiceKey { index: 0 },
                ChangeType::Invalidate,
                InvalidationSourcePriority::Normal
            )]),
            VersionNumber::new(1)
        );

        assert_eq!(
            core.update_state([(
                DiceKey { index: 1 },
                ChangeType::Invalidate,
                InvalidationSourcePriority::Normal
            )]),
            VersionNumber::new(2)
        );
    }

    #[test]
    fn state_ctx_at_version() {
        let mut core = CoreState::new();
        let v = VersionNumber::new(0);

        let (epoch, ctx) = core.ctx_at_version(v);

        let (epoch1, ctx1) = core.ctx_at_version(v);
        assert!(ctx.ptr_eq(&ctx1));
        assert_eq!(epoch, epoch1);

        // if you drop one, there is still reference so getting the same version should give the
        // same instance of ctx
        core.drop_ctx_at_version(v);
        let (epoch2, ctx2) = core.ctx_at_version(v);
        assert!(ctx.ptr_eq(&ctx2));
        assert_eq!(epoch1, epoch2);

        // drop all references, should give a different ctx instance
        core.drop_ctx_at_version(v);
        core.drop_ctx_at_version(v);
        let (another_epoch, another) = core.ctx_at_version(v);
        assert!(!ctx.ptr_eq(&another));
        assert_ne!(another_epoch, epoch);
    }

    #[test]
    fn cancellation_reason() {
        let mut core = CoreState::new();
        fn update(
            core: &mut CoreState,
            epoch: VersionEpoch,
            version: VersionNumber,
        ) -> CancellableResult<DiceComputedValue> {
            core.update_computed(
                VersionedGraphKey::new(version, DiceKey { index: 0 }),
                epoch,
                StorageType::Normal,
                DiceValidValue::testing_new(DiceKeyValue::<K>::new(1)),
                ValueReusable::EqualityBased,
                Arc::new(SeriesParallelDeps::None),
                TrackedInvalidationPaths::clean(),
            )
        }
        let v: VersionNumber = VersionNumber::new(0);
        let (epoch, _ctx) = core.ctx_at_version(v);
        let res = update(&mut core, epoch, v);
        assert_eq!(res.err(), None);

        core.unstable_drop_everything();
        let res = update(&mut core, epoch, v);
        assert_eq!(res.err(), Some(CancellationReason::Rejected));

        core.drop_ctx_at_version(v);
        let res = update(&mut core, epoch, v);
        assert_eq!(res.err(), Some(CancellationReason::OutdatedEpoch));
    }

    async fn make_completed_task(key: DiceKey, val: usize) -> DiceTask {
        let task = spawn_dice_task(key, &TokioSpawner, &(), |handle| {
            async move {
                handle.finished(DiceComputedValue::new(
                    MaybeValidDiceValue::valid(DiceValidValue::testing_new(
                        DiceKeyValue::<K>::new(val),
                    )),
                    Arc::new(VersionRanges::new()),
                    TrackedInvalidationPaths::clean(),
                ));

                Box::new(()) as Box<dyn Any + Send>
            }
            .boxed()
        });

        task.depended_on_by(ParentKey::None).unwrap().await.unwrap();

        task
    }

    async fn make_finished_cancelling_task(key: DiceKey) -> DiceTask {
        let finished_cancelling_tasks = spawn_dice_task(key, &TokioSpawner, &(), |handle| {
            async move {
                let _handle = handle;
                futures::future::pending().await
            }
            .boxed()
        });
        finished_cancelling_tasks.cancel(CancellationReason::ByTest);

        finished_cancelling_tasks.await_termination().await;

        finished_cancelling_tasks
    }

    struct BlockCancel(Arc<Semaphore>);

    impl Drop for BlockCancel {
        fn drop(&mut self) {
            self.0.add_permits(1)
        }
    }

    async fn make_yet_to_cancel_tasks(key: DiceKey) -> (DiceTask, BlockCancel, Arc<Semaphore>) {
        let block_cancel = Arc::new(Semaphore::new(0));
        let arrive_cancel = Arc::new(Semaphore::new(0));
        let yet_to_cancel_tasks = spawn_dice_task(key, &TokioSpawner, &(), |handle| {
            let block_cancel = block_cancel.dupe();
            let arrive_cancel = arrive_cancel.dupe();
            async move {
                handle
                    .cancellation_ctx()
                    .critical_section(|| async move {
                        arrive_cancel.add_permits(1);
                        let _guard = block_cancel.acquire().await.unwrap();
                        arrive_cancel.add_permits(1);
                    })
                    .await;

                Box::new(()) as Box<dyn Any + Send>
            }
            .boxed()
        });
        arrive_cancel.acquire().await.unwrap().forget();

        (
            yet_to_cancel_tasks,
            BlockCancel(block_cancel),
            arrive_cancel,
        )
    }

    async fn make_never_cancellable_task(key: DiceKey) -> DiceTask {
        let arrive_never_cancel = Arc::new(Semaphore::new(0));
        let never_cancel_tasks = spawn_dice_task(key, &TokioSpawner, &(), |handle| {
            let arrive_never_cancel = arrive_never_cancel.dupe();
            async move {
                handle
                    .cancellation_ctx()
                    .critical_section(|| async move {
                        arrive_never_cancel.add_permits(1);
                        futures::future::pending().await
                    })
                    .await
            }
            .boxed()
        });

        arrive_never_cancel.acquire().await.unwrap().forget();

        never_cancel_tasks
    }

    #[tokio::test]
    async fn state_tracks_pending_cancellation() {
        let mut core = CoreState::new();
        let v = VersionNumber::new(0);

        let (_epoch, cache) = core.ctx_at_version(v);

        let completed_task1 = make_completed_task(DiceKey { index: 10 }, 1).await;
        let completed_task2 = make_completed_task(DiceKey { index: 20 }, 2).await;

        let finished_cancelling_tasks1 = make_finished_cancelling_task(DiceKey { index: 30 }).await;
        let finished_cancelling_tasks2 = make_finished_cancelling_task(DiceKey { index: 40 }).await;

        let (yet_to_cancel_tasks1, guard1, arrive_cancel1) =
            make_yet_to_cancel_tasks(DiceKey { index: 50 }).await;
        let (yet_to_cancel_tasks2, guard2, arrive_cancel2) =
            make_yet_to_cancel_tasks(DiceKey { index: 60 }).await;

        let never_cancel_tasks1 = make_never_cancellable_task(DiceKey { index: 100500 }).await;

        cache
            .get(DiceKey { index: 1 })
            .testing_insert(completed_task1);
        cache
            .get(DiceKey { index: 2 })
            .testing_insert(completed_task2);
        cache
            .get(DiceKey { index: 3 })
            .testing_insert(finished_cancelling_tasks1);
        cache
            .get(DiceKey { index: 4 })
            .testing_insert(finished_cancelling_tasks2);
        cache
            .get(DiceKey { index: 5 })
            .testing_insert(yet_to_cancel_tasks1);
        cache
            .get(DiceKey { index: 6 })
            .testing_insert(yet_to_cancel_tasks2);
        cache
            .get(DiceKey { index: 7 })
            .testing_insert(never_cancel_tasks1);

        core.drop_ctx_at_version(v);

        assert_eq!(core.get_tasks_pending_cancellation().len(), 3);

        assert!(matches!(
            cache.get(DiceKey { index: 999 }),
            DiceTaskRef::TransactionCancelled
        ));

        // let the cancellable tasks cancel
        drop(guard1);
        drop(guard2);

        // wait for the cancellable tasks to actually cancel
        let _p = arrive_cancel1.acquire().await.unwrap();
        let _p = arrive_cancel2.acquire().await.unwrap();

        let (_epoch, cache) = core.ctx_at_version(v);

        let never_cancel_tasks2 = make_never_cancellable_task(DiceKey { index: 300 }).await;

        cache
            .get(DiceKey { index: 8 })
            .testing_insert(never_cancel_tasks2);

        core.drop_ctx_at_version(v);

        assert_eq!(core.get_tasks_pending_cancellation().len(), 2);
    }

    #[derive(Allocative, Clone, Debug, Display, Eq, PartialEq, Hash)]
    struct K;

    #[async_trait]
    impl Key for K {
        type Value = usize;

        async fn compute(
            &self,
            _ctx: &mut DiceComputations,
            _cancellations: &CancellationContext,
        ) -> Self::Value {
            unimplemented!("test")
        }

        fn equality(_: &Self::Value, _: &Self::Value) -> bool {
            true
        }
    }
}
