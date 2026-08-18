//! Rust bindings for the Slang shader language compiler

pub mod reflection;

#[cfg(test)]
mod tests;

use std::ffi::{CStr, CString};
use std::marker::PhantomData;
use std::ptr::{null, null_mut};

use bitflags::bitflags;
pub(crate) use shader_slang_sys as sys;
use sys::{
	slang_CompilerOptionName as CompilerOptionName,
	slang_CompilerOptionValueKind as CompilerOptionValueKind,
};

pub use sys::{slang_Modifier as Modifier, SlangUUID as UUID};

macro_rules! vcall {
	($self:expr, $method:ident($($args:expr),*)) => {
		unsafe { ($self.vtable().$method)($self.as_raw(), $($args),*) }
	};
}

const fn uuid(uuid: u128) -> UUID {
	UUID {
		data1: (uuid >> 96) as u32,
		data2: ((uuid >> 80) & 0xffff) as u16,
		data3: ((uuid >> 64) & 0xffff) as u16,
		data4: (uuid as u64).to_be_bytes(),
	}
}

pub enum Error {
	Code(sys::SlangResult),
	Blob(Blob),
}

impl std::fmt::Debug for Error {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Error::Code(code) => write!(f, "{}", code),
			Error::Blob(blob) => write!(f, "{}", blob.as_str().unwrap_or_default()),
		}
	}
}

impl std::fmt::Display for Error {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		std::fmt::Debug::fmt(self, f)
	}
}

unsafe impl Send for Error {}
unsafe impl Sync for Error {}
impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn succeeded(result: sys::SlangResult) -> bool {
	result >= 0
}

fn result_from_blob(code: sys::SlangResult, blob: *mut sys::slang_IBlob) -> Result<()> {
	if code < 0 && !blob.is_null() {
		Err(Error::Blob(Blob(IUnknown(
			std::ptr::NonNull::new(blob as *mut _).unwrap(),
		))))
	} else if code < 0 {
		Err(Error::Code(code))
	} else {
		Ok(())
	}
}

#[derive(Clone, Copy)]
pub struct ProfileID(sys::SlangProfileID);

impl ProfileID {
	pub const UNKNOWN: ProfileID = ProfileID(sys::SlangProfileID_SlangProfileUnknown);

	pub fn is_unknown(&self) -> bool {
		self.0 == sys::SlangProfileID_SlangProfileUnknown
	}
}

#[derive(Clone, Copy)]
pub struct CapabilityID(sys::SlangCapabilityID);

impl CapabilityID {
	pub const UNKNOWN: CapabilityID = CapabilityID(sys::SlangCapabilityID_SlangCapabilityUnknown);

	pub fn is_unknown(&self) -> bool {
		self.0 == sys::SlangCapabilityID_SlangCapabilityUnknown
	}
}

pub unsafe trait Interface: Sized + Clone {
	#[doc(hidden)]
	type Vtable;

	const IID: UUID;

	#[doc(hidden)]
	#[inline(always)]
	unsafe fn vtable(&self) -> &Self::Vtable {
		unsafe { &**(self.as_raw() as *mut *mut Self::Vtable) }
	}

	#[doc(hidden)]
	#[inline(always)]
	unsafe fn as_raw<T>(&self) -> *mut T {
		unsafe { std::mem::transmute_copy(self) }
	}

	fn as_unknown(&self) -> &IUnknown {
		// SAFETY: It is always safe to treat an `Interface` as an `IUnknown`.
		unsafe { std::mem::transmute(self) }
	}
}

#[repr(transparent)]
pub struct IUnknown(std::ptr::NonNull<std::ffi::c_void>);

unsafe impl Interface for IUnknown {
	type Vtable = sys::ISlangUnknown__bindgen_vtable;
	const IID: UUID = uuid(0x00000000_0000_0000_c000000000000046);
}

impl Clone for IUnknown {
	fn clone(&self) -> Self {
		vcall!(self, ISlangUnknown_addRef());
		Self(self.0)
	}
}

impl Drop for IUnknown {
	fn drop(&mut self) {
		vcall!(self, ISlangUnknown_release());
	}
}

#[repr(transparent)]
#[derive(Clone)]
pub struct Blob(IUnknown);

unsafe impl Interface for Blob {
	type Vtable = sys::IBlobVtable;
	const IID: UUID = uuid(0x8ba5fb08_5195_40e2_ac580d989c3a0102);
}

impl Blob {
	pub fn as_slice(&self) -> &[u8] {
		let ptr = vcall!(self, getBufferPointer());
		let size = vcall!(self, getBufferSize());
		unsafe { std::slice::from_raw_parts(ptr as *const u8, size) }
	}

	pub fn as_str(&self) -> std::result::Result<&str, std::str::Utf8Error> {
		std::str::from_utf8(self.as_slice())
	}
}

#[repr(transparent)]
#[derive(Clone)]
pub struct GlobalSession(IUnknown);

unsafe impl Interface for GlobalSession {
	type Vtable = sys::IGlobalSessionVtable;
	const IID: UUID = uuid(0xc140b5fd_0c78_452e_ba7c1a1e70c7f71c);
}

impl GlobalSession {
	pub fn new() -> Option<GlobalSession> {
		let mut global_session = null_mut();
		unsafe { sys::slang_createGlobalSession(sys::SLANG_API_VERSION as _, &mut global_session) };
		Some(GlobalSession(IUnknown(std::ptr::NonNull::new(
			global_session as *mut _,
		)?)))
	}

	pub fn new_without_core_module() -> Option<GlobalSession> {
		let mut global_session = null_mut();
		unsafe {
			sys::slang_createGlobalSessionWithoutCoreModule(
				sys::SLANG_API_VERSION as _,
				&mut global_session,
			)
		};
		Some(GlobalSession(IUnknown(std::ptr::NonNull::new(
			global_session as *mut _,
		)?)))
	}

	pub fn create_session(&self, desc: &SessionDesc) -> Option<Session> {
		let mut session = null_mut();
		vcall!(self, createSession(&**desc, &mut session));
		Some(Session(IUnknown(std::ptr::NonNull::new(
			session as *mut _,
		)?)))
	}

	pub fn find_profile(&self, name: &str) -> ProfileID {
		let name = CString::new(name).unwrap();
		ProfileID(vcall!(self, findProfile(name.as_ptr())))
	}

	pub fn find_capability(&self, name: &str) -> CapabilityID {
		let name = CString::new(name).unwrap();
		CapabilityID(vcall!(self, findCapability(name.as_ptr())))
	}

	pub fn build_tag_string(&self) -> &str {
		let tag = vcall!(self, getBuildTagString());
		unsafe { CStr::from_ptr(tag).to_str().unwrap() }
	}
}

#[repr(transparent)]
#[derive(Clone)]
pub struct Session(IUnknown);

unsafe impl Interface for Session {
	type Vtable = sys::ISessionVtable;
	const IID: UUID = uuid(0x67618701_d116_468f_ab3b474bedce0e3d);
}

impl Session {
	pub fn load_module(&self, name: &str) -> Result<Module> {
		let name = CString::new(name).unwrap();
		let mut diagnostics = null_mut();

		let module = vcall!(self, loadModule(name.as_ptr(), &mut diagnostics));

		if module.is_null() {
			let blob = Blob(IUnknown(
				std::ptr::NonNull::new(diagnostics as *mut _).unwrap(),
			));
			Err(Error::Blob(blob))
		} else {
			let module = Module(IUnknown(std::ptr::NonNull::new(module as *mut _).unwrap()));
			unsafe { (module.as_unknown().vtable().ISlangUnknown_addRef)(module.as_raw()) };
			Ok(module)
		}
	}

	pub fn load_module_from_source_string(
		&self,
		module_name: &str,
		path: &str,
		source: &str,
	) -> Result<Module> {
		let module_name = CString::new(module_name).unwrap();
		let path = CString::new(path).unwrap();
		let source = CString::new(source).unwrap();
		let mut diagnostics = null_mut();

		let module = vcall!(
			self,
			loadModuleFromSourceString(
				module_name.as_ptr(),
				path.as_ptr(),
				source.as_ptr(),
				&mut diagnostics
			)
		);

		if module.is_null() {
			let blob = Blob(IUnknown(
				std::ptr::NonNull::new(diagnostics as *mut _).unwrap(),
			));
			Err(Error::Blob(blob))
		} else {
			let module = Module(IUnknown(std::ptr::NonNull::new(module as *mut _).unwrap()));
			unsafe { (module.as_unknown().vtable().ISlangUnknown_addRef)(module.as_raw()) };
			Ok(module)
		}
	}

	pub fn load_module_from_ir_blob(
		&self,
		module_name: &str,
		path: &str,
		ir_blob: &Blob,
	) -> Result<Module> {
		let module_name = CString::new(module_name).unwrap();
		let path = CString::new(path).unwrap();
		let mut diagnostics = null_mut();

		let module = vcall!(
			self,
			loadModuleFromIRBlob(
				module_name.as_ptr(),
				path.as_ptr(),
				ir_blob.as_raw(),
				&mut diagnostics
			)
		);

		if module.is_null() {
			let blob = Blob(IUnknown(
				std::ptr::NonNull::new(diagnostics as *mut _).unwrap(),
			));
			Err(Error::Blob(blob))
		} else {
			let module = Module(IUnknown(std::ptr::NonNull::new(module as *mut _).unwrap()));
			unsafe { (module.as_unknown().vtable().ISlangUnknown_addRef)(module.as_raw()) };
			Ok(module)
		}
	}

	pub fn create_composite_component_type(
		&self,
		components: &[ComponentType],
	) -> Result<ComponentType> {
		let mut composite_component_type = null_mut();
		let mut diagnostics = null_mut();

		result_from_blob(
			vcall!(
				self,
				createCompositeComponentType(
					components.as_ptr() as _,
					components.len() as _,
					&mut composite_component_type,
					&mut diagnostics
				)
			),
			diagnostics,
		)?;

		Ok(ComponentType(IUnknown(
			std::ptr::NonNull::new(composite_component_type as *mut _).unwrap(),
		)))
	}
}

#[repr(transparent)]
#[derive(Clone)]
pub struct Metadata(IUnknown);

unsafe impl Interface for Metadata {
	type Vtable = sys::IMetadataVtable;
	const IID: UUID = uuid(0x8044a8a3_ddc0_4b7f_af8e026e905d7332);
}

impl Metadata {
	pub fn is_parameter_location_used(
		&self,
		category: ParameterCategory,
		space_index: u64,
		register_index: u64,
	) -> Option<bool> {
		let mut used = false;
		let result = vcall!(
			self,
			isParameterLocationUsed(category.into_raw(), space_index, register_index, &mut used)
		);
		succeeded(result).then(|| used)
	}
}

#[repr(transparent)]
#[derive(Clone)]
pub struct ComponentType(IUnknown);

unsafe impl Interface for ComponentType {
	type Vtable = sys::IComponentTypeVtable;
	const IID: UUID = uuid(0x5bc42be8_5c50_4929_9e5ed15e7c24015f);
}

impl ComponentType {
	pub fn layout(&self, target: i64) -> Result<&reflection::Shader> {
		let mut diagnostics = null_mut();
		let ptr = vcall!(self, getLayout(target, &mut diagnostics));

		if ptr.is_null() {
			Err(Error::Blob(Blob(IUnknown(
				std::ptr::NonNull::new(diagnostics as *mut _).unwrap(),
			))))
		} else {
			Ok(unsafe { &*(ptr as *const _) })
		}
	}

	pub fn link(&self) -> Result<ComponentType> {
		let mut linked_component_type = null_mut();
		let mut diagnostics = null_mut();

		result_from_blob(
			vcall!(self, link(&mut linked_component_type, &mut diagnostics)),
			diagnostics,
		)?;

		Ok(ComponentType(IUnknown(
			std::ptr::NonNull::new(linked_component_type as *mut _).unwrap(),
		)))
	}

	pub fn target_code(&self, target: i64) -> Result<Blob> {
		let mut code = null_mut();
		let mut diagnostics = null_mut();

		result_from_blob(
			vcall!(self, getTargetCode(target, &mut code, &mut diagnostics)),
			diagnostics,
		)?;

		Ok(Blob(IUnknown(
			std::ptr::NonNull::new(code as *mut _).unwrap(),
		)))
	}

	pub fn entry_point_code(&self, index: i64, target: i64) -> Result<Blob> {
		let mut code = null_mut();
		let mut diagnostics = null_mut();

		result_from_blob(
			vcall!(
				self,
				getEntryPointCode(index, target, &mut code, &mut diagnostics)
			),
			diagnostics,
		)?;

		Ok(Blob(IUnknown(
			std::ptr::NonNull::new(code as *mut _).unwrap(),
		)))
	}

	pub fn target_metadata(&self, target_index: i64) -> Result<Metadata> {
		let mut metadata = null_mut();
		let mut diagnostics = null_mut();

		result_from_blob(
			vcall!(
				self,
				getTargetMetadata(target_index, &mut metadata, &mut diagnostics)
			),
			diagnostics,
		)?;

		Ok(Metadata(IUnknown(
			std::ptr::NonNull::new(metadata as *mut _).unwrap(),
		)))
	}

	pub fn entry_point_metadata(
		&self,
		entry_point_index: i64,
		target_index: i64,
	) -> Result<Metadata> {
		let mut metadata = null_mut();
		let mut diagnostics = null_mut();

		result_from_blob(
			vcall!(
				self,
				getEntryPointMetadata(
					entry_point_index,
					target_index,
					&mut metadata,
					&mut diagnostics
				)
			),
			diagnostics,
		)?;

		Ok(Metadata(IUnknown(
			std::ptr::NonNull::new(metadata as *mut _).unwrap(),
		)))
	}
}

#[repr(transparent)]
#[derive(Clone)]
pub struct EntryPoint(IUnknown);

unsafe impl Interface for EntryPoint {
	type Vtable = sys::IEntryPointVtable;
	const IID: UUID = uuid(0x8f241361_f5bd_4ca0_a3ac02f7fa2402b8);
}

impl From<EntryPoint> for ComponentType {
	fn from(value: EntryPoint) -> Self {
		unsafe { std::mem::transmute(value) }
	}
}

impl EntryPoint {
	pub fn function_reflection(&self) -> &reflection::Function {
		let ptr = vcall!(self, getFunctionReflection());
		unsafe { &*(ptr as *const _) }
	}
}

#[repr(transparent)]
#[derive(Clone)]
pub struct TypeConformance(IUnknown);

unsafe impl Interface for TypeConformance {
	type Vtable = sys::ITypeConformanceVtable;
	const IID: UUID = uuid(0x73eb3147_e544_41b5_b8f0a244df21940b);
}

impl From<TypeConformance> for ComponentType {
	fn from(value: TypeConformance) -> Self {
		unsafe { std::mem::transmute(value) }
	}
}

#[repr(transparent)]
#[derive(Clone)]
pub struct Module(IUnknown);

unsafe impl Interface for Module {
	type Vtable = sys::IModuleVtable;
	const IID: UUID = uuid(0x0c720e64_8722_4d31_8990638a98b1c279);
}

impl From<Module> for ComponentType {
	fn from(value: Module) -> Self {
		unsafe { std::mem::transmute(value) }
	}
}

impl Module {
	pub fn find_entry_point_by_name(&self, name: &str) -> Option<EntryPoint> {
		let name = CString::new(name).unwrap();
		let mut entry_point = null_mut();
		vcall!(self, findEntryPointByName(name.as_ptr(), &mut entry_point));
		Some(EntryPoint(IUnknown(std::ptr::NonNull::new(
			entry_point as *mut _,
		)?)))
	}

	pub fn entry_point_count(&self) -> u32 {
		vcall!(self, getDefinedEntryPointCount()) as _
	}

	pub fn entry_point_by_index(&self, index: u32) -> Option<EntryPoint> {
		let mut entry_point = null_mut();
		vcall!(self, getDefinedEntryPoint(index as _, &mut entry_point));
		Some(EntryPoint(IUnknown(std::ptr::NonNull::new(
			entry_point as *mut _,
		)?)))
	}

	pub fn entry_points(&self) -> impl ExactSizeIterator<Item = EntryPoint> {
		(0..self.entry_point_count()).map(|i| self.entry_point_by_index(i).unwrap())
	}

	pub fn name(&self) -> &str {
		let name = vcall!(self, getName());
		unsafe { CStr::from_ptr(name).to_str().unwrap() }
	}

	pub fn file_path(&self) -> &str {
		let path = vcall!(self, getFilePath());
		unsafe { CStr::from_ptr(path).to_str().unwrap() }
	}

	pub fn unique_identity(&self) -> &str {
		let identity = vcall!(self, getUniqueIdentity());
		unsafe { CStr::from_ptr(identity).to_str().unwrap() }
	}

	pub fn dependency_file_count(&self) -> i32 {
		vcall!(self, getDependencyFileCount()) as i32
	}

	pub fn dependency_file_path(&self, index: i32) -> &str {
		let path = vcall!(self, getDependencyFilePath(index as i32));
		unsafe { CStr::from_ptr(path).to_str().unwrap() }
	}

	pub fn dependency_file_paths(&self) -> impl ExactSizeIterator<Item = &str> {
		(0..self.dependency_file_count()).map(|i| self.dependency_file_path(i))
	}

	pub fn module_reflection(&self) -> &reflection::Decl {
		let ptr = vcall!(self, getModuleReflection());
		unsafe { &*(ptr as *const _) }
	}
}

#[repr(transparent)]
pub struct TargetDesc<'a> {
	inner: sys::slang_TargetDesc,
	_phantom: PhantomData<&'a ()>,
}

impl std::ops::Deref for TargetDesc<'_> {
	type Target = sys::slang_TargetDesc;

	fn deref(&self) -> &Self::Target {
		&self.inner
	}
}

impl Default for TargetDesc<'_> {
	fn default() -> Self {
		Self {
			inner: sys::slang_TargetDesc {
				structureSize: std::mem::size_of::<sys::slang_TargetDesc>(),
				..unsafe { std::mem::zeroed() }
			},
			_phantom: PhantomData,
		}
	}
}

impl<'a> TargetDesc<'a> {
	pub fn format(mut self, format: CompileTarget) -> Self {
		self.inner.format = format as i32;
		self
	}

	pub fn profile(mut self, profile: ProfileID) -> Self {
		self.inner.profile = profile.0;
		self
	}

	pub fn options(mut self, options: &'a CompilerOptions) -> Self {
		self.inner.compilerOptionEntries = options.options.as_ptr() as _;
		self.inner.compilerOptionEntryCount = options.options.len() as _;
		self
	}
}

#[repr(transparent)]
pub struct SessionDesc<'a> {
	inner: sys::slang_SessionDesc,
	_phantom: PhantomData<&'a ()>,
}

impl std::ops::Deref for SessionDesc<'_> {
	type Target = sys::slang_SessionDesc;

	fn deref(&self) -> &Self::Target {
		&self.inner
	}
}

impl Default for SessionDesc<'_> {
	fn default() -> Self {
		Self {
			inner: sys::slang_SessionDesc {
				structureSize: std::mem::size_of::<sys::slang_SessionDesc>(),
				..unsafe { std::mem::zeroed() }
			},
			_phantom: PhantomData,
		}
	}
}

impl<'a> SessionDesc<'a> {
	pub fn targets(mut self, targets: &'a [TargetDesc]) -> Self {
		self.inner.targets = targets.as_ptr() as _;
		self.inner.targetCount = targets.len() as _;
		self
	}

	pub fn search_paths(mut self, paths: &'a [*const i8]) -> Self {
		self.inner.searchPaths = paths.as_ptr();
		self.inner.searchPathCount = paths.len() as _;
		self
	}

	pub fn options(mut self, options: &'a CompilerOptions) -> Self {
		self.inner.compilerOptionEntries = options.options.as_ptr() as _;
		self.inner.compilerOptionEntryCount = options.options.len() as _;
		self
	}
}

macro_rules! option {
	($name:ident, $func:ident($p_name:ident: $p_type:ident)) => {
		#[inline(always)]
		pub fn $func(self, $p_name: $p_type) -> Self {
			self.push_ints(CompilerOptionName::$name, $p_name as _, 0)
		}
	};

	($name:ident, $func:ident($p_name:ident: &str)) => {
		#[inline(always)]
		pub fn $func(self, $p_name: &str) -> Self {
			self.push_str1(CompilerOptionName::$name, $p_name)
		}
	};

	($name:ident, $func:ident($p_name1:ident: &str, $p_name2:ident: &str)) => {
		#[inline(always)]
		pub fn $func(self, $p_name1: &str, $p_name2: &str) -> Self {
			self.push_str2(CompilerOptionName::$name, $p_name1, $p_name2)
		}
	};
}

#[derive(Default)]
pub struct CompilerOptions {
	strings: Vec<CString>,
	options: Vec<sys::slang_CompilerOptionEntry>,
}

impl CompilerOptions {
	fn push_ints(mut self, name: CompilerOptionName, i0: i32, i1: i32) -> Self {
		self.options.push(sys::slang_CompilerOptionEntry {
			name: name,
			value: sys::slang_CompilerOptionValue {
				kind: CompilerOptionValueKind::Int,
				intValue0: i0,
				intValue1: i1,
				stringValue0: null(),
				stringValue1: null(),
			},
		});

		self
	}

	fn push_strings(mut self, name: CompilerOptionName, s0: *const i8, s1: *const i8) -> Self {
		self.options.push(sys::slang_CompilerOptionEntry {
			name: name,
			value: sys::slang_CompilerOptionValue {
				kind: CompilerOptionValueKind::String,
				intValue0: 0,
				intValue1: 0,
				stringValue0: s0,
				stringValue1: s1,
			},
		});

		self
	}

	fn push_str1(mut self, name: CompilerOptionName, s0: &str) -> Self {
		let s0 = CString::new(s0).unwrap();
		let s0_ptr = s0.as_ptr();
		self.strings.push(s0);

		self.push_strings(name, s0_ptr, null())
	}

	fn push_str2(mut self, name: CompilerOptionName, s0: &str, s1: &str) -> Self {
		let s0 = CString::new(s0).unwrap();
		let s0_ptr = s0.as_ptr();
		self.strings.push(s0);

		let s1 = CString::new(s1).unwrap();
		let s1_ptr = s1.as_ptr();
		self.strings.push(s1);

		self.push_strings(name, s0_ptr, s1_ptr)
	}
}

impl CompilerOptions {
	option!(MacroDefine, macro_define(key: &str, value: &str));
	option!(Include, include(path: &str));
	option!(Language, language(language: SourceLanguage));
	option!(MatrixLayoutColumn, matrix_layout_column(enable: bool));
	option!(MatrixLayoutRow, matrix_layout_row(enable: bool));

	#[inline(always)]
	pub fn profile(self, profile: ProfileID) -> Self {
		self.push_ints(CompilerOptionName::Profile, profile.0 as _, 0)
	}

	option!(Stage, stage(stage: Stage));
	option!(Target, target(target: CompileTarget));
	option!(WarningsAsErrors, warnings_as_errors(warning_codes: &str));
	option!(DisableWarnings, disable_warnings(warning_codes: &str));
	option!(EnableWarning, enable_warning(warning_code: &str));
	option!(DisableWarning, disable_warning(warning_code: &str));
	option!(ReportDownstreamTime, report_downstream_time(enable: bool));
	option!(ReportPerfBenchmark, report_perf_benchmark(enable: bool));
	option!(SkipSPIRVValidation, skip_spirv_validation(enable: bool));

	// Target
	#[inline(always)]
	pub fn capability(self, capability: CapabilityID) -> Self {
		self.push_ints(CompilerOptionName::Capability, capability.0 as _, 0)
	}

	option!(DefaultImageFormatUnknown, default_image_format_unknown(enable: bool));
	option!(DisableDynamicDispatch, disable_dynamic_dispatch(enable: bool));
	option!(DisableSpecialization, disable_specialization(enable: bool));
	option!(FloatingPointMode, floating_point_mode(mode: FloatingPointMode));
	option!(DebugInformation, debug_information(level: DebugInfoLevel));
	option!(LineDirectiveMode, line_directive_mode(mode: LineDirectiveMode));
	option!(Optimization, optimization(level: OptimizationLevel));
	option!(Obfuscate, obfuscate(enable: bool));
	option!(VulkanUseEntryPointName, vulkan_use_entry_point_name(enable: bool));
	option!(GLSLForceScalarLayout, glsl_force_scalar_layout(enable: bool));
	option!(EmitSpirvDirectly, emit_spirv_directly(enable: bool));

	// Debugging
	option!(NoCodeGen, no_code_gen(enable: bool));

	// Experimental
	option!(NoMangle, no_mangle(enable: bool));
	option!(ValidateUniformity, validate_uniformity(enable: bool));
}

/// Option to control emission of `#line` directives
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u32)]
#[non_exhaustive]
pub enum LineDirectiveMode {
	Default = sys::SlangLineDirectiveMode_Default,

	/// Emit GLSL-style directives with file number instead of name
	Glsl = sys::SlangLineDirectiveMode_Glsl,

	/// Don’t emit line directives at all.
	None = sys::SlangLineDirectiveMode_None,

	/// Use a source map to track line mappings (ie no #line will appear in emitting source)
	SourceMap = sys::SlangLineDirectiveMode_SourceMap,

	/// Emit standard C-style #line directives.
	Standard = sys::SlangLineDirectiveMode_Standard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u32)]
#[non_exhaustive]
pub enum TypeKind {
	None = sys::SlangTypeKind_None,
	Struct = sys::SlangTypeKind_Struct,
	Array = sys::SlangTypeKind_Array,
	Matrix = sys::SlangTypeKind_Matrix,
	Vector = sys::SlangTypeKind_Vector,
	Scalar = sys::SlangTypeKind_Scalar,
	ConstantBuffer = sys::SlangTypeKind_ConstantBuffer,
	Resource = sys::SlangTypeKind_Resource,
	SamplerState = sys::SlangTypeKind_SamplerState,
	TextureBuffer = sys::SlangTypeKind_TextureBuffer,
	ShaderStorageBuffer = sys::SlangTypeKind_ShaderStorageBuffer,
	ParameterBlock = sys::SlangTypeKind_ParameterBlock,
	GenericTypeParameter = sys::SlangTypeKind_GenericTypeParameter,
	Interface = sys::SlangTypeKind_Interface,
	OutputStream = sys::SlangTypeKind_OutputStream,
	MeshOutput = sys::SlangTypeKind_MeshOutput,
	Specialized = sys::SlangTypeKind_Specialized,
	Feedback = sys::SlangTypeKind_Feedback,
	Pointer = sys::SlangTypeKind_Pointer,
	DynamicResource = sys::SlangTypeKind_DynamicResource,
	Enum = sys::SlangTypeKind_Enum,
}

impl TypeKind {
	fn from_raw(raw: sys::SlangTypeKind) -> Self {
		match raw {
			sys::SlangTypeKind_None => Self::None,
			sys::SlangTypeKind_Struct => Self::Struct,
			sys::SlangTypeKind_Array => Self::Array,
			sys::SlangTypeKind_Matrix => Self::Matrix,
			sys::SlangTypeKind_Vector => Self::Vector,
			sys::SlangTypeKind_Scalar => Self::Scalar,
			sys::SlangTypeKind_ConstantBuffer => Self::ConstantBuffer,
			sys::SlangTypeKind_Resource => Self::Resource,
			sys::SlangTypeKind_SamplerState => Self::SamplerState,
			sys::SlangTypeKind_TextureBuffer => Self::TextureBuffer,
			sys::SlangTypeKind_ShaderStorageBuffer => Self::ShaderStorageBuffer,
			sys::SlangTypeKind_ParameterBlock => Self::ParameterBlock,
			sys::SlangTypeKind_GenericTypeParameter => Self::GenericTypeParameter,
			sys::SlangTypeKind_Interface => Self::Interface,
			sys::SlangTypeKind_OutputStream => Self::OutputStream,
			sys::SlangTypeKind_MeshOutput => Self::MeshOutput,
			sys::SlangTypeKind_Specialized => Self::Specialized,
			sys::SlangTypeKind_Feedback => Self::Feedback,
			sys::SlangTypeKind_Pointer => Self::Pointer,
			sys::SlangTypeKind_DynamicResource => Self::DynamicResource,
			sys::SlangTypeKind_Enum => Self::Enum,
			_ => unimplemented!(),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u32)]
#[non_exhaustive]
pub enum ScalarType {
	None = sys::SlangScalarType_None,
	Void = sys::SlangScalarType_Void,
	Bool = sys::SlangScalarType_Bool,
	Int32 = sys::SlangScalarType_Int32,
	Uint32 = sys::SlangScalarType_Uint32,
	Int64 = sys::SlangScalarType_Int64,
	Uint64 = sys::SlangScalarType_Uint64,
	Float16 = sys::SlangScalarType_Float16,
	Float32 = sys::SlangScalarType_Float32,
	Float64 = sys::SlangScalarType_Float64,
	Int8 = sys::SlangScalarType_Int8,
	Uint8 = sys::SlangScalarType_Uint8,
	Int16 = sys::SlangScalarType_Int16,
	Uint16 = sys::SlangScalarType_Uint16,
	Intptr = sys::SlangScalarType_Intptr,
	Uintptr = sys::SlangScalarType_Uintptr,
	Bfloat16 = sys::SlangScalarType_Bfloat16,
	FloatE4m3 = sys::SlangScalarType_FloatE4m3,
	FloatE5m2 = sys::SlangScalarType_FloatE5m2,
}

impl ScalarType {
	fn from_raw(raw: sys::SlangScalarType) -> Self {
		match raw {
			sys::SlangScalarType_None => Self::None,
			sys::SlangScalarType_Void => Self::Void,
			sys::SlangScalarType_Bool => Self::Bool,
			sys::SlangScalarType_Int32 => Self::Int32,
			sys::SlangScalarType_Uint32 => Self::Uint32,
			sys::SlangScalarType_Int64 => Self::Int64,
			sys::SlangScalarType_Uint64 => Self::Uint64,
			sys::SlangScalarType_Float16 => Self::Float16,
			sys::SlangScalarType_Float32 => Self::Float32,
			sys::SlangScalarType_Float64 => Self::Float64,
			sys::SlangScalarType_Int8 => Self::Int8,
			sys::SlangScalarType_Uint8 => Self::Uint8,
			sys::SlangScalarType_Int16 => Self::Int16,
			sys::SlangScalarType_Uint16 => Self::Uint16,
			sys::SlangScalarType_Intptr => Self::Intptr,
			sys::SlangScalarType_Uintptr => Self::Uintptr,
			sys::SlangScalarType_Bfloat16 => Self::Bfloat16,
			sys::SlangScalarType_FloatE4m3 => Self::FloatE4m3,
			sys::SlangScalarType_FloatE5m2 => Self::FloatE5m2,
			_ => unimplemented!(),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u32)]
#[non_exhaustive]
pub enum MatrixLayoutMode {
	ModeUnknown = sys::SlangMatrixLayoutMode_ModeUnknown,
	RowMajor = sys::SlangMatrixLayoutMode_RowMajor,
	ColumnMajor = sys::SlangMatrixLayoutMode_ColumnMajor,
}

impl MatrixLayoutMode {
	fn from_raw(raw: sys::SlangMatrixLayoutMode) -> Self {
		match raw {
			sys::SlangMatrixLayoutMode_ModeUnknown => Self::ModeUnknown,
			sys::SlangMatrixLayoutMode_RowMajor => Self::RowMajor,
			sys::SlangMatrixLayoutMode_ColumnMajor => Self::ColumnMajor,
			_ => unimplemented!(),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i32)]
#[non_exhaustive]
pub enum CompileTarget {
	Unknown = sys::SlangCompileTarget_TargetUnknown,
	None = sys::SlangCompileTarget_TargetNone,
	Glsl = sys::SlangCompileTarget_Glsl,
	GlslVulkanDeprecated = sys::SlangCompileTarget_GlslVulkanDeprecated,
	GlslVulkanOneDescDeprecated = sys::SlangCompileTarget_GlslVulkanOneDescDeprecated,
	Hlsl = sys::SlangCompileTarget_Hlsl,
	Spirv = sys::SlangCompileTarget_Spirv,
	SpirvAsm = sys::SlangCompileTarget_SpirvAsm,
	Dxbc = sys::SlangCompileTarget_Dxbc,
	DxbcAsm = sys::SlangCompileTarget_DxbcAsm,
	Dxil = sys::SlangCompileTarget_Dxil,
	DxilAsm = sys::SlangCompileTarget_DxilAsm,
	/// The C language
	CSource = sys::SlangCompileTarget_CSource,
	/// C++ code for shader kernels.
	CppSource = sys::SlangCompileTarget_CppSource,
	/// Standalone binary executable (for hosting CPU/OS)
	HostExecutable = sys::SlangCompileTarget_HostExecutable,
	/// A shared library/Dll for shader kernels (for hosting CPU/OS)
	ShaderSharedLibrary = sys::SlangCompileTarget_ShaderSharedLibrary,
	/// A CPU target that makes the compiled shader code available to be run immediately
	ShaderHostCallable = sys::SlangCompileTarget_ShaderHostCallable,
	/// Cuda source
	CudaSource = sys::SlangCompileTarget_CudaSource,
	/// PTX
	Ptx = sys::SlangCompileTarget_Ptx,
	/// Object code that contains CUDA functions.
	CudaObjectCode = sys::SlangCompileTarget_CudaObjectCode,
	/// Object code that can be used for later linking (kernel/shader)
	ObjectCode = sys::SlangCompileTarget_ObjectCode,
	/// C++ code for host library or executable.
	HostCppSource = sys::SlangCompileTarget_HostCppSource,
	/// Host callable host code (ie non kernel/shader)
	HostHostCallable = sys::SlangCompileTarget_HostHostCallable,
	/// C++ PyTorch binding code.
	CppPytorchBinding = sys::SlangCompileTarget_CppPytorchBinding,
	/// Metal shading language
	Metal = sys::SlangCompileTarget_Metal,
	/// Metal library
	MetalLib = sys::SlangCompileTarget_MetalLib,
	/// Metal library assembly
	MetalLibAsm = sys::SlangCompileTarget_MetalLibAsm,
	/// A shared library/Dll for host code (for hosting CPU/OS)
	HostSharedLibrary = sys::SlangCompileTarget_HostSharedLibrary,
	/// WebGPU shading language
	Wgsl = sys::SlangCompileTarget_Wgsl,
	/// SPIR-V assembly via WebGPU shading language
	WgslSpirvAsm = sys::SlangCompileTarget_WgslSpirvAsm,
	/// SPIR-V via WebGPU shading language
	WgslSpirv = sys::SlangCompileTarget_WgslSpirv,
	/// Bytecode that can be interpreted by the Slang VM
	HostVm = sys::SlangCompileTarget_HostVm,
	/// C++ header for shader kernels.
	CppHeader = sys::SlangCompileTarget_CppHeader,
	/// Cuda header
	CudaHeader = sys::SlangCompileTarget_CudaHeader,
	/// Host object code
	HostObjectCode = sys::SlangCompileTarget_HostObjectCode,
	/// Host LLVM IR assembly
	HostLlvmIr = sys::SlangCompileTarget_HostLlvmIr,
	/// Host LLVM IR assembly (kernel/shader)
	ShaderLlvmIr = sys::SlangCompileTarget_ShaderLlvmIr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u32)]
#[non_exhaustive]
pub enum DebugInfoLevel {
	/// Don't emit debug information at all.
	None = sys::SlangDebugInfoLevel_None,

	/// Emit as little debug information as possible, while still supporting stack trackers.
	Minimal = sys::SlangDebugInfoLevel_Minimal,

	/// Emit whatever is the standard level of debug information for each target.
	Standard = sys::SlangDebugInfoLevel_Standard,

	/// Emit as much debug information as possible for each target.
	Maximal = sys::SlangDebugInfoLevel_Maximal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u32)]
#[non_exhaustive]
pub enum OptimizationLevel {
	/// Don't optimize at all.
	None = sys::SlangOptimizationLevel_None,

	/// Default optimization level: balance code quality and compilation time.
	Default = sys::SlangOptimizationLevel_Default,

	/// Optimize aggressively.
	High = sys::SlangOptimizationLevel_High,

	/// Include optimizations that may take a very long time, or may involve severe space-vs-speed tradeoffs
	Maximal = sys::SlangOptimizationLevel_Maximal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u32)]
#[non_exhaustive]
pub enum DeclKind {
	UnsupportedForReflection = sys::SlangDeclKind_UnsupportedForReflection,
	Struct = sys::SlangDeclKind_Struct,
	Func = sys::SlangDeclKind_Func,
	Module = sys::SlangDeclKind_Module,
	Generic = sys::SlangDeclKind_Generic,
	Variable = sys::SlangDeclKind_Variable,
	Namespace = sys::SlangDeclKind_Namespace,
	Enum = sys::SlangDeclKind_Enum,
}

impl DeclKind {
	fn from_raw(raw: sys::SlangDeclKind) -> Self {
		match raw {
			sys::SlangDeclKind_UnsupportedForReflection => Self::UnsupportedForReflection,
			sys::SlangDeclKind_Struct => Self::Struct,
			sys::SlangDeclKind_Func => Self::Func,
			sys::SlangDeclKind_Module => Self::Module,
			sys::SlangDeclKind_Generic => Self::Generic,
			sys::SlangDeclKind_Variable => Self::Variable,
			sys::SlangDeclKind_Namespace => Self::Namespace,
			sys::SlangDeclKind_Enum => Self::Enum,
			_ => unimplemented!(),
		}
	}
}

/// Option to control floating-point precision guarantees for a target
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u32)]
#[non_exhaustive]
pub enum FloatingPointMode {
	Default = sys::SlangFloatingPointMode_Default,
	Fast = sys::SlangFloatingPointMode_Fast,
	Precise = sys::SlangFloatingPointMode_Precise,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u32)]
#[non_exhaustive]
pub enum ImageFormat {
	Unknown = sys::SlangImageFormat_SLANGIMAGEFORMATUnknown,
	Rgba32f = sys::SlangImageFormat_SLANGIMAGEFORMATRgba32f,
	Rgba16f = sys::SlangImageFormat_SLANGIMAGEFORMATRgba16f,
	Rg32f = sys::SlangImageFormat_SLANGIMAGEFORMATRg32f,
	Rg16f = sys::SlangImageFormat_SLANGIMAGEFORMATRg16f,
	R11fG11fB10f = sys::SlangImageFormat_SLANGIMAGEFORMATR11fG11fB10f,
	R32f = sys::SlangImageFormat_SLANGIMAGEFORMATR32f,
	R16f = sys::SlangImageFormat_SLANGIMAGEFORMATR16f,
	Rgba16 = sys::SlangImageFormat_SLANGIMAGEFORMATRgba16,
	Rgb10A2 = sys::SlangImageFormat_SLANGIMAGEFORMATRgb10A2,
	Rgba8 = sys::SlangImageFormat_SLANGIMAGEFORMATRgba8,
	Rg16 = sys::SlangImageFormat_SLANGIMAGEFORMATRg16,
	Rg8 = sys::SlangImageFormat_SLANGIMAGEFORMATRg8,
	R16 = sys::SlangImageFormat_SLANGIMAGEFORMATR16,
	R8 = sys::SlangImageFormat_SLANGIMAGEFORMATR8,
	Rgba16Snorm = sys::SlangImageFormat_SLANGIMAGEFORMATRgba16Snorm,
	Rgba8Snorm = sys::SlangImageFormat_SLANGIMAGEFORMATRgba8Snorm,
	Rg16Snorm = sys::SlangImageFormat_SLANGIMAGEFORMATRg16Snorm,
	Rg8Snorm = sys::SlangImageFormat_SLANGIMAGEFORMATRg8Snorm,
	R16Snorm = sys::SlangImageFormat_SLANGIMAGEFORMATR16Snorm,
	R8Snorm = sys::SlangImageFormat_SLANGIMAGEFORMATR8Snorm,
	Rgba32i = sys::SlangImageFormat_SLANGIMAGEFORMATRgba32i,
	Rgba16i = sys::SlangImageFormat_SLANGIMAGEFORMATRgba16i,
	Rgba8i = sys::SlangImageFormat_SLANGIMAGEFORMATRgba8i,
	Rg32i = sys::SlangImageFormat_SLANGIMAGEFORMATRg32i,
	Rg16i = sys::SlangImageFormat_SLANGIMAGEFORMATRg16i,
	Rg8i = sys::SlangImageFormat_SLANGIMAGEFORMATRg8i,
	R32i = sys::SlangImageFormat_SLANGIMAGEFORMATR32i,
	R16i = sys::SlangImageFormat_SLANGIMAGEFORMATR16i,
	R8i = sys::SlangImageFormat_SLANGIMAGEFORMATR8i,
	Rgba32ui = sys::SlangImageFormat_SLANGIMAGEFORMATRgba32ui,
	Rgba16ui = sys::SlangImageFormat_SLANGIMAGEFORMATRgba16ui,
	Rgb10A2ui = sys::SlangImageFormat_SLANGIMAGEFORMATRgb10A2ui,
	Rgba8ui = sys::SlangImageFormat_SLANGIMAGEFORMATRgba8ui,
	Rg32ui = sys::SlangImageFormat_SLANGIMAGEFORMATRg32ui,
	Rg16ui = sys::SlangImageFormat_SLANGIMAGEFORMATRg16ui,
	Rg8ui = sys::SlangImageFormat_SLANGIMAGEFORMATRg8ui,
	R32ui = sys::SlangImageFormat_SLANGIMAGEFORMATR32ui,
	R16ui = sys::SlangImageFormat_SLANGIMAGEFORMATR16ui,
	R8ui = sys::SlangImageFormat_SLANGIMAGEFORMATR8ui,
	R64ui = sys::SlangImageFormat_SLANGIMAGEFORMATR64ui,
	R64i = sys::SlangImageFormat_SLANGIMAGEFORMATR64i,
	Bgra8 = sys::SlangImageFormat_SLANGIMAGEFORMATBgra8,
}

impl ImageFormat {
	fn from_raw(raw: sys::SlangImageFormat) -> Self {
		match raw {
			sys::SlangImageFormat_SLANGIMAGEFORMATUnknown => Self::Unknown,
			sys::SlangImageFormat_SLANGIMAGEFORMATRgba32f => Self::Rgba32f,
			sys::SlangImageFormat_SLANGIMAGEFORMATRgba16f => Self::Rgba16f,
			sys::SlangImageFormat_SLANGIMAGEFORMATRg32f => Self::Rg32f,
			sys::SlangImageFormat_SLANGIMAGEFORMATRg16f => Self::Rg16f,
			sys::SlangImageFormat_SLANGIMAGEFORMATR11fG11fB10f => Self::R11fG11fB10f,
			sys::SlangImageFormat_SLANGIMAGEFORMATR32f => Self::R32f,
			sys::SlangImageFormat_SLANGIMAGEFORMATR16f => Self::R16f,
			sys::SlangImageFormat_SLANGIMAGEFORMATRgba16 => Self::Rgba16,
			sys::SlangImageFormat_SLANGIMAGEFORMATRgb10A2 => Self::Rgb10A2,
			sys::SlangImageFormat_SLANGIMAGEFORMATRgba8 => Self::Rgba8,
			sys::SlangImageFormat_SLANGIMAGEFORMATRg16 => Self::Rg16,
			sys::SlangImageFormat_SLANGIMAGEFORMATRg8 => Self::Rg8,
			sys::SlangImageFormat_SLANGIMAGEFORMATR16 => Self::R16,
			sys::SlangImageFormat_SLANGIMAGEFORMATR8 => Self::R8,
			sys::SlangImageFormat_SLANGIMAGEFORMATRgba16Snorm => Self::Rgba16Snorm,
			sys::SlangImageFormat_SLANGIMAGEFORMATRgba8Snorm => Self::Rgba8Snorm,
			sys::SlangImageFormat_SLANGIMAGEFORMATRg16Snorm => Self::Rg16Snorm,
			sys::SlangImageFormat_SLANGIMAGEFORMATRg8Snorm => Self::Rg8Snorm,
			sys::SlangImageFormat_SLANGIMAGEFORMATR16Snorm => Self::R16Snorm,
			sys::SlangImageFormat_SLANGIMAGEFORMATR8Snorm => Self::R8Snorm,
			sys::SlangImageFormat_SLANGIMAGEFORMATRgba32i => Self::Rgba32i,
			sys::SlangImageFormat_SLANGIMAGEFORMATRgba16i => Self::Rgba16i,
			sys::SlangImageFormat_SLANGIMAGEFORMATRgba8i => Self::Rgba8i,
			sys::SlangImageFormat_SLANGIMAGEFORMATRg32i => Self::Rg32i,
			sys::SlangImageFormat_SLANGIMAGEFORMATRg16i => Self::Rg16i,
			sys::SlangImageFormat_SLANGIMAGEFORMATRg8i => Self::Rg8i,
			sys::SlangImageFormat_SLANGIMAGEFORMATR32i => Self::R32i,
			sys::SlangImageFormat_SLANGIMAGEFORMATR16i => Self::R16i,
			sys::SlangImageFormat_SLANGIMAGEFORMATR8i => Self::R8i,
			sys::SlangImageFormat_SLANGIMAGEFORMATRgba32ui => Self::Rgba32ui,
			sys::SlangImageFormat_SLANGIMAGEFORMATRgba16ui => Self::Rgba16ui,
			sys::SlangImageFormat_SLANGIMAGEFORMATRgb10A2ui => Self::Rgb10A2ui,
			sys::SlangImageFormat_SLANGIMAGEFORMATRgba8ui => Self::Rgba8ui,
			sys::SlangImageFormat_SLANGIMAGEFORMATRg32ui => Self::Rg32ui,
			sys::SlangImageFormat_SLANGIMAGEFORMATRg16ui => Self::Rg16ui,
			sys::SlangImageFormat_SLANGIMAGEFORMATRg8ui => Self::Rg8ui,
			sys::SlangImageFormat_SLANGIMAGEFORMATR32ui => Self::R32ui,
			sys::SlangImageFormat_SLANGIMAGEFORMATR16ui => Self::R16ui,
			sys::SlangImageFormat_SLANGIMAGEFORMATR8ui => Self::R8ui,
			sys::SlangImageFormat_SLANGIMAGEFORMATR64ui => Self::R64ui,
			sys::SlangImageFormat_SLANGIMAGEFORMATR64i => Self::R64i,
			sys::SlangImageFormat_SLANGIMAGEFORMATBgra8 => Self::Bgra8,
			_ => unimplemented!(),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u32)]
#[non_exhaustive]
pub enum LayoutRules {
	Default = sys::SlangLayoutRules_Default,
	MetalArgumentBufferTier2 = sys::SlangLayoutRules_MetalArgumentBufferTier2,
	DefaultStructuredBuffer = sys::SlangLayoutRules_DefaultStructuredBuffer,
	DefaultConstantBuffer = sys::SlangLayoutRules_DefaultConstantBuffer,
}

/// Types of API-managed bindings that a parameter might use.
///
/// `BindingType` represents the distinct types of binding ranges that might be
/// understood by an underlying graphics API or cross-API abstraction layer.
/// Several of the enumeration cases here correspond to cases of `VkDescriptorType`
/// defined by the Vulkan API. Note however that the values of this enumeration
/// are not the same as those of any particular API.
///
/// The `BindingType` enumeration is distinct from `ParameterCategory`
/// because `ParameterCategory` differentiates the types of parameters for
/// the purposes of layout, where the layout rules of some targets will treat
/// parameters of different types as occupying the same binding space for layout
/// (e.g., in SPIR-V both a `Texture2D` and `SamplerState` use the same space of
/// `binding` indices, and are not allowed to overlap), while those same types
/// map to different types of bindings in the API (e.g., both textures and samplers
/// use different `VkDescriptorType` values).
///
/// When you want to answer "what register/binding did this parameter use?" you
/// should use `ParameterCategory`.
///
/// When you want to answer "what type of descriptor range should this parameter use?"
/// you should use `BindingType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u32)]
#[non_exhaustive]
pub enum BindingType {
	Unknown = sys::SlangBindingType_Unknown,
	Sampler = sys::SlangBindingType_Sampler,
	Texture = sys::SlangBindingType_Texture,
	ConstantBuffer = sys::SlangBindingType_ConstantBuffer,
	ParameterBlock = sys::SlangBindingType_ParameterBlock,
	TypedBuffer = sys::SlangBindingType_TypedBuffer,
	RawBuffer = sys::SlangBindingType_RawBuffer,
	CombinedTextureSampler = sys::SlangBindingType_CombinedTextureSampler,
	InputRenderTarget = sys::SlangBindingType_InputRenderTarget,
	InlineUniformData = sys::SlangBindingType_InlineUniformData,
	RayTracingAccelerationStructure = sys::SlangBindingType_RayTracingAccelerationStructure,
	VaryingInput = sys::SlangBindingType_VaryingInput,
	VaryingOutput = sys::SlangBindingType_VaryingOutput,
	ExistentialValue = sys::SlangBindingType_ExistentialValue,
	PushConstant = sys::SlangBindingType_PushConstant,
	MutableTexture = sys::SlangBindingType_MutableTeture,
	MutableTypedBuffer = sys::SlangBindingType_MutableTypedBuffer,
	MutableRawBuffer = sys::SlangBindingType_MutableRawBuffer,
}

impl BindingType {
	fn from_raw(raw: sys::SlangBindingType) -> Self {
		match raw {
			sys::SlangBindingType_Unknown => Self::Unknown,
			sys::SlangBindingType_Sampler => Self::Sampler,
			sys::SlangBindingType_Texture => Self::Texture,
			sys::SlangBindingType_ConstantBuffer => Self::ConstantBuffer,
			sys::SlangBindingType_ParameterBlock => Self::ParameterBlock,
			sys::SlangBindingType_TypedBuffer => Self::TypedBuffer,
			sys::SlangBindingType_RawBuffer => Self::RawBuffer,
			sys::SlangBindingType_CombinedTextureSampler => Self::CombinedTextureSampler,
			sys::SlangBindingType_InputRenderTarget => Self::InputRenderTarget,
			sys::SlangBindingType_InlineUniformData => Self::InlineUniformData,
			sys::SlangBindingType_RayTracingAccelerationStructure => {
				Self::RayTracingAccelerationStructure
			}
			sys::SlangBindingType_VaryingInput => Self::VaryingInput,
			sys::SlangBindingType_VaryingOutput => Self::VaryingOutput,
			sys::SlangBindingType_ExistentialValue => Self::ExistentialValue,
			sys::SlangBindingType_PushConstant => Self::PushConstant,
			sys::SlangBindingType_MutableTeture => Self::MutableTexture,
			sys::SlangBindingType_MutableTypedBuffer => Self::MutableTypedBuffer,
			sys::SlangBindingType_MutableRawBuffer => Self::MutableRawBuffer,
			_ => unimplemented!(),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ParameterCategory {
	None,
	Mixed,
	ConstantBuffer,
	ShaderResource,
	UnorderedAccess,
	VaryingInput,
	VaryingOutput,
	SamplerState,
	Uniform,
	DescriptorTableSlot,
	SpecializationConstant,
	PushConstantBuffer,
	RegisterSpace,
	Generic,
	RayPayload,
	HitAttributes,
	CallablePayload,
	ShaderRecord,
	ExistentialTypeParam,
	ExistentialObjectParam,
	SubElementRegisterSpace,
	Subpass,
	MetalArgumentBufferElement,
	MetalAttribute,
	MetalPayload,
	MetalBuffer,
	MetalTexture,
	MetalSampler,
}

impl ParameterCategory {
	fn from_raw(raw: sys::SlangParameterCategory) -> Self {
		match raw {
			sys::SlangParameterCategory_None => Self::None,
			sys::SlangParameterCategory_Mixed => Self::Mixed,
			sys::SlangParameterCategory_ConstantBuffer => Self::ConstantBuffer,
			sys::SlangParameterCategory_ShaderResource => Self::ShaderResource,
			sys::SlangParameterCategory_UnorderedAccess => Self::UnorderedAccess,
			sys::SlangParameterCategory_VaryingInput => Self::VaryingInput,
			sys::SlangParameterCategory_VaryingOutput => Self::VaryingOutput,
			sys::SlangParameterCategory_SamplerState => Self::SamplerState,
			sys::SlangParameterCategory_Uniform => Self::Uniform,
			sys::SlangParameterCategory_DescriptorTableSlot => Self::DescriptorTableSlot,
			sys::SlangParameterCategory_SpecializationConstant => Self::SpecializationConstant,
			sys::SlangParameterCategory_PushConstantBuffer => Self::PushConstantBuffer,
			sys::SlangParameterCategory_RegisterSpace => Self::RegisterSpace,
			sys::SlangParameterCategory_Generic => Self::Generic,
			sys::SlangParameterCategory_RayPayload => Self::RayPayload,
			sys::SlangParameterCategory_HitAttributes => Self::HitAttributes,
			sys::SlangParameterCategory_CallablePayload => Self::CallablePayload,
			sys::SlangParameterCategory_ShaderRecord => Self::ShaderRecord,
			sys::SlangParameterCategory_ExistentialTypeParam => Self::ExistentialTypeParam,
			sys::SlangParameterCategory_ExistentialObjectParam => Self::ExistentialObjectParam,
			sys::SlangParameterCategory_SubElementRegisterSpace => Self::SubElementRegisterSpace,
			sys::SlangParameterCategory_Subpass => Self::Subpass,
			sys::SlangParameterCategory_MetalArgumentBufferElement => {
				Self::MetalArgumentBufferElement
			}
			sys::SlangParameterCategory_MetalAttribute => Self::MetalAttribute,
			sys::SlangParameterCategory_MetalPayload => Self::MetalPayload,
			_ => unimplemented!(),
			// Would be unreachable:
			// sys::SlangParameterCategory_MetalBuffer => Self::MetalBuffer,
			// sys::SlangParameterCategory_MetalTexture => Self::MetalTexture,
			// sys::SlangParameterCategory_MetalSampler => Self::MetalSampler,
		}
	}

	fn into_raw(self) -> sys::SlangParameterCategory {
		match self {
			Self::None => sys::SlangParameterCategory_None,
			Self::Mixed => sys::SlangParameterCategory_Mixed,
			Self::ConstantBuffer => sys::SlangParameterCategory_ConstantBuffer,
			Self::ShaderResource => sys::SlangParameterCategory_ShaderResource,
			Self::UnorderedAccess => sys::SlangParameterCategory_UnorderedAccess,
			Self::VaryingInput => sys::SlangParameterCategory_VaryingInput,
			Self::VaryingOutput => sys::SlangParameterCategory_VaryingOutput,
			Self::SamplerState => sys::SlangParameterCategory_SamplerState,
			Self::Uniform => sys::SlangParameterCategory_Uniform,
			Self::DescriptorTableSlot => sys::SlangParameterCategory_DescriptorTableSlot,
			Self::SpecializationConstant => sys::SlangParameterCategory_SpecializationConstant,
			Self::PushConstantBuffer => sys::SlangParameterCategory_PushConstantBuffer,
			Self::RegisterSpace => sys::SlangParameterCategory_RegisterSpace,
			Self::Generic => sys::SlangParameterCategory_Generic,
			Self::RayPayload => sys::SlangParameterCategory_RayPayload,
			Self::HitAttributes => sys::SlangParameterCategory_HitAttributes,
			Self::CallablePayload => sys::SlangParameterCategory_CallablePayload,
			Self::ShaderRecord => sys::SlangParameterCategory_ShaderRecord,
			Self::ExistentialTypeParam => sys::SlangParameterCategory_ExistentialTypeParam,
			Self::ExistentialObjectParam => sys::SlangParameterCategory_ExistentialObjectParam,
			Self::SubElementRegisterSpace => sys::SlangParameterCategory_SubElementRegisterSpace,
			Self::Subpass => sys::SlangParameterCategory_Subpass,
			Self::MetalArgumentBufferElement => {
				sys::SlangParameterCategory_MetalArgumentBufferElement
			}
			Self::MetalAttribute => sys::SlangParameterCategory_MetalAttribute,
			Self::MetalPayload => sys::SlangParameterCategory_MetalPayload,
			Self::MetalBuffer => sys::SlangParameterCategory_MetalBuffer,
			Self::MetalTexture => sys::SlangParameterCategory_MetalTexture,
			Self::MetalSampler => sys::SlangParameterCategory_MetalSampler,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u32)]
#[non_exhaustive]
pub enum ResourceAccess {
	None = sys::SlangResourceAccess_None,
	Read = sys::SlangResourceAccess_Read,
	ReadWrite = sys::SlangResourceAccess_ReadWrite,
	RasterOrdered = sys::SlangResourceAccess_RasterOrdered,
	Append = sys::SlangResourceAccess_Append,
	Consume = sys::SlangResourceAccess_Consume,
	Write = sys::SlangResourceAccess_Write,
	Feedback = sys::SlangResourceAccess_Feedback,
	Unknown = sys::SlangResourceAccess_Unknown,
}

impl ResourceAccess {
	fn from_raw(raw: sys::SlangResourceAccess) -> Self {
		match raw {
			sys::SlangResourceAccess_None => Self::None,
			sys::SlangResourceAccess_Read => Self::Read,
			sys::SlangResourceAccess_ReadWrite => Self::ReadWrite,
			sys::SlangResourceAccess_RasterOrdered => Self::RasterOrdered,
			sys::SlangResourceAccess_Append => Self::Append,
			sys::SlangResourceAccess_Consume => Self::Consume,
			sys::SlangResourceAccess_Write => Self::Write,
			sys::SlangResourceAccess_Feedback => Self::Feedback,
			sys::SlangResourceAccess_Unknown => Self::Unknown,
			_ => unimplemented!(),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i32)]
#[non_exhaustive]
pub enum SourceLanguage {
	Unknown = sys::SlangSourceLanguage_Unknown,
	Slang = sys::SlangSourceLanguage_Slang,
	Hlsl = sys::SlangSourceLanguage_Hlsl,
	Glsl = sys::SlangSourceLanguage_Glsl,
	C = sys::SlangSourceLanguage_C,
	Cpp = sys::SlangSourceLanguage_Cpp,
	Cuda = sys::SlangSourceLanguage_Cuda,
	Spirv = sys::SlangSourceLanguage_Spirv,
	Metal = sys::SlangSourceLanguage_Metal,
	Wgsl = sys::SlangSourceLanguage_Wgsl,
	Llvm = sys::SlangSourceLanguage_Llvm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u32)]
#[non_exhaustive]
pub enum Stage {
	None = sys::SlangStage_None,
	Vertex = sys::SlangStage_Vertex,
	Hull = sys::SlangStage_Hull,
	Domain = sys::SlangStage_Domain,
	Geometry = sys::SlangStage_Geometry,
	Fragment = sys::SlangStage_Fragment,
	Compute = sys::SlangStage_Compute,
	RayGeneration = sys::SlangStage_RayGeneration,
	Intersection = sys::SlangStage_Intersection,
	AnyHit = sys::SlangStage_AnyHit,
	ClosestHit = sys::SlangStage_ClosestHit,
	Miss = sys::SlangStage_Miss,
	Callable = sys::SlangStage_Callable,
	Mesh = sys::SlangStage_Mesh,
	Amplification = sys::SlangStage_Amplification,
	Dispatch = sys::SlangStage_Dispatch,
}

impl Stage {
	pub const PIXEL: Self = Self::Fragment;

	fn from_raw(raw: sys::SlangStage) -> Self {
		match raw {
			sys::SlangStage_None => Self::None,
			sys::SlangStage_Vertex => Self::Vertex,
			sys::SlangStage_Hull => Self::Hull,
			sys::SlangStage_Domain => Self::Domain,
			sys::SlangStage_Geometry => Self::Geometry,
			sys::SlangStage_Fragment => Self::Fragment,
			sys::SlangStage_Compute => Self::Compute,
			sys::SlangStage_RayGeneration => Self::RayGeneration,
			sys::SlangStage_Intersection => Self::Intersection,
			sys::SlangStage_AnyHit => Self::AnyHit,
			sys::SlangStage_ClosestHit => Self::ClosestHit,
			sys::SlangStage_Miss => Self::Miss,
			sys::SlangStage_Callable => Self::Callable,
			sys::SlangStage_Mesh => Self::Mesh,
			sys::SlangStage_Amplification => Self::Amplification,
			sys::SlangStage_Dispatch => Self::Dispatch,
			_ => unimplemented!(),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u32)]
#[non_exhaustive]
pub enum ModifierID {
	Shared = sys::SlangModifierID_SlangModifierShared,
	NoDiff = sys::SlangModifierID_SlangModifierNoDiff,
	Static = sys::SlangModifierID_SlangModifierStatic,
	Const = sys::SlangModifierID_SlangModifierConst,
	Export = sys::SlangModifierID_SlangModifierExport,
	Extern = sys::SlangModifierID_SlangModifierExtern,
	Differentiable = sys::SlangModifierID_SlangModifierDifferentiable,
	Mutating = sys::SlangModifierID_SlangModifierMutating,
	In = sys::SlangModifierID_SlangModifierIn,
	Out = sys::SlangModifierID_SlangModifierOut,
	Inout = sys::SlangModifierID_SlangModifierInout,
}

bitflags! {
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceShape: u32 {
	const BASE_MASK = sys::SlangResourceShape_SlangResourceBaseShapeMask;
	const NONE = sys::SlangResourceShape_SlangResourceNone;
	const TEXTURE1D = sys::SlangResourceShape_SlangTexture1d;
	const TEXTURE2D = sys::SlangResourceShape_SlangTexture2d;
	const TEXTURE3D = sys::SlangResourceShape_SlangTexture3d;
	const TEXTURE_CUBE = sys::SlangResourceShape_SlangTextureCube;
	const TEXTURE_BUFFER = sys::SlangResourceShape_SlangTextureBuffer;
	const STRUCTURED_BUFFER = sys::SlangResourceShape_SlangStructuredBuffer;
	const BYTE_ADDRESS_BUFFER = sys::SlangResourceShape_SlangByteAddressBuffer;
	const UNKNOWN = sys::SlangResourceShape_SlangResourceUnknown;
	const ACCELERATION_STRUCTURE = sys::SlangResourceShape_SlangAccelerationStructure;
	const TEXTURE_SUBPASS = sys::SlangResourceShape_SlangTextureSubpass;
	const EXT_MASK = sys::SlangResourceShape_SlangResourceExtShapeMask;
	const TEXTURE_FEEDBACK_FLAG = sys::SlangResourceShape_SlangTextureFeedbackFlag;
	const TEXTURE_SHADOW_FLAG = sys::SlangResourceShape_SlangTextureShadowFlag;
	const TEXTURE_ARRAY_FLAG = sys::SlangResourceShape_SlangTextureArrayFlag;
	const TEXTURE_MULTISAMPLE_FLAG = sys::SlangResourceShape_SlangTextureMultisampleFlag;
	const TEXTURE_COMBINED_FLAG = sys::SlangResourceShape_SlangTextureCombinedFlag;
	const TEXTURE1D_ARRAY = sys::SlangResourceShape_SlangTexture1dArray;
	const TEXTURE2D_ARRAY = sys::SlangResourceShape_SlangTexture2dArray;
	const TEXTURE_CUBE_ARRAY = sys::SlangResourceShape_SlangTextureCubeArray;
	const TEXTURE2D_MULTISAMPLE = sys::SlangResourceShape_SlangTexture2dMultisample;
	const TEXTURE2D_MULTISAMPLE_ARRAY = sys::SlangResourceShape_SlangTexture2dMultisampleArray;
	const TEXTURE_SUBPASS_MULTISAMPLE = sys::SlangResourceShape_SlangTextureSubpassMultisample;
}
}
