# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under both the MIT license found in the
# LICENSE-MIT file in the root directory of this source tree and the Apache
# License, Version 2.0 found in the LICENSE-APACHE file in the root directory
# of this source tree.

def _test_platform_impl(ctx):
    _unused = ctx  # buildifier: disable=unused-variable
    return [
        DefaultInfo(),
        PlatformInfo(
            label = str(ctx.label.raw_target()),
            configuration = ConfigurationInfo(
                constraints = {},
                values = {},
            ),
        ),
    ]

test_platform = rule(
    impl = _test_platform_impl,
    attrs = {},
)

def _test_rule(ctx):
    _unused = ctx  # buildifier: disable=unused-variable
    return [
        DefaultInfo(),
    ]

test_rule = rule(
    impl = _test_rule,
    attrs = {
    },
)
