//! DO NOT EDIT
//!
//! Generated with ./scripts/scenarios/update.py
//! Scenarios from <https://github.com/zanieb/packse/tree/78f34eec66acfba9c723285764dc1f4b841f4961/scenarios>
//!
#![cfg(all(feature = "python", feature = "pypi"))]

use std::process::Command;

use anyhow::Result;
use assert_fs::fixture::{FileWriteStr, PathChild};
use insta_cmd::_macro_support::insta;
use insta_cmd::{assert_cmd_snapshot, get_cargo_bin};

use common::{create_venv, BIN_NAME, INSTA_FILTERS};

mod common;

/// requires-incompatible-python-version-compatible-override
///
/// The user requires a package which requires a Python version greater than the
/// current version, but they use an alternative Python version for package
/// resolution.
///
/// ```text
/// 818d78ce
/// ├── environment
/// │   └── python3.9
/// ├── root
/// │   └── requires a==1.0.0
/// │       └── satisfied by a-1.0.0
/// └── a
///     └── a-1.0.0
///         └── requires python>=3.10 (incompatible with environment)
/// ```
#[test]
fn requires_incompatible_python_version_compatible_override() -> Result<()> {
    let temp_dir = assert_fs::TempDir::new()?;
    let cache_dir = assert_fs::TempDir::new()?;
    let venv = create_venv(&temp_dir, &cache_dir, "3.9");

    // In addition to the standard filters, swap out package names for more realistic messages
    let mut filters = INSTA_FILTERS.to_vec();
    filters.push((r"a-818d78ce", "albatross"));
    filters.push((r"-818d78ce", ""));

    let requirements_in = temp_dir.child("requirements.in");
    requirements_in.write_str("a-818d78ce==1.0.0")?;

    insta::with_settings!({
        filters => filters
    }, {
        assert_cmd_snapshot!(Command::new(get_cargo_bin(BIN_NAME))
            .arg("pip")
            .arg("compile")
            .arg("requirements.in")
            .arg("--python-version=3.11")
            .arg("--extra-index-url")
            .arg("https://test.pypi.org/simple")
            .arg("--cache-dir")
            .arg(cache_dir.path())
            .env("VIRTUAL_ENV", venv.as_os_str())
            .env("PUFFIN_NO_WRAP", "1")
            .current_dir(&temp_dir), @r###"
        success: true
        exit_code: 0
        ----- stdout -----
        # This file was autogenerated by Puffin v[VERSION] via the following command:
        #    puffin pip compile requirements.in --python-version=3.11 --extra-index-url https://test.pypi.org/simple --cache-dir [CACHE_DIR]
        albatross==1.0.0

        ----- stderr -----
        Resolved 1 package in [TIME]
        "###);
    });

    Ok(())
}

/// requires-compatible-python-version-incompatible-override
///
/// The user requires a package which requires a compatible Python version, but they
/// request an incompatible Python version for package resolution.
///
/// ```text
/// e94b8bc2
/// ├── environment
/// │   └── python3.11
/// ├── root
/// │   └── requires a==1.0.0
/// │       └── satisfied by a-1.0.0
/// └── a
///     └── a-1.0.0
///         └── requires python>=3.10
/// ```
#[test]
fn requires_compatible_python_version_incompatible_override() -> Result<()> {
    let temp_dir = assert_fs::TempDir::new()?;
    let cache_dir = assert_fs::TempDir::new()?;
    let venv = create_venv(&temp_dir, &cache_dir, "3.11");

    // In addition to the standard filters, swap out package names for more realistic messages
    let mut filters = INSTA_FILTERS.to_vec();
    filters.push((r"a-e94b8bc2", "albatross"));
    filters.push((r"-e94b8bc2", ""));

    let requirements_in = temp_dir.child("requirements.in");
    requirements_in.write_str("a-e94b8bc2==1.0.0")?;

    insta::with_settings!({
        filters => filters
    }, {
        assert_cmd_snapshot!(Command::new(get_cargo_bin(BIN_NAME))
            .arg("pip")
            .arg("compile")
            .arg("requirements.in")
            .arg("--python-version=3.9")
            .arg("--extra-index-url")
            .arg("https://test.pypi.org/simple")
            .arg("--cache-dir")
            .arg(cache_dir.path())
            .env("VIRTUAL_ENV", venv.as_os_str())
            .env("PUFFIN_NO_WRAP", "1")
            .current_dir(&temp_dir), @r###"
        success: false
        exit_code: 1
        ----- stdout -----

        ----- stderr -----
        [crates/puffin-resolver/src/pubgrub/report.rs:113] &dependency_set = Range {
            segments: [
                (
                    Included(
                        "3.10",
                    ),
                    Unbounded,
                ),
            ],
        }
        [crates/puffin-resolver/src/pubgrub/report.rs:115] &dependency_set = Range {
            segments: [
                (
                    Included(
                        "3.10",
                    ),
                    Unbounded,
                ),
            ],
        }
        [crates/puffin-resolver/src/pubgrub/report.rs:113] &dependency_set = Range {
            segments: [
                (
                    Included(
                        "1.0.0",
                    ),
                    Included(
                        "1.0.0",
                    ),
                ),
            ],
        }
        [crates/puffin-resolver/src/pubgrub/report.rs:115] &dependency_set = Range {
            segments: [
                (
                    Included(
                        "1.0.0",
                    ),
                    Included(
                        "1.0.0",
                    ),
                ),
            ],
        }
          × No solution found when resolving dependencies:
          ╰─▶ Because the requested Python version (3.9) does not satisfy Python>=3.10 and albatross==1.0.0 depends on Python>=3.10, we can conclude that albatross==1.0.0 cannot be used.
              And because you require albatross==1.0.0, we can conclude that the requirements are unsatisfiable.
        "###);
    });

    Ok(())
}

/// requires-incompatible-python-version-compatible-override-no-wheels
///
/// The user requires a package which requires a incompatible Python version, but
/// they request a compatible Python version for package resolution. There are only
/// source distributions available for the package.
///
/// ```text
/// 367303df
/// ├── environment
/// │   └── python3.9
/// ├── root
/// │   └── requires a==1.0.0
/// │       └── satisfied by a-1.0.0
/// └── a
///     └── a-1.0.0
///         └── requires python>=3.10 (incompatible with environment)
/// ```
#[test]
fn requires_incompatible_python_version_compatible_override_no_wheels() -> Result<()> {
    let temp_dir = assert_fs::TempDir::new()?;
    let cache_dir = assert_fs::TempDir::new()?;
    let venv = create_venv(&temp_dir, &cache_dir, "3.9");

    // In addition to the standard filters, swap out package names for more realistic messages
    let mut filters = INSTA_FILTERS.to_vec();
    filters.push((r"a-367303df", "albatross"));
    filters.push((r"-367303df", ""));

    let requirements_in = temp_dir.child("requirements.in");
    requirements_in.write_str("a-367303df==1.0.0")?;

    // Since there are no wheels for the package and it is not compatible with the
    // local installation, we cannot build the source distribution to determine its
    // dependencies.
    insta::with_settings!({
        filters => filters
    }, {
        assert_cmd_snapshot!(Command::new(get_cargo_bin(BIN_NAME))
            .arg("pip")
            .arg("compile")
            .arg("requirements.in")
            .arg("--python-version=3.11")
            .arg("--extra-index-url")
            .arg("https://test.pypi.org/simple")
            .arg("--cache-dir")
            .arg(cache_dir.path())
            .env("VIRTUAL_ENV", venv.as_os_str())
            .env("PUFFIN_NO_WRAP", "1")
            .current_dir(&temp_dir), @r###"
        success: true
        exit_code: 0
        ----- stdout -----
        # This file was autogenerated by Puffin v[VERSION] via the following command:
        #    puffin pip compile requirements.in --python-version=3.11 --extra-index-url https://test.pypi.org/simple --cache-dir [CACHE_DIR]
        albatross==1.0.0

        ----- stderr -----
        Resolved 1 package in [TIME]
        "###);
    });

    Ok(())
}

/// requires-incompatible-python-version-compatible-override-no-compatible-wheels
///
/// The user requires a package which requires a incompatible Python version, but
/// they request a compatible Python version for package resolution. There is a
/// wheel available for the package, but it does not have a compatible tag.
///
/// ```text
/// 7d66d27e
/// ├── environment
/// │   └── python3.9
/// ├── root
/// │   └── requires a==1.0.0
/// │       └── satisfied by a-1.0.0
/// └── a
///     └── a-1.0.0
///         └── requires python>=3.10 (incompatible with environment)
/// ```
#[test]
fn requires_incompatible_python_version_compatible_override_no_compatible_wheels() -> Result<()> {
    let temp_dir = assert_fs::TempDir::new()?;
    let cache_dir = assert_fs::TempDir::new()?;
    let venv = create_venv(&temp_dir, &cache_dir, "3.9");

    // In addition to the standard filters, swap out package names for more realistic messages
    let mut filters = INSTA_FILTERS.to_vec();
    filters.push((r"a-7d66d27e", "albatross"));
    filters.push((r"-7d66d27e", ""));

    let requirements_in = temp_dir.child("requirements.in");
    requirements_in.write_str("a-7d66d27e==1.0.0")?;

    // Since there are no compatible wheels for the package and it is not compatible
    // with the local installation, we cannot build the source distribution to
    // determine its dependencies.
    insta::with_settings!({
        filters => filters
    }, {
        assert_cmd_snapshot!(Command::new(get_cargo_bin(BIN_NAME))
            .arg("pip")
            .arg("compile")
            .arg("requirements.in")
            .arg("--python-version=3.11")
            .arg("--extra-index-url")
            .arg("https://test.pypi.org/simple")
            .arg("--cache-dir")
            .arg(cache_dir.path())
            .env("VIRTUAL_ENV", venv.as_os_str())
            .env("PUFFIN_NO_WRAP", "1")
            .current_dir(&temp_dir), @r###"
        success: false
        exit_code: 2
        ----- stdout -----

        ----- stderr -----
        error: Package `albatross` was not found in the registry.
        "###);
    });

    Ok(())
}

/// requires-incompatible-python-version-compatible-override-other-wheel
///
/// The user requires a package which requires a incompatible Python version, but
/// they request a compatible Python version for package resolution. There are only
/// source distributions available for the compatible version of the package, but
/// there is an incompatible version with a wheel available.
///
/// ```text
/// 47c905cb
/// ├── environment
/// │   └── python3.9
/// ├── root
/// │   └── requires a
/// │       ├── satisfied by a-1.0.0
/// │       └── satisfied by a-2.0.0
/// └── a
///     ├── a-1.0.0
///     │   └── requires python>=3.10 (incompatible with environment)
///     └── a-2.0.0
///         └── requires python>=3.12 (incompatible with environment)
/// ```
#[test]
fn requires_incompatible_python_version_compatible_override_other_wheel() -> Result<()> {
    let temp_dir = assert_fs::TempDir::new()?;
    let cache_dir = assert_fs::TempDir::new()?;
    let venv = create_venv(&temp_dir, &cache_dir, "3.9");

    // In addition to the standard filters, swap out package names for more realistic messages
    let mut filters = INSTA_FILTERS.to_vec();
    filters.push((r"a-47c905cb", "albatross"));
    filters.push((r"-47c905cb", ""));

    let requirements_in = temp_dir.child("requirements.in");
    requirements_in.write_str("a-47c905cb")?;

    // Since there are no wheels for the version of the package compatible with the
    // target and it is not compatible with the local installation, we cannot build the
    // source distribution to determine its dependencies. The other version has wheels
    // available, but is not compatible with the target version and cannot be used.
    insta::with_settings!({
        filters => filters
    }, {
        assert_cmd_snapshot!(Command::new(get_cargo_bin(BIN_NAME))
            .arg("pip")
            .arg("compile")
            .arg("requirements.in")
            .arg("--python-version=3.11")
            .arg("--extra-index-url")
            .arg("https://test.pypi.org/simple")
            .arg("--cache-dir")
            .arg(cache_dir.path())
            .env("VIRTUAL_ENV", venv.as_os_str())
            .env("PUFFIN_NO_WRAP", "1")
            .current_dir(&temp_dir), @r###"
        success: true
        exit_code: 0
        ----- stdout -----
        # This file was autogenerated by Puffin v[VERSION] via the following command:
        #    puffin pip compile requirements.in --python-version=3.11 --extra-index-url https://test.pypi.org/simple --cache-dir [CACHE_DIR]
        albatross==1.0.0

        ----- stderr -----
        Resolved 1 package in [TIME]
        "###);
    });

    Ok(())
}
