//! Reads the declarations of a compiled Java class: its superclass, interfaces,
//! fields, and methods, with their descriptors and generic signatures.

pub const ACC_PUBLIC: u16 = 0x0001;
pub const ACC_PRIVATE: u16 = 0x0002;
pub const ACC_PROTECTED: u16 = 0x0004;
pub const ACC_STATIC: u16 = 0x0008;
pub const ACC_FINAL: u16 = 0x0010;
pub const ACC_BRIDGE: u16 = 0x0040;
pub const ACC_VARARGS: u16 = 0x0080;
pub const ACC_INTERFACE: u16 = 0x0200;
pub const ACC_ABSTRACT: u16 = 0x0400;
pub const ACC_SYNTHETIC: u16 = 0x1000;

#[derive(Debug)]
pub struct ClassInfo {
    pub annotations: Vec<Annotation>,
    pub name: String,
    pub access: u16,
    pub super_name: Option<String>,
    pub interfaces: Vec<String>,
    pub signature: Option<String>,
    pub fields: Vec<Member>,
    pub methods: Vec<Member>,
}

#[derive(Debug, Clone)]
pub struct Member {
    pub has_default: bool,
    pub access: u16,
    pub name: String,
    pub descriptor: String,
    pub signature: Option<String>,
}

#[derive(Debug)]
pub struct Annotation {
    pub descriptor: String,
    pub elements: Vec<(String, AnnotationValue)>,
}

#[derive(Debug)]
pub enum AnnotationValue {
    Enum(String, String),
    Array(Vec<AnnotationValue>),
    Other,
}

impl ClassInfo {
    pub fn is_interface(&self) -> bool {
        self.access & ACC_INTERFACE != 0
    }

    pub fn is_public(&self) -> bool {
        self.access & ACC_PUBLIC != 0
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> Result<u8, String> {
        let value = *self
            .bytes
            .get(self.position)
            .ok_or("class file ended early")?;
        self.position += 1;
        Ok(value)
    }

    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from(self.u8()?) << 8 | u16::from(self.u8()?))
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from(self.u16()?) << 16 | u32::from(self.u16()?))
    }

    fn skip(&mut self, count: usize) -> Result<(), String> {
        if self.position + count > self.bytes.len() {
            return Err("class file ended early".into());
        }
        self.position += count;
        Ok(())
    }
}

enum Constant {
    Utf8(String),
    Class(u16),
    Other,
}

pub fn parse(bytes: &[u8]) -> Result<ClassInfo, String> {
    let mut reader = Reader { bytes, position: 0 };
    if reader.u32()? != 0xcafebabe {
        return Err("not a Java class file".into());
    }
    reader.skip(4)?; // version
    let count = reader.u16()? as usize;
    let mut pool = Vec::with_capacity(count);
    pool.push(Constant::Other);
    while pool.len() < count {
        let tag = reader.u8()?;
        match tag {
            1 => {
                let length = reader.u16()? as usize;
                let start = reader.position;
                reader.skip(length)?;
                pool.push(Constant::Utf8(decode_modified_utf8(
                    &bytes[start..start + length],
                )));
            }
            7 => pool.push(Constant::Class(reader.u16()?)),
            3 | 4 => {
                reader.skip(4)?;
                pool.push(Constant::Other);
            }
            5 | 6 => {
                reader.skip(8)?;
                pool.push(Constant::Other);
                pool.push(Constant::Other);
            }
            8 | 16 | 19 | 20 => {
                reader.skip(2)?;
                pool.push(Constant::Other);
            }
            9 | 10 | 11 | 12 | 17 | 18 => {
                reader.skip(4)?;
                pool.push(Constant::Other);
            }
            15 => {
                reader.skip(3)?;
                pool.push(Constant::Other);
            }
            _ => return Err(format!("unknown constant pool tag {tag}")),
        }
    }
    let utf8 = |index: u16| -> Result<String, String> {
        match pool.get(index as usize) {
            Some(Constant::Utf8(text)) => Ok(text.clone()),
            _ => Err("expected a text constant".into()),
        }
    };
    let class_name = |index: u16| -> Result<String, String> {
        match pool.get(index as usize) {
            Some(Constant::Class(name)) => utf8(*name),
            _ => Err("expected a class constant".into()),
        }
    };
    let access = reader.u16()?;
    let name = class_name(reader.u16()?)?;
    let super_index = reader.u16()?;
    let super_name = if super_index == 0 {
        None
    } else {
        Some(class_name(super_index)?)
    };
    let interface_count = reader.u16()?;
    let mut interfaces = Vec::with_capacity(interface_count as usize);
    for _ in 0..interface_count {
        interfaces.push(class_name(reader.u16()?)?);
    }
    let members = |reader: &mut Reader<'_>| -> Result<Vec<Member>, String> {
        let count = reader.u16()?;
        let mut members = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let access = reader.u16()?;
            let name = utf8(reader.u16()?)?;
            let descriptor = utf8(reader.u16()?)?;
            let attributes = attributes(reader, &utf8)?;
            members.push(Member {
                has_default: attributes.has_default,
                access,
                name,
                descriptor,
                signature: attributes.signature,
            });
        }
        Ok(members)
    };
    let fields = members(&mut reader)?;
    let methods = members(&mut reader)?;
    let attributes = attributes(&mut reader, &utf8)?;
    Ok(ClassInfo {
        annotations: attributes.annotations,
        name,
        access,
        super_name,
        interfaces,
        signature: attributes.signature,
        fields,
        methods,
    })
}

#[derive(Default)]
struct Attributes {
    signature: Option<String>,
    annotations: Vec<Annotation>,
    has_default: bool,
}

fn attributes(
    reader: &mut Reader<'_>,
    utf8: &dyn Fn(u16) -> Result<String, String>,
) -> Result<Attributes, String> {
    let count = reader.u16()?;
    let mut result = Attributes::default();
    for _ in 0..count {
        let name = utf8(reader.u16()?)?;
        let length = reader.u32()? as usize;
        let start = reader.position;
        reader.skip(length)?;
        let mut body = Reader {
            bytes: &reader.bytes[start..reader.position],
            position: 0,
        };
        if name == "Signature" && length == 2 {
            result.signature = Some(utf8(body.u16()?)?);
        } else if name == "AnnotationDefault" {
            result.has_default = true;
        } else if matches!(
            name.as_str(),
            "RuntimeVisibleAnnotations" | "RuntimeInvisibleAnnotations"
        ) {
            for _ in 0..body.u16()? {
                result
                    .annotations
                    .push(read_annotation(&mut body, utf8, 0)?);
            }
            if body.position != body.bytes.len() {
                return Err("invalid annotation attribute length".into());
            }
        }
    }
    Ok(result)
}

fn read_annotation(
    reader: &mut Reader<'_>,
    utf8: &dyn Fn(u16) -> Result<String, String>,
    depth: usize,
) -> Result<Annotation, String> {
    if depth > 64 {
        return Err("annotation nesting is too deep".into());
    }
    let descriptor = utf8(reader.u16()?)?;
    let mut elements = Vec::new();
    for _ in 0..reader.u16()? {
        elements.push((
            utf8(reader.u16()?)?,
            read_annotation_value(reader, utf8, depth + 1)?,
        ));
    }
    Ok(Annotation {
        descriptor,
        elements,
    })
}

fn read_annotation_value(
    reader: &mut Reader<'_>,
    utf8: &dyn Fn(u16) -> Result<String, String>,
    depth: usize,
) -> Result<AnnotationValue, String> {
    if depth > 64 {
        return Err("annotation nesting is too deep".into());
    }
    Ok(match reader.u8()? {
        b'e' => AnnotationValue::Enum(utf8(reader.u16()?)?, utf8(reader.u16()?)?),
        b'[' => {
            let mut values = Vec::new();
            for _ in 0..reader.u16()? {
                values.push(read_annotation_value(reader, utf8, depth + 1)?);
            }
            AnnotationValue::Array(values)
        }
        b'@' => {
            read_annotation(reader, utf8, depth)?;
            AnnotationValue::Other
        }
        b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z' | b's' | b'c' => {
            reader.u16()?;
            AnnotationValue::Other
        }
        _ => return Err("invalid annotation value tag".into()),
    })
}

fn decode_modified_utf8(bytes: &[u8]) -> String {
    let mut units = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        let unit = if byte & 0x80 == 0 {
            index += 1;
            u16::from(byte)
        } else if byte & 0xe0 == 0xc0 && index + 1 < bytes.len() {
            let unit = (u16::from(byte & 0x1f) << 6) | u16::from(bytes[index + 1] & 0x3f);
            index += 2;
            unit
        } else if index + 2 < bytes.len() {
            let unit = (u16::from(byte & 0x0f) << 12)
                | (u16::from(bytes[index + 1] & 0x3f) << 6)
                | u16::from(bytes[index + 2] & 0x3f);
            index += 3;
            unit
        } else {
            index += 1;
            0xfffd
        };
        units.push(unit);
    }
    String::from_utf16_lossy(&units)
}
