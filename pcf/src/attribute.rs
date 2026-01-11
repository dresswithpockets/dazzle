use std::{backtrace::Backtrace, ffi::CString, fmt::Display, hash::Hash, io, vec};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use derive_more::{From, Into};
use ordered_float::OrderedFloat;
use thiserror::Error;

use crate::{index::ElementIdx, pcf::Element};
pub type NameIndex = u16;

#[derive(Debug, From, Clone, Hash, PartialEq, Eq)]
pub enum Attribute {
    Element(ElementIdx),
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
    ElementArray(Box<[ElementIdx]>),
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

impl From<f32> for Attribute {
    fn from(value: f32) -> Self {
        Self::Float(value.into())
    }
}

impl From<bool> for Attribute {
    fn from(value: bool) -> Self {
        Self::Bool(value.into())
    }
}

// impl From<Box<[u8]>> for Attribute {
//     fn from(value: Box<[u8]>) -> Self {
//         Self::Binary(value)
//     }
// }

impl Default for Attribute {
    fn default() -> Self {
        Self::Element(ElementIdx::INVALID)
    }
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

pub trait ReadAttribute: Sized {
    type Err: From<io::Error> = io::Error;
    fn read_attribute(reader: &mut impl io::BufRead) -> Result<Self, Self::Err>;
}

pub trait WriteAttribute: Sized {
    type Err: From<io::Error> = io::Error;
    fn write_attribute(&self, writer: &mut impl io::Write) -> Result<(), Self::Err>;
}

impl ReadAttribute for u32 {
    fn read_attribute(reader: &mut impl io::BufRead) -> Result<Self, Self::Err> {
        reader.read_u32::<LittleEndian>()
    }
}

impl ReadAttribute for ElementIdx {
    fn read_attribute(reader: &mut impl io::BufRead) -> Result<Self, Self::Err> {
        Ok(ElementIdx::from_unchecked(reader.read_u32::<LittleEndian>()?))
    }
}

impl WriteAttribute for ElementIdx {
    fn write_attribute(&self, writer: &mut impl io::Write) -> Result<(), Self::Err> {
        writer.write_u32::<LittleEndian>((*self).into())
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
        writer.write_all(self.as_bytes_with_nul())
    }
}

impl ReadAttribute for Box<[u8]> {
    fn read_attribute(reader: &mut impl io::BufRead) -> Result<Self, Self::Err> {
        let count = reader.read_u32::<LittleEndian>()? as usize;

        let mut buf = vec![0; count].into_boxed_slice();
        reader.read_exact(&mut buf)?;
        Ok(buf)
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

pub(crate) struct AttributeReader<'a, R: std::io::BufRead> {
    element_count: usize,
    first_attribute_count: usize,
    reader: &'a mut R,
}

pub(crate) struct AttributeIterator<'a, R: std::io::BufRead> {
    element_count: usize,
    current_element: usize,
    current_attribute_count: usize,
    current_attribute: usize,
    reader: AttributeReader<'a, R>,
}

pub(crate) struct AttributeWriter<'a, W: io::Write> {
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

    pub fn write_attributes(
        &mut self,
        particle_system_definitions_name_idx: NameIndex,
        root_definitions: &[ElementIdx],
        elements: &Vec<Element>,
    ) -> Result<(), io::Error> {
        const ELEMENT_ARRAY_TYPE: u8 = 15;

        // the root element always has only 1 attribute, and the element array type is always 15.
        self.writer.write_u32::<LittleEndian>(1)?;
        self.writer
            .write_u16::<LittleEndian>(particle_system_definitions_name_idx)?;
        self.writer.write_u8(ELEMENT_ARRAY_TYPE)?;
        self.write_array(root_definitions)?;

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
pub enum ReadError {
    #[error("{source}")]
    Io {
        #[from]
        source: io::Error,
        backtrace: Backtrace,
    },

    #[error(transparent)]
    CStringFromVec(#[from] std::ffi::FromVecWithNulError),

    #[error("the attribute type {0} is unsupported or invalid")]
    InvalidAttributeType(u8),
}

impl<'a, R: std::io::BufRead> Iterator for AttributeIterator<'a, R> {
    type Item = Result<(usize, NameIndex, Attribute), ReadError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_attribute < self.current_attribute_count {
            self.current_attribute += 1;
            match self.reader.read_attribute() {
                Ok((name_idx, attribute)) => Some(Ok((self.current_element, name_idx, attribute))),
                Err(err) => Some(Err(err)),
            }
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
                }
                Err(err) => Some(Err(err.into())),
            }
        }
    }
}

impl<'a, R: std::io::BufRead> IntoIterator for AttributeReader<'a, R> {
    type Item = Result<(usize, NameIndex, Attribute), ReadError>;

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
            1 => Ok(self.read::<ElementIdx>()?.into()),
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
            15 => Ok(self.read_array::<ElementIdx>()?.into()),
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
        }
        .map(|attr| (name_idx, attr))
    }
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

impl Display for Bool8 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0 == 0 {
            f.write_str("false")
        } else {
            f.write_str("true")
        }
    }
}

pub type Float = OrderedFloat<f32>;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, derive_more::Display)]
#[display("Color({_0}, {_1}, {_2}, {_3})")]
pub struct Color(pub u8, pub u8, pub u8, pub u8);

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, derive_more::Display)]
#[display("Vector2({_0:.2}, {_1:.2})")]
pub struct Vector2(pub Float, pub Float);

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, derive_more::Display)]
#[display("Vector3({_0:.2}, {_1:.2}, {_2:.2})")]
pub struct Vector3(pub Float, pub Float, pub Float);

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, derive_more::Display)]
#[display("Vector4({_0:.2}, {_1:.2}, {_2:.2}, {_3:.2})")]
pub struct Vector4(pub Float, pub Float, pub Float, pub Float);

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, derive_more::Display)]
#[display("Matrix(...)")]
pub struct Matrix(pub Vector4, pub Vector4, pub Vector4, pub Vector4);
