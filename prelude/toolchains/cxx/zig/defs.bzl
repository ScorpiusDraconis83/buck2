# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is dual-licensed under either the MIT license found in the
# LICENSE-MIT file in the root directory of this source tree or the Apache
# License, Version 2.0 found in the LICENSE-APACHE file in the root directory
# of this source tree. You may select, at your option, one of the
# above-listed licenses.

"""Self-contained C/C++ toolchain based on zig cc.

Most C/C++ compiler toolchains will depend on a system wide installation of
libc and other standard libraries. This means that the build outputs may vary
from system to system and that advanced use-cases, like cross-compilation,
require installation of special system packages.

The zig cc compiler is based on clang and comes bundled with the standard
library sources and supports on-the-fly cross-compilation, making it easier to
define a reproducible build setup and cross-compilation use-cases.

Further details on zig cc are available [here][zig-cc-announcement]. Note, at
the time of writing this is still experimental. If this is a problem for your
use-case then you may wish to rely on a system toolchain or define your own.

The toolchain is not fully hermetic as it still relies on system tools like nm.
It only works on Linux, and to a limited extent on MacOS.

[zig-cc-announcement]: https://andrewkelley.me/post/zig-cc-powerful-drop-in-replacement-gcc-clang.html

## Examples

To automatically fetch a distribution suitable for the host-platform configure
the toolchain like so:

`toolchains//BUILD`
```bzl
load("@prelude//toolchains/cxx/zig:defs.bzl", "download_zig_distribution", "cxx_zig_toolchain")

download_zig_distribution(
    name = "zig",
    version = "0.9.1",
)

cxx_zig_toolchain(
    name = "cxx",
    distribution = ":zig",
    visibility = ["PUBLIC"],
)
```

To define toolchains for multiple platforms and configure cross-compilation you
can configure the toolchain like so:

```bzl
load("@prelude//toolchains/cxx/zig:defs.bzl", "download_zig_distribution", "cxx_zig_toolchain")

download_zig_distribution(
    name = "zig-x86_64-linux",
    version = "0.9.1",
    arch = "x86_64",
    os = "linux",
)

download_zig_distribution(
    name = "zig-x86_64-macos",
    version = "0.9.1",
    arch = "x86_64",
    os = "macos",
)

download_zig_distribution(
    name = "zig-x86_64-windows",
    version = "0.9.1",
    arch = "x86_64",
    os = "windows",
)

alias(
    name = "zig",
    actual = select({
        "@prelude//os:linux": ":zig-x86_64-linux",
        "@prelude//os:macos": ":zig-x86_64-macos",
        "@prelude//os:windows": ":zig-x86_64-windows",
    }),
)

cxx_zig_toolchain(
    name = "cxx",
    distribution = ":zig",
    target = select({
        "@prelude//os:linux": "x86_64-linux-gnu",
        "@prelude//os:macos": "x86_64-macos-gnu",
        "@prelude//os:windows": "x86_64-windows-gnu",
    }),
    visibility = ["PUBLIC"],
)
```
"""

load(
    "@prelude//cxx:cxx_toolchain_types.bzl",
    "BinaryUtilitiesInfo",
    "CCompilerInfo",
    "CxxCompilerInfo",
    "CxxInternalTools",
    "LinkerInfo",
    "LinkerType",
    "ShlibInterfacesMode",
    "StripFlagsInfo",
    "cxx_toolchain_infos",
)
load(
    "@prelude//cxx:headers.bzl",
    "HeaderMode",
)
load(
    "@prelude//cxx:linker.bzl",
    "is_pdb_generated",
)
load(
    "@prelude//linking:link_info.bzl",
    "LinkStyle",
)
load(
    "@prelude//os_lookup:defs.bzl",
    "ScriptLanguage",
)
load(
    "@prelude//utils:cmd_script.bzl",
    "cmd_script",
)
load(
    ":releases.bzl",
    "releases",
)

ZigReleaseInfo = provider(
    # @unsorted-dict-items
    fields = {
        "version": provider_field(typing.Any, default = None),
        "url": provider_field(typing.Any, default = None),
        "sha256": provider_field(typing.Any, default = None),
    },
)

def _get_zig_release(
        version: str,
        platform: str) -> ZigReleaseInfo:
    if not version in releases:
        fail("Unknown zig release version '{}'. Available versions: {}".format(
            version,
            ", ".join(releases.keys()),
        ))
    zig_version = releases[version]
    if not platform in zig_version:
        fail("Unsupported platform '{}'. Supported platforms: {}".format(
            platform,
            ", ".join(zig_version.keys()),
        ))
    zig_platform = zig_version[platform]
    return ZigReleaseInfo(
        version = zig_version.get("version", version),
        url = zig_platform["tarball"],
        sha256 = zig_platform["shasum"],
    )

ZigDistributionInfo = provider(
    # @unsorted-dict-items
    fields = {
        "version": provider_field(typing.Any, default = None),
        "arch": provider_field(typing.Any, default = None),
        "os": provider_field(typing.Any, default = None),
    },
)

def _zig_distribution_impl(ctx: AnalysisContext) -> list[Provider]:
    dst = ctx.actions.declare_output("zig")
    path_tpl = "{}/" + ctx.attrs.prefix + "/zig" + ctx.attrs.suffix
    src = cmd_args(ctx.attrs.dist[DefaultInfo].default_outputs[0], format = path_tpl)
    ctx.actions.run(
        ["ln", "-sf", cmd_args(src, relative_to = (dst, 1)), dst.as_output()],
        category = "cp_compiler",
    )

    compiler = cmd_args(
        [dst],
        hidden = [
            ctx.attrs.dist[DefaultInfo].default_outputs,
            ctx.attrs.dist[DefaultInfo].other_outputs,
        ],
    )

    return [
        ctx.attrs.dist[DefaultInfo],
        RunInfo(args = compiler),
        ZigDistributionInfo(
            version = ctx.attrs.version,
            arch = ctx.attrs.arch,
            os = ctx.attrs.os,
        ),
    ]

zig_distribution = rule(
    impl = _zig_distribution_impl,
    attrs = {
        "arch": attrs.string(),
        "dist": attrs.dep(providers = [DefaultInfo]),
        "os": attrs.string(),
        "prefix": attrs.string(),
        "suffix": attrs.string(default = ""),
        "version": attrs.string(),
    },
)

def _http_archive_impl(ctx: AnalysisContext) -> list[Provider]:
    url = ctx.attrs.urls[0]
    if url.endswith(".tar.xz"):
        ext = "tar.xz"
        flags = ["tar", "xJf"]
    elif url.endswith(".zip"):
        flags = ["unzip"]
        ext = "zip"
    else:
        fail("Unknown archive type in URL '{}'".format(url))

    # Download archive.
    archive = ctx.actions.declare_output("archive." + ext)
    ctx.actions.download_file(archive.as_output(), url, sha256 = ctx.attrs.sha256)

    # Unpack archive to output directory.
    output = ctx.actions.declare_output(ctx.label.name)
    script, _ = ctx.actions.write(
        "unpack.sh",
        [
            cmd_args(output, format = "mkdir -p {}"),
            cmd_args(output, format = "cd {}"),
            cmd_args(flags, archive, delimiter = " ", relative_to = output),
        ],
        is_executable = True,
        allow_args = True,
    )
    ctx.actions.run(
        cmd_args(["/bin/sh", script], hidden = [archive, output.as_output()]),
        category = "http_archive",
    )

    return [DefaultInfo(default_output = output)]

# TODO Switch to http_archive once that supports zip download.
#   See https://github.com/facebook/buck2/issues/21
_http_archive = rule(
    impl = _http_archive_impl,
    attrs = {
        "sha256": attrs.string(default = ""),
        "urls": attrs.list(attrs.string(), default = []),
    },
)

def _host_arch() -> str:
    arch = host_info().arch
    if arch.is_x86_64:
        return "x86_64"
    elif host_info().arch.is_aarch64:
        return "aarch64"
    elif host_info().arch.is_arm:
        return "armv7a"
    elif host_info().arch.is_i386:
        return "i386"
    elif host_info().arch.is_i386:
        return "i386"
    else:
        fail("Unsupported host architecture.")

def _host_os() -> str:
    os = host_info().os
    if os.is_freebsd:
        return "freebsd"
    elif os.is_linux:
        return "linux"
    elif os.is_macos:
        return "macos"
    elif os.is_windows:
        return "windows"
    else:
        fail("Unsupported host os.")

def download_zig_distribution(
        name: str,
        version: str,
        arch: [None, str] = None,
        os: [None, str] = None):
    if arch == None:
        arch = _host_arch()
    if os == None:
        os = _host_os()
    archive_name = name + "-archive"
    release = _get_zig_release(version, "{}-{}".format(arch, os))
    _http_archive(
        name = archive_name,
        urls = [release.url],
        sha256 = release.sha256,
    )
    zig_distribution(
        name = name,
        dist = ":" + archive_name,
        prefix = "zig-{}-{}-{}/".format(os, arch, release.version),
        suffix = ".exe" if os == "windows" else "",
        version = release.version,
        arch = arch,
        os = os,
    )

def _get_linker_type(os: str) -> LinkerType:
    if os == "linux":
        return LinkerType("gnu")
    elif os == "macos" or os == "freebsd":
        # TODO[AH] return "darwin".
        #   The cc rules emit linker flags on MacOS that are not supported by Zig's linker.
        #   Declaring the linker as GNU style is not entirely correct, however it works better than
        #   declaring Darwin style at this point. See https://github.com/facebook/buck2/issues/470
        return LinkerType("gnu")
    elif os == "windows":
        return LinkerType("windows")
    else:
        fail("Cannot determine linker type: Unknown OS '{}'".format(os))

def _cxx_zig_toolchain_impl(ctx: AnalysisContext) -> list[Provider]:
    dist = ctx.attrs.distribution[ZigDistributionInfo]
    zig = ctx.attrs.distribution[RunInfo]
    target = ["-target", ctx.attrs.target] if ctx.attrs.target else []
    zig_cc = cmd_script(
        ctx = ctx,
        name = "zig_cc",
        cmd = cmd_args(zig, "cc"),
        language = ScriptLanguage("bat" if dist.os == "windows" else "sh"),
    )
    zig_cxx = cmd_script(
        ctx = ctx,
        name = "zig_cxx",
        cmd = cmd_args(zig, "c++"),
        language = ScriptLanguage("bat" if dist.os == "windows" else "sh"),
    )
    zig_ar = cmd_script(
        ctx = ctx,
        name = "zig_ar",
        cmd = cmd_args(zig, "ar"),
        language = ScriptLanguage("bat" if dist.os == "windows" else "sh"),
    )
    zig_ranlib = cmd_script(
        ctx = ctx,
        name = "zig_ranlib",
        cmd = cmd_args(zig, "ranlib"),
        language = ScriptLanguage("bat" if dist.os == "windows" else "sh"),
    )
    return [ctx.attrs.distribution[DefaultInfo]] + cxx_toolchain_infos(
        internal_tools = ctx.attrs._cxx_internal_tools[CxxInternalTools],
        platform_name = dist.arch,
        c_compiler_info = CCompilerInfo(
            compiler = RunInfo(args = cmd_args(zig_cc)),
            compiler_type = "clang",
            compiler_flags = cmd_args(target, ctx.attrs.c_compiler_flags),
            #preprocessor = None,
            #preprocessor_type = None,
            preprocessor_flags = cmd_args(ctx.attrs.c_preprocessor_flags),
            #dep_files_processor = None,
        ),
        cxx_compiler_info = CxxCompilerInfo(
            compiler = RunInfo(args = cmd_args(zig_cxx)),
            compiler_type = "clang",
            compiler_flags = cmd_args(target, ctx.attrs.cxx_compiler_flags),
            #preprocessor = None,
            #preprocessor_type = None,
            preprocessor_flags = cmd_args(ctx.attrs.cxx_preprocessor_flags),
            #dep_files_processor = None,
        ),
        linker_info = LinkerInfo(
            archiver = RunInfo(args = cmd_args(zig_ar)),
            archiver_type = "gnu",
            archiver_supports_argfiles = True,
            #archive_contents = None,
            archive_objects_locally = False,
            binary_extension = "",
            generate_linker_maps = False,
            link_binaries_locally = False,
            link_libraries_locally = False,
            link_style = LinkStyle(ctx.attrs.link_style),
            link_weight = 1,
            #link_ordering = None,
            linker = RunInfo(args = cmd_args(zig_cxx)),
            linker_flags = cmd_args(target, ctx.attrs.linker_flags),
            #lto_mode = None,  # TODO support LTO
            object_file_extension = "o",
            #mk_shlib_intf = None,  # not needed if shlib_interfaces = "disabled"
            shlib_interfaces = ShlibInterfacesMode("disabled"),
            shared_dep_runtime_ld_flags = ctx.attrs.shared_dep_runtime_ld_flags,
            shared_library_name_default_prefix = "lib",
            shared_library_name_format = "{}.so",
            shared_library_versioned_name_format = "{}.so.{}",
            static_dep_runtime_ld_flags = ctx.attrs.static_dep_runtime_ld_flags,
            static_library_extension = "a",
            static_pic_dep_runtime_ld_flags = ctx.attrs.static_pic_dep_runtime_ld_flags,
            #requires_archives = None,
            #requires_objects = None,
            #supports_distributed_thinlto = None,
            independent_shlib_interface_linker_flags = ctx.attrs.shared_library_interface_flags,
            type = _get_linker_type(dist.os),
            use_archiver_flags = True,
            is_pdb_generated = is_pdb_generated(_get_linker_type(dist.os), ctx.attrs.linker_flags),
        ),
        binary_utilities_info = BinaryUtilitiesInfo(
            bolt_msdk = None,
            dwp = None,
            nm = RunInfo(args = ["nm"]),  # not included in the zig distribution.
            objcopy = RunInfo(args = ["objcopy"]),  # not included in the zig distribution.
            ranlib = RunInfo(args = cmd_args(zig_ranlib)),
            strip = RunInfo(args = ["strip"]),  # not included in the zig distribution.
        ),
        header_mode = HeaderMode("symlink_tree_only"),  # header map modes require mk_hmap
        #headers_as_raw_headers_mode = None,
        #asm_compiler_info = None,
        #as_compiler_info = None,
        #hip_compiler_info = None,
        #cuda_compiler_info = None,
        #mk_hmap = None,
        #use_distributed_thinlto = False,
        #use_dep_files = False,  # requires dep_files_processor
        strip_flags_info = StripFlagsInfo(
            strip_debug_flags = ctx.attrs.strip_debug_flags,
            strip_non_global_flags = ctx.attrs.strip_non_global_flags,
            strip_all_flags = ctx.attrs.strip_all_flags,
        ),
        #dist_lto_tools_info: [DistLtoToolsInfo, None] = None,
        #split_debug_mode = SplitDebugMode("none"),
        #bolt_enabled = False,
    )

cxx_zig_toolchain = rule(
    impl = _cxx_zig_toolchain_impl,
    attrs = {
        "c_compiler_flags": attrs.list(attrs.arg(), default = []),
        "c_preprocessor_flags": attrs.list(attrs.arg(), default = []),
        "cxx_compiler_flags": attrs.list(attrs.arg(), default = []),
        "cxx_preprocessor_flags": attrs.list(attrs.arg(), default = []),
        "distribution": attrs.exec_dep(providers = [RunInfo, ZigDistributionInfo]),
        "link_style": attrs.enum(
            LinkStyle.values(),
            default = "static",
            doc = """
            The default value of the `link_style` attribute for rules that use this toolchain.
            """,
        ),
        "linker_flags": attrs.list(attrs.arg(), default = []),
        "shared_dep_runtime_ld_flags": attrs.list(attrs.arg(), default = []),
        "shared_library_interface_flags": attrs.list(attrs.string(), default = []),
        "static_dep_runtime_ld_flags": attrs.list(attrs.arg(), default = []),
        "static_pic_dep_runtime_ld_flags": attrs.list(attrs.arg(), default = []),
        "strip_all_flags": attrs.option(attrs.list(attrs.arg()), default = None),
        "strip_debug_flags": attrs.option(attrs.list(attrs.arg()), default = None),
        "strip_non_global_flags": attrs.option(attrs.list(attrs.arg()), default = None),
        "target": attrs.option(attrs.string(), default = None),
        "_cxx_internal_tools": attrs.default_only(attrs.dep(providers = [CxxInternalTools], default = "prelude//cxx/tools:internal_tools")),
    },
    is_toolchain_rule = True,
)
