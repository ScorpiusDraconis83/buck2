/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

use std::fmt::Debug;

use allocative::Allocative;
use buck2_build_api_derive::internal_provider;
use buck2_core::execution_types::execution_platforms::ExecutionPlatformFallback;
use starlark::any::ProvidesStaticType;
use starlark::coerce::Coerce;
use starlark::environment::GlobalsBuilder;
use starlark::values::Freeze;
use starlark::values::FrozenRef;
use starlark::values::FrozenValue;
use starlark::values::Trace;
use starlark::values::Value;
use starlark::values::ValueLifetimeless;
use starlark::values::ValueOf;
use starlark::values::ValueOfUnchecked;
use starlark::values::ValueOfUncheckedGeneric;
use starlark::values::ValueTypedComplex;
use starlark::values::list::ListRef;
use starlark::values::list::ListType;
use starlark::values::none::NoneOr;

use crate as buck2_build_api;
use crate::interpreter::rule_defs::provider::builtin::execution_platform_info::ExecutionPlatformInfo;
use crate::interpreter::rule_defs::provider::builtin::execution_platform_info::FrozenExecutionPlatformInfo;

#[derive(Debug, buck2_error::Error)]
#[buck2(tag = Input)]
enum ExecutionPlatformRegistrationTypeError {
    #[error("expected a list of ExecutionPlatformInfo, got `{0}` (type `{1}`)")]
    ExpectedListOfPlatforms(String, String),
    #[error("expected a ExecutionPlatformInfo, got `{0}` (type `{1}`)")]
    NotAPlatform(String, String),
    #[error(
        "expected an ExecutionPlatformInfo or one of the strings \"error\" or \"use_unspecified\", got `{0}` (type `{1}`)"
    )]
    InvalidFallback(String, String),
}

/// Provider that gives the list of all execution platforms available for this build.
#[internal_provider(info_creator)]
#[derive(Clone, Debug, Trace, Coerce, Freeze, ProvidesStaticType, Allocative)]
#[repr(C)]
pub struct ExecutionPlatformRegistrationInfoGen<V: ValueLifetimeless> {
    platforms: ValueOfUncheckedGeneric<V, Vec<FrozenExecutionPlatformInfo>>,
    // OneOf<ExecutionPlatformInfo, \"error\", \"unspecified\", None>
    // TODO(nga): specify type more precisely.
    fallback: ValueOfUncheckedGeneric<V, FrozenValue>,
}

impl FrozenExecutionPlatformRegistrationInfo {
    // TODO(cjhopman): If we impl this on the non-frozen one, we can check validity when constructed rather than only when used.
    pub fn platforms(
        &self,
    ) -> buck2_error::Result<Vec<FrozenRef<'static, FrozenExecutionPlatformInfo>>> {
        ListRef::from_frozen_value(self.platforms.get())
            .ok_or_else(|| {
                ExecutionPlatformRegistrationTypeError::ExpectedListOfPlatforms(
                    self.platforms.to_value().get().to_repr(),
                    self.platforms.to_value().get().get_type().to_owned(),
                )
            })?
            .iter()
            .map(|v| {
                v.unpack_frozen()
                    .expect("should be frozen")
                    .downcast_frozen_ref::<FrozenExecutionPlatformInfo>()
                    .ok_or_else(|| {
                        ExecutionPlatformRegistrationTypeError::NotAPlatform(
                            v.to_repr(),
                            v.get_type().to_owned(),
                        )
                        .into()
                    })
            })
            .collect::<buck2_error::Result<_>>()
    }

    pub fn fallback(&self) -> buck2_error::Result<ExecutionPlatformFallback> {
        if self.fallback.get().is_none() {
            return Ok(ExecutionPlatformFallback::UseUnspecifiedExec);
        }
        let fallback = self.fallback.get().to_value();
        if let Some(v) = ExecutionPlatformInfo::from_value(fallback) {
            return Ok(ExecutionPlatformFallback::Platform(
                v.to_execution_platform()?,
            ));
        }

        match fallback.unpack_str() {
            Some("error") => Ok(ExecutionPlatformFallback::Error),
            Some("use_unspecified") => Ok(ExecutionPlatformFallback::UseUnspecifiedExec),
            _ => Err(ExecutionPlatformRegistrationTypeError::InvalidFallback(
                fallback.to_repr(),
                fallback.get_type().to_owned(),
            )
            .into()),
        }
    }
}

#[starlark_module]
fn info_creator(globals: &mut GlobalsBuilder) {
    fn ExecutionPlatformRegistrationInfo<'v>(
        #[starlark(require = named)] platforms: ValueOf<
            'v,
            ListType<ValueTypedComplex<'v, ExecutionPlatformInfo<'v>>>,
        >,
        #[starlark(require = named, default = NoneOr::None)] fallback: NoneOr<Value<'v>>,
    ) -> starlark::Result<ExecutionPlatformRegistrationInfo<'v>> {
        Ok(ExecutionPlatformRegistrationInfo {
            platforms: ValueOfUnchecked::new(platforms.value),
            fallback: ValueOfUnchecked::new(match fallback {
                NoneOr::None => Value::new_none(),
                NoneOr::Other(v) => v,
            }),
        })
    }
}
