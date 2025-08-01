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

use async_trait::async_trait;
use buck2_build_api::bxl::calculation::BXL_CALCULATION_IMPL;
use buck2_build_api::bxl::calculation::BxlCalculationDyn;
use buck2_build_api::bxl::calculation::BxlComputeResult;
use buck2_core::deferred::base_deferred_key::BaseDeferredKeyBxl;
use buck2_futures::cancellation::CancellationContext;
use dice::DiceComputations;
use dice::Key;
use dupe::Dupe;
use futures::future::FutureExt;

use crate::bxl;
use crate::bxl::eval::eval;
use crate::bxl::key::BxlKey;

#[derive(Debug)]
struct BxlCalculationImpl;

#[async_trait]
impl BxlCalculationDyn for BxlCalculationImpl {
    async fn eval_bxl(
        &self,
        ctx: &mut DiceComputations<'_>,
        bxl: BaseDeferredKeyBxl,
    ) -> buck2_error::Result<BxlComputeResult> {
        eval_bxl(ctx, BxlKey::from_base_deferred_key_dyn_impl_err(bxl)?)
            .await
            .map_err(|e| e.error)
    }
}

pub(crate) fn init_bxl_calculation_impl() {
    BXL_CALCULATION_IMPL.init(&BxlCalculationImpl);
}

pub(crate) async fn eval_bxl(
    ctx: &mut DiceComputations<'_>,
    bxl: BxlKey,
) -> bxl::eval::Result<BxlComputeResult> {
    match ctx.compute(&internal::BxlComputeKey(bxl)).await {
        Ok(res) => res,
        Err(e) => Err(buck2_error::Error::from(e).into()),
    }
}

#[async_trait]
impl Key for internal::BxlComputeKey {
    type Value = bxl::eval::Result<BxlComputeResult>;

    async fn compute(
        &self,
        ctx: &mut DiceComputations,
        cancellation: &CancellationContext,
    ) -> Self::Value {
        let key = self.0.dupe();
        // TODO(cjhopman): send analysis started/finished events for bxl to support detailed aggregated metrics
        cancellation
            .with_structured_cancellation(|observer| {
                async move {
                    eval(ctx, key, observer)
                        .await
                        .map(|(result, _)| BxlComputeResult(Arc::new(result)))
                }
                .boxed()
            })
            .await
    }

    fn validity(x: &Self::Value) -> bool {
        // Evaluation may have been cancelled at the starlark-eval level...
        // TODO: "synchronous" starlark cancellations should cause "proper" cancellations at the dice layer
        x.is_ok()
    }

    fn equality(_: &Self::Value, _: &Self::Value) -> bool {
        false
    }
}

mod internal {
    use allocative::Allocative;
    use derive_more::Display;
    use dupe::Dupe;

    use crate::bxl::key::BxlKey;

    #[derive(Clone, Dupe, Display, Debug, Eq, Hash, PartialEq, Allocative)]
    pub(crate) struct BxlComputeKey(pub(crate) BxlKey);
}
