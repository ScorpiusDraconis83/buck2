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
async def test_configuration_rule_unbound(buck: Buck) -> None:
    result = await buck.cquery(
        # platform argument is ignored
        "--target-platforms=fbcode//buck2/tests/targets/configurations/configuration_rule_unbound:p",
        "fbcode//buck2/tests/targets/configurations/configuration_rule_unbound:the-test",
    )
    result.check_returncode()
    # Note configuration is unbound here.
    assert (
        "fbcode//buck2/tests/targets/configurations/configuration_rule_unbound:the-test (<unbound>)\n"
        == result.stdout
    )
