extern crate bindgen;

use std::{
	env,
	path::{Path, PathBuf},
};

use build_rs::{
	input::{cargo_cfg_target_arch, cargo_cfg_target_os, out_dir},
	output::{rerun_if_changed, rustc_link_search_kind},
};
use flate2::read::GzDecoder;
use tar::Archive as TarArchive;

const SLANG_LIB_RELEASES_URL: &str = "https://github.com/shader-slang/slang/releases";

const SLANG_LIB_VERSION: &str = "2026.8";

fn main() {
	println!("cargo:rerun-if-env-changed=SLANG_DIR");
	println!("cargo:rerun-if-env-changed=SLANG_INCLUDE_DIR");
	println!("cargo:rerun-if-env-changed=SLANG_LIB_DIR");
	println!("cargo:rerun-if-env-changed=SLANG_USE_PREBUILT");
	println!("cargo:rerun-if-env-changed=VULKAN_SDK");

	rerun_if_changed("build.rs");

	let slang_lib_release_extract_dir = out_dir().join("slang-lib-release");

	let env_vars = get_env_vars();

	let use_prebuilt = env_vars
		.slang_use_prebuilt
		.as_ref()
		.is_some_and(|use_prebuilt| use_prebuilt == "1");

	if use_prebuilt && !slang_lib_release_extract_dir.try_exists().unwrap() {
		download_and_extract_slang_lib_release(&slang_lib_release_extract_dir);
	}

	let dirs = if use_prebuilt {
		if env_vars.slang_dir.is_some()
			|| env_vars.slang_lib_dir.is_some()
			|| env_vars.slang_include_dir.is_some()
			|| env_vars.vulkan_sdk.is_some()
		{
			panic!("Using a prebuilt Slang library (SLANG_USE_PREBUILT) cannot be enabled if the environment variable SLANG_LIB_DIR, SLANG_DIR, or VULKAN_SDK are set");
		}

		get_dirs_for_slang_lib_release(&slang_lib_release_extract_dir)
	} else {
		let Some(dirs) = get_dirs_from_env_vars(&env_vars) else {
			panic!("The environment variable SLANG_LIB_DIR, SLANG_INCLUDE_DIR, SLANG_DIR, SLANG_USE_PREBUILT, or VULKAN_SDK must be set");
		};

		dirs
	};

	if !dirs.lib_dir.as_os_str().is_empty() {
		rustc_link_search_kind("native", &dirs.lib_dir);
	}

	println!("cargo:rustc-link-lib=dylib=slang-compiler");

	bindgen::builder()
		.header(
			dirs.include_dir
				.join("slang.h")
				.to_str()
				.expect("Include directory path is not valid UTF-8"),
		)
		.clang_arg("-v")
		.clang_arg("-xc++")
		.clang_arg("-std=c++17")
		.allowlist_function("spReflection.*")
		.allowlist_function("spComputeStringHash")
		.allowlist_function("slang_.*")
		.allowlist_type("slang.*")
		.allowlist_var("SLANG_.*")
		.with_codegen_config(
			bindgen::CodegenConfig::FUNCTIONS
				| bindgen::CodegenConfig::TYPES
				| bindgen::CodegenConfig::VARS,
		)
		.parse_callbacks(Box::new(ParseCallback {}))
		.default_enum_style(bindgen::EnumVariation::Rust {
			non_exhaustive: false,
		})
		.constified_enum("SlangProfileID")
		.constified_enum("SlangCapabilityID")
		.vtable_generation(true)
		.layout_tests(false)
		.derive_copy(true)
		.generate()
		.expect("Couldn't generate bindings.")
		.write_to_file(out_dir().join("bindings.rs"))
		.expect("Couldn't write bindings.");
}

#[derive(Debug)]
struct ParseCallback {}

impl bindgen::callbacks::ParseCallbacks for ParseCallback {
	fn enum_variant_name(
		&self,
		enum_name: Option<&str>,
		original_variant_name: &str,
		_variant_value: bindgen::callbacks::EnumVariantValue,
	) -> Option<String> {
		let enum_name = enum_name?;

		// Map enum names to the part of their variant names that needs to be trimmed.
		// When an enum name is not in this map the code below will try to trim the enum name itself.
		let mut map = std::collections::HashMap::new();
		map.insert("SlangMatrixLayoutMode", "SlangMatrixLayout");
		map.insert("SlangCompileTarget", "Slang");

		let trim = map.get(enum_name).unwrap_or(&enum_name);
		let new_variant_name = pascal_case_from_snake_case(original_variant_name);
		let new_variant_name = new_variant_name.trim_start_matches(trim);
		Some(new_variant_name.to_string())
	}

	#[cfg(feature = "serde")]
	fn add_derives(&self, info: &bindgen::callbacks::DeriveInfo<'_>) -> Vec<String> {
		if info.name.starts_with("Slang") && info.kind == bindgen::callbacks::TypeKind::Enum {
			return vec!["serde::Serialize".into(), "serde::Deserialize".into()];
		}
		vec![]
	}
}

/// Converts `snake_case` or `SNAKE_CASE` to `PascalCase`.
/// If the input is already in `PascalCase` it will be returned as is.
fn pascal_case_from_snake_case(snake_case: &str) -> String {
	let mut result = String::new();

	let should_lower = snake_case
		.chars()
		.filter(|c| c.is_alphabetic())
		.all(|c| c.is_uppercase());

	for part in snake_case.split('_') {
		for (i, c) in part.chars().enumerate() {
			if i == 0 {
				result.push(c.to_ascii_uppercase());
			} else if should_lower {
				result.push(c.to_ascii_lowercase());
			} else {
				result.push(c);
			}
		}
	}

	result
}

struct EnvVars {
	slang_dir: Option<String>,
	slang_include_dir: Option<String>,
	slang_lib_dir: Option<String>,
	slang_use_prebuilt: Option<String>,
	vulkan_sdk: Option<String>,
}

fn get_env_vars() -> EnvVars {
	EnvVars {
		slang_dir: env::var("SLANG_DIR").ok(),
		slang_include_dir: env::var("SLANG_INCLUDE_DIR").ok(),
		slang_lib_dir: env::var("SLANG_LIB_DIR").ok(),
		slang_use_prebuilt: env::var("SLANG_USE_PREBUILT").ok(),
		vulkan_sdk: env::var("VULKAN_SDK").ok(),
	}
}

struct Dirs {
	include_dir: PathBuf,
	lib_dir: PathBuf,
}

fn get_dirs_from_env_vars(env_vars: &EnvVars) -> Option<Dirs> {
	let include_dir = if let Some(include_dir) = &env_vars.slang_include_dir {
		Path::new(include_dir).to_path_buf()
	} else if let Some(dir) = &env_vars.slang_dir {
		Path::new(dir).join("include")
	} else if let Some(vulkan_sdk_dir) = &env_vars.vulkan_sdk {
		Path::new(vulkan_sdk_dir).join("include/slang")
	} else {
		return None;
	};

	let lib_dir = if let Some(lib_dir) = &env_vars.slang_lib_dir {
		Path::new(lib_dir).to_path_buf()
	} else if let Some(dir) = &env_vars.slang_dir {
		Path::new(dir).join("lib")
	} else if let Some(vulkan_sdk_dir) = &env_vars.vulkan_sdk {
		Path::new(vulkan_sdk_dir).join("lib")
	} else {
		return None;
	};

	Some(Dirs {
		include_dir,
		lib_dir,
	})
}

fn get_dirs_for_slang_lib_release(slang_lib_release_extract_dir: &Path) -> Dirs {
	Dirs {
		include_dir: slang_lib_release_extract_dir.join("include"),
		lib_dir: slang_lib_release_extract_dir.join("lib"),
	}
}

fn download_and_extract_slang_lib_release(extraction_dir: &Path) {
	let mut res_body = ureq::get(get_slang_lib_release_tarball_url())
		.call()
		.unwrap()
		.into_body();

	let res_body_reader = res_body.as_reader();

	let mut release_archive = TarArchive::new(GzDecoder::new(res_body_reader));

	release_archive.unpack(extraction_dir).unwrap();
}

fn get_slang_lib_release_tarball_url() -> String {
	let target_os = cargo_cfg_target_os();
	let target_arch = cargo_cfg_target_arch();

	if target_os != "linux" && target_os != "windows" && target_os != "macos" {
		panic!("Unsupported target OS '{target_os}'");
	}

	if target_arch != "x86_64" && target_arch != "aarch64" {
		panic!("Unsupported target architecture '{target_arch}'");
	}

	format!(
		"{releases_url}/download/v{ver}/slang-{ver}-{target_os}-{target_arch}.tar.gz",
		releases_url = SLANG_LIB_RELEASES_URL,
		ver = SLANG_LIB_VERSION,
	)
}
