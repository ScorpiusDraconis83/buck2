/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

use std::sync::Arc;

use allocative::Allocative;
use derivative::Derivative;
use dice_error::DiceError;
use dice_error::DiceResult;
use dupe::Dupe;

use crate::DiceModern;
use crate::HashMap;
use crate::api::key::InvalidationSourcePriority;
use crate::api::key::Key;
use crate::api::storage_type::StorageType;
use crate::api::user_data::UserComputationData;
use crate::impls::core::state::CoreStateHandle;
use crate::impls::ctx::BaseComputeCtx;
use crate::impls::ctx::SharedLiveTransactionCtx;
use crate::impls::key::DiceKey;
use crate::impls::value::DiceKeyValue;
use crate::impls::value::DiceValidValue;
use crate::impls::value::DiceValidity;
use crate::impls::value::MaybeValidDiceValue;
use crate::versions::VersionNumber;

// TODO fill this more
#[derive(Allocative)]
pub(crate) struct TransactionUpdater {
    dice: Arc<DiceModern>,
    scheduled_changes: Changes,
    user_data: Arc<UserComputationData>,
}

impl TransactionUpdater {
    pub(crate) fn new(dice: Arc<DiceModern>, user_data: Arc<UserComputationData>) -> Self {
        Self {
            dice: dice.dupe(),
            scheduled_changes: Changes::new(dice),
            user_data,
        }
    }

    /// Records a set of `Key`s as changed so that they, and any dependents will
    /// be recomputed on the next set of requests at the next version.
    pub(crate) fn changed<K, I>(&mut self, changed: I) -> DiceResult<()>
    where
        K: Key,
        I: IntoIterator<Item = K> + Send + Sync + 'static,
    {
        changed
            .into_iter()
            .try_for_each(|k| self.scheduled_changes.change(k, ChangeType::Invalidate))
    }

    /// Records a set of `Key`s as changed to a particular value so that any
    /// dependents will be recomputed on the next set of requests. The
    /// `Key`s themselves will be update to the new value such that they
    /// will not need to be recomputed as long as they aren't recorded to be
    /// `changed` again (or invalidated by other means). Calling this method
    /// does not in anyway alter the types of the key such that they
    /// permanently becomes a special "inject value only" key.
    pub(crate) fn changed_to<K, I>(&mut self, changed: I) -> DiceResult<()>
    where
        K: Key,
        I: IntoIterator<Item = (K, K::Value)> + Send + Sync + 'static,
    {
        changed.into_iter().try_for_each(|(k, new_value)| {
            match MaybeValidDiceValue::new(
                Arc::new(DiceKeyValue::<K>::new(new_value)),
                DiceValidity::Valid,
            )
            .into_valid_value()
            {
                Ok(validated_value) => self.scheduled_changes.change(
                    k,
                    ChangeType::UpdateValue(validated_value, K::storage_type()),
                ),
                Err(_) => Err(DiceError::invalid_change(Arc::new(k))),
            }
        })
    }

    /// Commit the changes registered via 'changed' and 'changed_to' to the current newest version.
    pub(crate) async fn commit(self) -> BaseComputeCtx {
        let user_data = self.user_data.dupe();
        let dice = self.dice.dupe();

        let (transaction, guard) = self.commit_to_state().await;

        BaseComputeCtx::new(transaction, user_data, dice, guard)
    }

    /// Commit the changes registered via 'changed' and 'changed_to' to the current newest version,
    /// replacing the user data with the given set
    pub(crate) async fn commit_with_data(self, extra: UserComputationData) -> BaseComputeCtx {
        let dice = self.dice.dupe();

        let (transaction, guard) = self.commit_to_state().await;

        BaseComputeCtx::new(transaction, Arc::new(extra), dice, guard)
    }

    pub(crate) async fn existing_state(&self) -> BaseComputeCtx {
        let v = self.dice.state_handle.current_version().await;
        let guard = ActiveTransactionGuard::new(v, self.dice.state_handle.dupe());
        let (transaction, guard) = self.dice.state_handle.ctx_at_version(v, guard).await;
        BaseComputeCtx::new(transaction, self.user_data.dupe(), self.dice.dupe(), guard)
    }

    pub(crate) fn unstable_take(&self) {
        self.dice.state_handle.unstable_drop_everything()
    }

    async fn commit_to_state(self) -> (SharedLiveTransactionCtx, ActiveTransactionGuard) {
        let v = self
            .dice
            .state_handle
            .update_state(
                self.scheduled_changes
                    .changes
                    .into_iter()
                    .map(|(k, (t, p))| (k, t, p))
                    .collect(),
            )
            .await;

        let guard = ActiveTransactionGuard::new(v, self.dice.state_handle.dupe());

        self.dice.state_handle.ctx_at_version(v, guard).await
    }
}

#[derive(Allocative, Dupe, Clone, Derivative)]
#[derivative(Debug)]
pub(crate) struct ActiveTransactionGuard(Arc<ActiveTransactionGuardInner>);

impl ActiveTransactionGuard {
    pub(crate) fn new(v: VersionNumber, state_handle: CoreStateHandle) -> Self {
        Self(Arc::new(ActiveTransactionGuardInner { v, state_handle }))
    }
}

#[derive(Allocative, Derivative)]
#[derivative(Debug)]
pub(crate) struct ActiveTransactionGuardInner {
    v: VersionNumber,
    #[derivative(Debug = "ignore")]
    state_handle: CoreStateHandle,
}

impl Drop for ActiveTransactionGuardInner {
    fn drop(&mut self) {
        self.state_handle.drop_ctx_at_version(self.v)
    }
}

#[derive(Allocative)]
struct Changes {
    changes: HashMap<DiceKey, (ChangeType, InvalidationSourcePriority)>,
    dice: Arc<DiceModern>,
}

impl Changes {
    pub(crate) fn new(dice: Arc<DiceModern>) -> Self {
        Self {
            changes: HashMap::default(),
            dice,
        }
    }

    pub(crate) fn change<K: Key>(&mut self, key: K, change: ChangeType) -> DiceResult<()> {
        match (change, K::storage_type()) {
            (ChangeType::Invalidate, StorageType::Injected) => {
                Err(DiceError::injected_key_invalidated(Arc::new(key)))
            }
            (change, _) => {
                let key = self.dice.key_index.index_key(key);
                if self
                    .changes
                    .insert(key, (change, K::invalidation_source_priority()))
                    .is_some()
                {
                    Err(DiceError::duplicate(
                        self.dice.key_index.get(key).dupe().downcast::<K>().unwrap(),
                    ))
                } else {
                    Ok(())
                }
            }
        }
    }
}

#[derive(Allocative, Debug)]
pub(crate) enum ChangeType {
    /// Just invalidate the key
    Invalidate,
    /// Update the key to the given value
    UpdateValue(DiceValidValue, StorageType),
    #[cfg(test)]
    /// testing only, set as recheck but not required to rerun
    /// TODO(cjhopman): Delete this, it's really hard to use correctly and
    /// it causes VersionedGraph to need to deal with flows of invalidations
    /// that it otherwise wouldn't.
    /// The right way to get a "soft-dirty", would be to have a dep and do a
    /// normal ChangeType::Invalidate on the dep.
    TestingSoftDirty,
}

#[cfg(test)]
mod tests {

    use allocative::Allocative;
    use assert_matches::assert_matches;
    use async_trait::async_trait;
    use buck2_futures::cancellation::CancellationContext;
    use derive_more::Display;

    use crate::api::computations::DiceComputations;
    use crate::api::data::DiceData;
    use crate::api::key::InvalidationSourcePriority;
    use crate::api::key::Key;
    use crate::impls::dice::DiceModern;
    use crate::impls::key::CowDiceKeyHashed;
    use crate::impls::transaction::ChangeType;
    use crate::versions::VersionNumber;

    #[derive(Allocative, Clone, PartialEq, Eq, Hash, Debug, Display)]
    struct K(usize);

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

        fn equality(_x: &Self::Value, _y: &Self::Value) -> bool {
            unimplemented!("test")
        }
    }

    #[test]
    fn changes_are_recorded() -> anyhow::Result<()> {
        let dice = DiceModern::new(DiceData::new());
        let mut updater = dice.updater();

        updater.changed(vec![K(1), K(2)])?;

        updater.changed_to(vec![(K(3), 3), (K(4), 4)])?;

        assert_matches!(
            updater
                .scheduled_changes
                .changes
                .get(&dice.key_index.index(CowDiceKeyHashed::key(K(1)))),
            Some((ChangeType::Invalidate, InvalidationSourcePriority::Normal))
        );
        assert_matches!(
            updater
                .scheduled_changes
                .changes
                .get(&dice.key_index.index(CowDiceKeyHashed::key(K(2)))),
            Some((ChangeType::Invalidate, InvalidationSourcePriority::Normal))
        );

        assert_matches!(
        updater
            .scheduled_changes
            .changes
            .get(&dice.key_index.index(CowDiceKeyHashed::key(K(3)))),
        Some((ChangeType::UpdateValue(x, _), _)) if *x.downcast_ref::<usize>().unwrap() == 3
            );

        assert_matches!(
        updater
            .scheduled_changes
            .changes
            .get(&dice.key_index.index(CowDiceKeyHashed::key(K(4)))),
        Some((ChangeType::UpdateValue(x, _), _)) if *x.downcast_ref::<usize>().unwrap() == 4
            );

        assert!(updater.changed(vec![K(1)]).is_err());

        Ok(())
    }

    #[tokio::test]
    async fn transaction_versions() -> anyhow::Result<()> {
        let dice = DiceModern::new(DiceData::new());
        let mut updater = dice.updater();

        updater.changed(vec![K(1), K(2)])?;

        let ctx = updater.existing_state().await;
        assert_eq!(ctx.get_version(), VersionNumber::new(0));

        let ctx = updater.commit().await;
        assert_eq!(ctx.get_version(), VersionNumber::new(1));

        Ok(())
    }
}
