extern crate bindgen;

use std::env;

#[cfg(not(any(feature = "static", feature = "dynamic")))]
compile_error!("You must enable either the 'static' or 'dynamic' feature.");
#[cfg(all(feature = "static", feature = "dynamic"))]
compile_error!("Both 'static' and 'dynamic' features cannot be enabled at the same time.");

fn main() {
	println!("cargo:rerun-if-env-changed=SLANG_DIR");
	println!("cargo:rerun-if-env-changed=SLANG_INCLUDE_DIR");
	println!("cargo:rerun-if-env-changed=SLANG_LIB_DIR");
	println!("cargo:rerun-if-env-changed=VULKAN_SDK");
	println!("cargo:rerun-if-changed=build.rs");

	let out_dir = env::var("OUT_DIR").expect("Output directory environment variable is not set");

	let include_dir = if let Ok(dir) = env::var("SLANG_INCLUDE_DIR") {
		dir
	} else if let Ok(dir) = env::var("SLANG_DIR") {
		format!("{dir}/include")
	} else if let Ok(dir) = env::var("VULKAN_SDK") {
		format!("{dir}/include/slang")
	} else if cfg!(feature = "static") {
		format!("{out_dir}/slang_build/Release/include")
	} else {
		panic!("The environment variable SLANG_INCLUDE_DIR, SLANG_DIR, or VULKAN_SDK must be set");
	};

	let lib_dir = if let Ok(dir) = env::var("SLANG_LIB_DIR") {
		dir
	} else if let Ok(dir) = env::var("SLANG_DIR") {
		format!("{dir}/lib")
	} else if let Ok(dir) = env::var("VULKAN_SDK") {
		format!("{dir}/lib")
	} else if cfg!(feature = "static") {
		format!("{out_dir}/slang_build/Release/lib",)
	} else {
		panic!("The environment variable SLANG_LIB_DIR, SLANG_DIR, or VULKAN_SDK must be set");
	};

	if !lib_dir.is_empty() {
		println!("cargo:rustc-link-search=native={lib_dir}");
	}

	#[cfg(feature = "dynamic")]
	{
		println!("cargo:rustc-link-lib=dylib=slang");
	}
	#[cfg(feature = "static")]
	{
		use std::fs::create_dir;
		use std::path::Path;
		use std::path::PathBuf;
		use std::process::Command;

		let slang_build_dir = Path::new(&out_dir).join("slang_build");

		if !slang_build_dir
			.join("Release/lib/libslang-compiler.a")
			.try_exists()
			.unwrap()
		{
			use std::fs::File;

			let cargo_manifest_dir = PathBuf::from(
				env::var("CARGO_MANIFEST_DIR")
					.expect("Cargo manifest directory environment variable is not set"),
			);

			if !slang_build_dir.try_exists().unwrap() {
				create_dir(&slang_build_dir).unwrap();
			}

			let configure_command_output = Command::new("cmake")
				.current_dir(&cargo_manifest_dir.join("slang"))
				.args([
					"--preset",
					"default",
					"-B",
					slang_build_dir.to_str().unwrap(),
					"-DSLANG_LIB_TYPE=STATIC",
					"-DSLANG_ENABLE_TESTS=FALSE",
					"-DSLANG_ENABLE_EXAMPLES=FALSE",
					"-DSLANG_ENABLE_RELEASE_DEBUG_INFO=FALSE",
					"-DSLANG_SLANG_LLVM_FLAVOR=DISABLE",
					"-DSLANG_ENABLE_SLANGD=FALSE",
					// "-DSLANG_ENABLE_SLANGC=FALSE",
					"-DSLANG_ENABLE_SLANGI=FALSE",
					"-DSLANG_ENABLE_GFX=FALSE",
					"-DSLANG_ENABLE_SLANG_RHI=FALSE",
				])
				.output()
				.unwrap();

			if !configure_command_output.status.success() {
				println!(
					"cargo::error={}",
					String::from_utf8(configure_command_output.stdout).unwrap()
				);
				println!(
					"cargo::error={}",
					String::from_utf8(configure_command_output.stderr).unwrap()
				);
				return;
			}

			let build_command_status = Command::new("cmake")
				.current_dir(&cargo_manifest_dir.join("slang"))
				.args([
					"--build",
					slang_build_dir.to_str().unwrap(),
					"--config",
					"Release",
				])
				.stdout(
					File::create(Path::new(&out_dir).join("slang_build_command_stdout")).unwrap(),
				)
				.stderr(
					File::create(Path::new(&out_dir).join("slang_build_command_stderr")).unwrap(),
				)
				.spawn()
				.unwrap()
				.wait()
				.unwrap();

			if !build_command_status.success() {
				println!("cargo::error=Build command exited with status {build_command_status}");
				return;
			}
		}

		let external_lib_dir = if let Ok(external_lib_dir) = env::var("SLANG_EXTERNAL_DIR") {
			external_lib_dir
		} else {
			format!("{out_dir}/slang_build/external",)
		};

		let miniz_lib_dir = Path::new(&external_lib_dir).join("miniz/Release/");
		let lz4_lib_dir = Path::new(&external_lib_dir).join("lz4/build/cmake/Release/");

		// Add Slang static library search path
		println!("cargo:rustc-link-search=native={}", lib_dir);
		println!("cargo:rustc-link-search=native={}", miniz_lib_dir.display());
		println!("cargo:rustc-link-search=native={}", lz4_lib_dir.display());

		// Link the core Slang static libraries
		println!("cargo:rustc-link-lib=static=slang-compiler");
		println!("cargo:rustc-link-lib=static=compiler-core");
		println!("cargo:rustc-link-lib=static=core");
		// External slang dependencies
		println!("cargo:rustc-link-lib=static=miniz");
		println!("cargo:rustc-link-lib=static=lz4");
		// C++ library
		// The C++ lib has to be the same as the one used to compile Slang,
		// otherwise you will get errors about missing symbols.
		// The following is only a best guess depending on platform's defaults.
		if cfg!(target_os = "windows") {
			if cfg!(not(target_env = "msvc")) {
				// MinGW: Link libstdc++
				println!("cargo:rustc-link-lib=stdc++");
			} else {
				// No linking necessary on Windows, the MSVC runtime is linked by default.
			}
		} else if cfg!(target_os = "macos") {
			// macOS uses Apple’s Clang/LLVM by default, which is tightly integrated with libc++.
			println!("cargo:rustc-link-lib=c++"); // Links libc++ on macOS
		} else if cfg!(target_os = "linux") {
			// Linux often uses GCC or Clang with libstdc++ as the default C++ standard library, and libc++ is not commonly installed.
			println!("cargo:rustc-link-lib=stdc++"); // Links libstdc++ on Linux
		} else {
			// Fallback
			println!("cargo:rustc-link-lib=stdc++");
		}
	}

	bindgen::builder()
		.header(format!("{include_dir}/slang.h").as_str())
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
		.write_to_file(format!("{out_dir}/bindings.rs").as_str())
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
