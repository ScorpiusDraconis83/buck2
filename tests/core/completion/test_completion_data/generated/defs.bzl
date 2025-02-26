# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under both the MIT license found in the
# LICENSE-MIT file in the root directory of this source tree and the Apache
# License, Version 2.0 found in the LICENSE-APACHE file in the root directory
# of this source tree.

def _impl(_ctx):
    return []

no_labels = rule(
    impl = _impl,
    attrs = {},
)

with_labels = rule(
    impl = _impl,
    attrs = {
        "labels": attrs.list(attrs.string()),
    },
)

with_weird_labels = rule(
    impl = _impl,
    attrs = {
        "labels": attrs.string(),
    },
)
