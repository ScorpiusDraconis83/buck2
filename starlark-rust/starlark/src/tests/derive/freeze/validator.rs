/*
 * Copyright 2018 The Starlark in Rust Authors.
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     https://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use crate as starlark;
use crate::values::Freeze;
use crate::values::FreezeError;
use crate::values::Freezer;
use crate::values::FrozenHeap;

#[derive(Freeze)]
#[freeze(validator = check_true)]
struct Test {
    field: bool,
}

fn check_true(test: &Test) -> anyhow::Result<()> {
    if !test.field {
        return Err(anyhow::anyhow!("Err"));
    }

    Ok(())
}

#[test]
fn test_ok() -> anyhow::Result<()> {
    let t = Test { field: true };
    let frozen_heap = FrozenHeap::new();
    let freezer = Freezer::new(&frozen_heap);
    t.freeze(&freezer)?;
    Ok(())
}

#[test]
fn test_fail() -> anyhow::Result<()> {
    let t = Test { field: false };
    let frozen_heap = FrozenHeap::new();
    let freezer = Freezer::new(&frozen_heap);
    assert!(t.freeze(&freezer).is_err());
    Ok(())
}
