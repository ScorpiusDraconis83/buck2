/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

use buck2_common::cas_digest::CasDigest;
use buck2_common::cas_digest::CasDigestConfig;
use buck2_common::cas_digest::CasDigestKind;
use buck2_common::cas_digest::TrackedCasDigest;

pub struct DepFileDigestKind {
    _private: (),
}

impl CasDigestKind for DepFileDigestKind {
    fn empty_digest(_config: CasDigestConfig) -> Option<TrackedCasDigest<Self>> {
        // No reason to optimize "empty" actions.
        None
    }
}

pub type DepFileDigest = CasDigest<DepFileDigestKind>;
pub type TrackedDepFileDigest = TrackedCasDigest<DepFileDigestKind>;
