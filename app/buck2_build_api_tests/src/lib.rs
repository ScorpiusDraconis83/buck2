/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

#![feature(error_generic_member_access)]
#![cfg(test)]
#![allow(clippy::bool_assert_comparison)]

mod actions;
mod analysis;
mod artifact_groups;
mod attrs;
mod build;
mod interpreter;
mod nodes;

#[test]
fn init_late_bindings_for_test() {
    #[ctor::ctor]
    fn init() {
        buck2_action_impl::init_late_bindings();
        buck2_analysis::init_late_bindings();
        buck2_anon_target::init_late_bindings();
        buck2_configured::init_late_bindings();
        buck2_events::init_late_bindings();
        buck2_interpreter_for_build::init_late_bindings();
        buck2_build_api::init_late_bindings();
        buck2_transition::init_late_bindings();
    }
}
