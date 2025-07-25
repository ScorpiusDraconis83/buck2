# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is dual-licensed under either the MIT license found in the
# LICENSE-MIT file in the root directory of this source tree or the Apache
# License, Version 2.0 found in the LICENSE-APACHE file in the root directory
# of this source tree. You may select, at your option, one of the
# above-listed licenses.

load("@prelude//:artifacts.bzl", "ArtifactOutputs")

# Resources for transitive deps, shared by C++ and Rust.
ResourceInfo = provider(fields = {
    # A map containing all resources from transitive dependencies.  The keys
    # are rule labels and the values are maps of resource names (the name used
    # to lookup the resource at runtime) and the actual resource artifact.
    "resources": provider_field(dict[Label, dict[str, ArtifactOutputs]]),
})

def gather_resources(
        label: Label,
        resources: dict[str, ArtifactOutputs] = {},
        deps: list[Dependency] = []) -> dict[Label, dict[str, ArtifactOutputs]]:
    """
    Return the resources for this rule and its transitive deps.
    """

    all_resources = {}

    # Resources for self.
    if resources:
        all_resources[label] = resources

    # Merge in resources for deps.
    for dep in deps:
        if ResourceInfo in dep:
            all_resources.update(dep[ResourceInfo].resources)

    return all_resources

def create_resource_db(
        ctx: AnalysisContext,
        name: str,
        binary: Artifact,
        resources: dict[str, ArtifactOutputs]) -> Artifact:
    """
    Generate a resource DB for resources for the given binary, relativized to
    the binary's working directory.
    """

    db = {
        name: cmd_args(resource.default_output, delimiter = "", relative_to = (binary, 1))
        for (name, resource) in resources.items()
    }
    return ctx.actions.write_json(name, db)
