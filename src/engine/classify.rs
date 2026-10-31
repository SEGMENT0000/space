//! Classification of a filesystem entry into one of eight product categories.
//!
//! Known extensions decide a file's category before directory context. A
//! picture inside `target` is still Media, and an archive inside `.cargo` is
//! still Archives. DLLs are Apps except inside the drive/share-root Windows
//! directory, where they are System.
//!
//! The hot scan path precomputes a [`DirContext`] once per directory and then
//! classifies every entry in it without touching the path again. The public
//! [`classify_file`] / [`classify_entry`] helpers build the same context from
//! a `Path` for callers outside the walker.
//!
//! Priority when several rules match:
//!
//! 1. The extension table (with the Windows DLL exception).
//! 2. Known system, cache and build filenames without a recognized extension.
//! 3. A strong path context: System, Apps, Code or Caches.
//! 4. A soft path context (Documents/Pictures/Archives/`src`), then `Other`.
//!
//! Missing and unrecognized extensions both reach the fallback rules.
//! Directories use path context only, never a filename-extension rule.

use std::path::Path;

use crate::model::Category;

/// Longest extension we lower-case into the stack buffer. Anything longer can
/// never match an ASCII table entry, so treating it as "no extension" is the
/// same outcome as a table miss.
const MAX_EXT: usize = 16;

/// Forced path contexts, ordered by precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Forced {
    Caches,
    Code,
    Apps,
    System,
}

impl Forced {
    fn rank(self) -> u8 {
        match self {
            Self::Caches => 1,
            Self::Code => 2,
            Self::Apps => 3,
            Self::System => 4,
        }
    }

    fn category(self) -> Category {
        match self {
            Self::Caches => Category::Caches,
            Self::Code => Category::Code,
            Self::Apps => Category::Apps,
            Self::System => Category::System,
        }
    }
}

/// Transient state used for the `AppData\Local\Programs` rule. It is only
/// carried for the immediate next component: `AppData\Local\Programs` is an
/// installed-apps location, while `AppData\Local\Foo\Programs` is not
/// automatically one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum AppDataState {
    #[default]
    None,
    Seen,
    Local,
}

/// Path context inherited by every entry inside a directory.
///
/// `Copy` and tiny: the walker stores one per directory, never one per file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct DirContext {
    forced: Option<Forced>,
    fallback: Option<Category>,
    appdata: AppDataState,
    /// Separate from general System context: project DLLs are not OS files.
    windows: bool,
}

impl DirContext {
    /// Context implied by a directory path. Used for the scan root and by the
    /// public helpers; the walker derives every deeper directory from its
    /// parent instead of re-scanning the full path.
    pub(crate) fn from_path(path: &Path) -> Self {
        let text = path.to_string_lossy();
        Self::from_str(&text)
    }

    fn from_str(text: &str) -> Self {
        let (components, root) = split_components(text);
        let mut ctx = Self::default();
        for (index, component) in components.iter().enumerate() {
            ctx.apply_component(component, index == root);
        }
        ctx
    }

    /// Context of a child directory, derived from the parent's context and
    /// the child's name. Exactly equivalent to [`Self::from_path`] on the
    /// child path for the rules that matter to a scan, without re-splitting
    /// the whole path per directory.
    pub(crate) fn child(&self, name: &str, parent_is_drive_root: bool) -> Self {
        let mut out = *self;

        // Transient AppData state.
        match self.appdata {
            AppDataState::None => {
                if name.eq_ignore_ascii_case("appdata") {
                    out.appdata = AppDataState::Seen;
                }
            }
            AppDataState::Seen => {
                out.appdata = if name.eq_ignore_ascii_case("local") {
                    AppDataState::Local
                } else {
                    AppDataState::None
                };
            }
            AppDataState::Local => {
                if name.eq_ignore_ascii_case("programs") {
                    out.set_forced(Forced::Apps);
                }
                out.appdata = AppDataState::None;
            }
        }

        if parent_is_drive_root {
            out.windows |= name.eq_ignore_ascii_case("Windows");
            if is_root_system(name) {
                out.set_forced(Forced::System);
            } else if is_root_apps(name) {
                out.set_forced(Forced::Apps);
            } else if name.eq_ignore_ascii_case("$Recycle.Bin") {
                out.set_forced(Forced::Caches);
            }
        }

        if name.eq_ignore_ascii_case("WindowsApps")
            || name.eq_ignore_ascii_case("Applications")
            || ends_with_ci(name, ".app")
        {
            out.set_forced(Forced::Apps);
        }

        if is_system_component(name) {
            out.set_forced(Forced::System);
        }
        if is_code_dependency(name) {
            out.set_forced(Forced::Code);
        }
        if is_cache_component(name) {
            out.set_forced(Forced::Caches);
        }

        if is_documents_component(name) {
            out.fallback = Some(Category::Documents);
        }
        if is_media_component(name) {
            out.fallback = Some(Category::Media);
        }
        if is_archive_component(name) {
            out.fallback = Some(Category::Archives);
        }
        if is_code_component(name) {
            out.fallback = Some(Category::Code);
        }

        out
    }

    fn apply_component(&mut self, name: &str, at_root: bool) {
        match self.appdata {
            AppDataState::None => {
                if name.eq_ignore_ascii_case("appdata") {
                    self.appdata = AppDataState::Seen;
                }
            }
            AppDataState::Seen => {
                self.appdata = if name.eq_ignore_ascii_case("local") {
                    AppDataState::Local
                } else {
                    AppDataState::None
                };
            }
            AppDataState::Local => {
                if name.eq_ignore_ascii_case("programs") {
                    self.set_forced(Forced::Apps);
                }
                self.appdata = AppDataState::None;
            }
        }

        if at_root {
            self.windows |= name.eq_ignore_ascii_case("Windows");
            if is_root_system(name) {
                self.set_forced(Forced::System);
            } else if is_root_apps(name) {
                self.set_forced(Forced::Apps);
            } else if name.eq_ignore_ascii_case("$Recycle.Bin") {
                self.set_forced(Forced::Caches);
            }
        }

        if name.eq_ignore_ascii_case("WindowsApps")
            || name.eq_ignore_ascii_case("Applications")
            || ends_with_ci(name, ".app")
        {
            self.set_forced(Forced::Apps);
        }
        if is_system_component(name) {
            self.set_forced(Forced::System);
        }
        if is_code_dependency(name) {
            self.set_forced(Forced::Code);
        }
        if is_cache_component(name) {
            self.set_forced(Forced::Caches);
        }

        if is_documents_component(name) {
            self.fallback = Some(Category::Documents);
        }
        if is_media_component(name) {
            self.fallback = Some(Category::Media);
        }
        if is_archive_component(name) {
            self.fallback = Some(Category::Archives);
        }
        if is_code_component(name) {
            self.fallback = Some(Category::Code);
        }
    }

    fn set_forced(&mut self, forced: Forced) {
        let upgrade = self
            .forced
            .map(|current| forced.rank() > current.rank())
            .unwrap_or(true);
        if upgrade {
            self.forced = Some(forced);
        }
    }
}

/// Classify a file by name and the directory that contains it.
///
/// This is the stable public entry point used by the UI and tests. It is
/// read-only and does no I/O. Files without an extension are [`Category::Other`]
/// unless a path context or a known system/build/cache filename applies.
pub fn classify_file(name: &str, parent: &Path) -> Category {
    classify_entry(name, parent, false)
}

/// Classify a file or directory. Directories use the context implied by their
/// own name (so `node_modules`, `Temp`, `Program Files`, and `Pictures` are
/// classified sensibly); files use only their parent's context.
///
/// Added alongside [`classify_file`] for callers that know whether an entry is
/// a directory. The scanner uses it for every node.
pub fn classify_entry(name: &str, parent: &Path, is_dir: bool) -> Category {
    let parent_ctx = DirContext::from_path(parent);
    if is_dir {
        let child_ctx = parent_ctx.child(name, is_drive_root(parent));
        classify_with_ctx(name, child_ctx, true)
    } else {
        classify_with_ctx(name, parent_ctx, false)
    }
}

/// Classify with an already-computed directory context. `pub(crate)` because
/// the walkers are its only hot-path caller.
pub(crate) fn classify_with_ctx(name: &str, ctx: DirContext, is_dir: bool) -> Category {
    if is_dir {
        return ctx
            .forced
            .map(Forced::category)
            .or(ctx.fallback)
            .unwrap_or(Category::Other);
    }

    if let Some(category) = extension_category(name) {
        return if ctx.windows && ends_with_ci(name, ".dll") {
            Category::System
        } else {
            category
        };
    }

    if is_system_file_name(name) {
        return Category::System;
    }
    if is_cache_file_name(name) {
        return Category::Caches;
    }
    if is_code_file_name(name) {
        return Category::Code;
    }

    if let Some(forced) = ctx.forced {
        return forced.category();
    }
    if let Some(category) = ctx.fallback {
        return category;
    }
    Category::Other
}

/// True for a drive root (`C:\`) or a UNC share root (`\\server\share`).
pub(crate) fn is_drive_root(path: &Path) -> bool {
    let text = path.to_string_lossy();
    let (components, root) = split_components(&text);
    components.len() == root
}

fn split_components(text: &str) -> (Vec<&str>, usize) {
    let mut body = text;
    let mut verbatim = false;
    let mut unc = false;

    for prefix in ["\\\\?\\UNC\\", "\\\\?\\", "\\\\.\\"] {
        if let Some(rest) = body.strip_prefix(prefix) {
            body = rest;
            verbatim = true;
            unc = prefix == "\\\\?\\UNC\\";
            break;
        }
    }
    if !verbatim && let Some(rest) = body.strip_prefix("\\\\") {
        // Plain UNC path: the first two components are server and share.
        body = rest;
        unc = true;
    }

    let components: Vec<&str> = body
        .split(['\\', '/'])
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();

    let root = if unc {
        components.len().min(2)
    } else if components.first().is_some_and(|part| is_drive(part)) {
        1
    } else {
        0
    };
    (components, root)
}

fn is_drive(part: &str) -> bool {
    let bytes = part.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// True when `name` equals `suffix` ignoring ASCII case, without allocating.
fn ends_with_ci(name: &str, suffix: &str) -> bool {
    name.len() >= suffix.len()
        && name
            .as_bytes()
            .get(name.len() - suffix.len()..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix.as_bytes()))
}

fn eq_any(name: &str, list: &[&str]) -> bool {
    list.iter().any(|item| name.eq_ignore_ascii_case(item))
}

fn is_root_system(name: &str) -> bool {
    eq_any(
        name,
        &[
            "Windows",
            "Windows.old",
            "ProgramData",
            "Boot",
            "EFI",
            "Recovery",
            "System Volume Information",
            "$WinREAgent",
            "$SysReset",
            "Config.Msi",
            "PerfLogs",
        ],
    )
}

fn is_root_apps(name: &str) -> bool {
    eq_any(
        name,
        &[
            "Program Files",
            "Program Files (x86)",
            "WindowsApps",
            "Applications",
        ],
    )
}

fn is_system_component(name: &str) -> bool {
    eq_any(
        name,
        &[
            "drivers",
            "DriverStore",
            "System32",
            "SysWOW64",
            "WinSxS",
            "Windows.old",
        ],
    )
}

fn is_code_dependency(name: &str) -> bool {
    eq_any(
        name,
        &[
            "node_modules",
            "bower_components",
            "jspm_packages",
            "vendor",
            "vendors",
            "third_party",
            "third-party",
            "site-packages",
            "dist-packages",
            ".venv",
            "venv",
            "virtualenv",
            ".tox",
            ".cargo",
            ".rustup",
            ".gradle",
            ".m2",
            ".nuget",
            ".npm",
            ".yarn",
            ".pnpm-store",
            ".git",
            ".hg",
            ".svn",
            "target",
            "build",
            "dist",
            "out",
            "obj",
            "bin",
        ],
    )
}

fn is_cache_component(name: &str) -> bool {
    eq_any(
        name,
        &[
            "Temp",
            "Tmp",
            "Cache",
            "Caches",
            "Cache2",
            "CacheStorage",
            "Code Cache",
            "GPUCache",
            "GrShaderCache",
            "ShaderCache",
            "DawnCache",
            "GraphiteCache",
            "StartupCache",
            "Service Worker",
            "INetCache",
            "INetCookies",
            "Temporary Internet Files",
            "Thumbnails",
            "IconCache",
            "JumpListCache",
            "WebCache",
            "Crashpad",
            "CrashDumps",
            "Dumps",
            ".cache",
            "__pycache__",
            "_cacache",
            "Package Cache",
            "ParcelCache",
            ".parcel-cache",
            ".next",
            ".nuxt",
            ".turbo",
            ".svelte-kit",
            "$Recycle.Bin",
            "Delivery Optimization",
        ],
    )
}

fn is_documents_component(name: &str) -> bool {
    eq_any(
        name,
        &[
            "Documents",
            "Desktop",
            "Downloads",
            "OneDrive",
            "Dropbox",
            "Google Drive",
            "iCloud Drive",
            "My Documents",
        ],
    )
}

fn is_media_component(name: &str) -> bool {
    eq_any(
        name,
        &[
            "Pictures",
            "Videos",
            "Music",
            "DCIM",
            "Camera Roll",
            "Photos",
            "Screenshots",
            "Screen Recordings",
            "Recordings",
            "Media",
            "Images",
            "Wallpapers",
            "Audio",
            "Movies",
        ],
    )
}

fn is_archive_component(name: &str) -> bool {
    eq_any(name, &["Archives", "Archive", "Backups", "Backup"])
}

fn is_code_component(name: &str) -> bool {
    eq_any(
        name,
        &[
            "src",
            "source",
            "sources",
            "include",
            "repos",
            "repo",
            "projects",
            "workspace",
            "code",
            "dev",
            "development",
            "github",
            "gitlab",
        ],
    )
}

fn is_system_file_name(name: &str) -> bool {
    eq_any(
        name,
        &[
            "pagefile.sys",
            "hiberfil.sys",
            "swapfile.sys",
            "DumpStack.log",
            "DumpStack.log.tmp",
            "MEMORY.DMP",
            "bootmgr",
            "BOOTNXT",
            "bootTel.dat",
            "ntldr",
            "NTDETECT.COM",
            "boot.ini",
            "desktop.ini",
            "ntuser.dat",
            "ntuser.ini",
            "UsrClass.dat",
            "SYSTEM",
            "SOFTWARE",
            "SAM",
            "SECURITY",
            "DEFAULT",
            "BCD",
            "BCD.LOG",
            "BCD-BACKUP",
            "$MFT",
            "$LogFile",
            "$Volume",
            "$Bitmap",
            "$Boot",
            "$BadClus",
            "$Secure",
            "$UpCase",
            "$Extend",
            "$AttrDef",
            "$Quota",
            "$ObjId",
            "$Reparse",
        ],
    )
}

fn is_cache_file_name(name: &str) -> bool {
    if eq_any(name, &["thumbs.db", "ehthumbs.db", "IconCache.db"]) {
        return true;
    }
    // Operate on bytes: slicing a `str` at these offsets could panic for a
    // non-ASCII filename even though the pattern is ASCII.
    let bytes = name.as_bytes();
    bytes.len() >= 12
        && bytes[..6].eq_ignore_ascii_case(b"thumbc")
        && bytes[bytes.len() - 3..].eq_ignore_ascii_case(b".db")
}

fn is_code_file_name(name: &str) -> bool {
    eq_any(
        name,
        &[
            "Makefile",
            "GNUmakefile",
            "Dockerfile",
            "Containerfile",
            "CMakeLists.txt",
            "meson.build",
            "build.gradle",
            "settings.gradle",
            "gradlew",
            "mvnw",
            "Rakefile",
            "Gemfile",
            "Podfile",
            "Procfile",
            "Brewfile",
            "Justfile",
            "Taskfile",
            "Vagrantfile",
            "Pipfile",
            "Cargo.lock",
            "package-lock.json",
            "npm-shrinkwrap.json",
            "yarn.lock",
            "pnpm-lock.yaml",
            "composer.lock",
            "poetry.lock",
            "go.mod",
            "go.sum",
            "requirements.txt",
            "pyproject.toml",
            "setup.py",
            "setup.cfg",
            "tsconfig.json",
            "jsconfig.json",
            "vite.config.js",
            "vite.config.ts",
            "webpack.config.js",
            "rollup.config.js",
            "eslint.config.js",
            ".gitignore",
            ".gitattributes",
            ".gitmodules",
            ".editorconfig",
            ".dockerignore",
            ".npmrc",
            ".yarnrc",
            ".prettierrc",
            ".eslintrc",
            ".babelrc",
            ".env",
        ],
    ) || name.eq_ignore_ascii_case(".env.local")
}

/// Lower-case the extension of `name` into `buf`, mirroring `Path::extension`
/// semantics (`.gitignore` has no extension, `foo.` has none). Returns `None`
/// for long or non-ASCII extensions; those can never match the ASCII table.
fn extension_lower<'a>(name: &str, buf: &'a mut [u8; MAX_EXT]) -> Option<&'a [u8]> {
    let bytes = name.as_bytes();
    let dot = bytes.iter().rposition(|&byte| byte == b'.')?;
    if dot == 0 || dot + 1 == bytes.len() {
        return None;
    }
    let raw = &bytes[dot + 1..];
    if raw.len() > MAX_EXT {
        return None;
    }
    for (slot, &byte) in buf[..raw.len()].iter_mut().zip(raw) {
        if !byte.is_ascii() {
            return None;
        }
        *slot = byte.to_ascii_lowercase();
    }
    Some(&buf[..raw.len()])
}

fn extension_category(name: &str) -> Option<Category> {
    let mut buf = [0u8; MAX_EXT];
    let ext = extension_lower(name, &mut buf)?;
    classify_ext(ext)
}

fn classify_ext(ext: &[u8]) -> Option<Category> {
    Some(match ext {
        // Media — images.
        b"jpg" | b"jpeg" | b"jpe" | b"png" | b"gif" | b"webp" | b"bmp" | b"svg"
        | b"svgz" | b"heic" | b"heif" | b"avif" | b"jxl" | b"raw" | b"tiff" | b"tif"
        | b"ico" | b"cr2" | b"cr3" | b"nef" | b"arw" | b"dng" | b"orf" | b"raf"
        | b"rw2" | b"psd" | b"ai" | b"indd" | b"eps" | b"blend" | b"fbx"
        | b"stl" | b"gltf" | b"glb"
        // Media — video.
        | b"mp4" | b"mov" | b"avi" | b"mkv" | b"webm" | b"m4v" | b"wmv" | b"flv"
        | b"mpg" | b"mpeg" | b"m2ts" | b"mts" | b"vob" | b"ogv" | b"3gp"
        // Media — audio.
        | b"mp3" | b"wav" | b"flac" | b"aac" | b"ogg" | b"m4a" | b"wma" | b"opus"
        | b"aiff" | b"aif" | b"alac" | b"ape" | b"mid" | b"midi"
        // Media — fonts and design assets.
        | b"ttf" | b"otf" | b"woff" | b"woff2" => Category::Media,

        // Documents.
        b"pdf" | b"doc" | b"docx" | b"xls" | b"xlsx" | b"ppt" | b"pptx" | b"txt"
        | b"md" | b"markdown" | b"rtf" | b"odt" | b"ods" | b"odp" | b"csv" | b"tsv"
        | b"epub" | b"mobi" | b"azw" | b"azw3" | b"pages" | b"numbers" | b"key"
        | b"tex" | b"rst" | b"org" | b"log" | b"msg" | b"eml" | b"xps" | b"oxps"
        | b"djvu" | b"ics" | b"vcf" => Category::Documents,

        // Code — source, project files and configuration.
        b"rs" | b"ts" | b"tsx" | b"js" | b"jsx" | b"mjs" | b"cjs" | b"py" | b"java"
        | b"kt" | b"kts" | b"go" | b"c" | b"cc" | b"cpp" | b"cxx" | b"h" | b"hh"
        | b"hpp" | b"hxx" | b"cs" | b"vb" | b"rb" | b"php" | b"swift" | b"m"
        | b"mm" | b"sh" | b"bash" | b"zsh" | b"fish" | b"ps1" | b"psm1" | b"bat"
        | b"cmd" | b"sql" | b"html" | b"htm" | b"css" | b"scss" | b"sass" | b"less"
        | b"vue" | b"svelte" | b"astro" | b"json" | b"json5" | b"jsonc" | b"yaml"
        | b"yml" | b"toml" | b"xml" | b"lock" | b"ipynb" | b"lua" | b"dart" | b"zig"
        | b"nim" | b"ex" | b"exs" | b"erl" | b"hrl" | b"clj" | b"cljs" | b"scala"
        | b"groovy" | b"gradle" | b"cmake" | b"mk" | b"make" | b"ninja" | b"bazel"
        | b"bzl" | b"proto" | b"graphql" | b"gql" | b"wasm" | b"wat" | b"asm"
        | b"s" | b"v" | b"sv" | b"sln" | b"vcxproj" | b"csproj" | b"fsproj"
        | b"vbproj" | b"props" | b"targets" | b"o" | b"obj" | b"a" | b"lib" | b"pdb"
        | b"rlib" | b"rmeta" | b"d" | b"class" | b"pyc" | b"pyo" | b"ini" | b"cfg" | b"conf"
        | b"config" | b"properties" | b"env" | b"editorconfig" => Category::Code,

        // Apps — installed programs, installers and app packages.
        b"exe" | b"com" | b"msi" | b"msix" | b"msixbundle" | b"appx" | b"appxbundle"
        | b"appimage" | b"deb" | b"rpm" | b"pkg" | b"apk" | b"ipa" | b"app"
        | b"scr" | b"dll" => Category::Apps,

        // System — drivers, system libraries and OS metadata.
        b"sys" | b"drv" | b"ocx" | b"cpl" | b"efi" | b"mui" | b"inf"
        | b"cat" | b"msstyles" | b"themepack" | b"reg" => Category::System,

        // Archives — compressed files, backups and disk images.
        b"zip" | b"zipx" | b"rar" | b"7z" | b"tar" | b"gz" | b"tgz" | b"bz2"
        | b"tbz2" | b"xz" | b"txz" | b"zst" | b"lz4" | b"lzma" | b"cab" | b"arj"
        | b"ace" | b"iso" | b"img" | b"vhd" | b"vhdx" | b"vmdk" | b"qcow2" | b"dmg"
        | b"wim" | b"swm" | b"esd" | b"jar" | b"war" | b"ear" | b"nupkg" | b"crate"
        | b"gem" | b"whl" | b"egg" | b"bak" | b"msu" => Category::Archives,

        // Caches / temporary data by extension.
        b"tmp" | b"temp" | b"dmp" => Category::Caches,

        _ => return None,
    })
}
