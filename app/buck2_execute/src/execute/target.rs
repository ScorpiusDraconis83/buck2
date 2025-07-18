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

pub trait CommandExecutionTarget: Send + Sync + Debug {
    fn re_action_key(&self) -> String;

    fn re_affinity_key(&self) -> String;

    fn as_proto_action_key(&self) -> buck2_data::ActionKey;

    fn as_proto_action_name(&self) -> buck2_data::ActionName;
}
