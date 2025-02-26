# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under both the MIT license found in the
# LICENSE-MIT file in the root directory of this source tree and the Apache
# License, Version 2.0 found in the LICENSE-APACHE file in the root directory
# of this source tree.

# TODO(cjhopman): This was generated by scripts/hacks/rules_shim_with_docs.py,
# but should be manually edited going forward. There may be some errors in
# the generated docs, and so those should be verified to be accurate and
# well-formatted (and then delete this TODO)

load("@prelude//decls:test_common.bzl", "test_common")
load(":common.bzl", "buck", "prelude_rule")
load(":cxx_common.bzl", "cxx_common")
load(":go_common.bzl", "go_common")
load(":re_test_common.bzl", "re_test_common")

BuildMode = ["executable", "c_shared", "c_archive"]

GoTestCoverStepMode = ["set", "count", "atomic", "none"]

go_binary = prelude_rule(
    name = "go_binary",
    docs = """
        A go\\_binary() rule builds a native executable from the supplied set of Go source files
         and dependencies. The files supplied are expected to be in the main package, implicitly.
    """,
    examples = """
        For more examples, check out our [integration tests](https://github.com/facebook/buck/tree/dev/test/com/facebook/buck/features/go/testdata).


        ```

        go_binary(
          name='greet',
          srcs=[
            'main.go',
          ],
          deps=[
            ':greeting',
          ],
        )

        go_library(
          name='greeting',
          srcs=[
            'greeting.go',
          ],
          deps=[
            ':join',
          ],
        )

        go_library(
          name='join',
          srcs=[
            'join.go',
          ],
        )

        ```
    """,
    further = None,
    attrs = (
        # @unsorted-dict-items
        go_common.package_name_arg() |
        go_common.srcs_arg() |
        go_common.deps_arg() |
        go_common.link_style_arg() |
        go_common.link_mode_arg() |
        go_common.compiler_flags_arg() |
        go_common.assembler_flags_arg() |
        go_common.linker_flags_arg() |
        go_common.external_linker_flags_arg() |
        go_common.embedcfg_arg() |
        go_common.package_root_arg() |
        go_common.cgo_enabled_arg() |
        go_common.race_arg() |
        go_common.asan_arg() |
        go_common.build_tags_arg() |
        cxx_common.headers_arg() |
        cxx_common.header_namespace_arg() |
        go_common.cxx_preprocessor_flags_arg() |
        go_common.cxx_compiler_flags_arg() |
        {
            "resources": attrs.list(attrs.source(), default = [], doc = """
                Static files to be symlinked into the working directory of the test. You can access these in your
                 by opening the files as relative paths, e.g. `ioutil.ReadFile("testdata/input")`.
            """),
            "contacts": attrs.list(attrs.string(), default = []),
            "default_host_platform": attrs.option(attrs.configuration_label(), default = None),
            "labels": attrs.list(attrs.string(), default = []),
            "licenses": attrs.list(attrs.source(), default = []),
            "platform": attrs.option(attrs.string(), default = None),
        }
    ),
)

go_exported_library = prelude_rule(
    name = "go_exported_library",
    docs = """
        A go\\_exported\\_library() rule builds a C library from the supplied set of Go source files
         and dependencies. This is done via `-buildmode` flag and "//export" annotations in the code.
    """,
    examples = """
        For more examples, check out our [integration tests](https://github.com/facebook/buck/tree/dev/test/com/facebook/buck/features/go/testdata).


        ```

        go_exported_library(
            name = "shared",
            srcs = ["main.go"],
            build_mode = "c_shared",
            compiler_flags = ["-shared"],
            deps = [":example"],
        )

        go_library(
            name = "example",
            package_name = "cgo",
            srcs = [
                "export-to-c.go",  # file with //export annotations
            ],
            compiler_flags = [],
            headers = [],
        )

        cxx_genrule(
            name = "cgo_exported_headers",
            out = "includes",
            cmd = (
                "mkdir -p $OUT && " +
                "cat `dirname $(location :shared)`/includes/*.h > $OUT/_cgo_export.h"
            ),
        )

        prebuilt_cxx_library(
            name = "cxx_so_with_header",
            header_dirs = [":cgo_exported_headers"],
            shared_lib = ":shared",
        )

        ```
    """,
    further = None,
    attrs = (
        # @unsorted-dict-items
        go_common.package_name_arg() |
        go_common.srcs_arg() |
        go_common.deps_arg() |
        {
            "build_mode": attrs.enum(BuildMode, doc = """
                Determines the build mode (equivalent of `-buildmode`). Can be
                 one of the following values: `c_archive`, `c_shared`.
                 This argument is valid only if at there is at least one `cgo_library declared in deps.
                 In addition you should make sure that `-shared` flag is added to `compiler_flags`
                 and go version under `go.goroot` is compiled with that flag present in:
                 `gcflags`, `ldflags` and `asmflags``
            """),
        } |
        cxx_common.headers_arg() |
        cxx_common.header_namespace_arg() |
        go_common.cxx_preprocessor_flags_arg() |
        go_common.cxx_compiler_flags_arg() |
        go_common.link_style_arg() |
        go_common.link_mode_arg() |
        go_common.compiler_flags_arg() |
        go_common.assembler_flags_arg() |
        go_common.linker_flags_arg() |
        go_common.external_linker_flags_arg() |
        go_common.package_root_arg() |
        go_common.cgo_enabled_arg() |
        go_common.race_arg() |
        go_common.asan_arg() |
        go_common.build_tags_arg() |
        go_common.generate_exported_header() |
        {
            "resources": attrs.list(attrs.source(), default = [], doc = """
                Static files to be symlinked into the working directory of the test. You can access these in your
                 by opening the files as relative paths, e.g. `ioutil.ReadFile("testdata/input")`.
            """),
            "contacts": attrs.list(attrs.string(), default = []),
            "default_host_platform": attrs.option(attrs.configuration_label(), default = None),
            "embedcfg": attrs.option(attrs.source(), default = None),
            "labels": attrs.list(attrs.string(), default = []),
            "licenses": attrs.list(attrs.source(), default = []),
            "platform": attrs.option(attrs.string(), default = None),
        }
    ),
)

go_library = prelude_rule(
    name = "go_library",
    docs = """
        A go\\_library() rule builds a native library from the supplied set of Go source files
         and dependencies.
    """,
    examples = """
        For more examples, check out our [integration tests](https://github.com/facebook/buck/tree/dev/test/com/facebook/buck/features/go/testdata).


        ```

        go_library(
          name='greeting',
          srcs=[
            'greeting.go',
          ],
          deps=[
            ':join',
          ],
        )

        ```
    """,
    further = None,
    attrs = (
        # @unsorted-dict-items
        go_common.srcs_arg() |
        go_common.package_name_arg() |
        go_common.deps_arg() |
        go_common.compiler_flags_arg() |
        go_common.assembler_flags_arg() |
        go_common.embedcfg_arg() |
        go_common.package_root_arg() |
        go_common.override_cgo_enabled_arg() |
        cxx_common.headers_arg() |
        cxx_common.header_namespace_arg() |
        go_common.cxx_preprocessor_flags_arg() |
        go_common.cxx_compiler_flags_arg() |
        go_common.external_linker_flags_arg() |
        go_common.link_style_arg() |
        go_common.generate_exported_header() |
        {
            "contacts": attrs.list(attrs.string(), default = []),
            "default_host_platform": attrs.option(attrs.configuration_label(), default = None),
            "labels": attrs.list(attrs.string(), default = []),
            "licenses": attrs.list(attrs.source(), default = []),
        }
    ),
)

go_test = prelude_rule(
    name = "go_test",
    docs = """
        A `go_test()` rule builds a native binary from the
         specified Go source and resource files—and a generated main file. It's
         similar to the `go test` command.


         If your test requires static files you should specify these in
         the **resources** argument. If you do not specify these
         files, they won't be available when your test runs.
    """,
    examples = """
        For more examples, check out our [integration tests](https://github.com/facebook/buck/tree/dev/test/com/facebook/buck/features/go/testdata).


        ```

        go_library(
          name='greeting',
          srcs=[
            'greeting.go',
          ],
          deps=[
            ':join',
          ],
        )

        go_test(
          name='greeting-test',
          srcs=[
            'greeting_ext_test.go',
          ],
          deps=[
            ':greeting'
          ],
        )

        go_test(
          name='greeting-internal-test',
          package_name='greeting',
          srcs=[
            'greeting.go',
            'greeting_test.go',
          ],
          deps=[
            ':join',
          ],
        )

        # Or

        go_test(
          name='greeting-better-internal-test',
          srcs=['greeting_test.go'],
          library=':greeting',
        )


        ```
    """,
    further = None,
    attrs = (
        # @unsorted-dict-items
        buck.inject_test_env_arg() |
        go_common.srcs_arg() |
        {
            "library": attrs.option(attrs.dep(), default = None, doc = """
                Specify the library that this internal test is testing. This will copy the `srcs`,
                 `package_name` and `deps` from the target specified so you don't have
                 to duplicate them.
            """),
            "package_name": attrs.option(attrs.string(), default = None, doc = """
                Sets the full name of the test package being compiled. This defaults to the path from the buck
                 root with "\\_test" appended. (e.g. given a ./.buckconfig, a rule in ./a/b/BUCK defaults to package "a/b\\_test")

                 Note: if you want to test packages internally (i.e. same package name), use the `library`
                 parameter instead of setting `package_name` to include the tested source files.
            """),
            "coverage_mode": attrs.option(attrs.enum(GoTestCoverStepMode), default = None, doc = """
                Test coverage functionality will be included in the executable. Modes: set, count, atomic
            """),
        } |
        go_common.deps_arg() |
        go_common.link_style_arg() |
        go_common.link_mode_arg() |
        go_common.compiler_flags_arg() |
        go_common.assembler_flags_arg() |
        go_common.linker_flags_arg() |
        go_common.external_linker_flags_arg() |
        go_common.embedcfg_arg() |
        go_common.package_root_arg() |
        go_common.cgo_enabled_arg() |
        go_common.race_arg() |
        go_common.asan_arg() |
        go_common.build_tags_arg() |
        cxx_common.headers_arg() |
        cxx_common.header_namespace_arg() |
        go_common.cxx_preprocessor_flags_arg() |
        go_common.cxx_compiler_flags_arg() |
        {
            "resources": attrs.list(attrs.source(), default = [], doc = """
                Static files that are symlinked into the working directory of the
                 test. You can access these files in your test by opening them using
                 relative paths, such as `ioutil.ReadFile("testdata/input")`.
            """),
        } |
        buck.test_label_arg() |
        buck.test_rule_timeout_ms() |
        {
            "env": attrs.dict(key = attrs.string(), value = attrs.arg(), sorted = False, default = {}, doc = """
                A map of environment variables and values to set when running the test.
            """),
        } |
        buck.run_test_separately_arg(run_test_separately_type = attrs.bool(default = False)) |
        {
            "contacts": attrs.list(attrs.string(), default = []),
            "default_host_platform": attrs.option(attrs.configuration_label(), default = None),
            "licenses": attrs.list(attrs.source(), default = []),
            "platform": attrs.option(attrs.string(), default = None),
            "runner": attrs.option(attrs.dep(), default = None),
            "specs": attrs.option(attrs.arg(json = True), default = None),
        } |
        re_test_common.test_args() |
        test_common.attributes()
    ),
)
go_bootstrap_binary = prelude_rule(
    name = "go_bootstrap_binary",
    attrs = (
        go_common.srcs_arg() |
        {
            "entrypoints": attrs.list(attrs.string(), default = [], doc = """Package name or file names"""),
            "workdir": attrs.string(default = "", doc = """Change to subdir before running the command"""),
        }
    ),
)

go_rules = struct(
    go_binary = go_binary,
    go_bootstrap_binary = go_bootstrap_binary,
    go_exported_library = go_exported_library,
    go_library = go_library,
    go_test = go_test,
)
