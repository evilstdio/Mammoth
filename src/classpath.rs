//! Finds compiled Java classes in the JDK's module image (`lib/modules`) and in
//! the directories and JAR files listed in `CLASSPATH`.

use std::{
    cell::RefCell,
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use crate::{
    classfile::{self, ClassInfo},
    inflate::inflate,
};

pub struct ClassPath {
    jdk: Option<JImage>,
    entries: Vec<Entry>,
    cache: RefCell<HashMap<String, Option<Rc<ClassInfo>>>>,
}

enum Entry {
    Directory(PathBuf),
    Jar(Zip),
}

impl ClassPath {
    /// Uses the JDK that runs Mammoth programs and the `CLASSPATH` entries.
    pub fn from_environment() -> Self {
        let jdk = find_jdk().and_then(|home| JImage::open(&home.join("lib").join("modules")));
        let mut entries = Vec::new();
        if let Some(classpath) = env::var_os("CLASSPATH") {
            for path in env::split_paths(&classpath) {
                if path.as_os_str().is_empty() {
                    continue;
                }
                if path.is_dir() {
                    entries.push(Entry::Directory(path));
                } else if path.file_name().is_some_and(|name| name == "*") {
                    // `dir/*` adds every JAR in the directory, as Java does.
                    if let Some(parent) = path.parent()
                        && let Ok(listing) = fs::read_dir(parent)
                    {
                        let mut jars: Vec<_> = listing
                            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                            .filter(|path| {
                                path.extension()
                                    .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"))
                            })
                            .collect();
                        jars.sort();
                        entries
                            .extend(jars.iter().filter_map(|jar| Zip::open(jar).map(Entry::Jar)));
                    }
                } else if let Some(zip) = Zip::open(&path) {
                    entries.push(Entry::Jar(zip));
                }
            }
        }
        Self {
            jdk,
            entries,
            cache: RefCell::new(HashMap::new()),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn has_jdk(&self) -> bool {
        self.jdk.is_some()
    }

    /// Loads a class by its internal name, such as `java/util/ArrayList`.
    pub fn load(&self, name: &str) -> Option<Rc<ClassInfo>> {
        if let Some(found) = self.cache.borrow().get(name) {
            return found.clone();
        }
        let bytes = self
            .jdk
            .as_ref()
            .and_then(|jdk| jdk.read(name))
            .or_else(|| {
                self.entries.iter().find_map(|entry| match entry {
                    Entry::Directory(directory) => {
                        fs::read(directory.join(format!("{name}.class"))).ok()
                    }
                    Entry::Jar(zip) => zip.read(&format!("{name}.class")),
                })
            });
        let info = bytes
            .and_then(|bytes| classfile::parse(&bytes).ok())
            .filter(|info| info.name == name)
            .map(Rc::new);
        self.cache
            .borrow_mut()
            .insert(name.to_owned(), info.clone());
        info
    }

    /// Whether a package (such as `java/util`) contains any classes we can see.
    pub fn has_package(&self, package: &str) -> bool {
        self.jdk
            .as_ref()
            .is_some_and(|jdk| jdk.packages.contains_key(package))
            || self.entries.iter().any(|entry| match entry {
                Entry::Directory(directory) => directory.join(package).is_dir(),
                Entry::Jar(zip) => {
                    let prefix = format!("{package}/");
                    zip.entries.keys().any(|name| name.starts_with(&prefix))
                }
            })
    }
}

fn find_jdk() -> Option<PathBuf> {
    let valid = |home: &Path| home.join("lib").join("modules").is_file();
    if let Some(home) = env::var_os("JAVA_HOME").map(PathBuf::from)
        && valid(&home)
    {
        return Some(home);
    }
    let executable = if cfg!(windows) { "java.exe" } else { "java" };
    let path = env::var_os("PATH")?;
    for directory in env::split_paths(&path) {
        let candidate = directory.join(executable);
        if !candidate.is_file() {
            continue;
        }
        let resolved = fs::canonicalize(&candidate).unwrap_or(candidate);
        if let Some(home) = resolved.parent().and_then(Path::parent)
            && valid(home)
        {
            return Some(home.to_path_buf());
        }
    }
    None
}

/// The JDK's `lib/modules` file in the jimage format.
struct JImage {
    file: RefCell<fs::File>,
    index_size: usize,
    /// Internal class name to (content offset, size).
    classes: HashMap<String, (usize, usize)>,
    packages: HashMap<String, ()>,
}

impl JImage {
    fn open(path: &Path) -> Option<Self> {
        use std::io::Read;
        let mut file = fs::File::open(path).ok()?;
        let mut header = [0u8; 28];
        file.read_exact(&mut header).ok()?;
        let header_u32 = |offset: usize, little: bool| {
            let raw: [u8; 4] = header[offset..offset + 4].try_into().unwrap();
            if little {
                u32::from_le_bytes(raw)
            } else {
                u32::from_be_bytes(raw)
            }
        };
        let little_endian = header_u32(0, true) == 0xcafedada;
        let index_length = 28
            + header_u32(16, little_endian) as usize * 8
            + header_u32(20, little_endian) as usize
            + header_u32(24, little_endian) as usize;
        let mut bytes = header.to_vec();
        bytes.resize(index_length, 0);
        file.read_exact(&mut bytes[28..]).ok()?;
        let little = u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?) == 0xcafedada;
        let big = u32::from_be_bytes(bytes.get(0..4)?.try_into().ok()?) == 0xcafedada;
        if !little && !big {
            return None;
        }
        let read_u32 = |offset: usize| -> Option<u32> {
            let raw: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
            Some(if little {
                u32::from_le_bytes(raw)
            } else {
                u32::from_be_bytes(raw)
            })
        };
        let table_length = read_u32(16)? as usize;
        let locations_size = read_u32(20)? as usize;
        let strings_size = read_u32(24)? as usize;
        let header_size = 28;
        let offsets_start = header_size + table_length * 4;
        let locations_start = offsets_start + table_length * 4;
        let strings_start = locations_start + locations_size;
        let index_size = strings_start + strings_size;
        if index_size > bytes.len() {
            return None;
        }
        let string = |offset: u64| -> &str {
            let start = strings_start + offset as usize;
            let tail = bytes.get(start..index_size).unwrap_or(&[]);
            let end = tail
                .iter()
                .position(|&byte| byte == 0)
                .unwrap_or(tail.len());
            std::str::from_utf8(&tail[..end]).unwrap_or("")
        };
        let mut classes = HashMap::new();
        let mut packages = HashMap::new();
        for entry in 0..table_length {
            let Some(offset) = read_u32(offsets_start + entry * 4) else {
                continue;
            };
            let mut position = locations_start + offset as usize;
            let mut attributes = [0u64; 8];
            while let Some(&data) = bytes.get(position) {
                position += 1;
                if data <= 0x7 {
                    break;
                }
                let kind = usize::from(data >> 3);
                let length = usize::from(data & 0x7) + 1;
                let mut value = 0u64;
                for _ in 0..length {
                    value = (value << 8) | u64::from(*bytes.get(position).unwrap_or(&0));
                    position += 1;
                }
                if kind < attributes.len() {
                    attributes[kind] = value;
                }
            }
            let [
                _,
                module,
                parent,
                base,
                extension,
                content,
                compressed,
                uncompressed,
            ] = attributes;
            if string(extension) != "class" || compressed != 0 || string(module).is_empty() {
                continue;
            }
            let parent = string(parent);
            let base = string(base);
            if base == "module-info" {
                continue;
            }
            let name = if parent.is_empty() {
                base.to_owned()
            } else {
                packages.insert(parent.to_owned(), ());
                format!("{parent}/{base}")
            };
            classes.insert(name, (content as usize, uncompressed as usize));
        }
        Some(Self {
            file: RefCell::new(file),
            index_size,
            classes,
            packages,
        })
    }

    fn read(&self, name: &str) -> Option<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let &(offset, size) = self.classes.get(name)?;
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start((self.index_size + offset) as u64))
            .ok()?;
        let mut bytes = vec![0; size];
        file.read_exact(&mut bytes).ok()?;
        Some(bytes)
    }
}

/// A ZIP or JAR archive, read through its central directory.
struct Zip {
    bytes: Vec<u8>,
    /// Entry name to (local header offset, method, compressed size, size).
    entries: HashMap<String, (usize, u16, usize, usize)>,
}

impl Zip {
    fn open(path: &Path) -> Option<Self> {
        let bytes = fs::read(path).ok()?;
        let u16_at = |offset: usize| -> Option<u16> {
            Some(u16::from_le_bytes(
                bytes.get(offset..offset + 2)?.try_into().ok()?,
            ))
        };
        let u32_at = |offset: usize| -> Option<u32> {
            Some(u32::from_le_bytes(
                bytes.get(offset..offset + 4)?.try_into().ok()?,
            ))
        };
        let search_start = bytes.len().saturating_sub(22 + 65535);
        let end = (search_start..bytes.len().saturating_sub(21))
            .rev()
            .find(|&offset| u32_at(offset) == Some(0x06054b50))?;
        let count = u16_at(end + 10)? as usize;
        let mut position = u32_at(end + 16)? as usize;
        let mut entries = HashMap::with_capacity(count);
        for _ in 0..count {
            if u32_at(position)? != 0x02014b50 {
                return None;
            }
            let method = u16_at(position + 10)?;
            let compressed = u32_at(position + 20)? as usize;
            let size = u32_at(position + 24)? as usize;
            let name_length = u16_at(position + 28)? as usize;
            let extra_length = u16_at(position + 30)? as usize;
            let comment_length = u16_at(position + 32)? as usize;
            let header = u32_at(position + 42)? as usize;
            let name = bytes.get(position + 46..position + 46 + name_length)?;
            let name = String::from_utf8_lossy(name).into_owned();
            entries.insert(name, (header, method, compressed, size));
            position += 46 + name_length + extra_length + comment_length;
        }
        Some(Self { bytes, entries })
    }

    fn read(&self, name: &str) -> Option<Vec<u8>> {
        let &(header, method, compressed, size) = self.entries.get(name)?;
        let u16_at = |offset: usize| -> Option<u16> {
            Some(u16::from_le_bytes(
                self.bytes.get(offset..offset + 2)?.try_into().ok()?,
            ))
        };
        let name_length = u16_at(header + 26)? as usize;
        let extra_length = u16_at(header + 28)? as usize;
        let start = header + 30 + name_length + extra_length;
        let data = self.bytes.get(start..start + compressed)?;
        match method {
            0 => Some(data.to_vec()),
            8 => inflate(data, size).ok(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_jdk_classes() {
        let classpath = ClassPath::from_environment();
        assert!(
            classpath.has_jdk(),
            "the tests need a JDK on PATH or JAVA_HOME"
        );
        let string = classpath.load("java/lang/String").expect("String loads");
        assert_eq!(string.super_name.as_deref(), Some("java/lang/Object"));
        assert!(
            string
                .methods
                .iter()
                .any(|method| method.name == "substring")
        );
        let list = classpath
            .load("java/util/ArrayList")
            .expect("ArrayList loads");
        assert!(list.signature.is_some());
        assert!(classpath.has_package("java/util"));
        assert!(classpath.load("java/util/DoesNotExist").is_none());
    }

    #[test]
    fn inflates_jar_entries() {
        // jrt-fs.jar ships with every JDK and stores its classes compressed.
        let home = find_jdk().expect("a JDK");
        let zip = Zip::open(&home.join("lib").join("jrt-fs.jar")).expect("jrt-fs.jar opens");
        let (name, _) = zip
            .entries
            .iter()
            .find(|(name, entry)| name.ends_with(".class") && entry.1 == 8)
            .expect("a compressed class");
        let bytes = zip.read(name).expect("entry inflates");
        let info = classfile::parse(&bytes).expect("class parses");
        assert_eq!(format!("{}.class", info.name), *name);
    }
}
