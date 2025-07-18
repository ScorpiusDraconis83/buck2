/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

import React from 'react';
import clsx from 'clsx';
import styles from './HomepageFeatures.module.css';

const FeatureList = [
  {
    icon: '🚀',
    title: 'Fast',
    description: (
      <>
        Buck2 is faster than Buck.
        If you've got nothing to do, Buck2 is significantly faster.
        If you've got lots to do, Buck2 will start doing it faster and be much closer to the critical path.
      </>
    ),
  },
  {
    icon: '🎯',
    title: 'Reliable',
    description: (
      <>
        Buck2 rules are hermetic by default. Missing dependencies are errors.
        These restrictions apply to both the user-written <code>BUCK</code> files and the language rules.
        Buck2 gives the right result more reliably.
      </>
    ),
  },
  {
    icon: '🧩',
    title: 'Extensible',
    description: (
      <>
        All rules are written in Starlark, with nothing in the core of Buck2 knowing anything about languages.
        That means that Buck2 users can define their own rules as first-class citizens.
      </>
    ),
  },
];

function Feature({icon, title, description}) {
  return (
    <div className={clsx('col col--4')}>
      <div className="text--center" style={{fontSize: '400%'}}>
        {icon}
      </div>
      <div className="text--center padding-horiz--md">
        <h3>{title}</h3>
        <p>{description}</p>
      </div>
    </div>
  );
}

export default function HomepageFeatures() {
  return (
    <section className={styles.features}>
      <div className="container">
        <div className="row">
          {FeatureList.map((props, idx) => (
            <Feature key={idx} {...props} />
          ))}
        </div>
      </div>
    </section>
  );
}
