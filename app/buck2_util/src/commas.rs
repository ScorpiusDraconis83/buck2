/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

use std::fmt;

// Returns a function that produces commas every time apart from the first
pub fn commas<W>() -> impl FnMut(&mut W) -> fmt::Result
where
    W: fmt::Write,
{
    let mut with_comma = false;
    move |f: &mut W| -> fmt::Result {
        if with_comma {
            write!(f, ", ")?;
        }
        with_comma = true;
        Ok(())
    }
}
