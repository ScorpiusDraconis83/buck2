/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

use std::collections::HashSet;

use buck2_core::provider::label::ConfiguredProvidersLabel;
use buck2_events::dispatch::EventDispatcher;

/// Common code executed in the end of command to produce `CommandEnd`.
pub fn command_end<R, D>(result: &buck2_error::Result<R>, data: D) -> buck2_data::CommandEnd
where
    D: Into<buck2_data::command_end::Data>,
{
    command_end_ext(result, data.into(), |_| None)
}

pub fn command_end_ext<R, D, F>(
    result: &buck2_error::Result<R>,
    data: D,
    build_result: F,
) -> buck2_data::CommandEnd
where
    F: FnOnce(&R) -> Option<buck2_data::BuildResult>,
    D: Into<buck2_data::command_end::Data>,
{
    buck2_data::CommandEnd {
        data: Some(data.into()),
        build_result: result.as_ref().ok().and_then(build_result),
        ..Default::default()
    }
}

/// Common code to send TargetCfg event after command execution.
pub fn send_target_cfg_event(
    event_dispatcher: &EventDispatcher,
    conf_labels: impl IntoIterator<Item = &ConfiguredProvidersLabel>,
    target_cfg: &Option<buck2_cli_proto::TargetCfg>,
) {
    let mut target_platforms = HashSet::new();
    for conf in conf_labels {
        // cfg can be unbound
        if let Ok(label) = conf.cfg().label() {
            if !target_platforms.contains(label) {
                target_platforms.insert(label.to_owned());
            }
        }
    }

    let cli_modifiers = target_cfg
        .as_ref()
        .map(|cfg| cfg.cli_modifiers.clone())
        .unwrap_or_default();

    event_dispatcher.instant_event(buck2_data::TargetCfg {
        target_platforms: target_platforms.into_iter().collect(),
        cli_modifiers,
    });
}
