//! Encode and decode Valve's binary particle config file format.
//!
//! # Example
//!
//! Decode & modify a pcf file using a buffered reader:
//! ```
//!     let file = File::open("particles.pcf")?;
//!     let mut reader = BufReader::new(file);
//!     let mut pcf = Pcf::decode(reader)?;
//!     println!("particles.pcf has {} particle systems.", pcf.elements.len());
//!     // modify pcf elements or attributes...
//!     // ...
//! ```
//!
//! Encode a pcf back into a file
//! ```
//!     let file = File::open("new_particles.pcf")?;
//!     let mut writer = BufWriter::new(file);
//!     pcf.encode(writer)?;
//! ```

#![feature(buf_read_has_data_left)]
#![feature(read_array)]
#![feature(trim_prefix_suffix)]
#![feature(associated_type_defaults)]

use std::{collections::{HashMap, HashSet}, ffi::{CStr, CString}, hash::Hash, io, marker::PhantomData, vec};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use derive_more::{From, Into};
use either::Either::{self, Left, Right};
use ordered_float::OrderedFloat;
use thiserror::Error;

#[derive(Debug)]
/// A Valve Particles Config File.
pub struct Pcf {
    pub version: Version,
    pub strings: Vec<CString>,
    pub elements: Vec<Element>,
    elements_by_name: HashMap<CString, u32>,
}

impl Pcf {
    pub fn get_element(&self, name: &CString) -> Option<&Element> {
        match self.elements_by_name.get(name) {
            Some(idx) => self.elements.get(*idx as usize),
            None => None,
        }
    }

    pub fn get_element_index(&self, name: &CString) -> Option<u32> {
        self.elements_by_name.get(name).cloned()
    }

    pub fn get_dependent_indices(&self, name: &CString) -> Option<HashSet<u32>> {
        fn visit(visited: &mut HashSet<u32>, elements: &Vec<Element>, idx: u32) {
            // NB insert returns false when insertion fails
            if !visited.insert(idx) {
                return
            }

            let Some(element) = elements.get(idx as usize) else {
                return;
            };

            // Element and Element Array attributes contain indices for other elements
            for (_, attribute) in &element.attributes {
                match attribute {
                    Attribute::Element(value) if *value != u32::MAX => {
                        visit(visited, elements, *value);
                    }
                    Attribute::ElementArray(values) => {
                        for value in values {
                            visit(visited, elements, *value);
                        }
                    }
                    _ => continue,
                }
            }
        }

        let idx = self.get_element_index(name)?;

        let mut visited = HashSet::new();
        visit(&mut visited, &self.elements, idx);

        Some(visited)
    }

    pub fn builder() -> PcfBuilder<NoVersion, NoStrings, NoElements> {
        PcfBuilder { version: NoVersion, strings: Vec::new(), elements: Vec::new(), elements_by_name: HashMap::new(), _phantom_elements: PhantomData, _phantom_strings: PhantomData }
    }

    pub fn builder_from(pcf: &Pcf) -> PcfBuilder<Version, IncompleteStrings, IncompleteElements> {
        PcfBuilder { version: pcf.version, strings: pcf.strings.clone(), elements: pcf.elements.clone(), elements_by_name: pcf.elements_by_name.clone(), _phantom_elements: PhantomData, _phantom_strings: PhantomData }
    }
}

#[derive(Default, Debug)]
pub struct PcfBuilder<A, B, C> {
    version: A,
    strings: Vec<CString>,
    elements: Vec<Element>,
    elements_by_name: HashMap<CString, u32>,

    _phantom_strings: PhantomData<B>,
    _phantom_elements: PhantomData<C>,
}

impl PcfBuilder<Version, Strings, Elements> {
    pub fn build(self) -> Pcf {
        Pcf { version: self.version, strings: self.strings, elements: self.elements, elements_by_name: self.elements_by_name }
    }
}

impl<A, C> PcfBuilder<A, IncompleteStrings, C> {
    pub fn complete_strings(self) -> Result<PcfBuilder<A, Strings, C>, PcfBuilder<A, NoStrings, C>> {
        if self.strings.is_empty() {
            Err(PcfBuilder { version: self.version, strings: self.strings, elements: self.elements, elements_by_name: self.elements_by_name, _phantom_strings: PhantomData, _phantom_elements: PhantomData })
        } else {
            Ok(PcfBuilder { version: self.version, strings: self.strings, elements: self.elements, elements_by_name: self.elements_by_name, _phantom_strings: PhantomData, _phantom_elements: PhantomData })
        }
    }
}

impl <A, B> PcfBuilder<A, B, IncompleteElements> {
    pub fn complete_elements(self) -> Either<PcfBuilder<A, B, Elements>, PcfBuilder<A, B, NoElements>> {
        if self.elements.is_empty() {
            Right(PcfBuilder { version: self.version, strings: self.strings, elements: self.elements, elements_by_name: self.elements_by_name, _phantom_strings: PhantomData, _phantom_elements: PhantomData })
        } else {
            Left(PcfBuilder { version: self.version, strings: self.strings, elements: self.elements, elements_by_name: self.elements_by_name, _phantom_strings: PhantomData, _phantom_elements: PhantomData })
        }
    }
}

impl<B, C> PcfBuilder<NoVersion, B, C> {
    pub fn version(self, version: Version) -> PcfBuilder<Version, B, C> {
        PcfBuilder { version, strings: self.strings, elements: self.elements, elements_by_name: self.elements_by_name, _phantom_elements: PhantomData, _phantom_strings: PhantomData }
    }
}

impl<A, C> PcfBuilder<A, NoStrings, C> {
    pub fn strings(self, strings: Vec<CString>) -> PcfBuilder<A, Strings, C> {
        PcfBuilder { version: self.version, strings, elements: self.elements, elements_by_name: self.elements_by_name, _phantom_elements: PhantomData, _phantom_strings: PhantomData }
    }

    pub fn string(self, string: CString) -> PcfBuilder<A, Strings, C> {
        PcfBuilder { version: self.version, strings: vec![string], elements: self.elements, elements_by_name: self.elements_by_name, _phantom_elements: PhantomData, _phantom_strings: PhantomData }
    }
}

impl<A, C> PcfBuilder<A, Strings, C> {
    pub fn string(self, string: CString) -> PcfBuilder<A, Strings, C> {
        let mut strings = self.strings;
        strings.push(string);

        PcfBuilder { version: self.version, strings, elements: self.elements, elements_by_name: self.elements_by_name, _phantom_elements: PhantomData, _phantom_strings: PhantomData }
    }
}

impl<A, B> PcfBuilder<A, B, NoElements> {
    pub fn elements(self, elements: Vec<Element>) -> PcfBuilder<A, B, Elements> {
        let mut elements_by_name = self.elements_by_name;
        for (idx, element) in elements.iter().enumerate() {
            elements_by_name.insert(element.name.clone(), idx as u32);
        }

        PcfBuilder { version: self.version, strings: self.strings, elements, elements_by_name, _phantom_elements: PhantomData, _phantom_strings: PhantomData }
    }

    pub fn element(self, element: Element) -> PcfBuilder<A, B, Elements> {
        let mut elements_by_name = self.elements_by_name;
        elements_by_name.insert(element.name.clone(), 0);

        PcfBuilder { version: self.version, strings: self.strings, elements: vec![element], elements_by_name, _phantom_elements: PhantomData, _phantom_strings: PhantomData }
    }
}

impl<A, B> PcfBuilder<A, B, Elements> {
    pub fn element(self, element: Element) -> PcfBuilder<A, B, Elements> {
        let element_name = element.name.clone();

        let mut elements = self.elements;
        elements.push(element);

        let mut elements_by_name = self.elements_by_name;
        elements_by_name.insert(element_name, (elements.len() - 1) as u32);

        PcfBuilder { version: self.version, strings: self.strings, elements, elements_by_name, _phantom_elements: PhantomData, _phantom_strings: PhantomData }
    }
}

#[derive(Default, Debug, PartialEq)]
pub struct NoVersion;
#[derive(Default, Debug, PartialEq)]
pub struct NoStrings;
#[derive(Default, Debug, PartialEq)]
pub struct IncompleteStrings;
#[derive(Default, Debug, PartialEq)]
pub struct Strings;
#[derive(Default, Debug, PartialEq)]
pub struct NoElements;
#[derive(Default, Debug, PartialEq)]
pub struct IncompleteElements;
#[derive(Default, Debug, PartialEq)]
pub struct Elements;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Version {
    Binary2Dmx1,
    Binary2Pcf1,
    Binary3Pcf1,
}

#[derive(Error, Debug)]
pub enum ParseVersionError {
    #[error("the version string was invalid: '{0}'")]
    Invalid(String),
}

impl TryFrom<&CStr> for Version {
    type Error = ParseVersionError;

    fn try_from(s: &CStr) -> Result<Self, Self::Error> {
        const BINARY2_DMX1: &CStr = c"<!-- dmx encoding binary 2 format dmx 1 -->\x0A";
        const BINARY2_PCF1: &CStr = c"<!-- dmx encoding binary 2 format pcf 1 -->\x0A";
        const BINARY3_PCF1: &CStr = c"<!-- dmx encoding binary 3 format pcf 1 -->\x0A";
        if s.eq(BINARY2_DMX1) {
            Ok(Self::Binary2Dmx1)
        } else if s.eq(BINARY2_PCF1) {
            Ok(Self::Binary2Pcf1)
        } else if s.eq(BINARY3_PCF1) {
            Ok(Self::Binary3Pcf1)
        } else {
            Err(Self::Error::Invalid(s.to_str().unwrap_or("").to_string()))
        }
    }
}

impl Version {
    fn as_cstr_with_nul_terminator(&self) -> &'static CStr {
        match self {
            Version::Binary2Dmx1 => c"<!-- dmx encoding binary 2 format dmx 1 -->\x0A",
            Version::Binary2Pcf1 => c"<!-- dmx encoding binary 2 format pcf 1 -->\x0A",
            Version::Binary3Pcf1 => c"<!-- dmx encoding binary 3 format pcf 1 -->\x0A",
        }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Element {
    pub type_idx: u16,
    pub name: CString,
    pub signature: [u8; 16],
    pub attributes: Vec<(NameIndex, Attribute)>,
}

type NameIndex = u16;

#[derive(Debug, From, Clone, Hash, PartialEq, Eq)]
pub enum Attribute {
    Element(u32),
    Integer(i32),
    Float(Float),
    Bool(Bool8),
    String(CString),
    Binary(Box<[u8]>),
    Color(Color),
    Vector2(Vector2),
    Vector3(Vector3),
    Vector4(Vector4),
    Matrix(Matrix),
    ElementArray(Box<[u32]>),
    IntegerArray(Box<[i32]>),
    FloatArray(Box<[Float]>),
    BoolArray(Box<[Bool8]>),
    StringArray(Box<[CString]>),
    BinaryArray(Box<[Box<[u8]>]>),
    ColorArray(Box<[Color]>),
    Vector2Array(Box<[Vector2]>),
    Vector3Array(Box<[Vector3]>),
    Vector4Array(Box<[Vector4]>),
    MatrixArray(Box<[Matrix]>),
}

impl Attribute {
    fn as_type(&self) -> u8 {
        match self {
            Attribute::Element(_) => 1,
            Attribute::Integer(_) => 2,
            Attribute::Float(_) => 3,
            Attribute::Bool(_) => 4,
            Attribute::String(_) => 5,
            Attribute::Binary(_) => 6,
            Attribute::Color(_) => 8,
            Attribute::Vector2(_) => 9,
            Attribute::Vector3(_) => 10,
            Attribute::Vector4(_) => 11,
            Attribute::Matrix(_) => 14,
            Attribute::ElementArray(_) => 15,
            Attribute::IntegerArray(_) => 16,
            Attribute::FloatArray(_) => 17,
            Attribute::BoolArray(_) => 18,
            Attribute::StringArray(_) => 19,
            Attribute::BinaryArray(_) => 20,
            Attribute::ColorArray(_) => 22,
            Attribute::Vector2Array(_) => 23,
            Attribute::Vector3Array(_) => 24,
            Attribute::Vector4Array(_) => 25,
            Attribute::MatrixArray(_) => 28,
        }
    }
}

trait ReadAttribute: Sized {
    type Err: From<io::Error> = io::Error;
    fn read_attribute(reader: &mut impl io::BufRead) -> Result<Self, Self::Err>;
}

trait WriteAttribute: Sized {
    type Err: From<io::Error> = io::Error;
    fn write_attribute(&self, writer: &mut impl io::Write) -> Result<(), Self::Err>;
}

impl ReadAttribute for u32 {
    fn read_attribute(reader: &mut impl io::BufRead) -> Result<Self, Self::Err> {
        reader.read_u32::<LittleEndian>()
    }
}

impl WriteAttribute for u32 {
    fn write_attribute(&self, writer: &mut impl io::Write) -> Result<(), Self::Err> {
        writer.write_u32::<LittleEndian>(*self)
    }
}

impl ReadAttribute for i32 {
    fn read_attribute(reader: &mut impl io::BufRead) -> Result<Self, Self::Err> {
        reader.read_i32::<LittleEndian>()
    }
}

impl WriteAttribute for i32 {
    fn write_attribute(&self, writer: &mut impl io::Write) -> Result<(), Self::Err> {
        writer.write_i32::<LittleEndian>(*self)
    }
}

impl ReadAttribute for OrderedFloat<f32> {
    fn read_attribute(reader: &mut impl io::BufRead) -> Result<Self, Self::Err> {
        Ok(Self::from(reader.read_f32::<LittleEndian>()?))
    }
}

impl WriteAttribute for OrderedFloat<f32> {
    fn write_attribute(&self, writer: &mut impl io::Write) -> Result<(), Self::Err> {
        writer.write_f32::<LittleEndian>(self.into_inner())
    }
}

impl ReadAttribute for Bool8 {
    fn read_attribute(reader: &mut impl io::BufRead) -> Result<Self, Self::Err> {
        Ok(Self::from(reader.read_u8()?))
    }
}

impl WriteAttribute for Bool8 {
    fn write_attribute(&self, writer: &mut impl io::Write) -> Result<(), Self::Err> {
        writer.write_u8(self.0)
    }
}

impl ReadAttribute for CString {
    type Err = ReadError;
    fn read_attribute(reader: &mut impl io::BufRead) -> Result<Self, Self::Err> {
        let mut header_buf = Vec::new();
        reader.read_until(0, &mut header_buf)?;
        Ok(Self::from_vec_with_nul(header_buf)?)
    }
}

impl WriteAttribute for CString {
    fn write_attribute(&self, writer: &mut impl io::Write) -> Result<(), Self::Err> {
        Ok(writer.write_all(self.as_bytes_with_nul())?)
    }
}

impl ReadAttribute for Box<[u8]> {
    fn read_attribute(reader: &mut impl io::BufRead) -> Result<Self, Self::Err> {
        let count = reader.read_u32::<LittleEndian>()? as usize;

        let mut buf = vec![0; count].into_boxed_slice();
        reader.read_exact(&mut buf)?;
        Ok(buf.into())
    }
}

impl WriteAttribute for Box<[u8]> {
    fn write_attribute(&self, writer: &mut impl io::Write) -> Result<(), Self::Err> {
        writer.write_u32::<LittleEndian>(self.len() as u32)?;
        writer.write_all(self)
    }
}

impl ReadAttribute for Color {
    fn read_attribute(reader: &mut impl io::BufRead) -> Result<Self, Self::Err> {
        Ok(Self(
            reader.read_u8()?,
            reader.read_u8()?,
            reader.read_u8()?,
            reader.read_u8()?,
        ))
    }
}

impl WriteAttribute for Color {
    fn write_attribute(&self, writer: &mut impl io::Write) -> Result<(), Self::Err> {
        writer.write_u8(self.0)?;
        writer.write_u8(self.1)?;
        writer.write_u8(self.2)?;
        writer.write_u8(self.3)?;
        Ok(())
    }
}

impl ReadAttribute for Vector2 {
    fn read_attribute(reader: &mut impl io::BufRead) -> Result<Self, Self::Err> {
        Ok(Self(
            reader.read_f32::<LittleEndian>()?.into(),
            reader.read_f32::<LittleEndian>()?.into(),
        ))
    }
}

impl WriteAttribute for Vector2 {
    fn write_attribute(&self, writer: &mut impl io::Write) -> Result<(), Self::Err> {
        writer.write_f32::<LittleEndian>(self.0.into_inner())?;
        writer.write_f32::<LittleEndian>(self.1.into_inner())?;
        Ok(())
    }
}

impl ReadAttribute for Vector3 {
    fn read_attribute(reader: &mut impl io::BufRead) -> Result<Self, Self::Err> {
        Ok(Self(
            reader.read_f32::<LittleEndian>()?.into(),
            reader.read_f32::<LittleEndian>()?.into(),
            reader.read_f32::<LittleEndian>()?.into(),
        ))
    }
}

impl WriteAttribute for Vector3 {
    fn write_attribute(&self, writer: &mut impl io::Write) -> Result<(), Self::Err> {
        writer.write_f32::<LittleEndian>(self.0.into_inner())?;
        writer.write_f32::<LittleEndian>(self.1.into_inner())?;
        writer.write_f32::<LittleEndian>(self.2.into_inner())?;
        Ok(())
    }
}

impl ReadAttribute for Vector4 {
    fn read_attribute(reader: &mut impl io::BufRead) -> Result<Self, Self::Err> {
        Ok(Self(
            reader.read_f32::<LittleEndian>()?.into(),
            reader.read_f32::<LittleEndian>()?.into(),
            reader.read_f32::<LittleEndian>()?.into(),
            reader.read_f32::<LittleEndian>()?.into(),
        ))
    }
}

impl WriteAttribute for Vector4 {
    fn write_attribute(&self, writer: &mut impl io::Write) -> Result<(), Self::Err> {
        writer.write_f32::<LittleEndian>(self.0.into_inner())?;
        writer.write_f32::<LittleEndian>(self.1.into_inner())?;
        writer.write_f32::<LittleEndian>(self.2.into_inner())?;
        writer.write_f32::<LittleEndian>(self.3.into_inner())?;
        Ok(())
    }
}

impl ReadAttribute for Matrix {
    fn read_attribute(reader: &mut impl io::BufRead) -> Result<Self, Self::Err> {
        Ok(Self(
            Vector4::read_attribute(reader)?,
            Vector4::read_attribute(reader)?,
            Vector4::read_attribute(reader)?,
            Vector4::read_attribute(reader)?,
        ))
    }
}

impl WriteAttribute for Matrix {
    fn write_attribute(&self, writer: &mut impl io::Write) -> Result<(), Self::Err> {
        self.0.write_attribute(writer)?;
        self.1.write_attribute(writer)?;
        self.2.write_attribute(writer)?;
        self.3.write_attribute(writer)?;
        Ok(())
    }
}

struct AttributeReader<'a, R: std::io::BufRead> {
    element_count: usize,
    first_attribute_count: usize,
    reader: &'a mut R,
}

struct AttributeIterator<'a, R: std::io::BufRead> {
    element_count: usize,
    current_element: usize,
    current_attribute_count: usize,
    current_attribute: usize,
    reader: AttributeReader<'a, R>,
}

struct AttributeWriter<'a, W: io::Write> {
    writer: &'a mut W,
}

impl<'a, W: io::Write> From<&'a mut W> for AttributeWriter<'a, W> {
    fn from(writer: &'a mut W) -> Self {
        Self { writer }
    }
}

impl<'a, W: io::Write> AttributeWriter<'a, W> {
    fn write<T: WriteAttribute>(&mut self, value: &T) -> Result<(), T::Err> {
        value.write_attribute(&mut self.writer)
    }

    fn write_array<T: WriteAttribute>(&mut self, values: &[T]) -> Result<(), T::Err> {
        self.writer.write_u32::<LittleEndian>(values.len() as u32)?;
        for value in values {
            value.write_attribute(&mut self.writer)?;
        }

        Ok(())
    }

    fn write_attribute(&mut self, attribute: &Attribute) -> Result<(), io::Error> {
        match attribute {
            Attribute::Element(element) => self.write(element),
            Attribute::Integer(integer) => self.write(integer),
            Attribute::Float(ordered_float) => self.write(ordered_float),
            Attribute::Bool(bool8) => self.write(bool8),
            Attribute::String(cstring) => self.write(cstring),
            Attribute::Binary(items) => self.write(items),
            Attribute::Color(color) => self.write(color),
            Attribute::Vector2(vector2) => self.write(vector2),
            Attribute::Vector3(vector3) => self.write(vector3),
            Attribute::Vector4(vector4) => self.write(vector4),
            Attribute::Matrix(matrix) => self.write(matrix),
            Attribute::ElementArray(elements) => self.write_array(elements),
            Attribute::IntegerArray(integers) => self.write_array(integers),
            Attribute::FloatArray(ordered_floats) => self.write_array(ordered_floats),
            Attribute::BoolArray(bool8s) => self.write_array(bool8s),
            Attribute::StringArray(cstrings) => self.write_array(cstrings),
            Attribute::BinaryArray(items) => self.write_array(items),
            Attribute::ColorArray(colors) => self.write_array(colors),
            Attribute::Vector2Array(vector2s) => self.write_array(vector2s),
            Attribute::Vector3Array(vector3s) => self.write_array(vector3s),
            Attribute::Vector4Array(vector4s) => self.write_array(vector4s),
            Attribute::MatrixArray(items) => self.write_array(items),
        }
    }

    pub fn write_attributes(&mut self, elements: &Vec<Element>) -> Result<(), io::Error> {
        for element in elements {
            self.writer.write_u32::<LittleEndian>(element.attributes.len() as u32)?;
            for (name_idx, attribute) in &element.attributes {
                self.writer.write_u16::<LittleEndian>(*name_idx)?;
                self.writer.write_u8(attribute.as_type())?;
                self.write_attribute(attribute)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
enum ReadError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    CString(#[from] std::ffi::FromVecWithNulError),

    #[error("the attribute type {0} is unsupported or invalid")]
    InvalidAttributeType(u8),
}

impl<'a, R: std::io::BufRead> Iterator for AttributeIterator<'a, R> {
    type Item = Result<(NameIndex, Attribute), ReadError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_attribute < self.current_attribute_count {
            self.current_attribute += 1;
            Some(self.reader.read_attribute())
        } else if self.current_element == self.element_count - 1 {
            // this was the last element, so there are no more attributes
            None
        } else {
            // we're at the end of our current block of element attributes, so we can move on to the next
            self.current_element += 1;
            self.current_attribute = 0;
            match self.reader.read::<u32>() {
                Ok(value) => {
                    self.current_attribute_count = value as usize;
                    self.next()
                },
                Err(err) => Some(Err(ReadError::Io(err))),
            }
        }
    }
}

impl<'a, R: std::io::BufRead> IntoIterator for AttributeReader<'a, R> {
    type Item = Result<(NameIndex, Attribute), ReadError>;

    type IntoIter = AttributeIterator<'a, R>;

    fn into_iter(self) -> Self::IntoIter {
        Self::IntoIter {
            element_count: self.element_count,
            current_element: 0,
            current_attribute_count: self.first_attribute_count,
            current_attribute: 0,
            reader: self,
        }
    }
}

impl<'a, R: std::io::BufRead> AttributeReader<'a, R> {
    pub fn try_from(reader: &'a mut R, element_count: usize) -> Result<Self, ReadError> {
        // we always read the first attribute count; next() expects that the element_count and 
        // current_attribute_count have both been set when applicable.
        let current_attribute_count = if element_count > 0 {
            reader.read_u32::<LittleEndian>()? as usize
        } else {
            0
        };

        Ok(Self {
            reader,
            element_count,
            first_attribute_count: current_attribute_count,
        })
    }

    pub fn read<T: ReadAttribute>(&mut self) -> Result<T, T::Err> {
        T::read_attribute(&mut self.reader)
    }

    pub fn read_array<T: ReadAttribute>(&mut self) -> Result<Box<[T]>, T::Err> {
        let count = self.reader.read_u32::<LittleEndian>()? as usize;
        let mut buf = Vec::with_capacity(count);

        for _ in 0..count {
            buf.push(self.read::<T>()?)
        }

        Ok(buf.into_boxed_slice())
    }

    pub fn read_attribute(&mut self) -> Result<(NameIndex, Attribute), ReadError> {
        let name_idx = self.reader.read_u16::<LittleEndian>()?;
        let type_idx = self.reader.read_u8()?;

        match type_idx {
            1 => Ok(self.read::<u32>()?.into()),
            2 => Ok(self.read::<i32>()?.into()),
            3 => Ok(self.read::<Float>()?.into()),
            4 => Ok(self.read::<Bool8>()?.into()),
            5 => Ok(self.read::<CString>()?.into()),
            6 => Ok(self.read::<Box<[u8]>>()?.into()),
            8 => Ok(self.read::<Color>()?.into()),
            9 => Ok(self.read::<Vector2>()?.into()),
            10 => Ok(self.read::<Vector3>()?.into()),
            11 => Ok(self.read::<Vector4>()?.into()),
            14 => Ok(self.read::<Matrix>()?.into()),
            15 => Ok(self.read_array::<u32>()?.into()),
            16 => Ok(self.read_array::<i32>()?.into()),
            17 => Ok(self.read_array::<Float>()?.into()),
            18 => Ok(self.read_array::<Bool8>()?.into()),
            19 => Ok(self.read_array::<CString>()?.into()),
            20 => Ok(self.read_array::<Box<[u8]>>()?.into()),
            22 => Ok(self.read_array::<Color>()?.into()),
            23 => Ok(self.read_array::<Vector2>()?.into()),
            24 => Ok(self.read_array::<Vector3>()?.into()),
            25 => Ok(self.read_array::<Vector4>()?.into()),
            28 => Ok(self.read_array::<Matrix>()?.into()),
            invalid_type => Err(ReadError::InvalidAttributeType(invalid_type)),
        }.map(|attr| (name_idx, attr))
    }
    /*
        match type_idx {
            1 => Ok(read_element(file)?.into()),
            2 => Ok(file.read_i32::<LittleEndian>()?.into()),
            3 => Ok(Float::from(file.read_f32::<LittleEndian>()?).into()),
            4 => Ok(Bool8::from(file.read_u8()? != 0).into()),
            5 => Ok(Self::read_terminated_string(file)?.into()),
            6 => {
                let count = file.read_u32::<LittleEndian>()? as usize;
                let mut buf = vec![0; count];
                file.read_exact(&mut buf)?;
                Ok(buf.into())
            }
            8 => Ok(Color(file.read_u8()?, file.read_u8()?, file.read_u8()?, file.read_u8()?).into()),
            9 => Ok(Vector2(file.read_f32::<LittleEndian>()?.into(), file.read_f32::<LittleEndian>()?.into()).into()),
            10 => Ok(Vector3(
                file.read_f32::<LittleEndian>()?.into(),
                file.read_f32::<LittleEndian>()?.into(),
                file.read_f32::<LittleEndian>()?.into(),
            )
            .into()),
            11 => Ok(read_vector4(file)?.into()),
            14 => Ok(Matrix(
                read_vector4(file)?,
                read_vector4(file)?,
                read_vector4(file)?,
                read_vector4(file)?,
            )
            .into()),
            15..=20 | 21..=24 | 27 => {
                let count = file.read_u32::<LittleEndian>()? as usize;
                let mut buf = Vec::with_capacity(count);
                for _idx in 0..count {
                    buf.push(Self::read_attribute_data(file, type_idx)?)
                }
                Ok((type_idx, buf).into())
            }
            _ => Err(anyhow!("unsupported attribute type: {type_idx}")),
        }
     */
}

#[derive(Debug, From, Into, Clone, Copy, Hash, PartialEq, Eq)]
/// An 8-bit boolean value. 0 is false, all other values are truthy.
pub struct Bool8(u8);

impl From<Bool8> for bool {
    fn from(value: Bool8) -> Self {
        value.0 != 0
    }
}

impl From<bool> for Bool8 {
    fn from(value: bool) -> Self {
        if value { Self(1) } else { Self(0) }
    }
}

type Float = OrderedFloat<f32>;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Color(pub u8, pub u8, pub u8, pub u8);
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Vector2(pub Float, pub Float);
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Vector3(pub Float, pub Float, pub Float);
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Vector4(pub Float, pub Float, pub Float, pub Float);
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Matrix(pub Vector4, pub Vector4, pub Vector4, pub Vector4);

// reading functions
impl Pcf {
    pub fn decode(buf: &mut impl std::io::BufRead) -> anyhow::Result<Pcf> {
        let version = Self::read_magic_version(buf)?;
        let strings = Self::read_strings(buf)?;
        let elements = Self::read_elements(buf)?; 
        
        let mut elements_by_name = HashMap::new();
        for (idx, element) in elements.iter().enumerate() {
            elements_by_name.insert(element.name.clone(), idx as u32);
        }

        Ok(Self { version, strings, elements, elements_by_name })
    }

    fn read_terminated_string(file: &mut impl std::io::BufRead) -> anyhow::Result<CString> {
        let mut header_buf = Vec::new();
        file.read_until(0, &mut header_buf)?;

        Ok(CString::from_vec_with_nul(header_buf)?)
    }

    fn read_magic_version(file: &mut impl std::io::BufRead) -> anyhow::Result<Version> {
        let mut header_buf = Vec::new();
        file.read_until(0, &mut header_buf)?;

        let version = CStr::from_bytes_with_nul(&header_buf)?;
        let version = Version::try_from(version)?;
        Ok(version)
    }

    fn read_strings(file: &mut impl std::io::BufRead) -> anyhow::Result<Vec<CString>> {
        let string_count = file.read_u16::<LittleEndian>()? as usize;
        let mut strings = Vec::with_capacity(string_count);
        for _ in 0..string_count {
            strings.push(Self::read_terminated_string(file)?)
        }

        Ok(strings)
    }

    fn read_elements(file: &mut impl std::io::BufRead) -> anyhow::Result<Vec<Element>> {
        let element_count = file.read_u32::<LittleEndian>()? as usize;
        let mut elements = Vec::with_capacity(element_count);
        for _idx in 0..element_count {
            let type_idx = file.read_u16::<LittleEndian>()?;
            let name = Self::read_terminated_string(file)?;
            let signature = file.read_array::<16>()?;

            elements.push(Element {
                type_idx,
                name,
                signature,
                attributes: Vec::new(),
            });
        }

        for element in &mut elements {
            let reader = AttributeReader::try_from(file, element_count)?.into_iter();
            let attributes: Result<Vec<(NameIndex, Attribute)>, ReadError> = reader.collect();
            element.attributes = attributes?;
        }

        Ok(elements)
    }

    // fn read_attribute_data(file: &mut impl std::io::BufRead, type_idx: u8) -> anyhow::Result<Attribute> {
    //     fn read_element(file: &mut impl std::io::BufRead) -> anyhow::Result<u32> {
    //         Ok(file.read_u32::<LittleEndian>()?)
    //     }

    //     fn read_vector4(file: &mut impl std::io::BufRead) -> anyhow::Result<Vector4> {
    //         Ok(Vector4(
    //             file.read_f32::<LittleEndian>()?.into(),
    //             file.read_f32::<LittleEndian>()?.into(),
    //             file.read_f32::<LittleEndian>()?.into(),
    //             file.read_f32::<LittleEndian>()?.into(),
    //         ))
    //     }

    //     let reader = AttributeReader::from(file);

    //     match type_idx {
    //         1 => Ok(read_element(file)?.into()),
    //         2 => Ok(file.read_i32::<LittleEndian>()?.into()),
    //         3 => Ok(Float::from(file.read_f32::<LittleEndian>()?).into()),
    //         4 => Ok(Bool8::from(file.read_u8()? != 0).into()),
    //         5 => Ok(Self::read_terminated_string(file)?.into()),
    //         6 => {
    //             let count = file.read_u32::<LittleEndian>()? as usize;
    //             let mut buf = vec![0; count];
    //             file.read_exact(&mut buf)?;
    //             Ok(buf.into())
    //         }
    //         8 => Ok(Color(file.read_u8()?, file.read_u8()?, file.read_u8()?, file.read_u8()?).into()),
    //         9 => Ok(Vector2(file.read_f32::<LittleEndian>()?.into(), file.read_f32::<LittleEndian>()?.into()).into()),
    //         10 => Ok(Vector3(
    //             file.read_f32::<LittleEndian>()?.into(),
    //             file.read_f32::<LittleEndian>()?.into(),
    //             file.read_f32::<LittleEndian>()?.into(),
    //         )
    //         .into()),
    //         11 => Ok(read_vector4(file)?.into()),
    //         14 => Ok(Matrix(
    //             read_vector4(file)?,
    //             read_vector4(file)?,
    //             read_vector4(file)?,
    //             read_vector4(file)?,
    //         )
    //         .into()),
    //         15..=20 | 21..=24 | 27 => {
    //             let count = file.read_u32::<LittleEndian>()? as usize;
    //             let mut buf = Vec::with_capacity(count);
    //             for _idx in 0..count {
    //                 buf.push(Self::read_attribute_data(file, type_idx)?)
    //             }
    //             Ok((type_idx, buf).into())
    //         }
    //         _ => Err(anyhow!("unsupported attribute type: {type_idx}")),
    //     }
    // }
}

// writing functions
impl Pcf {
    pub fn encode(&self, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        Self::write_magic_version(&self.version, file)?;
        Self::write_strings(&self.strings, file)?;
        Self::write_elements(&self.elements, file)?;

        Ok(())
    }

    fn write_magic_version(version: &Version, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        let version = version.as_cstr_with_nul_terminator().to_bytes_with_nul();
        file.write_all(version)?;

        Ok(())
    }

    fn write_strings(strings: &Vec<CString>, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        file.write_u16::<LittleEndian>(strings.len() as u16)?;

        for string in strings {
            file.write_all(string.to_bytes_with_nul())?;
        }

        Ok(())
    }

    fn write_elements(elements: &Vec<Element>, file: &mut impl std::io::Write) -> anyhow::Result<()> {
        file.write_u32::<LittleEndian>(elements.len() as u32)?;
        for element in elements {
            file.write_u16::<LittleEndian>(element.type_idx)?;
            file.write_all(element.name.to_bytes_with_nul())?;
            file.write_all(&element.signature)?;
        }

        AttributeWriter::from(file).write_attributes(elements)?;

        Ok(())
    }

    // fn write_attribute_data(attribute: &Attribute, file: &mut impl std::io::Write) -> anyhow::Result<()> {
    //     fn write_vector4(vector4: &Vector4, file: &mut impl std::io::Write) -> anyhow::Result<()> {
    //         file.write_f32::<LittleEndian>(vector4.0.into())?;
    //         file.write_f32::<LittleEndian>(vector4.1.into())?;
    //         file.write_f32::<LittleEndian>(vector4.2.into())?;
    //         file.write_f32::<LittleEndian>(vector4.3.into())?;
    //         Ok(())
    //     }

    //     match attribute {
    //         Attribute::Element(element) => file.write_u32::<LittleEndian>(*element)?,
    //         Attribute::Integer(integer) => file.write_i32::<LittleEndian>(*integer)?,
    //         Attribute::Float(float) => file.write_f32::<LittleEndian>(float.into_inner())?,
    //         Attribute::Bool(bool) => file.write_u8(bool.0)?,
    //         Attribute::String(cstring) => file.write_all(cstring.as_bytes_with_nul())?,
    //         Attribute::Binary(items) => {
    //             file.write_u32::<LittleEndian>(items.len() as u32)?;
    //             file.write_all(items)?;
    //         }
    //         Attribute::Color(color) => {
    //             file.write_u8(color.0)?;
    //             file.write_u8(color.1)?;
    //             file.write_u8(color.2)?;
    //             file.write_u8(color.3)?;
    //         }
    //         Attribute::Vector2(vector2) => {
    //             file.write_f32::<LittleEndian>(vector2.0.into())?;
    //             file.write_f32::<LittleEndian>(vector2.1.into())?;
    //         }
    //         Attribute::Vector3(vector3) => {
    //             file.write_f32::<LittleEndian>(vector3.0.into())?;
    //             file.write_f32::<LittleEndian>(vector3.1.into())?;
    //             file.write_f32::<LittleEndian>(vector3.2.into())?;
    //         }
    //         Attribute::Vector4(vector4) => write_vector4(vector4, file)?,
    //         Attribute::Matrix(matrix) => {
    //             write_vector4(&matrix.0, file)?;
    //             write_vector4(&matrix.1, file)?;
    //             write_vector4(&matrix.2, file)?;
    //             write_vector4(&matrix.3, file)?;
    //         }
    //         Attribute::Array(_, attributes) => {
    //             file.write_u32::<LittleEndian>(attributes.len() as u32)?;
    //             for attribute in attributes {
    //                 Self::write_attribute_data(attribute, file)?;
    //             }
    //         }
    //     }

    //     Ok(())
    // }
}

#[cfg(test)]
mod tests {
    use bytes::{Buf, BufMut, Bytes, BytesMut};

    use super::*;

    const TEST_PCF: &[u8] = include_bytes!("rankup.pcf");

    #[test]
    fn encodes_and_decodes_valid_pcf() {
        let mut reader = Bytes::from(TEST_PCF).reader();

        let pcf = Pcf::decode(&mut reader).unwrap();
        assert_eq!(pcf.version, Version::Binary2Pcf1);
        assert_eq!(pcf.strings.len(), 231);

        // spot checking a few random strings to ensure they're correct
        assert_eq!(pcf.strings[79], CString::from(c"rotation_offset_max"));
        assert_eq!(pcf.strings[160], CString::from(c"end time max"));
        assert_eq!(pcf.strings[220], CString::from(c"warp max"));

        assert_eq!(pcf.elements.len(), 2028);

        let buf = BytesMut::with_capacity(TEST_PCF.len());
        let mut writer = buf.writer();
        pcf.encode(&mut writer).expect("writing failed");

        let bytes = writer.get_ref();
        assert_eq!(TEST_PCF.len(), bytes.len());
        assert_eq!(
            TEST_PCF,
            &writer.get_ref()[..],
            "expected decoded buf and encoded buf to be identical."
        );
    }
}
