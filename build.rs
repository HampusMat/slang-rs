fn main() {
	let shared_libs = std::env::var("DEP_SLANG_COMPILER_SHARED_LIBS").unwrap();

	build_rs::output::metadata("ARTIFACTS", &shared_libs);
}
