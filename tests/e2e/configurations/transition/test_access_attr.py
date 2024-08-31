# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under both the MIT license found in the
# LICENSE-MIT file in the root directory of this source tree and the Apache
# License, Version 2.0 found in the LICENSE-APACHE file in the root directory
# of this source tree.

# pyre-strict


from buck2.tests.e2e_util.api.buck import Buck
from buck2.tests.e2e_util.buck_workspace import buck_test


@buck_test(inplace=True)
async def test_configuration_transition_access_attr(buck: Buck) -> None:
    # Trigger assertions in transition function implementation.
    await buck.cquery(
        "fbcode//buck2/tests/targets/configurations/transition/access_attr:faithful"
    )
